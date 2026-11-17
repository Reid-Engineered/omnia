//! Spike US2 and US3 (ui-plan.md section 9; ui-roadmap.md U0.4):
//!
//! - US2: Over the `empty` sheet (`omnia_canvas::SheetCanvas`), a cached prototype menu bar
//!   whose popover is a `deferred` child. Hovers across all nine menu labels for 300 frames,
//!   then verifies both fresh and cached-replayed popover hitbox occlusion over the sheet.
//! - US3: Over the same window, a prototype `Readout` element in a cached status view fed by
//!   `CanvasEvent::Status`. Drives the `pan` script (1 device px per frame) for 600 frames and
//!   measures view renders plus `ours` and `total` allocations in the status subscribe, render,
//!   layout, prepaint, and paint scopes as well as whole-frame allocations.

use super::SpikeError;
use crate::alloc::{AllocCounts, not_ours, ours};
use crate::harness::{environment, watchdog};
use crate::stats::{self, Summary, round};
use gpui::{
    Anchor, AnyElement, App, AppContext, Bounds, Context, Element, ElementId, Entity,
    FrameDurationSnapshot, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    PlatformInput, Point, Render, ScrollDelta, ScrollEvent, ScrollSource, ShapedLine, SharedString,
    Style, StyleRefinement, Subscription, TextRun, TouchPhase, Window, WindowBounds, WindowOptions,
    anchored, deferred, div, font, point, prelude::*, px, relative, size,
};
use omnia_canvas::{CanvasEvent, CanvasStatus, SheetCanvas};
use omnia_doc::Pt;
use omnia_theme::{GRAPHITE, Rgba, Theme};
use serde_json::{Map, Value, json};
use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;
use std::time::{Duration, Instant};

const MENU_BAR_HEIGHT: f32 = 36.0;
const STATUS_BAR_HEIGHT: f32 = 40.0;
const READOUT_FONT_SIZE: f32 = 11.5;
const READOUT_VIEW_WIDTH: f32 = 440.0;
const POPOVER_WIDTH: f32 = 220.0;
const POPOVER_ITEMS: usize = 6;
const NM_PER_MM: f64 = 1_000_000.0;

const MENU_LABELS: [&str; 9] = [
    "File", "Edit", "View", "Place", "Simulate", "Tools", "Reports", "Window", "Help",
];

fn color(c: Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadoutLayout {
    Nested,
    Sibling,
}

impl ReadoutLayout {
    fn as_str(self) -> &'static str {
        match self {
            Self::Nested => "nested",
            Self::Sibling => "sibling",
        }
    }
}

/// CLI options for `bench spike shell-cache`.
#[derive(Clone, Debug)]
struct Config {
    hover_frames: u32,
    pan_frames: u32,
    warmup: u32,
    size: (f32, f32),
    readout_layout: ReadoutLayout,
    json_path: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hover_frames: 300,
            pan_frames: 600,
            warmup: 60,
            size: (1440.0, 900.0),
            readout_layout: ReadoutLayout::Nested,
            json_path: None,
        }
    }
}

fn parse_size(text: &str) -> Result<(f32, f32), String> {
    let (w, h) = text
        .split_once('x')
        .ok_or("--size must look like 1440x900")?;
    let parse = |v: &str| {
        v.parse::<u32>()
            .ok()
            .filter(|n| (320..=16_384).contains(n))
            .map(|n| n as f32)
            .ok_or_else(|| format!("--size: `{v}` is not a size in 320..=16384"))
    };
    Ok((parse(w)?, parse(h)?))
}

fn parse_args(args: Vec<String>) -> Result<Config, SpikeError> {
    let usage = |m: String| SpikeError::Usage(format!("spike shell-cache: {m}"));
    let mut cfg = Config::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let mut val = |name: &str| {
            iter.next()
                .ok_or_else(|| usage(format!("--{name} needs a value")))
        };
        match arg.as_str() {
            "--hover-frames" => {
                let raw = val("hover-frames")?;
                cfg.hover_frames = raw
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| usage(format!("--hover-frames: invalid `{raw}`")))?;
            }
            "--pan-frames" => {
                let raw = val("pan-frames")?;
                cfg.pan_frames = raw
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| usage(format!("--pan-frames: invalid `{raw}`")))?;
            }
            "--warmup" => {
                let raw = val("warmup")?;
                cfg.warmup = raw
                    .parse::<u32>()
                    .ok()
                    .filter(|&n| n >= 1)
                    .ok_or_else(|| usage(format!("--warmup: invalid `{raw}`")))?;
            }
            "--size" => {
                cfg.size = parse_size(&val("size")?).map_err(usage)?;
            }
            "--readout-layout" => {
                let raw = val("readout-layout")?;
                cfg.readout_layout = match raw.as_str() {
                    "nested" => ReadoutLayout::Nested,
                    "sibling" => ReadoutLayout::Sibling,
                    _ => {
                        return Err(usage(format!(
                            "--readout-layout: expected `nested` or `sibling`, got `{raw}`"
                        )));
                    }
                };
            }
            "--json" => {
                cfg.json_path = Some(val("json")?);
            }
            other => return Err(usage(format!("unexpected argument `{other}`"))),
        }
    }
    Ok(cfg)
}

pub fn main(args: Vec<String>) -> Result<(), SpikeError> {
    let cfg = parse_args(args)?;
    let report = run_spike(cfg.clone()).map_err(SpikeError::Run)?;
    let text = serde_json::to_string_pretty(&report).expect("report serialises");
    println!("{text}");
    if let Some(path) = cfg.json_path {
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|e| SpikeError::Run(format!("write {path}: {e}")))?;
        eprintln!("bench: wrote {path}");
    }
    Ok(())
}

// ---- Fixed-capacity stack text & readout glyph cache ----------------------------------------

/// Stack-allocated UTF-8 buffer for formatting status readouts without heap allocation.
#[derive(Clone, Copy)]
struct FixedText<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> FixedText<N> {
    fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        // SAFETY: only valid UTF-8 slices are ever pushed into `buf`.
        unsafe { std::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }

    fn push_str(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let avail = N.saturating_sub(self.len);
        let take = bytes.len().min(avail);
        if take == 0 {
            return;
        }
        // Keep UTF-8 boundary when truncating.
        let mut end = take;
        while end > 0 && (bytes[end - 1] & 0b1100_0000) == 0b1000_0000 && end < bytes.len() {
            end -= 1;
        }
        if end < bytes.len() && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
        }
        self.buf[self.len..self.len + end].copy_from_slice(&bytes[..end]);
        self.len += end;
    }

    fn pop_byte(&mut self) {
        if self.len > 0 {
            self.len -= 1;
        }
    }
}

impl<const N: usize> core::fmt::Write for FixedText<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.push_str(s);
        Ok(())
    }
}

fn format_cursor_into(out: &mut FixedText<128>, cursor: Option<Pt>) {
    match cursor {
        Some(p) => {
            let _ = write!(
                out,
                "{:.2}, {:.2} mm",
                f64::from(p.x) / NM_PER_MM,
                f64::from(p.y) / NM_PER_MM
            );
        }
        None => out.push_str("—"),
    }
}

fn format_grid_into(out: &mut FixedText<128>, step_nm: i32) {
    let mut tmp = FixedText::<32>::new();
    let _ = write!(tmp, "{:.6}", f64::from(step_nm) / NM_PER_MM);
    let dot = tmp.as_str().find('.').unwrap_or(tmp.len);
    while tmp.as_str().ends_with('0') && tmp.len - dot > 3 {
        tmp.pop_byte();
    }
    out.push_str(tmp.as_str());
}

fn format_status_into(out: &mut FixedText<128>, status: &CanvasStatus) {
    format_cursor_into(out, status.cursor);
    out.push_str("     grid ");
    format_grid_into(out, status.grid);
    out.push_str("     zoom ");
    let _ = write!(out, "{:.0}%", f64::from(status.zoom) * 100.0);
}

/// Cached single-character `ShapedLine`s in Source Code Pro for zero-allocation readout painting.
struct ReadoutCache {
    /// ASCII `0x20..=0x7E` at indices `0..95`, `'—'` at `95`, `'·'` at `96`.
    lines: Vec<ShapedLine>,
    advance: Pixels,
    ascent: Pixels,
    descent: Pixels,
    font_size: Pixels,
}

impl ReadoutCache {
    fn new(font_size: Pixels, window: &Window) -> Self {
        let mut lines = Vec::with_capacity(97);
        let scp = font("Source Code Pro");
        let text_system = window.text_system();
        let shape_char = |ch: char| {
            let mut buf = [0u8; 4];
            let s = ch.encode_utf8(&mut buf);
            let text: SharedString = s.to_owned().into();
            let run = TextRun {
                len: text.len(),
                font: scp.clone(),
                color: color(GRAPHITE.text_secondary),
                ..Default::default()
            };
            not_ours(|| text_system.shape_line(text, font_size, &[run], None))
        };
        for code in 0x20u8..=0x7Eu8 {
            lines.push(shape_char(code as char));
        }
        lines.push(shape_char('—'));
        lines.push(shape_char('·'));

        let zero = &lines[(b'0' - 0x20) as usize];
        Self {
            advance: zero.width(),
            ascent: zero.ascent,
            descent: zero.descent,
            font_size,
            lines,
        }
    }

    fn line_for(&self, ch: char) -> Option<&ShapedLine> {
        if ch.is_ascii() {
            let b = ch as u8;
            if (0x20..=0x7E).contains(&b) {
                return self.lines.get((b - 0x20) as usize);
            }
        }
        match ch {
            '—' => self.lines.get(95),
            '·' => self.lines.get(96),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ScopeAllocSample {
    ours_allocs: u64,
    ours_bytes: u64,
    total_allocs: u64,
    total_bytes: u64,
}

impl ScopeAllocSample {
    fn from_delta(d: AllocCounts) -> Self {
        Self {
            ours_allocs: d.ours_allocs,
            ours_bytes: d.ours_bytes,
            total_allocs: d.total_allocs,
            total_bytes: d.total_bytes,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ReadoutFrameScopes {
    subscribe: ScopeAllocSample,
    render: ScopeAllocSample,
    layout: ScopeAllocSample,
    prepaint: ScopeAllocSample,
    paint: ScopeAllocSample,
}

struct ReadoutStats {
    recording: bool,
    current: ReadoutFrameScopes,
    frames: Vec<ReadoutFrameScopes>,
}

impl ReadoutStats {
    fn with_capacity(cap: usize) -> Self {
        Self {
            recording: false,
            current: ReadoutFrameScopes::default(),
            frames: Vec::with_capacity(cap),
        }
    }

    fn finish_frame(&mut self) {
        if self.recording {
            self.frames.push(self.current);
            self.current = ReadoutFrameScopes::default();
        }
    }
}

/// Custom element that paints a fixed-buffer readout string using cached `ShapedLine` glyphs.
struct Readout {
    text: FixedText<128>,
    cache: Rc<ReadoutCache>,
    color: Hsla,
    stats: Rc<RefCell<ReadoutStats>>,
}

impl Element for Readout {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let before = AllocCounts::now();
        let layout_id = ours(|| {
            let mut style = Style::default();
            style.size.width = relative(1.).into();
            style.size.height = relative(1.).into();
            not_ours(|| window.request_layout(style, [], cx))
        });
        let delta = AllocCounts::now().since(before);
        let mut stats = self.stats.borrow_mut();
        if stats.recording {
            stats.current.layout = ScopeAllocSample::from_delta(delta);
        }
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let before = AllocCounts::now();
        ours(|| {});
        let delta = AllocCounts::now().since(before);
        let mut stats = self.stats.borrow_mut();
        if stats.recording {
            stats.current.prepaint = ScopeAllocSample::from_delta(delta);
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let before = AllocCounts::now();
        ours(|| {
            not_ours(|| {
                window.paint_layer(|window| {
                    let char_count = self.text.as_str().chars().count();
                    let total_width = self.cache.advance * char_count as f32;
                    let mut x = bounds.right() - total_width - px(14.0);
                    let baseline_y = bounds.origin.y
                        + (bounds.size.height - self.cache.ascent - self.cache.descent) / 2.0
                        + self.cache.ascent;
                    for ch in self.text.as_str().chars() {
                        if let Some(line) = self.cache.line_for(ch) {
                            for run in &line.runs {
                                for glyph in &run.glyphs {
                                    let _ = window.paint_glyph(
                                        point(x, baseline_y) + glyph.position,
                                        run.font_id,
                                        glyph.id,
                                        self.cache.font_size,
                                        self.color,
                                    );
                                }
                            }
                        }
                        x += self.cache.advance;
                    }
                });
            });
        });
        let delta = AllocCounts::now().since(before);
        let mut stats = self.stats.borrow_mut();
        if stats.recording {
            stats.current.paint = ScopeAllocSample::from_delta(delta);
        }
    }
}

// ---- Prototype views ------------------------------------------------------------------------

/// Small cached view containing ONLY the dynamic `Readout` element, fed by `CanvasEvent::Status`.
struct PrototypeReadoutsView {
    status: CanvasStatus,
    cache: Option<Rc<ReadoutCache>>,
    theme: &'static Theme,
    renders: u64,
    status_events: u64,
    stats: Rc<RefCell<ReadoutStats>>,
    _subscription: Subscription,
}

impl PrototypeReadoutsView {
    fn new(
        canvas: &Entity<SheetCanvas>,
        theme: &'static Theme,
        stats: Rc<RefCell<ReadoutStats>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let stats_sub = stats.clone();
        let subscription = cx.subscribe(canvas, move |this, _, event: &CanvasEvent, cx| {
            let before = AllocCounts::now();
            ours(|| match event {
                CanvasEvent::Status(status) => {
                    this.status_events += 1;
                    if this.status != *status {
                        this.status = *status;
                        not_ours(|| cx.notify());
                    }
                }
            });
            let delta = AllocCounts::now().since(before);
            let mut s = stats_sub.borrow_mut();
            if s.recording {
                s.current.subscribe = ScopeAllocSample::from_delta(delta);
            }
        });
        Self {
            status: canvas.read(cx).status(),
            cache: None,
            theme,
            renders: 0,
            status_events: 0,
            stats,
            _subscription: subscription,
        }
    }
}

impl Render for PrototypeReadoutsView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let cache = self
            .cache
            .get_or_insert_with(|| Rc::new(ReadoutCache::new(px(READOUT_FONT_SIZE), window)))
            .clone();
        let before = AllocCounts::now();
        let element = ours(|| {
            let mut text = FixedText::<128>::new();
            format_status_into(&mut text, &self.status);
            Readout {
                text,
                cache,
                color: color(self.theme.text_secondary),
                stats: self.stats.clone(),
            }
        });
        let delta = AllocCounts::now().since(before);
        let mut s = self.stats.borrow_mut();
        if s.recording {
            s.current.render = ScopeAllocSample::from_delta(delta);
        }
        element
    }
}

/// Outer cached status bar (static left slot + optional nested cached `PrototypeReadoutsView`).
struct PrototypeStatusBar {
    readouts: Entity<PrototypeReadoutsView>,
    include_readouts: bool,
    theme: &'static Theme,
    renders: u64,
}

impl Render for PrototypeStatusBar {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div()
            .w_full()
            .h(px(STATUS_BAR_HEIGHT))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .pl(px(14.0))
            .bg(color(self.theme.bg_panel))
            .border_t_1()
            .border_color(color(self.theme.rule))
            .child(
                div()
                    .font_family("Inter")
                    .text_size(px(12.0))
                    .text_color(color(self.theme.text_secondary))
                    .child("Ready · F1 for help"),
            )
            .when(self.include_readouts, |d| {
                d.child(
                    self.readouts.clone().cached(
                        StyleRefinement::default()
                            .w(px(READOUT_VIEW_WIDTH))
                            .h(px(STATUS_BAR_HEIGHT))
                            .flex_none(),
                    ),
                )
            })
    }
}

/// Tiny element wrapper that records an element's window `Bounds` during `prepaint` and
/// optionally counts `prepaint` and `paint` calls.
struct ProbeElement {
    child: Option<AnyElement>,
    bounds_out: Rc<Cell<Option<Bounds<Pixels>>>>,
    prepaints: Option<Rc<Cell<u64>>>,
    paints: Option<Rc<Cell<u64>>>,
}

impl Element for ProbeElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child_id = self.child.as_mut().unwrap().request_layout(window, cx);
        (child_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.bounds_out.set(Some(bounds));
        if let Some(p) = &self.prepaints {
            p.set(p.get() + 1);
        }
        self.child.as_mut().unwrap().prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(p) = &self.paints {
            p.set(p.get() + 1);
        }
        self.child.as_mut().unwrap().paint(window, cx);
    }
}

/// Cached prototype menu bar whose popover is a `deferred` child (UD2, US2).
struct PrototypeMenuBar {
    theme: &'static Theme,
    active_menu: Option<usize>,
    hovered_item: Option<usize>,
    renders: u64,
    label_bounds: Vec<Rc<Cell<Option<Bounds<Pixels>>>>>,
    popover_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    popover_prepaints: Rc<Cell<u64>>,
    popover_paints: Rc<Cell<u64>>,
}

impl PrototypeMenuBar {
    fn new(theme: &'static Theme) -> Self {
        Self {
            theme,
            active_menu: None,
            hovered_item: None,
            renders: 0,
            label_bounds: (0..MENU_LABELS.len())
                .map(|_| Rc::new(Cell::new(None)))
                .collect(),
            popover_bounds: Rc::new(Cell::new(None)),
            popover_prepaints: Rc::new(Cell::new(0)),
            popover_paints: Rc::new(Cell::new(0)),
        }
    }

    fn label_center(&self, idx: usize) -> Option<Point<Pixels>> {
        self.label_bounds
            .get(idx)
            .and_then(|b| b.get())
            .map(|b| b.center())
    }
}

impl Render for PrototypeMenuBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let mut bar = div()
            .w_full()
            .h(px(MENU_BAR_HEIGHT))
            .bg(color(self.theme.bg_backdrop))
            .flex()
            .flex_row()
            .items_center()
            .pl(px(48.0))
            .gap(px(4.0));

        for (idx, label) in MENU_LABELS.iter().enumerate() {
            let is_active = self.active_menu == Some(idx);
            let item = div()
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .font_family("Inter")
                .text_size(px(13.0))
                .text_color(color(self.theme.text_primary))
                .when(is_active, |d| d.bg(color(self.theme.bg_raised)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.active_menu = if this.active_menu == Some(idx) {
                            None
                        } else {
                            Some(idx)
                        };
                        this.hovered_item = None;
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.active_menu.is_some() && this.active_menu != Some(idx) {
                        this.active_menu = Some(idx);
                        this.hovered_item = None;
                        cx.notify();
                    }
                }))
                .child(*label);

            bar = bar.child(ProbeElement {
                child: Some(item.into_any_element()),
                bounds_out: self.label_bounds[idx].clone(),
                prepaints: None,
                paints: None,
            });
        }

        if let Some(menu_idx) = self.active_menu {
            let target = self.label_bounds[menu_idx].get().unwrap_or_else(|| {
                Bounds::new(
                    point(px(48.0 + menu_idx as f32 * 56.0), px(4.0)),
                    size(px(48.0), px(28.0)),
                )
            });

            let mut popover = div()
                .w(px(POPOVER_WIDTH))
                .p(px(4.0))
                .bg(color(self.theme.bg_card))
                .border_1()
                .border_color(color(self.theme.rule))
                .rounded(px(8.0))
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation());

            for item_idx in 0..POPOVER_ITEMS {
                let is_hovered = self.hovered_item == Some(item_idx);
                let row = div()
                    .w_full()
                    .h(px(28.0))
                    .px(px(10.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .font_family("Inter")
                    .text_size(px(12.5))
                    .text_color(color(self.theme.text_primary))
                    .when(is_hovered, |d| d.bg(color(self.theme.bg_raised)))
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.hovered_item != Some(item_idx) {
                            this.hovered_item = Some(item_idx);
                            cx.notify();
                        }
                    }))
                    .child(format!("{} action {}", MENU_LABELS[menu_idx], item_idx + 1));
                popover = popover.child(row);
            }

            let probed = ProbeElement {
                child: Some(popover.into_any_element()),
                bounds_out: self.popover_bounds.clone(),
                prepaints: Some(self.popover_prepaints.clone()),
                paints: Some(self.popover_paints.clone()),
            };

            bar = bar.child(
                deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .attach(target, Anchor::BottomLeft)
                        .snap_to_window()
                        .child(probed),
                )
                .with_priority(1),
            );
        }

        bar
    }
}

/// Root view holding cached `MenuBar`, cached `SheetCanvas`, and cached `StatusBar`.
struct ShellCacheView {
    menu_bar: Entity<PrototypeMenuBar>,
    sheet: Entity<SheetCanvas>,
    status_bar: Entity<PrototypeStatusBar>,
    readouts: Entity<PrototypeReadoutsView>,
    sheet_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    root_renders: u64,
    driver_started: bool,
    config: Config,
    readout_stats: Rc<RefCell<ReadoutStats>>,
    outcome: Rc<RefCell<Option<Result<Value, String>>>>,
}

impl ShellCacheView {
    fn new(
        config: Config,
        outcome: Rc<RefCell<Option<Result<Value, String>>>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let theme = &GRAPHITE;
        let readout_stats = Rc::new(RefCell::new(ReadoutStats::with_capacity(
            config.pan_frames as usize + 16,
        )));
        let sheet = cx.new(|cx| {
            SheetCanvas::new(
                omnia_canvas::Camera::new(Pt::ZERO, 1.0, Bounds::default()),
                theme,
                cx,
            )
        });
        let readouts =
            cx.new(|cx| PrototypeReadoutsView::new(&sheet, theme, readout_stats.clone(), cx));
        let include_readouts = config.readout_layout == ReadoutLayout::Nested;
        let status_bar = cx.new(|_| PrototypeStatusBar {
            readouts: readouts.clone(),
            include_readouts,
            theme,
            renders: 0,
        });
        let menu_bar = cx.new(|_| PrototypeMenuBar::new(theme));
        Self {
            menu_bar,
            sheet,
            status_bar,
            readouts,
            sheet_bounds: Rc::new(Cell::new(None)),
            root_renders: 0,
            driver_started: false,
            config,
            readout_stats,
            outcome,
        }
    }
}

impl Render for ShellCacheView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.root_renders += 1;
        if !self.driver_started {
            self.driver_started = true;
            let state = Rc::new(RefCell::new(DriverState::from_view(self, cx.entity())));
            schedule_driver_tick(window, state);
        }

        let cached_sheet = self
            .sheet
            .clone()
            .cached(StyleRefinement::default().size_full());

        let bottom_bar = match self.config.readout_layout {
            ReadoutLayout::Nested => self
                .status_bar
                .clone()
                .cached(
                    StyleRefinement::default()
                        .w_full()
                        .h(px(STATUS_BAR_HEIGHT))
                        .flex_none(),
                )
                .into_any_element(),
            ReadoutLayout::Sibling => div()
                .w_full()
                .h(px(STATUS_BAR_HEIGHT))
                .flex()
                .flex_row()
                .items_center()
                .bg(color(GRAPHITE.bg_panel))
                .border_t_1()
                .border_color(color(GRAPHITE.rule))
                .child(
                    self.status_bar
                        .clone()
                        .cached(StyleRefinement::default().flex_1().h(px(STATUS_BAR_HEIGHT))),
                )
                .child(
                    self.readouts.clone().cached(
                        StyleRefinement::default()
                            .w(px(READOUT_VIEW_WIDTH))
                            .h(px(STATUS_BAR_HEIGHT))
                            .flex_none(),
                    ),
                )
                .into_any_element(),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(GRAPHITE.bg_backdrop))
            .child(
                self.menu_bar.clone().cached(
                    StyleRefinement::default()
                        .w_full()
                        .h(px(MENU_BAR_HEIGHT))
                        .flex_none(),
                ),
            )
            .child(div().flex_1().min_h_0().child(ProbeElement {
                child: Some(cached_sheet.into_any_element()),
                bounds_out: self.sheet_bounds.clone(),
                prepaints: None,
                paints: None,
            }))
            .child(bottom_bar)
    }
}

// ---- Finite frame-by-frame spike driver -----------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Warmup(u32),
    OpenForHover,
    Hover(u32),
    HoverCollect,
    ProbeOpenForOcclusion,
    ProbeFreshMove,
    ProbeFreshMiddleDown,
    ProbeFreshMiddleUp,
    ProbeFreshScroll,
    ProbeReplayRootNotify,
    ProbeReplayVerifyAndMove,
    ProbeOutsideMove,
    ProbeCloseMenu,
    PanStart,
    Pan(u32),
    PanCollect,
    Done,
}

#[derive(Clone, Copy, Default)]
struct CountsSnapshot {
    root_renders: u64,
    menu_renders: u64,
    sheet_renders: u64,
    status_bar_renders: u64,
    readouts_renders: u64,
    status_events: u64,
    popover_prepaints: u64,
    popover_paints: u64,
}

#[derive(Default)]
struct OcclusionResults {
    popover_overlaps_sheet: bool,
    popover_bounds: Option<[f32; 4]>,
    sheet_bounds: Option<[f32; 4]>,
    fresh_move_occluded: bool,
    fresh_middle_drag_occluded: bool,
    fresh_scroll_occluded: bool,
    fresh_sheet_paints: u64,
    replay_menu_renders: u64,
    replay_sheet_paints: u64,
    replay_popover_prepaints: u64,
    replay_popover_paints: u64,
    replay_move_occluded: bool,
    replay_scroll_occluded: bool,
    outside_move_reached_sheet: bool,
    #[cfg(feature = "scene-counts")]
    scene_quads_closed: usize,
    #[cfg(feature = "scene-counts")]
    scene_quads_open_fresh: usize,
    #[cfg(feature = "scene-counts")]
    scene_quads_open_replayed: usize,
}

struct DriverState {
    config: Config,
    root: Entity<ShellCacheView>,
    menu_bar: Entity<PrototypeMenuBar>,
    sheet: Entity<SheetCanvas>,
    status_bar: Entity<PrototypeStatusBar>,
    readouts: Entity<PrototypeReadoutsView>,
    sheet_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    readout_stats: Rc<RefCell<ReadoutStats>>,
    outcome: Rc<RefCell<Option<Result<Value, String>>>>,
    started: Instant,
    phase: Phase,
    prev_allocs: AllocCounts,
    hover_start_counts: CountsSnapshot,
    hover_end_counts: CountsSnapshot,
    hover_allocs: Vec<AllocCounts>,
    pan_start_counts: CountsSnapshot,
    pan_end_counts: CountsSnapshot,
    pan_allocs: Vec<AllocCounts>,
    pan_baseline: Option<FrameDurationSnapshot>,
    occlusion: OcclusionResults,
    probe_snap: CountsSnapshot,
    probe_cursor_before: Option<Pt>,
    probe_zoom_before: f64,
    probe_inside: Option<Point<Pixels>>,
}

impl DriverState {
    fn from_view(view: &ShellCacheView, root: Entity<ShellCacheView>) -> Self {
        let hover_cap = view.config.hover_frames as usize;
        let pan_cap = view.config.pan_frames as usize;
        Self {
            config: view.config.clone(),
            root,
            menu_bar: view.menu_bar.clone(),
            sheet: view.sheet.clone(),
            status_bar: view.status_bar.clone(),
            readouts: view.readouts.clone(),
            sheet_bounds: view.sheet_bounds.clone(),
            readout_stats: view.readout_stats.clone(),
            outcome: view.outcome.clone(),
            started: Instant::now(),
            phase: Phase::Warmup(0),
            prev_allocs: AllocCounts::now(),
            hover_start_counts: CountsSnapshot::default(),
            hover_end_counts: CountsSnapshot::default(),
            hover_allocs: Vec::with_capacity(hover_cap),
            pan_start_counts: CountsSnapshot::default(),
            pan_end_counts: CountsSnapshot::default(),
            pan_allocs: Vec::with_capacity(pan_cap),
            pan_baseline: None,
            occlusion: OcclusionResults::default(),
            probe_snap: CountsSnapshot::default(),
            probe_cursor_before: None,
            probe_zoom_before: 1.0,
            probe_inside: None,
        }
    }

    fn snapshot(&self, cx: &App) -> CountsSnapshot {
        let menu = self.menu_bar.read(cx);
        let readouts = self.readouts.read(cx);
        CountsSnapshot {
            root_renders: self.root.read(cx).root_renders,
            menu_renders: menu.renders,
            sheet_renders: self.sheet.read(cx).render_count(),
            status_bar_renders: self.status_bar.read(cx).renders,
            readouts_renders: readouts.renders,
            status_events: readouts.status_events,
            popover_prepaints: menu.popover_prepaints.get(),
            popover_paints: menu.popover_paints.get(),
        }
    }

    fn sheet_center(&self) -> Point<Pixels> {
        self.sheet_bounds
            .get()
            .map(|b| b.center())
            .unwrap_or_else(|| point(px(640.0), px(400.0)))
    }

    fn step(&mut self, window: &mut Window, cx: &mut App) -> bool {
        let now_allocs = AllocCounts::now();
        let frame_alloc_delta = now_allocs.since(self.prev_allocs);

        match self.phase {
            Phase::Warmup(i) => {
                if i == 0 {
                    let center = self.sheet_center();
                    window.dispatch_event(mouse_move(center, None), cx);
                } else if i <= 9 {
                    if let Some(p) = self.menu_bar.read(cx).label_center((i - 1) as usize) {
                        window.dispatch_event(mouse_move(p, None), cx);
                    }
                } else if i == 10 {
                    if let Some(p) = self.menu_bar.read(cx).label_center(0) {
                        window.dispatch_event(mouse_down(MouseButton::Left, p), cx);
                        window.dispatch_event(mouse_up(MouseButton::Left, p), cx);
                    }
                } else if i == 11 {
                    if let Some(p) = self.menu_bar.read(cx).label_center(0) {
                        window.dispatch_event(mouse_down(MouseButton::Left, p), cx);
                        window.dispatch_event(mouse_up(MouseButton::Left, p), cx);
                    }
                    window.dispatch_event(mouse_move(self.sheet_center(), None), cx);
                } else {
                    let scale = window.scale_factor();
                    let center = self.sheet_center();
                    let phase = if i == 12 {
                        TouchPhase::Started
                    } else if i + 1 == self.config.warmup {
                        TouchPhase::Ended
                    } else {
                        TouchPhase::Moved
                    };
                    window.dispatch_event(
                        scroll_pixels(center, px(-1.0 / scale), px(0.0), phase),
                        cx,
                    );
                }
                if i + 1 < self.config.warmup {
                    self.phase = Phase::Warmup(i + 1);
                } else {
                    self.phase = Phase::OpenForHover;
                }
            }
            Phase::OpenForHover => {
                // Click label 0 to open the popover cleanly before the 300-frame hover run.
                let p = self
                    .menu_bar
                    .read(cx)
                    .label_center(0)
                    .unwrap_or(point(px(64.0), px(18.0)));
                window.dispatch_event(mouse_down(MouseButton::Left, p), cx);
                window.dispatch_event(mouse_up(MouseButton::Left, p), cx);
                self.phase = Phase::Hover(0);
            }
            Phase::Hover(i) => {
                if i == 0 {
                    self.hover_start_counts = self.snapshot(cx);
                } else {
                    self.hover_allocs.push(frame_alloc_delta);
                }
                // Bounce across all 9 labels so every single frame lands on a different label.
                let idx = bounce_index(i + 1, MENU_LABELS.len());
                let p = self
                    .menu_bar
                    .read(cx)
                    .label_center(idx)
                    .unwrap_or(point(px(64.0), px(18.0)));
                window.dispatch_event(mouse_move(p, None), cx);
                if i + 1 < self.config.hover_frames {
                    self.phase = Phase::Hover(i + 1);
                } else {
                    self.phase = Phase::HoverCollect;
                }
            }
            Phase::HoverCollect => {
                self.hover_allocs.push(frame_alloc_delta);
                self.hover_end_counts = self.snapshot(cx);
                // Switch to menu 2 ("View") so its popover sits near the upper-left/middle of sheet.
                let p = self
                    .menu_bar
                    .read(cx)
                    .label_center(2)
                    .unwrap_or(point(px(160.0), px(18.0)));
                window.dispatch_event(mouse_move(p, None), cx);
                self.phase = Phase::ProbeOpenForOcclusion;
            }
            Phase::ProbeOpenForOcclusion => {
                let menu = self.menu_bar.read(cx);
                let pop = menu.popover_bounds.get();
                let sheet_b = self.sheet_bounds.get();
                self.occlusion.popover_bounds = pop.map(bounds_array);
                self.occlusion.sheet_bounds = sheet_b.map(bounds_array);
                if let (Some(pop), Some(sb)) = (pop, sheet_b) {
                    let inside = pop.center();
                    self.occlusion.popover_overlaps_sheet =
                        pop.contains(&inside) && sb.contains(&inside);
                    self.probe_inside = Some(inside);
                }
                #[cfg(feature = "scene-counts")]
                {
                    self.occlusion.scene_quads_open_fresh = window.rendered_scene().quads.len();
                }
                self.probe_snap = self.snapshot(cx);
                self.probe_cursor_before = self.sheet.read(cx).status().cursor;
                self.probe_zoom_before = self.sheet.read(cx).camera().zoom();
                if let Some(inside) = self.probe_inside {
                    window.dispatch_event(mouse_move(inside, None), cx);
                }
                self.phase = Phase::ProbeFreshMove;
            }
            Phase::ProbeFreshMove => {
                let sheet_after = self.sheet.read(cx);
                self.occlusion.fresh_move_occluded =
                    sheet_after.status().cursor == self.probe_cursor_before;
                if let Some(inside) = self.probe_inside {
                    window.dispatch_event(mouse_down(MouseButton::Middle, inside), cx);
                }
                self.phase = Phase::ProbeFreshMiddleDown;
            }
            Phase::ProbeFreshMiddleDown => {
                self.occlusion.fresh_middle_drag_occluded =
                    !self.sheet.read(cx).navigator().is_dragging();
                if let Some(inside) = self.probe_inside {
                    window.dispatch_event(mouse_up(MouseButton::Middle, inside), cx);
                }
                self.phase = Phase::ProbeFreshMiddleUp;
            }
            Phase::ProbeFreshMiddleUp => {
                if let Some(inside) = self.probe_inside {
                    window.dispatch_event(scroll_lines(inside, 3.0), cx);
                }
                self.phase = Phase::ProbeFreshScroll;
            }
            Phase::ProbeFreshScroll => {
                let sheet_after = self.sheet.read(cx);
                self.occlusion.fresh_scroll_occluded =
                    (sheet_after.camera().zoom() - self.probe_zoom_before).abs() < 1e-12;
                let snap_now = self.snapshot(cx);
                self.occlusion.fresh_sheet_paints =
                    snap_now.sheet_renders - self.probe_snap.sheet_renders;
                // Now test cached replay of MenuBar (and its deferred popover) by notifying ONLY root!
                self.probe_snap = snap_now;
                self.root.update(cx, |_, cx| cx.notify());
                self.phase = Phase::ProbeReplayRootNotify;
            }
            Phase::ProbeReplayRootNotify => {
                let snap_now = self.snapshot(cx);
                self.occlusion.replay_menu_renders =
                    snap_now.menu_renders - self.probe_snap.menu_renders;
                self.occlusion.replay_sheet_paints =
                    snap_now.sheet_renders - self.probe_snap.sheet_renders;
                self.occlusion.replay_popover_prepaints =
                    snap_now.popover_prepaints - self.probe_snap.popover_prepaints;
                self.occlusion.replay_popover_paints =
                    snap_now.popover_paints - self.probe_snap.popover_paints;
                #[cfg(feature = "scene-counts")]
                {
                    self.occlusion.scene_quads_open_replayed = window.rendered_scene().quads.len();
                }
                if let Some(inside) = self.probe_inside {
                    window.dispatch_event(mouse_move(inside, None), cx);
                    window.dispatch_event(scroll_lines(inside, 3.0), cx);
                }
                self.phase = Phase::ProbeReplayVerifyAndMove;
            }
            Phase::ProbeReplayVerifyAndMove => {
                let sheet_after = self.sheet.read(cx);
                self.occlusion.replay_move_occluded =
                    sheet_after.status().cursor == self.probe_cursor_before;
                self.occlusion.replay_scroll_occluded =
                    (sheet_after.camera().zoom() - self.probe_zoom_before).abs() < 1e-12;
                // Control: move to a point clearly outside the popover on the sheet.
                if let Some(sb) = self.sheet_bounds.get() {
                    let outside = point(sb.origin.x + px(40.0), sb.bottom() - px(40.0));
                    window.dispatch_event(mouse_move(outside, None), cx);
                }
                self.phase = Phase::ProbeOutsideMove;
            }
            Phase::ProbeOutsideMove => {
                let cursor_now = self.sheet.read(cx).status().cursor;
                self.occlusion.outside_move_reached_sheet =
                    cursor_now.is_some() && cursor_now != self.probe_cursor_before;
                // Close the menu popover before starting the pan run.
                self.menu_bar.update(cx, |m, cx| {
                    m.active_menu = None;
                    m.hovered_item = None;
                    cx.notify();
                });
                self.phase = Phase::ProbeCloseMenu;
            }
            Phase::ProbeCloseMenu => {
                #[cfg(feature = "scene-counts")]
                {
                    self.occlusion.scene_quads_closed = window.rendered_scene().quads.len();
                }
                let center = self.sheet_center();
                window.dispatch_event(mouse_move(center, None), cx);
                window.dispatch_event(
                    scroll_pixels(center, px(0.0), px(0.0), TouchPhase::Started),
                    cx,
                );
                self.phase = Phase::PanStart;
            }
            Phase::PanStart => {
                self.pan_baseline = Some(window.frame_duration_snapshot());
                self.pan_start_counts = self.snapshot(cx);
                self.readout_stats.borrow_mut().recording = true;
                self.phase = Phase::Pan(0);
            }
            Phase::Pan(i) => {
                if i > 0 {
                    self.pan_allocs.push(frame_alloc_delta);
                    self.readout_stats.borrow_mut().finish_frame();
                }
                let scale = window.scale_factor();
                let center = self.sheet_center();
                window.dispatch_event(
                    scroll_pixels(center, px(-1.0 / scale), px(0.0), TouchPhase::Moved),
                    cx,
                );
                if i + 1 < self.config.pan_frames {
                    self.phase = Phase::Pan(i + 1);
                } else {
                    self.phase = Phase::PanCollect;
                }
            }
            Phase::PanCollect => {
                self.pan_allocs.push(frame_alloc_delta);
                self.readout_stats.borrow_mut().finish_frame();
                self.readout_stats.borrow_mut().recording = false;
                self.pan_end_counts = self.snapshot(cx);
                let center = self.sheet_center();
                window.dispatch_event(
                    scroll_pixels(center, px(0.0), px(0.0), TouchPhase::Ended),
                    cx,
                );
                let report = self.build_report(window, cx);
                *self.outcome.borrow_mut() = Some(report);
                self.phase = Phase::Done;
                window.on_next_frame(|_, cx| cx.quit());
                return false;
            }
            Phase::Done => return false,
        }

        self.prev_allocs = AllocCounts::now();
        true
    }

    fn build_report(&self, window: &Window, cx: &App) -> Result<Value, String> {
        let end = window.frame_duration_snapshot();
        let base = self
            .pan_baseline
            .as_ref()
            .ok_or("no pan baseline snapshot")?;
        let delta =
            |later, earlier| stats::delta(later, earlier).ok_or("histogram subtraction failed");
        let draw = delta(&end.draw_duration_histogram, &base.draw_duration_histogram)?;
        let dirty_to_present = delta(
            &end.dirty_to_present_histogram,
            &base.dirty_to_present_histogram,
        )?;
        let present_interval = delta(
            &end.present_interval_histogram,
            &base.present_interval_histogram,
        )?;

        let summarize_allocs = |list: &[AllocCounts], f: fn(&AllocCounts) -> u64| {
            let mut samples: Vec<f64> = list.iter().map(|a| f(a) as f64).collect();
            Summary::from_samples(&mut samples).map_or(Value::Null, |s| s.to_json())
        };

        let hover_delta = CountsDelta::between(self.hover_start_counts, self.hover_end_counts);
        let pan_delta = CountsDelta::between(self.pan_start_counts, self.pan_end_counts);

        let rstats = self.readout_stats.borrow();
        let summarize_scope = |f: fn(&ReadoutFrameScopes) -> ScopeAllocSample| {
            let mut ours_a: Vec<f64> = rstats
                .frames
                .iter()
                .map(|s| f(s).ours_allocs as f64)
                .collect();
            let mut ours_b: Vec<f64> = rstats
                .frames
                .iter()
                .map(|s| f(s).ours_bytes as f64)
                .collect();
            let mut total_a: Vec<f64> = rstats
                .frames
                .iter()
                .map(|s| f(s).total_allocs as f64)
                .collect();
            let mut total_b: Vec<f64> = rstats
                .frames
                .iter()
                .map(|s| f(s).total_bytes as f64)
                .collect();
            json!({
                "samples": rstats.frames.len(),
                "ours_allocs": Summary::from_samples(&mut ours_a).map_or(Value::Null, |s| s.to_json()),
                "ours_bytes": Summary::from_samples(&mut ours_b).map_or(Value::Null, |s| s.to_json()),
                "total_allocs": Summary::from_samples(&mut total_a).map_or(Value::Null, |s| s.to_json()),
                "total_bytes": Summary::from_samples(&mut total_b).map_or(Value::Null, |s| s.to_json()),
            })
        };

        let mut pan_metrics = Map::new();
        if let Some(s) = Summary::from_nanos(&draw) {
            pan_metrics.insert("draw_ms".into(), s.to_json());
        }
        if let Some(s) = Summary::from_nanos(&dirty_to_present) {
            pan_metrics.insert("dirty_to_present_ms".into(), s.to_json());
        }
        if let Some(s) = Summary::from_nanos(&present_interval) {
            pan_metrics.insert("present_interval_ms".into(), s.to_json());
        }
        pan_metrics.insert(
            "frame_allocs".into(),
            json!({
                "ours": summarize_allocs(&self.pan_allocs, |a| a.ours_allocs),
                "total": summarize_allocs(&self.pan_allocs, |a| a.total_allocs),
            }),
        );
        pan_metrics.insert(
            "frame_alloc_bytes".into(),
            json!({
                "ours": summarize_allocs(&self.pan_allocs, |a| a.ours_bytes),
                "total": summarize_allocs(&self.pan_allocs, |a| a.total_bytes),
            }),
        );

        let occlusion_json = json!({
            "popover_overlaps_sheet": self.occlusion.popover_overlaps_sheet,
            "popover_bounds": self.occlusion.popover_bounds,
            "sheet_bounds": self.occlusion.sheet_bounds,
            "fresh_move_occluded": self.occlusion.fresh_move_occluded,
            "fresh_middle_drag_occluded": self.occlusion.fresh_middle_drag_occluded,
            "fresh_scroll_occluded": self.occlusion.fresh_scroll_occluded,
            "fresh_sheet_paints": self.occlusion.fresh_sheet_paints,
            "replay_menu_renders": self.occlusion.replay_menu_renders,
            "replay_sheet_paints": self.occlusion.replay_sheet_paints,
            "replay_popover_prepaints": self.occlusion.replay_popover_prepaints,
            "replay_popover_paints": self.occlusion.replay_popover_paints,
            "replay_move_occluded": self.occlusion.replay_move_occluded,
            "replay_scroll_occluded": self.occlusion.replay_scroll_occluded,
            "outside_move_reached_sheet": self.occlusion.outside_move_reached_sheet,
        });
        #[cfg(feature = "scene-counts")]
        let occlusion_json = {
            let mut occlusion_json = occlusion_json;
            occlusion_json["scene_quads_closed"] = json!(self.occlusion.scene_quads_closed);
            occlusion_json["scene_quads_open_fresh"] = json!(self.occlusion.scene_quads_open_fresh);
            occlusion_json["scene_quads_open_replayed"] =
                json!(self.occlusion.scene_quads_open_replayed);
            occlusion_json
        };

        Ok(json!({
            "schema": crate::compare::SCHEMA,
            "kind": "spike-shell-cache",
            "config": {
                "hover_frames": self.config.hover_frames,
                "pan_frames": self.config.pan_frames,
                "warmup": self.config.warmup,
                "window_logical": [self.config.size.0, self.config.size.1],
                "readout_layout": self.config.readout_layout.as_str(),
                "scene_counts": cfg!(feature = "scene-counts"),
            },
            "environment": environment(window, cx),
            "wall_s": round(self.started.elapsed().as_secs_f64()),
            "us2_hover": {
                "frames": self.hover_allocs.len(),
                "counts": hover_delta.to_json(),
                "frame_allocs": {
                    "ours": summarize_allocs(&self.hover_allocs, |a| a.ours_allocs),
                    "total": summarize_allocs(&self.hover_allocs, |a| a.total_allocs),
                },
                "occlusion_and_replay": occlusion_json,
            },
            "us3_pan": {
                "frames": self.pan_allocs.len(),
                "counts": pan_delta.to_json(),
                "metrics": pan_metrics,
                "readout_scopes": {
                    "subscribe": summarize_scope(|s| s.subscribe),
                    "render": summarize_scope(|s| s.render),
                    "layout": summarize_scope(|s| s.layout),
                    "prepaint": summarize_scope(|s| s.prepaint),
                    "paint": summarize_scope(|s| s.paint),
                },
            },
        }))
    }
}

struct CountsDelta {
    root_renders: u64,
    menu_renders: u64,
    sheet_paints: u64,
    status_bar_renders: u64,
    readouts_renders: u64,
    status_events: u64,
    popover_prepaints: u64,
    popover_paints: u64,
}

impl CountsDelta {
    fn between(start: CountsSnapshot, end: CountsSnapshot) -> Self {
        Self {
            root_renders: end.root_renders.saturating_sub(start.root_renders),
            menu_renders: end.menu_renders.saturating_sub(start.menu_renders),
            sheet_paints: end.sheet_renders.saturating_sub(start.sheet_renders),
            status_bar_renders: end
                .status_bar_renders
                .saturating_sub(start.status_bar_renders),
            readouts_renders: end.readouts_renders.saturating_sub(start.readouts_renders),
            status_events: end.status_events.saturating_sub(start.status_events),
            popover_prepaints: end
                .popover_prepaints
                .saturating_sub(start.popover_prepaints),
            popover_paints: end.popover_paints.saturating_sub(start.popover_paints),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "root_renders": self.root_renders,
            "menu_renders": self.menu_renders,
            "sheet_paints": self.sheet_paints,
            "status_bar_renders": self.status_bar_renders,
            "readouts_renders": self.readouts_renders,
            "status_events": self.status_events,
            "popover_prepaints": self.popover_prepaints,
            "popover_paints": self.popover_paints,
        })
    }
}

fn schedule_driver_tick(window: &Window, state: Rc<RefCell<DriverState>>) {
    window.on_next_frame(move |window, cx| {
        let keep_going = state.borrow_mut().step(window, cx);
        if keep_going {
            schedule_driver_tick(window, state);
        }
    });
}

fn bounce_index(step: u32, len: usize) -> usize {
    debug_assert!(len >= 2);
    let period = (len - 1) * 2;
    let pos = (step as usize) % period;
    if pos < len { pos } else { period - pos }
}

fn bounds_array(b: Bounds<Pixels>) -> [f32; 4] {
    [
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.size.width),
        f32::from(b.size.height),
    ]
}

fn mouse_move(position: Point<Pixels>, pressed_button: Option<MouseButton>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button,
        modifiers: Modifiers::none(),
    })
}

fn mouse_down(button: MouseButton, position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    })
}

fn mouse_up(button: MouseButton, position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
    })
}

fn scroll_pixels(
    position: Point<Pixels>,
    dx: Pixels,
    dy: Pixels,
    touch_phase: TouchPhase,
) -> PlatformInput {
    PlatformInput::Scroll(ScrollEvent {
        position,
        delta: ScrollDelta::Pixels(point(dx, dy)),
        modifiers: Modifiers::none(),
        touch_phase,
        ..Default::default()
    })
}

fn scroll_lines(position: Point<Pixels>, dy: f32) -> PlatformInput {
    PlatformInput::Scroll(ScrollEvent {
        position,
        delta: ScrollDelta::Lines(point(0.0, dy)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    })
}

fn load_fonts(cx: &App) -> gpui::Result<()> {
    cx.text_system().add_fonts(vec![
        std::borrow::Cow::Borrowed(
            include_bytes!("../../../../vendor/fonts/inter/Inter-Regular.ttf").as_slice(),
        ),
        std::borrow::Cow::Borrowed(
            include_bytes!("../../../../vendor/fonts/inter/Inter-Medium.ttf").as_slice(),
        ),
        std::borrow::Cow::Borrowed(
            include_bytes!("../../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice(),
        ),
        std::borrow::Cow::Borrowed(
            include_bytes!("../../../../vendor/fonts/source-code-pro/SourceCodePro-Regular.ttf")
                .as_slice(),
        ),
        std::borrow::Cow::Borrowed(
            include_bytes!("../../../../vendor/fonts/source-code-pro/SourceCodePro-Medium.ttf")
                .as_slice(),
        ),
        std::borrow::Cow::Borrowed(
            include_bytes!("../../../../vendor/fonts/source-code-pro/SourceCodePro-Semibold.ttf")
                .as_slice(),
        ),
    ])
}

fn run_spike(config: Config) -> Result<Value, String> {
    let total_frames = config.warmup + config.hover_frames + config.pan_frames + 30;
    watchdog(
        "spike shell-cache",
        Duration::from_secs(60) + Duration::from_millis(100) * total_frames,
    );
    let outcome: Rc<RefCell<Option<Result<Value, String>>>> = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        if let Err(err) = load_fonts(cx) {
            *slot.borrow_mut() = Some(Err(format!("load fonts: {err}")));
            cx.quit();
            return;
        }
        let bounds = Bounds::centered(None, size(px(config.size.0), px(config.size.1)), cx);
        let mut options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(config.size.0), px(config.size.1))),
            ..Default::default()
        };
        options.inactive_frame_interval = None;
        options.low_power_frame_interval = None;
        options.thermal_frame_interval = None;

        let win_slot = slot.clone();
        if let Err(err) = cx.open_window(options, move |window, cx| {
            #[cfg(not(target_os = "linux"))]
            window.resize(size(px(config.size.0), px(config.size.1)));
            cx.new(|cx| ShellCacheView::new(config, win_slot, cx))
        }) {
            *slot.borrow_mut() = Some(Err(format!("open window: {err}")));
            cx.quit();
            return;
        }
        cx.activate(true);
    });
    outcome
        .borrow_mut()
        .take()
        .unwrap_or_else(|| Err("spike shell-cache: app quit without a result".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::count_allocs;
    use omnia_doc::{GRID_NM, Pt};

    #[test]
    fn fixed_text_formats_without_heap_allocations() {
        let status = CanvasStatus {
            cursor: Some(Pt {
                x: 12_700_000,
                y: -5_080_000,
            }),
            grid: GRID_NM >> 5,
            zoom: 1.19,
        };
        let (text, allocs) = count_allocs(|| {
            let mut buf = FixedText::<128>::new();
            format_status_into(&mut buf, &status);
            buf
        });
        assert_eq!(allocs, 0);
        assert_eq!(
            text.as_str(),
            "12.70, -5.08 mm     grid 0.079375     zoom 119%"
        );

        let mut none_buf = FixedText::<128>::new();
        format_status_into(
            &mut none_buf,
            &CanvasStatus {
                cursor: None,
                grid: GRID_NM,
                zoom: 1.0,
            },
        );
        assert_eq!(none_buf.as_str(), "—     grid 2.54     zoom 100%");
    }

    #[test]
    fn bounce_index_changes_every_step_across_all_labels() {
        let seq: Vec<usize> = (0..20).map(|i| bounce_index(i, 9)).collect();
        assert_eq!(
            seq,
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2, 3]
        );
        for w in seq.windows(2) {
            assert_ne!(w[0], w[1]);
        }
    }
}
