//! The GPUI side of the bench: `run`, `idle` and the child process of `start`.
//!
//! Every window here ends by itself (roadmap rule 9): runs quit after their last frame, and
//! a watchdog thread exits the process if a run stalls (for example a minimised window that
//! stops receiving frame requests).

use crate::alloc::{self, AllocCounts, ours};
use crate::compare::SCHEMA;
use crate::driver::{Camera, Script};
use crate::scene::{BenchScene, PAPER, SceneElement, Sheet};
use crate::stats::{self, Summary, round};
use crate::sys;
use gpui::{
    AnyElement, AnyWindowHandle, App, AppContext, AsyncApp, Bounds, Context, FrameDurationSnapshot,
    IntoElement, Render, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// T0.1's window size, in logical pixels.
pub const T01_WINDOW: (f32, f32) = (960.0, 640.0);

/// Exits the process with code 3 if it is still running after `limit`.
pub fn watchdog(what: &'static str, limit: Duration) {
    std::thread::Builder::new()
        .name("bench-watchdog".into())
        .spawn(move || {
            std::thread::sleep(limit);
            eprintln!("bench {what}: still running after {limit:?}; giving up");
            std::process::exit(3);
        })
        .expect("spawn watchdog thread");
}

type Outcome = Rc<RefCell<Option<Result<Value, String>>>>;

fn run_app(
    what: &str,
    open: impl FnOnce(&mut App, Outcome) -> Result<(), String> + 'static,
) -> Result<Value, String> {
    let outcome: Outcome = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(error) = open(cx, slot.clone()) {
            *slot.borrow_mut() = Some(Err(error));
            cx.quit();
        }
    });
    outcome
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err(format!("bench {what}: the app quit without a result")))
}

/// The window content for `sheet`. The `t01` sheet is the T0.1 view, built the same way.
fn content(
    sheet: Sheet,
    scene: &Option<Rc<RefCell<dyn BenchScene>>>,
    camera: Camera,
    paint_time: &Rc<Cell<Option<Duration>>>,
) -> AnyElement {
    match (sheet, scene) {
        (_, Some(scene)) => SceneElement {
            scene: scene.clone(),
            camera,
            paint_time: paint_time.clone(),
        }
        .into_any_element(),
        _ => div().size_full().bg(rgb(PAPER)).into_any_element(),
    }
}

fn window_options(size_logical: (f32, f32), cx: &App, unthrottled: bool) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(size_logical.0), px(size_logical.1)), cx);
    let mut options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(size(px(size_logical.0), px(size_logical.1))),
        ..Default::default()
    };
    // Linux reference runs use the panel's exact logical size, without desktop decorations.
    // WindowBounds::Fullscreen is verified at the pinned GPUI platform.rs:2802.
    #[cfg(target_os = "linux")]
    if std::env::var_os("OMNIA_BENCH_FULLSCREEN").is_some() {
        options.window_bounds = Some(WindowBounds::Fullscreen(bounds));
        options.window_min_size = None;
    }
    if unthrottled {
        // GPUI caps inactive and low-power windows at about 30 fps by default; an unattended
        // run must not depend on which window has focus or on the power plan.
        options.inactive_frame_interval = None;
        options.low_power_frame_interval = None;
        options.thermal_frame_interval = None;
    }
    options
}

pub(crate) fn environment(window: &Window, cx: &App) -> Value {
    let scale = window.scale_factor();
    let viewport = window.viewport_size();
    let (w, h) = (f32::from(viewport.width), f32::from(viewport.height));
    json!({
        "gpu": window.gpu_specs(),
        "fullscreen": window.is_fullscreen(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "logical_cpus": std::thread::available_parallelism().map_or(0, |n| n.get()),
        "scale_factor": scale,
        "viewport_logical": [w, h],
        "viewport_device": [(w * scale).round(), (h * scale).round()],
        "refresh_hz": window
            .refresh_interval(cx)
            .map(|interval| round(1.0 / interval.as_secs_f64())),
    })
}

fn memory_metrics(metrics: &mut Map<String, Value>) {
    let mb = |bytes: u64| round(bytes as f64 / (1024.0 * 1024.0));
    if let Some(memory) = sys::memory() {
        metrics.insert("memory_mb".into(), json!(mb(memory.private_bytes)));
        metrics.insert("working_set_mb".into(), json!(mb(memory.working_set_bytes)));
    }
    let (_, heap_peak) = alloc::heap_bytes();
    metrics.insert("heap_peak_mb".into(), json!(mb(heap_peak as u64)));
}

fn insert_summary(metrics: &mut Map<String, Value>, key: &str, summary: Option<Summary>) {
    if let Some(summary) = summary {
        metrics.insert(key.into(), summary.to_json());
    }
}

// ---- run ----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct RunConfig {
    pub sheet: Sheet,
    pub script: Script,
    /// Measured frames.
    pub frames: u32,
    /// Frames drawn before measuring starts.
    pub warmup: u32,
    /// Window content size, in logical pixels.
    pub size: (f32, f32),
}

impl RunConfig {
    fn to_json(self) -> Value {
        json!({
            "sheet": self.sheet.name(),
            "script": self.script.name(),
            "frames": self.frames,
            "warmup": self.warmup,
            "window_logical": [self.size.0, self.size.1],
            // Reports with and without primitive counts are not comparable.
            "scene_counts": cfg!(feature = "scene-counts"),
        })
    }
}

/// What a `run` render schedules after drawing frame `frame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterFrame {
    /// Draw the next frame.
    Draw,
    /// Frame `frame` is the last warm-up frame. Before the first measured frame, take the
    /// baseline snapshot, then draw. The snapshot is taken in a next-frame callback, which
    /// GPUI runs before it dirties the window and before `Window::draw`, so it is outside the
    /// first measured frame's draw, work and dirty-to-present spans and its allocations.
    BaselineThenDraw,
    /// Frame `frame` read the last measured frame's costs: report and quit.
    Report,
    /// Past the end: nothing more.
    Stop,
}

fn after_frame(frame: u32, warmup: u32, frames: u32) -> AfterFrame {
    let last = warmup + frames;
    if frame == last {
        AfterFrame::Report
    } else if frame > last {
        AfterFrame::Stop
    } else if frame + 1 == warmup {
        AfterFrame::BaselineThenDraw
    } else {
        AfterFrame::Draw
    }
}

struct RunView {
    config: RunConfig,
    cached: Option<gpui::Entity<CachedSheet>>,
    scene: Option<Rc<RefCell<dyn BenchScene>>>,
    /// Index of the frame being rendered, from 0.
    frame: u32,
    baseline: Option<FrameDurationSnapshot>,
    previous_counts: AllocCounts,
    /// One entry per measured frame; capacity reserved up front so pushing never allocates.
    allocs: Vec<AllocCounts>,
    paint_ms: Vec<f64>,
    paint_time: Rc<Cell<Option<Duration>>>,
    started: Instant,
    outcome: Outcome,
    trace: Option<crate::trace::FrameTrace>,
}

impl Render for RunView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = self.frame;
        self.frame += 1;
        let first_measured = self.config.warmup;
        let last = self.config.warmup + self.config.frames;

        // Frame `k`'s costs are read at the start of render `k + 1`. GPUI has drawn and
        // presented frame `k` by then, so its histograms include it.
        if frame == first_measured && self.baseline.is_none() {
            // The baseline must already cover every warm-up frame and nothing after it.
            *self.outcome.borrow_mut() = Some(Err(
                "baseline snapshot was not taken before the first measured frame".into(),
            ));
            window.on_next_frame(|_, cx| cx.quit());
            return content(
                self.config.sheet,
                &None,
                Camera::default(),
                &self.paint_time,
            );
        }
        let counts = AllocCounts::now();
        let paint = self.paint_time.take();
        if frame > first_measured && frame <= last {
            self.allocs.push(counts.since(self.previous_counts));
            if let Some(paint) = paint {
                self.paint_ms.push(paint.as_secs_f64() * 1000.0);
            }
        }
        self.previous_counts = counts;
        if let Some(scene) = &self.scene {
            scene.borrow_mut().after_frame(window);
        }

        match after_frame(frame, self.config.warmup, self.config.frames) {
            AfterFrame::Draw => window.request_animation_frame(),
            AfterFrame::BaselineThenDraw => {
                let view = cx.entity();
                window.on_next_frame(move |window, cx| {
                    let baseline = window.frame_duration_snapshot();
                    view.update(cx, |view, cx| {
                        view.baseline = Some(baseline);
                        cx.notify();
                    });
                });
            }
            AfterFrame::Report => {
                let report = self.report(window, cx);
                *self.outcome.borrow_mut() = Some(report);
                window.on_next_frame(|_, cx| cx.quit());
            }
            AfterFrame::Stop => {}
        }

        let script = self.config.script;
        let camera = ours(|| script.camera(frame));
        if matches!(self.config.sheet, Sheet::Spike(c) if c.scene == 6) {
            let cached = self
                .cached
                .get_or_insert_with(|| {
                    cx.new(|_| CachedSheet {
                        sheet: self.config.sheet,
                        scene: self.scene.clone(),
                        paint_time: self.paint_time.clone(),
                    })
                })
                .clone();
            if matches!(self.config.sheet, Sheet::Spike(c) if c.mode == 0) {
                cached.update(cx, |_, cx| cx.notify());
            }
            return div()
                .size_full()
                .child(cached.cached(gpui::StyleRefinement::default().size_full()))
                .child(
                    div()
                        .absolute()
                        .left(px((frame % 200) as f32))
                        .top(px(4.))
                        .size(px(4.))
                        // Mirrors the Q2 `stage/select` overlay token until T1.2 lands tokens.
                        .bg(rgb(0x2f6feb)),
                )
                .into_any_element();
        }
        content(self.config.sheet, &self.scene, camera, &self.paint_time)
    }
}

impl RunView {
    fn report(&mut self, window: &mut Window, cx: &App) -> Result<Value, String> {
        let end = window.frame_duration_snapshot();
        let base = self.baseline.as_ref().ok_or("no baseline snapshot")?;
        let delta =
            |later, earlier| stats::delta(later, earlier).ok_or("histogram subtraction failed");

        let baseline_draws = base.draw_duration_histogram.len();
        let draw = delta(&end.draw_duration_histogram, &base.draw_duration_histogram)?;
        let dirty_to_present = delta(
            &end.dirty_to_present_histogram,
            &base.dirty_to_present_histogram,
        )?;
        let present_interval = delta(
            &end.present_interval_histogram,
            &base.present_interval_histogram,
        )?;
        let by_refresh: Vec<_> = end
            .by_refresh_interval
            .iter()
            .map(|later| {
                match base
                    .by_refresh_interval
                    .iter()
                    .find(|earlier| earlier.refresh_interval == later.refresh_interval)
                {
                    Some(earlier) => later.since(earlier),
                    None => later.clone(),
                }
            })
            .collect();
        let work = stats::merged(by_refresh.iter().map(|r| &r.work));
        let sum =
            |f: fn(&gpui::RefreshIntervalFrames) -> u64| by_refresh.iter().map(f).sum::<u64>();
        let drawn = draw.len();
        let on_time = sum(|r| r.frames_on_time);
        let late = sum(|r| r.frames_late);
        let unmeasured = sum(|r| r.frames_unmeasured);
        // GPUI's own count: refreshes a frame's *work* overran. Not the B3 metric.
        let work_missed = sum(|r| r.missed_refreshes);
        // B3/B4 (owner decision 2026-11-16, T1.6): a present interval longer than 1.5 refresh
        // intervals is a missed refresh; B4 allows none longer than 2.5.
        let refresh_nanos = window
            .refresh_interval(cx)
            .map(|interval| interval.as_nanos() as u64);
        let over = |threshold| {
            refresh_nanos.map(|r| stats::count_over_refreshes(&present_interval, r, threshold))
        };

        let mut ours_allocs: Vec<f64> = self.allocs.iter().map(|a| a.ours_allocs as f64).collect();
        let mut total_allocs: Vec<f64> =
            self.allocs.iter().map(|a| a.total_allocs as f64).collect();
        let mut ours_bytes: Vec<f64> = self.allocs.iter().map(|a| a.ours_bytes as f64).collect();
        let mut total_bytes: Vec<f64> = self.allocs.iter().map(|a| a.total_bytes as f64).collect();
        let alloc_json = |samples: &mut Vec<f64>| {
            Summary::from_samples(samples).map_or(Value::Null, |s| s.to_json())
        };

        let mut metrics = Map::new();
        insert_summary(
            &mut metrics,
            "frame_ms",
            work.as_ref().and_then(Summary::from_nanos),
        );
        insert_summary(&mut metrics, "draw_ms", Summary::from_nanos(&draw));
        insert_summary(
            &mut metrics,
            "dirty_to_present_ms",
            Summary::from_nanos(&dirty_to_present),
        );
        insert_summary(
            &mut metrics,
            "present_interval_ms",
            Summary::from_nanos(&present_interval),
        );
        // Our paint is timed only on frames that paint the scene. S6 cached replays the sheet
        // without painting it, so its paint samples are sparse by design.
        let paint_samples = self.paint_ms.len();
        let paint_sampling = match (self.config.sheet, &self.scene) {
            (Sheet::Spike(c), _) if c.scene == 6 && c.mode == 1 => "sparse-cached",
            (_, Some(_)) => "every-frame",
            (_, None) => "none",
        };
        metrics.insert("paint_samples".into(), json!(paint_samples));
        metrics.insert("paint_sampling".into(), json!(paint_sampling));
        insert_summary(
            &mut metrics,
            "paint_ms",
            Summary::from_samples(&mut self.paint_ms),
        );
        metrics.insert(
            "allocs_per_frame".into(),
            json!({ "ours": alloc_json(&mut ours_allocs), "total": alloc_json(&mut total_allocs) }),
        );
        metrics.insert(
            "alloc_bytes_per_frame".into(),
            json!({ "ours": alloc_json(&mut ours_bytes), "total": alloc_json(&mut total_bytes) }),
        );
        metrics.insert("late_frames".into(), json!(late));
        metrics.insert("missed_refreshes".into(), json!(over(1.5)));
        metrics.insert(
            "present_intervals_over_2_5_refreshes".into(),
            json!(over(2.5)),
        );
        metrics.insert(
            "max_present_interval_ms".into(),
            json!(
                (!present_interval.is_empty()).then(|| round(present_interval.max() as f64 / 1e6))
            ),
        );
        metrics.insert("work_missed_refreshes".into(), json!(work_missed));
        memory_metrics(&mut metrics);

        #[cfg(feature = "scene-counts")]
        metrics.insert("primitives".into(), primitive_counts(window));

        let mut warnings = Vec::new();
        // The baseline should hold exactly the warm-up draws.
        if baseline_draws != u64::from(self.config.warmup) {
            warnings.push(format!(
                "baseline snapshot covers {baseline_draws} draws, expected {} warm-up frames",
                self.config.warmup
            ));
        }
        if drawn != u64::from(self.config.frames) {
            warnings.push(format!(
                "GPUI drew {drawn} frames, expected {}",
                self.config.frames
            ));
        }
        if self.allocs.len() != self.config.frames as usize {
            warnings.push(format!("sampled {} frames", self.allocs.len()));
        }
        if paint_sampling == "every-frame" && paint_samples != self.config.frames as usize {
            warnings.push(format!("sampled paint on {paint_samples} frames"));
        }
        if refresh_nanos.is_none() {
            warnings.push("refresh interval unknown: missed refreshes not counted".into());
        }
        if !window.is_window_active() {
            warnings.push("window was not active: present intervals are not recorded".into());
        }

        let diagnostic_trace = self.trace.as_mut().map(|trace| trace.collect());
        let mut config = self.config.to_json();
        if diagnostic_trace.is_some() {
            // compare requires matching config: traced costs must not be compared to gates.
            config["diagnostic_trace"] = json!(true);
        }
        Ok(json!({
            "schema": SCHEMA,
            "kind": "run",
            "config": config,
            "scene_details": self.scene.as_ref().map(|s|s.borrow().details()),
            "spike": match self.config.sheet { Sheet::Spike(c) => Some(c.json()), _ => None },
            "environment": environment(window, cx),
            "frames": {
                "drawn": drawn,
                "on_time": on_time,
                "late": late,
                "unmeasured": unmeasured,
                "wall_s": round(self.started.elapsed().as_secs_f64()),
            },
            "warnings": warnings,
            "metrics": metrics,
            "diagnostic_trace": diagnostic_trace,
        }))
    }
}

/// Primitive counts by kind in the last measured frame's scene.
#[cfg(feature = "scene-counts")]
pub(crate) fn primitive_counts(window: &Window) -> Value {
    let scene = window.rendered_scene();
    json!({
        "shadows": scene.shadows.len(),
        "quads": scene.quads.len(),
        "paths": scene.paths.len(),
        "underlines": scene.underlines.len(),
        "monochrome_sprites": scene.monochrome_sprites.len(),
        "subpixel_sprites": scene.subpixel_sprites.len(),
        "polychrome_sprites": scene.polychrome_sprites.len(),
        "surfaces": scene.surfaces.len(),
        "batches": scene.batches().count(),
        "paint_operations": scene.len(),
    })
}

pub fn run(config: RunConfig) -> Result<Value, String> {
    let tracing = std::env::var_os("OMNIA_BENCH_TRACE").is_some();
    let enabled_trace = tracing && gpui::set_trace_enabled(true);
    // Generous: 600 frames take 10 s at 60 Hz.
    watchdog(
        "run",
        Duration::from_secs(60) + Duration::from_millis(100) * (config.frames + config.warmup),
    );
    let result = run_app("run", move |cx, outcome| {
        if matches!(config.sheet, Sheet::Spike(c) if c.scene==5) {
            cx.text_system()
                .add_fonts(vec![std::borrow::Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice(),
                )])
                .map_err(|e| format!("load bundled Inter: {e}"))?;
        }
        let options = window_options(config.size, cx, true);
        let frames = config.frames as usize;
        cx.open_window(options, move |_window, cx| {
            #[cfg(not(target_os = "linux"))]
            _window.resize(size(px(config.size.0), px(config.size.1)));
            let trace = tracing.then(|| crate::trace::FrameTrace::new(cx));
            cx.new(|_| RunView {
                config,
                cached: None,
                scene: config.sheet.scene(),
                frame: 0,
                baseline: None,
                previous_counts: AllocCounts::now(),
                allocs: Vec::with_capacity(frames),
                paint_ms: Vec::with_capacity(frames),
                paint_time: Rc::new(Cell::new(None)),
                started: Instant::now(),
                outcome,
                trace,
            })
        })
        .map_err(|e| format!("open window: {e}"))?;
        cx.activate(true);
        Ok(())
    });
    if enabled_trace {
        gpui::set_trace_enabled(false);
    }
    result
}

// ---- idle ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct IdleConfig {
    pub sheet: Sheet,
    pub size: (f32, f32),
    pub seconds: f64,
    /// Time allowed for start-up frames before the idle window is measured.
    pub settle: Duration,
}

/// One render of the idle window, kept so any idle frame can be attributed: GPUI also
/// redraws on outside events such as activation and the pointer entering or leaving.
#[derive(Clone, Copy)]
struct RenderEvent {
    at: Instant,
    active: bool,
    hovered: bool,
}

type RenderLog = Rc<RefCell<Vec<RenderEvent>>>;

/// The idle window: `sheet` at rest, with the same window options as `omnia-app` without
/// `--frames`.
struct IdleView {
    sheet: Sheet,
    scene: Option<Rc<RefCell<dyn BenchScene>>>,
    paint_time: Rc<Cell<Option<Duration>>>,
    renders: RenderLog,
}

impl Render for IdleView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.borrow_mut().push(RenderEvent {
            at: Instant::now(),
            active: window.is_window_active(),
            hovered: window.is_window_hovered(),
        });
        content(self.sheet, &self.scene, Camera::default(), &self.paint_time)
    }
}

pub fn idle(config: IdleConfig) -> Result<Value, String> {
    let measure = Duration::from_secs_f64(config.seconds);
    watchdog("idle", config.settle + measure + Duration::from_secs(30));
    run_app("idle", move |cx, outcome| {
        let renders: RenderLog = Rc::new(RefCell::new(Vec::with_capacity(1024)));
        let view_renders = renders.clone();
        let handle = cx
            .open_window(
                window_options(config.size, cx, false),
                move |_window, cx| {
                    #[cfg(not(target_os = "linux"))]
                    _window.resize(size(px(config.size.0), px(config.size.1)));
                    cx.new(|_| IdleView {
                        sheet: config.sheet,
                        scene: config.sheet.scene(),
                        paint_time: Rc::new(Cell::new(None)),
                        renders: view_renders,
                    })
                },
            )
            .map_err(|e| format!("open window: {e}"))?;
        cx.activate(true);
        let window = handle.into();
        cx.spawn(async move |cx| {
            let report = measure_idle(cx, window, renders, config).await;
            *outcome.borrow_mut() = Some(report);
            cx.update(|cx| cx.quit());
        })
        .detach();
        Ok(())
    })
}

async fn measure_idle(
    cx: &mut AsyncApp,
    window: AnyWindowHandle,
    renders: RenderLog,
    config: IdleConfig,
) -> Result<Value, String> {
    // Reading the profiler through `update_window` neither notifies a view nor dirties the
    // window, so measuring adds no frame demand.
    let draws = |cx: &mut AsyncApp| {
        cx.update_window(window, |_, window, _| {
            window
                .frame_duration_snapshot()
                .draw_duration_histogram
                .len()
        })
        .map_err(|e| e.to_string())
    };
    cx.background_executor().timer(config.settle).await;
    let startup_draws = draws(cx)?;
    let startup_renders = renders.borrow().len();
    let start_cpu = sys::cpu_time();
    let start_wall = Instant::now();
    cx.background_executor()
        .timer(Duration::from_secs_f64(config.seconds))
        .await;
    let wall = start_wall.elapsed();
    let end_cpu = sys::cpu_time();
    let end_draws = draws(cx)?;

    let mut metrics = Map::new();
    metrics.insert("frames".into(), json!(end_draws - startup_draws));
    let measured_renders: Vec<Value> = renders.borrow()[startup_renders..]
        .iter()
        .map(|r| {
            json!({
                "t_s": round(r.at.saturating_duration_since(start_wall).as_secs_f64()),
                "window_active": r.active,
                "pointer_over_window": r.hovered,
            })
        })
        .collect();
    metrics.insert("renders".into(), json!(measured_renders.len()));
    if let (Some(a), Some(b)) = (start_cpu, end_cpu) {
        let percent = (b - a).as_secs_f64() / wall.as_secs_f64() * 100.0;
        metrics.insert("cpu_percent".into(), json!(round(percent)));
    }
    memory_metrics(&mut metrics);
    let environment = cx
        .update_window(window, |_, window, cx| environment(window, cx))
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "schema": SCHEMA,
        "kind": "idle",
        "config": {
            "sheet": config.sheet.name(),
            "seconds": config.seconds,
            "settle_ms": config.settle.as_millis() as u64,
            "window_logical": [config.size.0, config.size.1],
        },
        "environment": environment,
        "startup": { "frames": startup_draws, "renders": startup_renders },
        "measured_s": round(wall.as_secs_f64()),
        "measured_renders": measured_renders,
        "metrics": metrics,
    }))
}

// ---- start (child process) -----------------------------------------------------------------

/// The line a `start` child prints once its first frame has been drawn and presented.
pub const READY_LINE: &str = "omnia-bench-ready";

struct StartView {
    sheet: Sheet,
    scene: Option<Rc<RefCell<dyn BenchScene>>>,
    paint_time: Rc<Cell<Option<Duration>>>,
    reported: bool,
}

impl Render for StartView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.reported {
            self.reported = true;
            // A task spawned during the draw runs after GPUI returns from the frame request,
            // which is after this frame's `Window::draw` and present.
            cx.spawn(async move |_, cx| {
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{READY_LINE}");
                let _ = stdout.flush();
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
        content(self.sheet, &self.scene, Camera::default(), &self.paint_time)
    }
}

/// Opens one window, reports its first presented frame on stdout and quits.
pub fn start_child(sheet: Sheet, window_size: (f32, f32)) -> Result<(), String> {
    watchdog("start child", Duration::from_secs(30));
    let outcome: Outcome = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        let opened = cx.open_window(
            window_options(window_size, cx, false),
            move |_window, cx| {
                #[cfg(not(target_os = "linux"))]
                _window.resize(size(px(window_size.0), px(window_size.1)));
                cx.new(|_| StartView {
                    sheet,
                    scene: sheet.scene(),
                    paint_time: Rc::new(Cell::new(None)),
                    reported: false,
                })
            },
        );
        match opened {
            Ok(_) => cx.activate(true),
            Err(error) => {
                *slot.borrow_mut() = Some(Err(format!("open window: {error}")));
                cx.quit();
            }
        }
    });
    match outcome.borrow_mut().take() {
        Some(Err(error)) => Err(error),
        _ => Ok(()),
    }
}

struct CachedSheet {
    sheet: Sheet,
    scene: Option<Rc<RefCell<dyn BenchScene>>>,
    paint_time: Rc<Cell<Option<Duration>>>,
}
impl Render for CachedSheet {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        content(self.sheet, &self.scene, Camera::default(), &self.paint_time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_is_scheduled_between_warmup_and_the_first_measured_frame() {
        let (warmup, frames) = (3, 4);
        let plan: Vec<_> = (0..=8).map(|f| after_frame(f, warmup, frames)).collect();
        use AfterFrame::*;
        // Frames 0..3 are warm-up, 3..7 are measured, render 7 reads frame 6 and reports.
        assert_eq!(
            plan,
            [
                Draw,
                Draw,
                BaselineThenDraw,
                Draw,
                Draw,
                Draw,
                Draw,
                Report,
                Stop
            ]
        );
        // Only the last warm-up frame schedules it, so no measured render takes it.
        for f in warmup..warmup + frames {
            assert_ne!(after_frame(f, warmup, frames), BaselineThenDraw);
        }
        assert_eq!(after_frame(0, 1, 1), BaselineThenDraw);
        assert_eq!(after_frame(1, 1, 1), Draw);
        assert_eq!(after_frame(2, 1, 1), Report);
    }
}
