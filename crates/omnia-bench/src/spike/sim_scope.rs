//! SS2 (S0.4): live scope updates above the real cached 1k sheet.

use crate::alloc::{AllocCounts, not_ours, ours};
use crate::generate::StressSheet;
use crate::harness::{
    environment, memory_metrics, register_sheet_fonts, run_app, watchdog, window_options,
};
use crate::scene::{STRESS_SEED, Sheet};
use crate::stats::{self, Summary, round};
use crate::trace::FrameTrace;
use gpui::{
    App, Bounds, Context, Entity, FrameDurationSnapshot, Hsla, IntoElement, Pixels, Render,
    StyleRefinement, Window, canvas, div, fill, point, prelude::*, px, size,
};
use omnia_canvas::sheet::{SheetDocument, SheetPaintScope};
use omnia_canvas::{Camera, SheetView};
use omnia_doc::Pt;
use omnia_theme::{GRAPHITE, Rgba};
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const SAMPLE_RATE: u64 = 100_000;
const RING_CAPACITY: usize = 65_536;
const COLUMNS: usize = 512;
const HISTORY: usize = 4 * COLUMNS;
const SCOPE_WIDTH: f32 = 576.;
const SCOPE_HEIGHT: f32 = 468.;
const SHEET: Sheet = Sheet::Stress(StressSheet::Parts1k);

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Sample {
    t: f64,
    y: f64,
}

type Pair = [Sample; 2];

struct SampleRing {
    slots: Box<[UnsafeCell<Pair>]>,
    read: AtomicUsize,
    written: AtomicUsize,
    attempted: AtomicU64,
    dropped: AtomicU64,
}

// SAFETY: Only the non-cloneable Producer writes and only the non-cloneable Consumer
// reads. A release of written publishes initialized slots to the reader's acquire.
// A release of read returns those slots to the writer after all readers are done.
unsafe impl Sync for SampleRing {}

struct Producer(Arc<SampleRing>);
struct Consumer(Arc<SampleRing>);

impl SampleRing {
    fn split(capacity: usize) -> (Producer, Consumer) {
        assert!(capacity.is_power_of_two());
        let ring = Arc::new(Self {
            slots: (0..capacity)
                .map(|_| UnsafeCell::new(Pair::default()))
                .collect(),
            read: AtomicUsize::new(0),
            written: AtomicUsize::new(0),
            attempted: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
        });
        (Producer(ring.clone()), Consumer(ring))
    }
}

impl Producer {
    /// Publishes a batch and drops its new arrivals that do not fit (SK6).
    /// Both channels are published together with a single cursor.
    fn publish(&mut self, count: u64, mut sample: impl FnMut(u64) -> Pair) {
        let ring = &self.0;
        let write = ring.written.load(Ordering::Relaxed);
        let read = ring.read.load(Ordering::Acquire);
        let first = ring.attempted.load(Ordering::Relaxed);
        let free = ring.slots.len() - write.wrapping_sub(read);
        let accepted = count.min(free as u64);
        let mask = ring.slots.len() - 1;
        for i in 0..accepted {
            let slot = write.wrapping_add(i as usize) & mask;
            // SAFETY: These free slots are exclusively owned by this producer.
            unsafe { *ring.slots[slot].get() = sample(first + i) };
        }
        ring.written
            .store(write.wrapping_add(accepted as usize), Ordering::Release);
        ring.dropped.fetch_add(count - accepted, Ordering::Relaxed);
        ring.attempted.store(first + count, Ordering::Relaxed);
    }
}

impl Consumer {
    /// The acquire fixes a finite batch even if the producer keeps writing.
    fn drain(&mut self, mut visit: impl FnMut(Pair)) {
        let ring = &self.0;
        let read = ring.read.load(Ordering::Relaxed);
        let written = ring.written.load(Ordering::Acquire);
        let mask = ring.slots.len() - 1;
        for i in 0..written.wrapping_sub(read) {
            let slot = read.wrapping_add(i) & mask;
            // SAFETY: Published slots remain owned by this consumer until read advances.
            visit(unsafe { *ring.slots[slot].get() });
        }
        ring.read.store(written, Ordering::Release);
    }
}

fn synthesize(index: u64) -> Pair {
    let t = index as f64 / SAMPLE_RATE as f64;
    let phase = t * std::f64::consts::TAU * 723.4;
    [
        Sample { t, y: phase.sin() },
        Sample {
            t,
            y: 0.75 * phase.cos(),
        },
    ]
}

struct TraceBuffer {
    history: Box<[Pair]>,
    consumed: u64,
    empty_updates: u64,
}

impl TraceBuffer {
    fn new() -> Self {
        Self {
            history: vec![Pair::default(); HISTORY].into_boxed_slice(),
            consumed: 0,
            empty_updates: 0,
        }
    }

    fn update(&mut self, consumer: &mut Consumer) {
        let before = self.consumed;
        consumer.drain(|pair| {
            self.history[self.consumed as usize % HISTORY] = pair;
            self.consumed += 1;
        });
        self.empty_updates += u64::from(before == self.consumed);
    }

    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let plot = Bounds {
            origin: bounds.origin + point(px(16.), px(24.)),
            size: size(bounds.size.width - px(32.), bounds.size.height - px(40.)),
        };
        let quad = |window: &mut Window, bounds, color| {
            not_ours(|| window.paint_quad(fill(bounds, color)));
        };
        quad(window, bounds, color(GRAPHITE.stage_instrument_body));
        quad(window, plot, color(GRAPHITE.instrument_screen));
        for division in 1..10 {
            let x = plot.origin.x + plot.size.width * (division as f32 / 10.);
            quad(
                window,
                Bounds {
                    origin: point(x, plot.origin.y),
                    size: size(px(1.), plot.size.height),
                },
                color(GRAPHITE.rule),
            );
        }
        for division in 1..8 {
            let y = plot.origin.y + plot.size.height * (division as f32 / 8.);
            quad(
                window,
                Bounds {
                    origin: point(plot.origin.x, y),
                    size: size(plot.size.width, px(1.)),
                },
                color(GRAPHITE.rule),
            );
        }

        let len = (self.consumed as usize).min(HISTORY);
        let start = self.consumed as usize - len;
        let x_step = plot.size.width / COLUMNS as f32;
        for (channel, ink) in [GRAPHITE.probe_a, GRAPHITE.probe_b].into_iter().enumerate() {
            for (column, offset) in (0..len).step_by(HISTORY / COLUMNS).enumerate() {
                let count = (HISTORY / COLUMNS).min(len - offset);
                let first = self.history[(start + offset) % HISTORY][channel].y;
                let (lo, hi) = (1..count).fold((first, first), |(lo, hi), i| {
                    let y = self.history[(start + offset + i) % HISTORY][channel].y;
                    (lo.min(y), hi.max(y))
                });
                let y = |volts: f64| plot.origin.y + plot.size.height * (0.5 - 0.45 * volts as f32);
                quad(
                    window,
                    Bounds {
                        origin: point(plot.origin.x + x_step * column as f32, y(hi)),
                        size: size(x_step, (y(lo) - y(hi)).max(px(1.))),
                    },
                    color(ink),
                );
            }
        }
    }
}

fn color(rgba: Rgba) -> Hsla {
    gpui::Rgba {
        r: rgba.r,
        g: rgba.g,
        b: rgba.b,
        a: rgba.a,
    }
    .into()
}

struct ScopeView {
    consumer: Consumer,
    trace: Rc<RefCell<TraceBuffer>>,
    paints: Rc<Cell<u64>>,
}

impl Render for ScopeView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        ours(|| self.trace.borrow_mut().update(&mut self.consumer));
        let trace = self.trace.clone();
        let paints = self.paints.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                paints.set(paints.get() + 1);
                window.paint_layer(|window| ours(|| trace.borrow().paint(bounds, window)));
            },
        )
        .size_full()
    }
}

struct SheetAllocScope;

impl SheetPaintScope for SheetAllocScope {
    fn omnia(&self, paint: impl FnOnce()) {
        ours(paint);
    }

    fn gpui(&self, paint: impl FnOnce()) {
        not_ours(paint);
    }
}

struct SheetLayer {
    doc: SheetDocument,
    paints: Rc<Cell<u64>>,
}

impl Render for SheetLayer {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let doc = self.doc.clone();
        let paints = self.paints.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, cx| {
                paints.set(paints.get() + 1);
                let view =
                    SheetView::with_document(Camera::new(Pt::ZERO, 1., bounds), &GRAPHITE, doc);
                window.paint_layer(|window| {
                    view.paint_with_scope(bounds, window, cx, &SheetAllocScope)
                });
            },
        )
        .size_full()
    }
}

#[derive(Clone, Copy)]
struct Config {
    hz: u32,
    frames: u32,
    warmup: u32,
    size: (f32, f32),
}

#[derive(Clone, Copy)]
struct Counters {
    at: Instant,
    sheet_paints: u64,
    scope_paints: u64,
    attempted: u64,
    dropped: u64,
    consumed: u64,
    empty_updates: u64,
    latest_sample_s: Option<f64>,
}

struct Baseline {
    frames: FrameDurationSnapshot,
    counts: Counters,
}

struct Driver {
    config: Config,
    frame: u32,
    baseline: Option<Baseline>,
    previous_allocs: AllocCounts,
    allocs: Vec<AllocCounts>,
    trace: Option<FrameTrace>,
}

type Outcome = Rc<RefCell<Option<Result<Value, String>>>>;

struct RunView {
    sheet: Entity<SheetLayer>,
    scope: Entity<ScopeView>,
    driver: Driver,
    outcome: Outcome,
}

impl Render for RunView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let outcome = self.outcome.clone();
        let root = cx.entity();
        // This runs before the *next* draw, after the current frame has presented.
        window.on_next_frame(move |window, cx| {
            let report = root.update(cx, |view, root_cx| {
                let driver = &mut view.driver;
                let frame = driver.frame + 1;
                driver.frame = frame;
                let config = driver.config;
                if frame > config.warmup {
                    let allocs = AllocCounts::now().since(driver.previous_allocs);
                    ours(|| driver.allocs.push(allocs));
                }
                if frame == config.warmup {
                    driver.baseline = Some(Baseline {
                        frames: window.frame_duration_snapshot(),
                        counts: Counters::read(&view.sheet, &view.scope, root_cx),
                    });
                }
                if frame == config.warmup + config.frames {
                    return Some(driver.report(
                        window,
                        root_cx,
                        Counters::read(&view.sheet, &view.scope, root_cx),
                    ));
                }
                driver.previous_allocs = AllocCounts::now();
                if frame.is_multiple_of(60 / config.hz) {
                    view.scope.update(root_cx, |_, cx| cx.notify());
                }
                // Keep B3's 60 Hz demand in both cases. On the intervening frames
                // at 30 Hz, both children replay, with no sheet/scope invalidation.
                root_cx.notify();
                None
            });
            if let Some(report) = report {
                *outcome.borrow_mut() = Some(report);
                cx.quit();
            }
        });

        div()
            .relative()
            .size_full()
            .child(
                self.sheet
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .child(
                self.scope.clone().cached(
                    StyleRefinement::default()
                        .absolute()
                        .right(px(20.))
                        .top(px(20.))
                        .w(px(SCOPE_WIDTH))
                        .h(px(SCOPE_HEIGHT)),
                ),
            )
    }
}

impl Counters {
    fn read(sheet: &Entity<SheetLayer>, scope: &Entity<ScopeView>, cx: &App) -> Self {
        let scope = scope.read(cx);
        let trace = scope.trace.borrow();
        Counters {
            at: Instant::now(),
            sheet_paints: sheet.read(cx).paints.get(),
            scope_paints: scope.paints.get(),
            attempted: scope.consumer.0.attempted.load(Ordering::Relaxed),
            dropped: scope.consumer.0.dropped.load(Ordering::Relaxed),
            consumed: trace.consumed,
            empty_updates: trace.empty_updates,
            latest_sample_s: trace
                .consumed
                .checked_sub(1)
                .map(|last| trace.history[last as usize % HISTORY][0].t),
        }
    }
}

impl Driver {
    fn report(&mut self, window: &Window, cx: &App, counts: Counters) -> Result<Value, String> {
        let base = self.baseline.as_ref().ok_or("missing warm-up baseline")?;
        let end = window.frame_duration_snapshot();
        let draw = stats::delta(
            &end.draw_duration_histogram,
            &base.frames.draw_duration_histogram,
        )
        .ok_or("draw histogram subtraction failed")?;
        let present = stats::delta(
            &end.present_interval_histogram,
            &base.frames.present_interval_histogram,
        )
        .ok_or("present histogram subtraction failed")?;
        let by_refresh: Vec<_> = end
            .by_refresh_interval
            .iter()
            .map(|later| {
                match base
                    .frames
                    .by_refresh_interval
                    .iter()
                    .find(|b| b.refresh_interval == later.refresh_interval)
                {
                    Some(earlier) => later.since(earlier),
                    None => later.clone(),
                }
            })
            .collect();
        let work = stats::merged(by_refresh.iter().map(|counts| &counts.work))
            .ok_or("missing frame work histogram")?;
        let refresh = window
            .refresh_interval(cx)
            .ok_or("unknown refresh interval")?;
        let over = |ratio| stats::count_over_refreshes(&present, refresh.as_nanos() as u64, ratio);
        let summary = |histogram| Summary::from_nanos(histogram).map(|s| s.to_json());
        let allocation_summary = |pick: fn(&AllocCounts) -> u64| {
            let mut values: Vec<_> = self.allocs.iter().map(|a| pick(a) as f64).collect();
            Summary::from_samples(&mut values).map(|s| s.to_json())
        };
        let config = self.config;
        let wall_s = (counts.at - base.counts.at).as_secs_f64();
        let scope_paints = counts.scope_paints - base.counts.scope_paints;
        let stride = 60 / config.hz;
        let expected =
            (config.warmup + config.frames).div_ceil(stride) - config.warmup.div_ceil(stride);
        let expected_frames = u64::from(config.frames);
        if draw.len() != expected_frames
            || work.len() != expected_frames
            || present.len() != expected_frames
            || scope_paints < u64::from(expected)
            || base.counts.sheet_paints == 0
        {
            return Err(format!(
                "incomplete measurement: expected {expected_frames} frames, got {} draws, {} work samples, {} presents; scope painted {scope_paints} times (scheduled {expected}); warm-up sheet paints {}",
                draw.len(),
                work.len(),
                present.len(),
                base.counts.sheet_paints
            ));
        }
        let mut metrics = Map::new();
        memory_metrics(&mut metrics);
        metrics.extend([
            ("frame_ms".into(), json!(summary(&work))),
            ("draw_ms".into(), json!(summary(&draw))),
            ("present_interval_ms".into(), json!(summary(&present))),
            ("missed_refreshes".into(), json!(over(1.5))),
            (
                "present_intervals_over_2_5_refreshes".into(),
                json!(over(2.5)),
            ),
            (
                "sheet_paints".into(),
                json!(counts.sheet_paints - base.counts.sheet_paints),
            ),
            (
                "allocs_per_frame".into(),
                json!({
                    "ours": allocation_summary(|a| a.ours_allocs),
                    "total": allocation_summary(|a| a.total_allocs),
                }),
            ),
        ]);
        Ok(json!({
            "schema": crate::compare::SCHEMA,
            "kind": "spike-sim-scope",
            "config": {
                "hz": config.hz, "frames": config.frames, "warmup": config.warmup,
                "window_logical": config.size, "sheet": SHEET.name(), "seed": STRESS_SEED,
                "scope_logical": [SCOPE_WIDTH, SCOPE_HEIGHT],
                "sample_rate_per_channel": SAMPLE_RATE, "capacity_per_channel": RING_CAPACITY,
            },
            "environment": environment(window, cx),
            "frames": {
                "drawn": draw.len(), "presents": present.len(), "work_samples": work.len(),
                "startup_draws": base.frames.draw_duration_histogram.len().checked_sub(u64::from(config.warmup)),
                "wall_s": round(wall_s),
            },
            "stream": {
                "scope_paints": scope_paints, "scheduled_scope_updates": expected,
                "measured_scope_hz": round(scope_paints as f64 / wall_s),
                "empty_scope_updates": counts.empty_updates - base.counts.empty_updates,
                "attempted_pairs": counts.attempted - base.counts.attempted,
                "attempted_pairs_per_second": round((counts.attempted - base.counts.attempted) as f64 / wall_s),
                "dropped_pairs": counts.dropped - base.counts.dropped,
                "consumed_pairs": counts.consumed - base.counts.consumed,
                "latest_sample_s": counts.latest_sample_s,
                "ring_bytes": RING_CAPACITY * size_of::<Pair>(),
            },
            "metrics": metrics,
            "diagnostic_trace": self.trace.as_mut().map(FrameTrace::collect),
        }))
    }
}

pub fn run(hz: u32, frames: u32, warmup: u32, size: (f32, f32)) -> Result<Value, String> {
    if !matches!(hz, 30 | 60) || !(1..=3600).contains(&frames) || !(2..=3600).contains(&warmup) {
        return Err(
            "expected 30 or 60 Hz, 1..=3600 measured frames and 2..=3600 warm-up frames".into(),
        );
    }
    let config = Config {
        hz,
        frames,
        warmup,
        size,
    };
    watchdog(
        "spike sim-scope",
        Duration::from_secs(30) + Duration::from_millis(100) * (frames + warmup),
    );
    let tracing = std::env::var_os("OMNIA_BENCH_TRACE").is_some();
    let enabled = tracing && gpui::set_trace_enabled(true);
    let doc = crate::generate::generate(StressSheet::Parts1k, STRESS_SEED);
    let (mut producer, consumer) = SampleRing::split(RING_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = thread::Builder::new()
        .name("ss2-samples".into())
        .spawn(move || {
            let start = Instant::now();
            let mut attempted = 0;
            while !worker_stop.load(Ordering::Relaxed) {
                // Derive the sample clock from wall time so a late wake does
                // not change the rate or timestamps. Never busy-wait.
                let due =
                    (start.elapsed().as_micros() * u128::from(SAMPLE_RATE) / 1_000_000) as u64;
                ours(|| producer.publish(due - attempted, synthesize));
                attempted = due;
                thread::park_timeout(Duration::from_millis(2));
            }
        })
        .map_err(|e| format!("start signal producer: {e}"))?;
    let result = run_app("spike sim-scope", move |cx, outcome| {
        register_sheet_fonts(SHEET, cx)?;
        let doc = cx.new(|_| doc);
        let options = window_options(size, cx, true);
        let opened = cx.open_window(options, move |window, cx| {
            #[cfg(not(target_os = "linux"))]
            window.resize(gpui::size(px(config.size.0), px(config.size.1)));
            #[cfg(target_os = "linux")]
            let _ = window;
            let trace = tracing.then(|| FrameTrace::new(cx));
            let sheet = cx.new(|_| SheetLayer {
                doc: SheetDocument::new(doc),
                paints: Rc::default(),
            });
            let scope = cx.new(|_| ScopeView {
                consumer,
                trace: Rc::new(RefCell::new(TraceBuffer::new())),
                paints: Rc::default(),
            });
            cx.new(|_| RunView {
                sheet,
                scope,
                driver: Driver {
                    config,
                    frame: 0,
                    baseline: None,
                    previous_allocs: AllocCounts::now(),
                    allocs: Vec::with_capacity(frames as usize),
                    trace,
                },
                outcome,
            })
        });
        opened.map_err(|e| format!("open window: {e}"))?;
        cx.activate(true);
        Ok(())
    });
    stop.store(true, Ordering::Relaxed);
    worker.thread().unpark();
    let worker_result = worker.join();
    if enabled {
        gpui::set_trace_enabled(false);
    }
    worker_result.map_err(|_| "signal producer panicked")?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sk6_drops_newest_and_preserves_both_channels_on_wrap() {
        let (mut producer, mut consumer) = SampleRing::split(4);
        producer.publish(6, synthesize);
        let mut drained = Vec::new();
        consumer.drain(|pair| drained.push(pair));
        assert_eq!(drained, (0..4).map(synthesize).collect::<Vec<_>>());
        assert_eq!(consumer.0.dropped.load(Ordering::Relaxed), 2);

        drained.clear();
        producer.publish(3, synthesize);
        consumer.drain(|pair| drained.push(pair));
        assert_eq!(drained, (6..9).map(synthesize).collect::<Vec<_>>());
        assert_eq!(consumer.0.dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn sk6_concurrent_publication_preserves_fifo_and_accounts_for_drops() {
        let (mut producer, mut consumer) = SampleRing::split(256);
        let mut last = None;
        let mut consumed = 0;
        let ring = producer.0.clone();
        thread::scope(|scope| {
            let worker = scope.spawn(move || {
                for _ in 0..2_000 {
                    producer.publish(19, synthesize);
                    thread::yield_now();
                }
            });
            let mut drain = || {
                consumer.drain(|[a, b]| {
                    assert_eq!(a.t, b.t);
                    assert!(last.is_none_or(|last| a.t > last));
                    let index = (a.t * SAMPLE_RATE as f64).round() as u64;
                    assert_eq!([a, b], synthesize(index));
                    last = Some(a.t);
                    consumed += 1;
                })
            };
            while !worker.is_finished() {
                drain();
            }
            worker.join().unwrap();
            drain();
        });
        assert!(consumed > 0);
        assert_eq!(consumed + ring.dropped.load(Ordering::Relaxed), 2_000 * 19);
    }

    #[test]
    fn sinv9_warm_ring_and_trace_update_do_not_allocate() {
        let (mut producer, mut consumer) = SampleRing::split(RING_CAPACITY);
        let mut trace = TraceBuffer::new();
        let ((), allocations) = crate::count_allocs(|| {
            producer.publish(3_334, synthesize);
            trace.update(&mut consumer);
        });
        assert_eq!(trace.consumed, 3_334);
        assert_eq!(allocations, 0);
    }
}
