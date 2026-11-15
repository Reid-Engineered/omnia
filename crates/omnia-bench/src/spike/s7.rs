//! Spike S7 (plan section 8; roadmap T0.5): a log of real input.
//!
//! `bench spike s7 [--seconds 60] [--out FILE]` opens a window with instructions, logs every
//! `ScrollEvent`, `PinchEvent`, mouse move, mouse down and mouse up as one JSON line each, plus
//! one line per drawn frame with its dirty-to-present and draw times, then closes by itself
//! and prints the summary. `bench spike s7 --summarise FILE` prints the summary of a log.
//!
//! Every logged event dirties the window, as a canvas would on pan, zoom or hover, so each
//! frame shows what one frame's worth of input looks like. A left-button drag draws a dot at
//! the cursor.
//!
//! Log format (`omnia-bench/s7-log/1`): JSON lines. `t_ns` is nanoseconds since the window
//! opened, read with `Instant::now()` when GPUI dispatches the event (not the OS time stamp).
//! `frame` on an input line is the index of the frame that will show it: the number of
//! renders so far. A `frame` line for frame `k` is written at the start of render `k + 1`,
//! when GPUI has drawn and presented frame `k`; its times are the difference between two
//! `Window::frame_duration_snapshot()` histograms (G13), so `draws` says how many draws the
//! difference covers.

use super::SpikeError;
use crate::harness::watchdog;
use crate::scene::PAPER;
use crate::stats::{self, Summary, round};
use gpui::{
    App, AppContext, Bounds, Context, DispatchPhase, FrameDurationSnapshot, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point, Render,
    ScrollDelta, ScrollEvent, ScrollSource, TouchPhase, Window, WindowBounds, WindowHandle,
    WindowOptions, canvas, div, prelude::*, px, rgb, size,
};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SCHEMA: &str = "omnia-bench/s7-log/1";

/// Window content size, in logical pixels.
const WINDOW: (f32, f32) = (1100.0, 760.0);

/// Scroll and pinch events this close together belong to one gesture switching kind.
const TRANSITION_GAP_NS: u64 = 50_000_000;

/// Same-shape input separated by more than this starts a new burst in the timeline.
const BURST_GAP_NS: u64 = 300_000_000;

pub fn main(args: Vec<String>) -> Result<(), SpikeError> {
    let usage = |message: String| SpikeError::Usage(format!("spike s7: {message}"));
    let mut seconds: Option<f64> = None;
    let mut out: Option<PathBuf> = None;
    let mut summarise: Option<PathBuf> = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| usage(format!("--{name} needs a value")))
        };
        match arg.as_str() {
            "--seconds" if seconds.is_none() => {
                let text = value("seconds")?;
                let parsed: f64 = text
                    .parse()
                    .map_err(|_| usage(format!("--seconds: `{text}` is not a number")))?;
                if !(parsed.is_finite() && parsed > 0.0 && parsed <= 3600.0) {
                    return Err(usage("--seconds must be in (0, 3600]".into()));
                }
                seconds = Some(parsed);
            }
            "--out" if out.is_none() => out = Some(value("out")?.into()),
            "--summarise" if summarise.is_none() => summarise = Some(value("summarise")?.into()),
            other => return Err(usage(format!("unexpected argument `{other}`"))),
        }
    }

    if let Some(path) = summarise {
        if seconds.is_some() || out.is_some() {
            return Err(usage("--summarise takes no other option".into()));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| SpikeError::Usage(format!("{}: {e}", path.display())))?;
        print!("{}", analyse(&parse_log(&text)).render());
        return Ok(());
    }

    let seconds = seconds.unwrap_or(60.0);
    let out = out.unwrap_or_else(|| {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        PathBuf::from(format!("s7-input-{stamp}.jsonl"))
    });
    // A log is a person's time: never overwrite one.
    let file = File::create_new(&out).map_err(|e| {
        usage(format!(
            "{}: {e} (existing logs are not overwritten; pick a new --out)",
            out.display()
        ))
    })?;
    let totals = record(seconds, file, &out).map_err(SpikeError::Run)?;
    eprintln!(
        "bench spike s7: wrote {} input events and {} frames to {}",
        totals.0,
        totals.1,
        out.display()
    );
    let text = std::fs::read_to_string(&out)
        .map_err(|e| SpikeError::Run(format!("{}: {e}", out.display())))?;
    print!("{}", analyse(&parse_log(&text)).render());
    Ok(())
}

// ---- log entries ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

impl Phase {
    fn from_gpui(phase: TouchPhase) -> Self {
        match phase {
            TouchPhase::Started => Self::Started,
            TouchPhase::Moved => Self::Moved,
            TouchPhase::Ended => Self::Ended,
            TouchPhase::Cancelled => Self::Cancelled,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Moved => "moved",
            Self::Ended => "ended",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Started, Self::Moved, Self::Ended, Self::Cancelled]
            .into_iter()
            .find(|p| p.name() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeltaKind {
    Lines,
    Pixels,
}

impl DeltaKind {
    fn name(self) -> &'static str {
        match self {
            Self::Lines => "lines",
            Self::Pixels => "pixels",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Lines, Self::Pixels]
            .into_iter()
            .find(|k| k.name() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Pointer,
    Touch,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Touch => "touch",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        [Self::Pointer, Self::Touch]
            .into_iter()
            .find(|s| s.name() == text)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputKind {
    Scroll {
        delta: DeltaKind,
        dx: f32,
        dy: f32,
        phase: Phase,
        source: Source,
        modifiers: String,
    },
    Pinch {
        delta: f32,
        phase: Phase,
        modifiers: String,
    },
    MouseMove {
        button: Option<String>,
    },
    MouseDown {
        button: String,
        clicks: u64,
    },
    MouseUp {
        button: String,
    },
}

impl InputKind {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Scroll { .. } => "scroll",
            Self::Pinch { .. } => "pinch",
            Self::MouseMove { .. } => "mouse_move",
            Self::MouseDown { .. } => "mouse_down",
            Self::MouseUp { .. } => "mouse_up",
        }
    }
}

/// One input event, in logical window pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub t_ns: u64,
    pub frame: u64,
    pub x: f32,
    pub y: f32,
    pub kind: InputKind,
}

/// The cost of one drawn frame, from the difference of two profiler snapshots.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameTiming {
    pub frame: u64,
    pub t_ns: u64,
    /// Draws recorded between the two snapshots; 1 when the times belong to one frame.
    pub draws: u64,
    /// The largest dirty-to-present sample in the difference; `None` when GPUI recorded none
    /// (an invalid sample, for example while the window was hidden).
    pub dirty_to_present_ms: Option<f64>,
    pub draw_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    Header(Value),
    Input(Input),
    Frame(FrameTiming),
    End(Value),
}

fn coord(value: f32) -> f64 {
    round(f64::from(value))
}

impl Entry {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Header(value) | Self::End(value) => value.clone(),
            Self::Frame(f) => json!({
                "type": "frame",
                "frame": f.frame,
                "t_ns": f.t_ns,
                "draws": f.draws,
                "dirty_to_present_ms": f.dirty_to_present_ms.map(round),
                "draw_ms": f.draw_ms.map(round),
            }),
            Self::Input(input) => {
                let mut o = Map::new();
                o.insert("type".into(), json!(input.kind.type_name()));
                o.insert("t_ns".into(), json!(input.t_ns));
                o.insert("frame".into(), json!(input.frame));
                o.insert("x".into(), json!(coord(input.x)));
                o.insert("y".into(), json!(coord(input.y)));
                match &input.kind {
                    InputKind::Scroll {
                        delta,
                        dx,
                        dy,
                        phase,
                        source,
                        modifiers,
                    } => {
                        o.insert("delta_kind".into(), json!(delta.name()));
                        o.insert("dx".into(), json!(coord(*dx)));
                        o.insert("dy".into(), json!(coord(*dy)));
                        o.insert("phase".into(), json!(phase.name()));
                        o.insert("source".into(), json!(source.name()));
                        o.insert("mods".into(), json!(modifiers));
                    }
                    InputKind::Pinch {
                        delta,
                        phase,
                        modifiers,
                    } => {
                        // Pinch deltas are small: keep 8 decimals.
                        o.insert(
                            "delta".into(),
                            json!((f64::from(*delta) * 1e8).round() / 1e8),
                        );
                        o.insert("phase".into(), json!(phase.name()));
                        o.insert("mods".into(), json!(modifiers));
                    }
                    InputKind::MouseMove { button } => {
                        o.insert("button".into(), json!(button));
                    }
                    InputKind::MouseDown { button, clicks } => {
                        o.insert("button".into(), json!(button));
                        o.insert("clicks".into(), json!(clicks));
                    }
                    InputKind::MouseUp { button } => {
                        o.insert("button".into(), json!(button));
                    }
                }
                Value::Object(o)
            }
        }
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        let num = |k: &str| value.get(k).and_then(Value::as_f64);
        let uint = |k: &str| value.get(k).and_then(Value::as_u64);
        let text = |k: &str| value.get(k).and_then(Value::as_str).map(str::to_string);
        let kind = match value.get("type")?.as_str()? {
            "header" => return Some(Self::Header(value.clone())),
            "end" => return Some(Self::End(value.clone())),
            "frame" => {
                return Some(Self::Frame(FrameTiming {
                    frame: uint("frame")?,
                    t_ns: uint("t_ns")?,
                    draws: uint("draws")?,
                    dirty_to_present_ms: num("dirty_to_present_ms"),
                    draw_ms: num("draw_ms"),
                }));
            }
            "scroll" => InputKind::Scroll {
                delta: DeltaKind::parse(&text("delta_kind")?)?,
                dx: num("dx")? as f32,
                dy: num("dy")? as f32,
                phase: Phase::parse(&text("phase")?)?,
                source: Source::parse(&text("source")?)?,
                modifiers: text("mods").unwrap_or_default(),
            },
            "pinch" => InputKind::Pinch {
                delta: num("delta")? as f32,
                phase: Phase::parse(&text("phase")?)?,
                modifiers: text("mods").unwrap_or_default(),
            },
            "mouse_move" => InputKind::MouseMove {
                button: text("button"),
            },
            "mouse_down" => InputKind::MouseDown {
                button: text("button")?,
                clicks: uint("clicks")?,
            },
            "mouse_up" => InputKind::MouseUp {
                button: text("button")?,
            },
            _ => return None,
        };
        Some(Self::Input(Input {
            t_ns: uint("t_ns")?,
            frame: uint("frame")?,
            x: num("x")? as f32,
            y: num("y")? as f32,
            kind,
        }))
    }
}

/// A parsed log. Lines that are not a known entry (for example a line cut short when the
/// process was killed) are counted, not fatal.
#[derive(Debug, Default)]
pub struct Log {
    pub entries: Vec<Entry>,
    pub unreadable: usize,
}

pub fn parse_log(text: &str) -> Log {
    let mut log = Log::default();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match serde_json::from_str::<Value>(line)
            .ok()
            .as_ref()
            .and_then(Entry::from_json)
        {
            Some(entry) => log.entries.push(entry),
            None => log.unreadable += 1,
        }
    }
    log
}

// ---- recording ------------------------------------------------------------------------------

fn modifiers_text(m: &Modifiers) -> String {
    let names = [
        (m.control, "ctrl"),
        (m.alt, "alt"),
        (m.shift, "shift"),
        (m.platform, "win"),
        (m.function, "fn"),
    ];
    let held: Vec<&str> = names
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, n)| *n)
        .collect();
    held.join("+")
}

fn button_name(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "left",
        MouseButton::Right => "right",
        MouseButton::Middle => "middle",
        MouseButton::Navigate(gpui::NavigationDirection::Back) => "back",
        MouseButton::Navigate(gpui::NavigationDirection::Forward) => "forward",
    }
    .to_string()
}

fn xy(p: Point<Pixels>) -> (f32, f32) {
    (f32::from(p.x), f32::from(p.y))
}

/// The log file, shared by the view and the task that ends the run, so the end line is
/// written even if the window was closed early.
struct Logger {
    out: BufWriter<File>,
    started: Instant,
    inputs: u64,
    frames: u64,
    error: Option<String>,
}

impl Logger {
    fn t_ns(&self) -> u64 {
        self.started.elapsed().as_nanos() as u64
    }

    fn write(&mut self, entry: &Entry) {
        if self.error.is_some() {
            return;
        }
        match entry {
            Entry::Input(_) => self.inputs += 1,
            Entry::Frame(_) => self.frames += 1,
            _ => {}
        }
        let result = serde_json::to_writer(&mut self.out, &entry.to_json())
            .map_err(|e| e.to_string())
            .and_then(|()| self.out.write_all(b"\n").map_err(|e| e.to_string()));
        if let Err(error) = result {
            self.error = Some(error);
        }
    }

    fn flush(&mut self) {
        if self.error.is_none()
            && let Err(error) = self.out.flush()
        {
            self.error = Some(error.to_string());
        }
    }
}

#[derive(Default)]
struct Counts {
    lines: u64,
    pixels: u64,
    touch: u64,
    pinch: u64,
    moves: u64,
    drag_moves: u64,
    buttons: u64,
}

struct S7View {
    logger: Rc<RefCell<Logger>>,
    out: PathBuf,
    deadline: Instant,
    /// Renders so far; the index of the next frame.
    frame: u64,
    previous: Option<FrameDurationSnapshot>,
    counts: Counts,
    last: String,
    /// The cursor while the left button is held.
    drag: Option<Point<Pixels>>,
}

impl S7View {
    fn log(&mut self, position: Point<Pixels>, kind: InputKind, cx: &mut Context<Self>) {
        let (x, y) = xy(position);
        let mut logger = self.logger.borrow_mut();
        let entry = Entry::Input(Input {
            t_ns: logger.t_ns(),
            frame: self.frame,
            x,
            y,
            kind,
        });
        logger.write(&entry);
        drop(logger);
        if let Entry::Input(input) = &entry {
            self.last = describe(input);
        }
        cx.notify();
    }

    fn scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let (delta, dx, dy) = match event.delta {
            ScrollDelta::Lines(d) => (DeltaKind::Lines, d.x, d.y),
            ScrollDelta::Pixels(d) => (DeltaKind::Pixels, f32::from(d.x), f32::from(d.y)),
        };
        let source = match event.source {
            ScrollSource::Pointer => Source::Pointer,
            ScrollSource::Touch => Source::Touch,
        };
        match (source, delta) {
            (Source::Touch, _) => self.counts.touch += 1,
            (_, DeltaKind::Lines) => self.counts.lines += 1,
            (_, DeltaKind::Pixels) => self.counts.pixels += 1,
        }
        let kind = InputKind::Scroll {
            delta,
            dx,
            dy,
            phase: Phase::from_gpui(event.touch_phase),
            source,
            modifiers: modifiers_text(&event.modifiers),
        };
        self.log(event.position, kind, cx);
    }

    fn pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        self.counts.pinch += 1;
        let kind = InputKind::Pinch {
            delta: event.delta,
            phase: Phase::from_gpui(event.phase),
            modifiers: modifiers_text(&event.modifiers),
        };
        self.log(event.position, kind, cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        self.counts.moves += 1;
        if event.dragging() {
            self.counts.drag_moves += 1;
            self.drag = Some(event.position);
        }
        let kind = InputKind::MouseMove {
            button: event.pressed_button.map(button_name),
        };
        self.log(event.position, kind, cx);
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        self.counts.buttons += 1;
        if event.button == MouseButton::Left {
            self.drag = Some(event.position);
        }
        let kind = InputKind::MouseDown {
            button: button_name(event.button),
            clicks: event.click_count as u64,
        };
        self.log(event.position, kind, cx);
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        self.counts.buttons += 1;
        if event.button == MouseButton::Left {
            self.drag = None;
        }
        let kind = InputKind::MouseUp {
            button: button_name(event.button),
        };
        self.log(event.position, kind, cx);
    }

    /// Writes the timing of the frame drawn before this render.
    fn record_previous_frame(&mut self, window: &Window) {
        let now = window.frame_duration_snapshot();
        if let Some(before) = self.previous.as_ref()
            && let Some(frame) = self.frame.checked_sub(1)
        {
            let draw = stats::delta(
                &now.draw_duration_histogram,
                &before.draw_duration_histogram,
            );
            let d2p = stats::delta(
                &now.dirty_to_present_histogram,
                &before.dirty_to_present_histogram,
            );
            let largest_ms = |h: &Option<hdrhistogram::Histogram<u64>>| {
                h.as_ref()
                    .filter(|h| !h.is_empty())
                    .map(|h| h.max() as f64 / 1e6)
            };
            let mut logger = self.logger.borrow_mut();
            let entry = Entry::Frame(FrameTiming {
                frame,
                t_ns: logger.t_ns(),
                draws: draw.as_ref().map_or(0, |h| h.len()),
                dirty_to_present_ms: largest_ms(&d2p),
                draw_ms: largest_ms(&draw),
            });
            logger.write(&entry);
        }
        self.previous = Some(now);
    }

    fn header(&mut self, window: &Window, cx: &App, seconds: f64) {
        let viewport = window.viewport_size();
        let header = json!({
            "type": "header",
            "schema": SCHEMA,
            "seconds": seconds,
            "unix_ms": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            "environment": {
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "scale_factor": window.scale_factor(),
                "viewport_logical": [f32::from(viewport.width), f32::from(viewport.height)],
                "refresh_hz": window
                    .refresh_interval(cx)
                    .map(|interval| round(1.0 / interval.as_secs_f64())),
            },
        });
        self.logger.borrow_mut().write(&Entry::Header(header));
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        self.logger.borrow_mut().flush();
        cx.notify();
    }

    /// GPUI's own input counters, for comparison with the log.
    fn gpui_input_stats(&self, window: &Window) -> Value {
        let snapshot = window.input_latency_snapshot();
        let per_frame = &snapshot.events_per_frame_histogram;
        let counts = if per_frame.is_empty() {
            Value::Null
        } else {
            json!({
                "frames": per_frame.len(),
                "p50": per_frame.value_at_quantile(0.5),
                "p99": per_frame.value_at_quantile(0.99),
                "max": per_frame.max(),
            })
        };
        json!({
            "events_per_frame": counts,
            "input_to_present_ms": Summary::from_nanos(&snapshot.latency_histogram)
                .map_or(Value::Null, |s| s.to_json()),
            "mid_draw_events_dropped": snapshot.mid_draw_events_dropped,
        })
    }
}

fn describe(input: &Input) -> String {
    match &input.kind {
        InputKind::Scroll {
            delta,
            dx,
            dy,
            phase,
            source,
            ..
        } => format!(
            "scroll {} ({dx:.2}, {dy:.2}) {} {}",
            delta.name(),
            phase.name(),
            source.name()
        ),
        InputKind::Pinch { delta, phase, .. } => {
            format!("pinch {delta:+.4} {}", phase.name())
        }
        InputKind::MouseMove { button } => format!(
            "move ({:.0}, {:.0}){}",
            input.x,
            input.y,
            button
                .as_ref()
                .map_or(String::new(), |b| format!(" with {b}"))
        ),
        InputKind::MouseDown { button, .. } => format!("{button} down"),
        InputKind::MouseUp { button } => format!("{button} up"),
    }
}

const INSTRUCTIONS: [&str; 6] = [
    "1. Mouse wheel: a few notches up, then a few down.",
    "2. Touchpad: two-finger pan up, down, left and right.",
    "3. Touchpad: pinch in, then pinch out.",
    "4. Touchpad: two-finger pan, then pinch, without lifting your fingers.",
    "5. Mouse: hold the left button and drag fast around the window for a few seconds.",
    "Pause a second between steps. Keep the pointer over this window.",
];

impl Render for S7View {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.record_previous_frame(window);
        self.frame += 1;

        let left = self.deadline.saturating_duration_since(Instant::now());
        let c = &self.counts;
        let status = format!(
            "Closes by itself in {} s.  Logged: wheel/Lines {}, Pixels scroll {}, touch scroll {}, \
             pinch {}, moves {} (drag {}), buttons {}.",
            left.as_secs_f64().ceil() as u64,
            c.lines,
            c.pixels,
            c.touch,
            c.pinch,
            c.moves,
            c.drag_moves,
            c.buttons
        );
        let error = self.logger.borrow().error.clone();

        let view = cx.weak_entity();
        // Window-level listeners in the capture phase see every event, wherever it lands.
        let listeners = canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                macro_rules! listen {
                    ($event:ty, $method:ident) => {{
                        let view = view.clone();
                        window.on_mouse_event(move |event: &$event, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                view.update(cx, |view, cx| view.$method(event, cx)).ok();
                            }
                        });
                    }};
                }
                listen!(ScrollEvent, scroll);
                listen!(PinchEvent, pinch);
                listen!(MouseMoveEvent, mouse_move);
                listen!(MouseDownEvent, mouse_down);
                listen!(MouseUpEvent, mouse_up);
            },
        )
        .absolute()
        .size_full();

        let text = rgb(0x24292f);
        let mut root = div()
            .size_full()
            .bg(rgb(PAPER))
            .text_color(text)
            .p(px(32.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(listeners)
            .child(
                div()
                    .text_size(px(22.0))
                    .child("Spike S7: input log. Do these over this window:"),
            )
            .children(
                INSTRUCTIONS
                    .iter()
                    .map(|line| div().text_size(px(18.0)).child(*line)),
            )
            .child(div().pt(px(16.0)).text_size(px(15.0)).child(status))
            .child(
                div()
                    .text_size(px(15.0))
                    .child(format!("Last event: {}", self.last)),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .child(format!("Log: {}", self.out.display())),
            );
        if let Some(error) = error {
            root = root.child(
                div()
                    .text_size(px(15.0))
                    .text_color(rgb(0xb42318))
                    .child(format!("Log write failed: {error}")),
            );
        }
        if let Some(at) = self.drag {
            const DOT: f32 = 14.0;
            root = root.child(
                div()
                    .absolute()
                    .left(at.x - px(DOT / 2.0))
                    .top(at.y - px(DOT / 2.0))
                    .size(px(DOT))
                    .rounded_full()
                    .bg(rgb(0x2f6fde)),
            );
        }
        root
    }
}

/// Opens the window, logs to `file` (named `out` in the window) for `seconds` and returns the
/// number of input events and frames written.
pub fn record(seconds: f64, file: File, out: &Path) -> Result<(u64, u64), String> {
    let run_for = Duration::from_secs_f64(seconds);
    watchdog("spike s7", run_for + Duration::from_secs(30));

    let logger = Rc::new(RefCell::new(Logger {
        out: BufWriter::new(file),
        started: Instant::now(),
        inputs: 0,
        frames: 0,
        error: None,
    }));
    let outcome: Rc<RefCell<Option<Result<(), String>>>> = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    let task_logger = logger.clone();
    let totals = logger.clone();
    let out = out.to_path_buf();
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WINDOW.0), px(WINDOW.1)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            // As in `bench run`: frame pacing must not depend on focus or the power plan.
            inactive_frame_interval: None,
            low_power_frame_interval: None,
            thermal_frame_interval: None,
            ..Default::default()
        };
        let deadline = Instant::now() + run_for;
        let view_logger = logger.clone();
        let opened: Result<WindowHandle<S7View>, _> = cx.open_window(options, move |_, cx| {
            cx.new(|_| S7View {
                logger: view_logger,
                out,
                deadline,
                frame: 0,
                previous: None,
                counts: Counts::default(),
                last: "none yet".into(),
                drag: None,
            })
        });
        let handle = match opened {
            Ok(handle) => handle,
            Err(error) => {
                *slot.borrow_mut() = Some(Err(format!("open window: {error}")));
                cx.quit();
                return;
            }
        };
        cx.activate(true);
        let slot = slot.clone();
        cx.spawn(async move |cx| {
            handle
                .update(cx, |view, window, cx| view.header(window, cx, seconds))
                .ok();
            let mut closed = false;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                // Wake on whole seconds left so the countdown reads true.
                let tick = Duration::from_nanos((left.as_nanos() % 1_000_000_000) as u64);
                let wait = if tick.is_zero() {
                    Duration::from_secs(1)
                } else {
                    tick
                };
                cx.background_executor().timer(wait.min(left)).await;
                if handle.update(cx, |view, _, cx| view.tick(cx)).is_err() {
                    closed = true;
                    break;
                }
            }
            let gpui = handle
                .update(cx, |view, window, _| view.gpui_input_stats(window))
                .unwrap_or(Value::Null);
            let mut logger = task_logger.borrow_mut();
            let end = json!({
                "type": "end",
                "t_ns": logger.t_ns(),
                "closed_early": closed,
                "inputs": logger.inputs,
                "frames": logger.frames,
                "gpui": gpui,
            });
            logger.write(&Entry::End(end));
            logger.flush();
            let result = match (&logger.error, closed) {
                (Some(error), _) => Err(format!("writing the log: {error}")),
                (None, true) => Err("the window was closed before the time was up".into()),
                (None, false) => Ok(()),
            };
            drop(logger);
            *slot.borrow_mut() = Some(result);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });

    let result = outcome
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err("the app quit without a result".into()));
    let mut totals = totals.borrow_mut();
    totals.flush();
    result.map(|()| (totals.inputs, totals.frames))
}

// ---- summary --------------------------------------------------------------------------------

/// The shape of an input event. The device is inferred from GPUI's Windows code (G10):
/// `WM_MOUSEWHEEL` gives `Lines`; DirectManipulation (precision touchpads) gives `Pixels`
/// scrolls and pinches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    ScrollLines,
    ScrollPixels,
    TouchScroll,
    Pinch,
    Drag,
    Hover,
    Button,
}

impl Class {
    pub const ALL: [Class; 7] = [
        Class::ScrollLines,
        Class::ScrollPixels,
        Class::TouchScroll,
        Class::Pinch,
        Class::Drag,
        Class::Hover,
        Class::Button,
    ];

    pub fn of(kind: &InputKind) -> Self {
        match kind {
            InputKind::Scroll {
                source: Source::Touch,
                ..
            } => Self::TouchScroll,
            InputKind::Scroll {
                delta: DeltaKind::Lines,
                ..
            } => Self::ScrollLines,
            InputKind::Scroll { .. } => Self::ScrollPixels,
            InputKind::Pinch { .. } => Self::Pinch,
            InputKind::MouseMove { button } if button.as_deref() == Some("left") => Self::Drag,
            InputKind::MouseMove { .. } => Self::Hover,
            InputKind::MouseDown { .. } | InputKind::MouseUp { .. } => Self::Button,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ScrollLines => "scroll, Lines delta",
            Self::ScrollPixels => "scroll, Pixels delta",
            Self::TouchScroll => "scroll, touch source",
            Self::Pinch => "pinch",
            Self::Drag => "mouse move, left held",
            Self::Hover => "mouse move, no left",
            Self::Button => "mouse down/up",
        }
    }

    pub fn device(self) -> &'static str {
        match self {
            Self::ScrollLines => "mouse wheel (WM_MOUSEWHEEL; also a non-precision touchpad)",
            Self::ScrollPixels | Self::Pinch => "precision touchpad (DirectManipulation)",
            Self::TouchScroll => "touchscreen (GPUI touch pan)",
            Self::Drag | Self::Hover | Self::Button => "mouse",
        }
    }
}

/// What one input shape looked like over the whole log.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassRow {
    pub class: Class,
    pub events: usize,
    pub phases: Vec<(Phase, usize)>,
    pub first_s: f64,
    pub last_s: f64,
    /// Distinct non-zero `Lines` steps, for example `[-3.0, 3.0]`.
    pub line_steps: Vec<f64>,
    /// Size of each event's delta: pixels for `Pixels` scrolls, `|delta|` for pinches.
    pub magnitude: Option<Summary>,
}

/// A run of same-shape input with no gap over [`BURST_GAP_NS`], for lining the log up with
/// what the person did.
#[derive(Clone, Debug, PartialEq)]
pub struct Burst {
    pub class: Class,
    pub start_s: f64,
    pub end_s: f64,
    pub events: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PinchGesture {
    pub start_s: f64,
    pub end_s: f64,
    pub phases: Vec<Phase>,
    /// Product of `1 + delta` over the gesture.
    pub scale: f64,
    /// Began within [`TRANSITION_GAP_NS`] of a scroll gesture's `Ended`.
    pub after_scroll: bool,
}

impl PinchGesture {
    fn new(t_ns: u64, after_scroll: bool) -> Self {
        Self {
            start_s: secs(t_ns),
            end_s: secs(t_ns),
            phases: Vec::new(),
            scale: 1.0,
            after_scroll,
        }
    }

    /// The phase sequence with runs collapsed, for example `started, moved x41, ended`.
    pub fn sequence(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.phases.len() {
            let phase = self.phases[i];
            let run = self.phases[i..].iter().take_while(|p| **p == phase).count();
            parts.push(if run == 1 {
                phase.name().to_string()
            } else {
                format!("{} x{run}", phase.name())
            });
            i += run;
        }
        parts.join(", ")
    }
}

/// A touchpad gesture that switched from scroll to pinch (DirectManipulation ends the scroll
/// and starts a pinch in the same update).
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub at_s: f64,
    pub gap_ms: f64,
    /// Scroll events in the gesture before the switch, `Ended` included.
    pub scroll_events: usize,
    /// Total pan in logical pixels before the switch.
    pub pan_px: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameStats {
    /// Frames that showed at least one such event.
    pub frames: usize,
    /// Of those, frames with a dirty-to-present sample.
    pub timed: usize,
    /// Of the timed frames, those whose snapshot difference covered more than one draw.
    pub multi_draw: usize,
    pub dirty_to_present: Option<Summary>,
    pub draw: Option<Summary>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub header: Option<Value>,
    pub end: Option<Value>,
    pub unreadable: usize,
    pub inputs: usize,
    pub frames_logged: usize,
    pub rows: Vec<ClassRow>,
    pub bursts: Vec<Burst>,
    /// Events per frame over frames with at least one such event: all input, then by class.
    pub per_frame: Vec<(String, Summary)>,
    pub pinches: Vec<PinchGesture>,
    pub scroll_to_pinch: Vec<Transition>,
    pub pinch_to_scroll: usize,
    pub drag: FrameStats,
    pub any_input: FrameStats,
}

fn secs(t_ns: u64) -> f64 {
    t_ns as f64 / 1e9
}

fn frame_stats(frames: &BTreeSet<u64>, timings: &BTreeMap<u64, &FrameTiming>) -> FrameStats {
    let mut d2p = Vec::new();
    let mut draw = Vec::new();
    let mut multi_draw = 0;
    for timing in frames.iter().filter_map(|f| timings.get(f)) {
        if let Some(ms) = timing.dirty_to_present_ms {
            d2p.push(ms);
            if timing.draws > 1 {
                multi_draw += 1;
            }
        }
        if let Some(ms) = timing.draw_ms {
            draw.push(ms);
        }
    }
    FrameStats {
        frames: frames.len(),
        timed: d2p.len(),
        multi_draw,
        dirty_to_present: Summary::from_samples(&mut d2p),
        draw: Summary::from_samples(&mut draw),
    }
}

fn class_row(class: Class, events: &[&Input]) -> Option<ClassRow> {
    let (first, last) = (events.first()?, events.last()?);
    let mut phases: BTreeMap<Phase, usize> = BTreeMap::new();
    let mut steps: Vec<f64> = Vec::new();
    let mut sizes: Vec<f64> = Vec::new();
    for input in events {
        match &input.kind {
            InputKind::Scroll {
                delta,
                dx,
                dy,
                phase,
                ..
            } => {
                *phases.entry(*phase).or_default() += 1;
                match delta {
                    DeltaKind::Lines => {
                        for step in [*dx, *dy].into_iter().filter(|s| *s != 0.0) {
                            steps.push((f64::from(step) * 100.0).round() / 100.0);
                        }
                    }
                    DeltaKind::Pixels => sizes.push(f64::from(dx.hypot(*dy))),
                }
            }
            InputKind::Pinch { delta, phase, .. } => {
                *phases.entry(*phase).or_default() += 1;
                sizes.push(f64::from(delta.abs()));
            }
            _ => {}
        }
    }
    steps.sort_by(f64::total_cmp);
    steps.dedup();
    Some(ClassRow {
        class,
        events: events.len(),
        phases: phases.into_iter().collect(),
        first_s: secs(first.t_ns),
        last_s: secs(last.t_ns),
        line_steps: steps,
        magnitude: Summary::from_samples(&mut sizes),
    })
}

/// Timeline of deliberate input; hover moves and button presses are left out.
fn bursts(inputs: &[&Input]) -> Vec<Burst> {
    let mut bursts: Vec<Burst> = Vec::new();
    let mut last_t = 0;
    for input in inputs {
        let class = Class::of(&input.kind);
        if matches!(class, Class::Hover | Class::Button) {
            continue;
        }
        match bursts.last_mut() {
            Some(b) if b.class == class && input.t_ns - last_t <= BURST_GAP_NS => {
                b.end_s = secs(input.t_ns);
                b.events += 1;
            }
            _ => bursts.push(Burst {
                class,
                start_s: secs(input.t_ns),
                end_s: secs(input.t_ns),
                events: 1,
            }),
        }
        last_t = input.t_ns;
    }
    bursts
}

fn events_per_frame(inputs: &[&Input]) -> Vec<(String, Summary)> {
    let mut by_frame: BTreeMap<u64, BTreeMap<Class, usize>> = BTreeMap::new();
    for input in inputs {
        *by_frame
            .entry(input.frame)
            .or_default()
            .entry(Class::of(&input.kind))
            .or_default() += 1;
    }
    let mut per_frame = Vec::new();
    let mut all: Vec<f64> = by_frame
        .values()
        .map(|c| c.values().sum::<usize>() as f64)
        .collect();
    if let Some(s) = Summary::from_samples(&mut all) {
        per_frame.push(("all input".to_string(), s));
    }
    for class in Class::ALL {
        let mut counts: Vec<f64> = by_frame
            .values()
            .filter_map(|c| c.get(&class).map(|n| *n as f64))
            .collect();
        if let Some(s) = Summary::from_samples(&mut counts) {
            per_frame.push((class.label().to_string(), s));
        }
    }
    per_frame
}

/// Touchpad gestures: pinch phase sequences and scroll <-> pinch switches. Only pointer
/// `Pixels` scrolls (DirectManipulation) and pinches take part.
fn gestures(inputs: &[&Input]) -> (Vec<PinchGesture>, Vec<Transition>, usize) {
    let mut pinches: Vec<PinchGesture> = Vec::new();
    let mut open: Option<PinchGesture> = None;
    let mut scroll_to_pinch = Vec::new();
    let mut pinch_to_scroll = 0;
    // The previous gesture event: (time, phase, was it a pinch).
    let mut previous: Option<(u64, Phase, bool)> = None;
    // Scroll events and pan distance since the last scroll `Started`.
    let mut scroll_run = (0usize, 0.0f64);
    for input in inputs {
        let (phase, is_pinch) = match &input.kind {
            InputKind::Scroll {
                delta: DeltaKind::Pixels,
                source: Source::Pointer,
                phase,
                dx,
                dy,
                ..
            } => {
                if *phase == Phase::Started {
                    scroll_run = (0, 0.0);
                    if let Some((t, Phase::Ended | Phase::Cancelled, true)) = previous
                        && input.t_ns - t <= TRANSITION_GAP_NS
                    {
                        pinch_to_scroll += 1;
                    }
                }
                scroll_run.0 += 1;
                scroll_run.1 += f64::from(dx.hypot(*dy));
                (*phase, false)
            }
            InputKind::Pinch { phase, delta, .. } => {
                if *phase == Phase::Started {
                    pinches.extend(open.take());
                    let mut after_scroll = false;
                    if let Some((t, Phase::Ended, false)) = previous
                        && input.t_ns - t <= TRANSITION_GAP_NS
                    {
                        after_scroll = true;
                        scroll_to_pinch.push(Transition {
                            at_s: secs(input.t_ns),
                            gap_ms: (input.t_ns - t) as f64 / 1e6,
                            scroll_events: scroll_run.0,
                            pan_px: scroll_run.1,
                        });
                    }
                    open = Some(PinchGesture::new(input.t_ns, after_scroll));
                }
                let gesture = open.get_or_insert_with(|| PinchGesture::new(input.t_ns, false));
                gesture.phases.push(*phase);
                gesture.end_s = secs(input.t_ns);
                gesture.scale *= 1.0 + f64::from(*delta);
                if matches!(phase, Phase::Ended | Phase::Cancelled) {
                    pinches.extend(open.take());
                }
                (*phase, true)
            }
            _ => continue,
        };
        previous = Some((input.t_ns, phase, is_pinch));
    }
    pinches.extend(open);
    (pinches, scroll_to_pinch, pinch_to_scroll)
}

pub fn analyse(log: &Log) -> Analysis {
    let mut header = None;
    let mut end = None;
    let mut inputs: Vec<&Input> = Vec::new();
    let mut timings: BTreeMap<u64, &FrameTiming> = BTreeMap::new();
    for entry in &log.entries {
        match entry {
            Entry::Header(v) => header = Some(v.clone()),
            Entry::End(v) => end = Some(v.clone()),
            Entry::Input(input) => inputs.push(input),
            Entry::Frame(timing) => {
                timings.insert(timing.frame, timing);
            }
        }
    }
    // Stable: events with the same time stamp keep their log order.
    inputs.sort_by_key(|i| i.t_ns);

    let rows = Class::ALL
        .into_iter()
        .filter_map(|class| {
            let events: Vec<&Input> = inputs
                .iter()
                .copied()
                .filter(|i| Class::of(&i.kind) == class)
                .collect();
            class_row(class, &events)
        })
        .collect();
    let (pinches, scroll_to_pinch, pinch_to_scroll) = gestures(&inputs);
    let drag_frames: BTreeSet<u64> = inputs
        .iter()
        .filter(|i| Class::of(&i.kind) == Class::Drag)
        .map(|i| i.frame)
        .collect();
    let input_frames: BTreeSet<u64> = inputs.iter().map(|i| i.frame).collect();

    Analysis {
        header,
        end,
        unreadable: log.unreadable,
        inputs: inputs.len(),
        frames_logged: timings.len(),
        rows,
        bursts: bursts(&inputs),
        per_frame: events_per_frame(&inputs),
        pinches,
        scroll_to_pinch,
        pinch_to_scroll,
        drag: frame_stats(&drag_frames, &timings),
        any_input: frame_stats(&input_frames, &timings),
    }
}

fn percentiles(s: &Summary) -> String {
    format!(
        "p50 {}  p95 {}  p99 {}  max {}  (n {})",
        round(s.p50),
        round(s.p95),
        round(s.p99),
        round(s.max),
        s.count
    )
}

impl Analysis {
    pub fn render(&self) -> String {
        let mut o = String::new();
        // Writing to a String cannot fail.
        macro_rules! line {
            ($($arg:tt)*) => { let _ = writeln!(o, $($arg)*); };
        }
        const SHOWN: usize = 60;

        let env = self.header.as_ref().and_then(|h| h.get("environment"));
        let field = |key: &str| {
            env.and_then(|e| e.get(key)).map_or("?".to_string(), |v| {
                v.as_str().map_or_else(|| v.to_string(), str::to_string)
            })
        };
        line!("S7 input log summary ({SCHEMA})");
        match &self.header {
            Some(h) => {
                line!(
                    "  run: {} s requested; scale factor {}; refresh {} Hz; {}/{}",
                    h.get("seconds").map_or("?".into(), Value::to_string),
                    field("scale_factor"),
                    field("refresh_hz"),
                    field("os"),
                    field("arch"),
                );
            }
            None => {
                line!("  run: no header line in the log");
            }
        }
        let ending = match &self.end {
            Some(end) if end.get("closed_early") == Some(&Value::Bool(true)) => {
                "; window closed early"
            }
            Some(_) => "",
            None => "; no end line (run cut short?)",
        };
        line!(
            "  {} input events, {} frame lines, {} unreadable lines{ending}",
            self.inputs,
            self.frames_logged,
            self.unreadable
        );

        line!("\nDelta kind by input shape (device inferred from GPUI's Windows code, G10):");
        if self.rows.is_empty() {
            line!("  no input events");
        }
        for row in &self.rows {
            line!(
                "  {:<22} {:>6} events  {:>7.2}-{:<7.2} s  -> {}",
                row.class.label(),
                row.events,
                row.first_s,
                row.last_s,
                row.class.device()
            );
            if !row.phases.is_empty() {
                let phases: Vec<String> = row
                    .phases
                    .iter()
                    .map(|(p, n)| format!("{} x{n}", p.name()))
                    .collect();
                line!("      phases: {}", phases.join(", "));
            }
            if !row.line_steps.is_empty() {
                let steps: Vec<String> = row.line_steps.iter().map(f64::to_string).collect();
                line!("      line steps seen: {}", steps.join(", "));
            }
            if let Some(m) = &row.magnitude {
                let unit = if row.class == Class::Pinch {
                    "|delta|"
                } else {
                    "px"
                };
                line!("      delta size ({unit}): {}", percentiles(m));
            }
        }

        line!(
            "\nTimeline (same-shape input split at gaps over {} ms; hover and clicks left out):",
            BURST_GAP_NS / 1_000_000
        );
        if self.bursts.is_empty() {
            line!("  none");
        }
        for b in self.bursts.iter().take(SHOWN) {
            line!(
                "  {:>7.2}-{:<7.2} s  {:<22} {:>5} events",
                b.start_s,
                b.end_s,
                b.class.label(),
                b.events
            );
        }
        if self.bursts.len() > SHOWN {
            line!("  ... {} more", self.bursts.len() - SHOWN);
        }

        line!("\nEvents per frame (over frames that showed at least one such event):");
        if self.per_frame.is_empty() {
            line!("  no input events");
        }
        for (name, s) in &self.per_frame {
            line!("  {name:<22} {}", percentiles(s));
        }
        if let Some(gpui) = self.end.as_ref().and_then(|e| e.get("gpui")) {
            line!("  GPUI's own input counters: {gpui}");
        }

        line!("\nPinch gestures: {}", self.pinches.len());
        for (i, p) in self.pinches.iter().enumerate().take(SHOWN) {
            line!(
                "  #{:<3} {:>7.2}-{:<7.2} s  scale x{:.3}{}  {}",
                i + 1,
                p.start_s,
                p.end_s,
                p.scale,
                if p.after_scroll { ", after a pan" } else { "" },
                p.sequence()
            );
        }
        line!(
            "Scroll -> pinch switches in one gesture: {}",
            self.scroll_to_pinch.len()
        );
        for t in &self.scroll_to_pinch {
            line!(
                "  at {:.2} s after {} scroll events ({:.1} px of pan), gap {:.2} ms",
                t.at_s,
                t.scroll_events,
                t.pan_px,
                t.gap_ms
            );
        }
        line!(
            "Pinch -> scroll within {} ms (GPUI does not emit this inside one gesture): {}",
            TRANSITION_GAP_NS / 1_000_000,
            self.pinch_to_scroll
        );

        line!("\nDirty-to-present, ms:");
        for (name, stats) in [
            ("during the drag", &self.drag),
            ("all frames with input", &self.any_input),
        ] {
            match &stats.dirty_to_present {
                Some(s) => {
                    line!("  {name:<22} {}", percentiles(s));
                }
                None => {
                    line!("  {name:<22} no timed frames");
                }
            }
            if stats.frames > 0 {
                line!(
                    "      {} frames, {} timed, {} timed over more than one draw; draw ms {}",
                    stats.frames,
                    stats.timed,
                    stats.multi_draw,
                    stats.draw.as_ref().map_or("none".into(), percentiles)
                );
            }
        }
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    fn input(t_ms: u64, frame: u64, kind: InputKind) -> Entry {
        Entry::Input(Input {
            t_ns: t_ms * MS,
            frame,
            x: 100.0,
            y: 200.25,
            kind,
        })
    }

    fn wheel(dy: f32) -> InputKind {
        InputKind::Scroll {
            delta: DeltaKind::Lines,
            dx: 0.0,
            dy,
            phase: Phase::Moved,
            source: Source::Pointer,
            modifiers: String::new(),
        }
    }

    fn pan(dy: f32, phase: Phase) -> InputKind {
        InputKind::Scroll {
            delta: DeltaKind::Pixels,
            dx: 0.0,
            dy,
            phase,
            source: Source::Pointer,
            modifiers: String::new(),
        }
    }

    fn pinch(delta: f32, phase: Phase) -> InputKind {
        InputKind::Pinch {
            delta,
            phase,
            modifiers: "ctrl".into(),
        }
    }

    fn drag() -> InputKind {
        InputKind::MouseMove {
            button: Some("left".into()),
        }
    }

    fn frame(frame: u64, d2p: f64) -> Entry {
        Entry::Frame(FrameTiming {
            frame,
            t_ns: frame * 16 * MS,
            draws: 1,
            dirty_to_present_ms: Some(d2p),
            draw_ms: Some(d2p / 2.0),
        })
    }

    fn lines(entries: &[Entry]) -> String {
        entries
            .iter()
            .map(|e| format!("{}\n", e.to_json()))
            .collect()
    }

    /// A synthetic run: wheel, a pan that turns into a pinch, a plain pinch, a drag, a hover.
    fn synthetic() -> String {
        use Phase::*;
        let mut entries = vec![Entry::Header(json!({
            "type": "header",
            "schema": SCHEMA,
            "seconds": 60.0,
            "environment": {
                "scale_factor": 1.25, "refresh_hz": 60.0, "os": "windows", "arch": "x86_64"
            },
        }))];
        // Wheel: two notches in frame 1, one in frame 2.
        entries.push(input(1000, 1, wheel(3.0)));
        entries.push(input(1001, 1, wheel(3.0)));
        entries.push(input(1100, 2, wheel(-3.0)));
        // Pan then pinch in one gesture: scroll Ended and pinch Started in the same update.
        entries.push(input(3000, 10, pan(0.0, Started)));
        entries.push(input(3016, 11, pan(4.0, Moved)));
        entries.push(input(3032, 12, pan(0.0, Ended)));
        entries.push(input(3032, 12, pinch(0.0, Started)));
        entries.push(input(3048, 13, pinch(0.1, Moved)));
        entries.push(input(3064, 14, pinch(0.1, Moved)));
        entries.push(input(3080, 15, pinch(0.0, Ended)));
        // A plain pinch out, much later.
        entries.push(input(5000, 20, pinch(0.0, Started)));
        entries.push(input(5016, 21, pinch(-0.5, Moved)));
        entries.push(input(5032, 22, pinch(0.0, Ended)));
        // A drag over frames 30..=33, three moves in frame 31.
        let down = InputKind::MouseDown {
            button: "left".into(),
            clicks: 1,
        };
        entries.push(input(7000, 30, down));
        for (t, f) in [
            (7010, 30),
            (7020, 31),
            (7021, 31),
            (7022, 31),
            (7030, 32),
            (7040, 33),
        ] {
            entries.push(input(t, f, drag()));
        }
        let up = InputKind::MouseUp {
            button: "left".into(),
        };
        entries.push(input(7050, 33, up));
        // A hover move after the drag.
        entries.push(input(8000, 40, InputKind::MouseMove { button: None }));
        for (f, ms) in [(30, 5.0), (31, 7.0), (32, 6.0), (33, 20.0), (40, 1.0)] {
            entries.push(frame(f, ms));
        }
        entries.push(Entry::End(json!({
            "type": "end", "t_ns": 9000 * MS, "closed_early": false
        })));
        let mut text = lines(&entries);
        text.push_str("{\"type\":\"scroll\",\"t_ns\":1\n"); // a line cut short
        text
    }

    #[test]
    fn entries_round_trip_through_json() {
        let log = parse_log(&synthetic());
        assert_eq!(log.unreadable, 1);
        assert_eq!(log.entries.len(), 29);
        for entry in &log.entries {
            assert_eq!(Entry::from_json(&entry.to_json()).as_ref(), Some(entry));
        }
    }

    #[test]
    fn summarises_a_synthetic_log() {
        let a = analyse(&parse_log(&synthetic()));
        assert_eq!(a.inputs, 22);
        assert_eq!(a.unreadable, 1);
        assert_eq!(a.frames_logged, 5);

        let row = |c: Class| a.rows.iter().find(|r| r.class == c).unwrap();
        assert_eq!(row(Class::ScrollLines).events, 3);
        assert_eq!(row(Class::ScrollLines).line_steps, vec![-3.0, 3.0]);
        assert_eq!(row(Class::ScrollLines).phases, vec![(Phase::Moved, 3)]);
        assert_eq!(row(Class::ScrollPixels).events, 3);
        assert_eq!(
            row(Class::ScrollPixels).phases,
            vec![(Phase::Started, 1), (Phase::Moved, 1), (Phase::Ended, 1)]
        );
        assert_eq!(row(Class::Pinch).events, 7);
        assert_eq!(row(Class::Drag).events, 6);
        assert_eq!(row(Class::Hover).events, 1);
        assert_eq!(row(Class::Button).events, 2);
        assert!(a.rows.iter().all(|r| r.class != Class::TouchScroll));

        let classes: Vec<Class> = a.bursts.iter().map(|b| b.class).collect();
        assert_eq!(
            classes,
            [
                Class::ScrollLines,
                Class::ScrollPixels,
                Class::Pinch,
                Class::Pinch,
                Class::Drag
            ]
        );

        let per_frame = |name: &str| a.per_frame.iter().find(|(n, _)| n == name).unwrap().1;
        let wheel = per_frame(Class::ScrollLines.label());
        assert_eq!((wheel.count, wheel.p50, wheel.max), (2, 1.0, 2.0));
        let drag = per_frame(Class::Drag.label());
        assert_eq!((drag.count, drag.p50, drag.max), (4, 1.0, 3.0));
        // Frame 12 shows the scroll Ended and the pinch Started; frame 33 a move and the up.
        let all = per_frame("all input");
        assert_eq!((all.count, all.max), (16, 3.0));

        assert_eq!(a.pinches.len(), 2);
        assert_eq!(a.pinches[0].sequence(), "started, moved x2, ended");
        assert!(a.pinches[0].after_scroll);
        assert!((a.pinches[0].scale - 1.21).abs() < 1e-6);
        assert_eq!(a.pinches[1].sequence(), "started, moved, ended");
        assert!(!a.pinches[1].after_scroll);
        assert_eq!(a.scroll_to_pinch.len(), 1);
        assert_eq!(a.scroll_to_pinch[0].scroll_events, 3);
        assert_eq!(a.scroll_to_pinch[0].pan_px, 4.0);
        assert_eq!(a.scroll_to_pinch[0].gap_ms, 0.0);
        assert_eq!(a.pinch_to_scroll, 0);

        // Drag frames 30..=33 have d2p 5, 7, 6 and 20 ms; frame 40 (hover) is not one.
        assert_eq!((a.drag.frames, a.drag.timed, a.drag.multi_draw), (4, 4, 0));
        let d2p = a.drag.dirty_to_present.unwrap();
        assert_eq!((d2p.p50, d2p.p99, d2p.max), (6.0, 20.0, 20.0));
        assert_eq!(a.drag.draw.unwrap().p99, 10.0);
        assert_eq!((a.any_input.frames, a.any_input.timed), (16, 5));

        let text = a.render();
        for needle in [
            "scale factor 1.25",
            "scroll, Lines delta",
            "line steps seen: -3, 3",
            "started, moved x2, ended",
            "Scroll -> pinch switches in one gesture: 1",
            "Pinch -> scroll within 50 ms (GPUI does not emit this inside one gesture): 0",
            "during the drag        p50 6  p95 20  p99 20  max 20  (n 4)",
            "1 unreadable lines",
        ] {
            assert!(text.contains(needle), "missing `{needle}` in:\n{text}");
        }
    }

    #[test]
    fn counts_a_pinch_followed_at_once_by_a_scroll() {
        use Phase::*;
        let a = analyse(&parse_log(&lines(&[
            input(0, 0, pinch(0.0, Started)),
            input(16, 1, pinch(0.0, Ended)),
            input(20, 1, pan(0.0, Started)),
            input(36, 2, pan(0.0, Ended)),
        ])));
        assert_eq!(a.pinch_to_scroll, 1);
        assert!(a.scroll_to_pinch.is_empty());
    }

    #[test]
    fn a_pinch_without_started_is_still_a_gesture() {
        let a = analyse(&parse_log(&lines(&[
            input(0, 0, pinch(0.2, Phase::Moved)),
            input(16, 1, pinch(0.0, Phase::Ended)),
        ])));
        assert_eq!(a.pinches.len(), 1);
        assert_eq!(a.pinches[0].sequence(), "moved, ended");
    }

    #[test]
    fn summarises_an_empty_log() {
        let no_input = "\
{\"type\":\"header\",\"schema\":\"omnia-bench/s7-log/1\",\"seconds\":5}
{\"type\":\"frame\",\"frame\":0,\"t_ns\":1,\"draws\":1,\"dirty_to_present_ms\":3.0,\"draw_ms\":1.0}
{\"type\":\"end\",\"t_ns\":5000000000,\"closed_early\":false}
";
        for text in ["", no_input] {
            let a = analyse(&parse_log(text));
            assert_eq!(a.inputs, 0);
            assert!(a.rows.is_empty() && a.pinches.is_empty() && a.per_frame.is_empty());
            assert!(a.bursts.is_empty() && a.scroll_to_pinch.is_empty());
            assert_eq!(a.drag.dirty_to_present, None);
            let rendered = a.render();
            assert!(rendered.contains("no input events"), "{rendered}");
            assert!(rendered.contains("during the drag        no timed frames"));
        }
    }
}
