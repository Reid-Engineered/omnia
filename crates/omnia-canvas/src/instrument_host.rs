//! T7.3: animated instrument stub content (`canvas-plan.md` section 6, G5, G6, G8, G16, D7,
//! B8, B10, INV6, INV11, `ui-plan.md` UD5, UK10, UQ6).
//!
//! Holds ONLY the animated stub content (no floating frame, drag, stacking, or focus handling,
//! which belong to `ui-roadmap.md` `U4.1` and mount this stub in `U5.5`). Mounted inside a
//! `.cached()` view above the cached sheet so self-notification via GPUI's next-frame callback
//! repaints only this stub without invalidating or repainting `SheetView` (INV11).

use crate::snap_px::{device_rect, round_device, stroke_device_px, to_device};
use gpui::{
    App, Bounds, Context, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, PaintQuad, Pixels, Render, Style, WeakEntity, Window, fill, relative,
};
use omnia_theme::Theme;
use std::{cell::Cell, rc::Rc, time::Duration};

/// Default logical dimensions of the oscilloscope stub content (`ui-plan.md` UK10).
pub const DEFAULT_INSTRUMENT_SIZE: (f32, f32) = (576.0, 468.0);
/// Default horizontal trace resolution in quad columns (matching `S0.4` dual-channel scope).
pub const DEFAULT_TRACE_COLUMNS: usize = 512;

const PLOT_INSET_PX: f64 = 8.0;
const TRACE_STROKE_PX: f64 = 1.5;
const SWEEP_SPEED_HZ: f64 = 1.0;
const GRID_DIVS_X: usize = 10;
const GRID_DIVS_Y: usize = 8;
const GRID_MINOR_ALPHA: f32 = 0.10;
const GRID_AXIS_ALPHA: f32 = 0.24;

/// Target refresh cadence for [`InstrumentStub`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstrumentRate {
    #[default]
    Hz60,
    Hz30,
}

impl InstrumentRate {
    /// Cadence in hertz (`60` or `30`).
    pub const fn hz(self) -> u32 {
        match self {
            Self::Hz60 => 60,
            Self::Hz30 => 30,
        }
    }

    /// Nominal frame interval for this cadence.
    pub const fn interval(self) -> Duration {
        match self {
            Self::Hz60 => Duration::from_nanos(16_666_667),
            Self::Hz30 => Duration::from_nanos(33_333_333),
        }
    }

    /// Parses a cadence in hertz (`60` or `30`).
    pub const fn from_hz(hz: u32) -> Option<Self> {
        match hz {
            60 => Some(Self::Hz60),
            30 => Some(Self::Hz30),
            _ => None,
        }
    }
}

/// Number of display refreshes between consecutive updates of an [`InstrumentStub`] at `rate`.
///
/// Uses nearest-integer rounding against the panel's `refresh_interval` so a 60.008 Hz panel
/// steps every refresh (`stride = 1`) at 60 Hz and every second refresh (`stride = 2`) at 30 Hz.
pub fn frame_stride(refresh_interval: Option<Duration>, rate: InstrumentRate) -> u32 {
    let Some(interval) = refresh_interval else {
        return 60 / rate.hz();
    };
    let secs = interval.as_secs_f64();
    if !secs.is_finite() || secs <= 0.0 {
        return 60 / rate.hz();
    }
    let display_hz = 1.0 / secs;
    (display_hz / f64::from(rate.hz())).round().max(1.0) as u32
}

/// Attributes stub paint work separately from GPUI `paint_quad` calls (B10, Q5).
pub trait InstrumentPaintScope {
    fn omnia(&self, work: &mut dyn FnMut()) {
        work();
    }

    fn gpui(&self, work: &mut dyn FnMut()) {
        work();
    }
}

fn run_omnia<R>(scope: Option<&dyn InstrumentPaintScope>, f: impl FnOnce() -> R) -> R {
    match scope {
        Some(s) => {
            let mut out = None;
            let mut f = Some(f);
            s.omnia(&mut || {
                if let Some(f) = f.take() {
                    out = Some(f());
                }
            });
            out.expect("InstrumentPaintScope::omnia must execute its closure")
        }
        None => f(),
    }
}

fn run_gpui<R>(scope: Option<&dyn InstrumentPaintScope>, f: impl FnOnce() -> R) -> R {
    match scope {
        Some(s) => {
            let mut out = None;
            let mut f = Some(f);
            s.gpui(&mut || {
                if let Some(f) = f.take() {
                    out = Some(f());
                }
            });
            out.expect("InstrumentPaintScope::gpui must execute its closure")
        }
        None => f(),
    }
}

fn gpui_color(c: omnia_theme::Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

fn gpui_color_alpha(c: omnia_theme::Rgba, alpha_scale: f32) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a * alpha_scale,
    }
    .into()
}

/// Animated dual-trace instrument stub view (T7.3, G6, G8, G16, INV6, INV11).
///
/// Embed inside `.cached(...)` so its frame-rate self-notifications only repaint this view and
/// replay `SheetView` without running `SheetView::paint`.
pub struct InstrumentStub {
    theme: &'static Theme,
    rate: InstrumentRate,
    columns: usize,
    running: bool,
    tick: u64,
    pending_tick: bool,
    schedule_gen: u64,
    scheduled: Rc<Cell<bool>>,
    renders: u64,
    paints: Rc<Cell<u64>>,
    scope: Option<Rc<dyn InstrumentPaintScope>>,
}

impl InstrumentStub {
    /// Creates a running 60 Hz animated stub using `theme` tokens.
    pub fn new(theme: &'static Theme) -> Self {
        Self {
            theme,
            rate: InstrumentRate::Hz60,
            columns: DEFAULT_TRACE_COLUMNS,
            running: true,
            tick: 0,
            pending_tick: false,
            schedule_gen: 0,
            scheduled: Rc::new(Cell::new(false)),
            renders: 0,
            paints: Rc::new(Cell::new(0)),
            scope: None,
        }
    }

    pub fn with_rate(mut self, rate: InstrumentRate) -> Self {
        self.rate = rate;
        self
    }

    pub fn with_columns(mut self, columns: usize) -> Self {
        self.columns = columns.max(1);
        self
    }

    pub fn with_running(mut self, running: bool) -> Self {
        self.running = running;
        self
    }

    pub fn with_paint_scope(mut self, scope: Rc<dyn InstrumentPaintScope>) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn set_paint_scope(&mut self, scope: Option<Rc<dyn InstrumentPaintScope>>) {
        self.scope = scope;
    }

    pub fn theme(&self) -> &'static Theme {
        self.theme
    }

    /// Updates the theme and notifies only this stub view.
    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        if std::ptr::eq(self.theme, theme) {
            return;
        }
        self.theme = theme;
        cx.notify();
    }

    pub fn rate(&self) -> InstrumentRate {
        self.rate
    }

    /// Changes the animation cadence and reschedules from the current phase.
    pub fn set_rate(&mut self, rate: InstrumentRate, cx: &mut Context<Self>) {
        if self.rate == rate {
            return;
        }
        self.rate = rate;
        self.pending_tick = false;
        self.schedule_gen = self.schedule_gen.wrapping_add(1);
        self.scheduled.set(false);
        cx.notify();
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Starts or stops self-notification; stopping cancels pending ticks so idle frames drop to 0 (B8).
    pub fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        if self.running == running {
            return;
        }
        self.running = running;
        self.pending_tick = false;
        self.schedule_gen = self.schedule_gen.wrapping_add(1);
        self.scheduled.set(false);
        if running {
            cx.notify();
        }
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    pub fn render_count(&self) -> u64 {
        self.renders
    }

    pub fn paint_count(&self) -> u64 {
        self.paints.get()
    }

    /// Current waveform phase in radians.
    pub fn phase_rad(&self) -> f64 {
        (self.tick as f64 / f64::from(self.rate.hz())) * std::f64::consts::TAU * SWEEP_SPEED_HZ
    }

    /// Emits all quads for the current frame without allocating (B10).
    pub fn paint_quads(
        &self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        sink: impl FnMut(PaintQuad),
    ) {
        paint_trace(
            bounds,
            scale_factor,
            self.theme,
            self.phase_rad(),
            self.columns,
            sink,
        );
    }
}

fn schedule_countdown(
    window: &Window,
    entity: WeakEntity<InstrumentStub>,
    scheduled: Rc<Cell<bool>>,
    generation: u64,
    remaining: u32,
) {
    if remaining <= 1 {
        window.on_next_frame(move |_, cx| {
            let _ = entity.update(cx, |stub, cx| {
                if stub.schedule_gen != generation {
                    return;
                }
                stub.scheduled.set(false);
                if !stub.running {
                    return;
                }
                stub.pending_tick = true;
                cx.notify();
            });
        });
    } else {
        window.on_next_frame(move |window, cx| {
            let keep_going = entity
                .read_with(cx, |stub, _| {
                    stub.running && stub.schedule_gen == generation
                })
                .unwrap_or(false);
            if keep_going {
                schedule_countdown(window, entity, scheduled, generation, remaining - 1);
            } else if let Some(strong) = entity.upgrade()
                && strong.read(cx).schedule_gen == generation
            {
                scheduled.set(false);
            }
        });
    }
}

impl Render for InstrumentStub {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        if self.pending_tick {
            self.pending_tick = false;
            self.tick = self.tick.wrapping_add(1);
        }
        if self.running && !self.scheduled.get() {
            let stride = frame_stride(window.refresh_interval(cx), self.rate);
            self.scheduled.set(true);
            schedule_countdown(
                window,
                cx.entity().downgrade(),
                self.scheduled.clone(),
                self.schedule_gen,
                stride,
            );
        }
        InstrumentTraceElement {
            theme: self.theme,
            phase: self.phase_rad(),
            columns: self.columns,
            paints: self.paints.clone(),
            scope: self.scope.clone(),
        }
    }
}

/// Custom [`Element`] painting the stub screen directly inside `paint_layer` (G5, G8, B10).
struct InstrumentTraceElement {
    theme: &'static Theme,
    phase: f64,
    columns: usize,
    paints: Rc<Cell<u64>>,
    scope: Option<Rc<dyn InstrumentPaintScope>>,
}

impl Element for InstrumentTraceElement {
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
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
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
        self.paints.set(self.paints.get() + 1);
        let sf = window.scale_factor();
        let theme = self.theme;
        let phase = self.phase;
        let columns = self.columns;
        let scope = self.scope.as_deref();
        window.paint_layer(|window| {
            run_omnia(scope, || {
                paint_trace(bounds, sf, theme, phase, columns, |quad| {
                    run_gpui(scope, || window.paint_quad(quad));
                });
            });
        });
    }
}

fn paint_trace(
    bounds: Bounds<Pixels>,
    scale_factor: f32,
    theme: &'static Theme,
    phase: f64,
    columns: usize,
    mut sink: impl FnMut(PaintQuad),
) {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return;
    }
    let left_dev = round_device(to_device(f64::from(bounds.origin.x), scale_factor));
    let top_dev = round_device(to_device(f64::from(bounds.origin.y), scale_factor));
    let right_dev = round_device(to_device(f64::from(bounds.right()), scale_factor));
    let bottom_dev = round_device(to_device(f64::from(bounds.bottom()), scale_factor));
    if right_dev <= left_dev || bottom_dev <= top_dev {
        return;
    }
    let width_dev = (right_dev - left_dev) as u32;
    let height_dev = (bottom_dev - top_dev) as u32;

    let screen_bounds = device_rect(left_dev, top_dev, width_dev, height_dev, scale_factor);
    sink(fill(screen_bounds, gpui_color(theme.instrument_screen)));

    let inset_dev = round_device(to_device(PLOT_INSET_PX, scale_factor)).max(0.0);
    let plot_left = left_dev + inset_dev;
    let plot_top = top_dev + inset_dev;
    let plot_right = (right_dev - inset_dev).max(plot_left);
    let plot_bottom = (bottom_dev - inset_dev).max(plot_top);
    let plot_w = plot_right - plot_left;
    let plot_h = plot_bottom - plot_top;
    if plot_w < 1.0 || plot_h < 1.0 {
        return;
    }
    let plot_w_u32 = plot_w as u32;
    let plot_h_u32 = plot_h as u32;

    let line_dev = stroke_device_px(1.0, scale_factor);
    let minor_color = gpui_color_alpha(theme.stage_tag_text, GRID_MINOR_ALPHA);
    let axis_color = gpui_color_alpha(theme.stage_tag_text, GRID_AXIS_ALPHA);

    for div in 1..GRID_DIVS_X {
        let x = round_device(plot_left + plot_w * (div as f64 / GRID_DIVS_X as f64));
        let color = if div == GRID_DIVS_X / 2 {
            axis_color
        } else {
            minor_color
        };
        sink(fill(
            device_rect(x, plot_top, line_dev, plot_h_u32, scale_factor),
            color,
        ));
    }
    for div in 1..GRID_DIVS_Y {
        let y = round_device(plot_top + plot_h * (div as f64 / GRID_DIVS_Y as f64));
        let color = if div == GRID_DIVS_Y / 2 {
            axis_color
        } else {
            minor_color
        };
        sink(fill(
            device_rect(plot_left, y, plot_w_u32, line_dev, scale_factor),
            color,
        ));
    }

    let columns = columns.max(1);
    let stroke_dev = stroke_device_px(TRACE_STROKE_PX, scale_factor);
    let mid_y = plot_top + plot_h * 0.5;
    let max_y_dev = (plot_bottom - f64::from(stroke_dev)).max(plot_top);
    let col_step = plot_w / columns as f64;
    let geom = PlotGeometry {
        columns,
        phase,
        mid_y,
        plot_h,
        min_y_dev: plot_top,
        max_y_dev,
    };
    let mut prev_a_y = geom.sample_y(0, false);
    let mut prev_b_y = geom.sample_y(0, true);

    let color_a = gpui_color(theme.probe_a);
    let color_b = gpui_color(theme.probe_b);

    for col in 0..columns {
        let x0 = round_device(plot_left + col_step * col as f64);
        let x1 = round_device(plot_left + col_step * (col + 1) as f64);
        let col_w = ((x1 - x0) as u32).max(1);

        let next_a_y = geom.sample_y(col + 1, false);
        let top_a = prev_a_y.min(next_a_y);
        let bot_a = prev_a_y.max(next_a_y);
        let span_a = ((bot_a - top_a) as u32)
            .saturating_add(stroke_dev)
            .max(stroke_dev);
        sink(fill(
            device_rect(x0, top_a, col_w, span_a, scale_factor),
            color_a,
        ));
        prev_a_y = next_a_y;

        let next_b_y = geom.sample_y(col + 1, true);
        let top_b = prev_b_y.min(next_b_y);
        let bot_b = prev_b_y.max(next_b_y);
        let span_b = ((bot_b - top_b) as u32)
            .saturating_add(stroke_dev)
            .max(stroke_dev);
        sink(fill(
            device_rect(x0, top_b, col_w, span_b, scale_factor),
            color_b,
        ));
        prev_b_y = next_b_y;
    }
}

#[derive(Clone, Copy)]
struct PlotGeometry {
    columns: usize,
    phase: f64,
    mid_y: f64,
    plot_h: f64,
    min_y_dev: f64,
    max_y_dev: f64,
}

impl PlotGeometry {
    #[inline]
    fn sample_y(self, col: usize, channel_b: bool) -> f64 {
        let u = col as f64 / self.columns as f64;
        let angle = std::f64::consts::TAU * 4.0 * u - self.phase;
        let env = (0.18 + 0.82 * u).min(1.0);
        let signal = if channel_b {
            0.22 * env * angle.cos()
        } else {
            0.38 * env * angle.sin()
        };
        round_device(self.mid_y - signal * self.plot_h).clamp(self.min_y_dev, self.max_y_dev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::overlay::{CachedSheet, OverlayView};
    use crate::sheet::SheetDocument;
    use gpui::{
        AppContext, Bounds, Entity, IntoElement, ParentElement, Render, StyleRefinement, Styled,
        TestAppContext, Window, div, point, px, size,
    };
    use omnia_doc::{Doc, GRID_NM, Label, Orient, Part, Pt, Wire};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::{GRAPHITE, GRAY};

    fn sample_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let cap = lib.by_name("Capacitor-IEEE-NonPolarized").unwrap();
        let res_def = lib.get(res);
        let cap_def = lib.get(cap);
        let g = GRID_NM;

        Doc::from_items(
            [
                (
                    Part {
                        symbol: res,
                        at: Pt { x: -8 * g, y: 0 },
                        orient: Orient::IDENTITY,
                        designator: "R1".into(),
                        value: "10k".into(),
                    },
                    res_def.pins.iter().map(|p| p.at).collect(),
                    res_def.bounds,
                ),
                (
                    Part {
                        symbol: cap,
                        at: Pt { x: 8 * g, y: 0 },
                        orient: Orient::IDENTITY,
                        designator: "C1".into(),
                        value: "100n".into(),
                    },
                    cap_def.pins.iter().map(|p| p.at).collect(),
                    cap_def.bounds,
                ),
            ],
            [Wire {
                a: Pt { x: -6 * g, y: 0 },
                b: Pt { x: 6 * g, y: 0 },
            }],
            [Label {
                at: Pt { x: 0, y: 2 * g },
                text: "SIG".into(),
            }],
        )
        .unwrap()
    }

    #[test]
    fn frame_stride_matches_display_refresh_and_rate() {
        assert_eq!(frame_stride(None, InstrumentRate::Hz60), 1);
        assert_eq!(frame_stride(None, InstrumentRate::Hz30), 2);
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(16_664_444)), InstrumentRate::Hz60),
            1
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(16_664_444)), InstrumentRate::Hz30),
            2
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(16_666_667)), InstrumentRate::Hz60),
            1
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(16_666_667)), InstrumentRate::Hz30),
            2
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(8_333_333)), InstrumentRate::Hz60),
            2
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(8_333_333)), InstrumentRate::Hz30),
            4
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(6_060_606)), InstrumentRate::Hz60),
            3
        );
        assert_eq!(
            frame_stride(Some(Duration::from_nanos(6_060_606)), InstrumentRate::Hz30),
            6
        );
    }

    #[test]
    fn paint_quads_is_deterministic_and_changes_with_phase() {
        let stub = InstrumentStub::new(&GRAPHITE);
        let bounds = Bounds::new(
            point(px(24.0), px(24.0)),
            size(px(DEFAULT_INSTRUMENT_SIZE.0), px(DEFAULT_INSTRUMENT_SIZE.1)),
        );
        let mut quads_a = Vec::new();
        stub.paint_quads(bounds, 1.25, |q| quads_a.push(q.bounds));
        let mut quads_b = Vec::new();
        stub.paint_quads(bounds, 1.25, |q| quads_b.push(q.bounds));
        assert_eq!(
            quads_a.len(),
            1 + (GRID_DIVS_X - 1) + (GRID_DIVS_Y - 1) + 2 * DEFAULT_TRACE_COLUMNS
        );
        assert_eq!(quads_a, quads_b);

        let mut moved = InstrumentStub::new(&GRAPHITE);
        moved.tick = 12;
        let mut quads_c = Vec::new();
        moved.paint_quads(bounds, 1.25, |q| quads_c.push(q.bounds));
        assert_ne!(quads_a, quads_c);

        // Zero / tiny bounds never panic or emit underflow quads.
        let mut empty = Vec::new();
        stub.paint_quads(
            Bounds::new(point(px(0.0), px(0.0)), size(px(0.0), px(0.0))),
            1.0,
            |q| empty.push(q.bounds),
        );
        assert!(empty.is_empty());
    }

    struct CachedSheetHost {
        sheet: Entity<CachedSheet>,
        stub: Entity<InstrumentStub>,
    }

    impl Render for CachedSheetHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(
                    self.sheet
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
                .child(
                    self.stub.clone().cached(
                        StyleRefinement::default()
                            .absolute()
                            .right(px(20.0))
                            .top(px(20.0))
                            .w(px(DEFAULT_INSTRUMENT_SIZE.0))
                            .h(px(DEFAULT_INSTRUMENT_SIZE.1)),
                    ),
                )
        }
    }

    #[gpui::test]
    fn inv11_stub_animating_at_60hz_and_30hz_never_repaints_sheet(cx: &mut TestAppContext) {
        let doc = cx.new(|_| sample_doc());
        let mut sheet_handle = None;
        let mut stub_handle = None;
        let (_host, cx) = cx.add_window_view(|_, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let sheet =
                cx.new(|_| CachedSheet::new(camera, &GRAPHITE, Some(SheetDocument::new(doc))));
            let stub = cx.new(|_| InstrumentStub::new(&GRAPHITE));
            sheet_handle = Some(sheet.clone());
            stub_handle = Some(stub.clone());
            CachedSheetHost { sheet, stub }
        });
        let sheet = sheet_handle.expect("sheet created");
        let stub = stub_handle.expect("stub created");

        let sheet_notifies = Rc::new(Cell::new(0u32));
        let sn = sheet_notifies.clone();
        cx.update(|_, cx| {
            cx.observe(&sheet, move |_, _| sn.set(sn.get() + 1))
                .detach();
        });

        let initial_sheet_paints = cx.update(|_, cx| sheet.read(cx).paint_count());
        let initial_sheet_renders = cx.update(|_, cx| sheet.read(cx).render_count());
        let initial_stub_paints = cx.update(|_, cx| stub.read(cx).paint_count());
        assert_eq!(initial_sheet_paints, 1);
        assert_eq!(initial_stub_paints, 1);

        // 60 Hz: 60 next-frame ticks animate the stub 60 times while SheetView paints 0 times.
        for _ in 0..60 {
            let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
            assert_eq!(ran, 1);
            cx.run_until_parked();
        }
        assert_eq!(
            cx.update(|_, cx| stub.read(cx).paint_count()),
            initial_stub_paints + 60
        );
        assert_eq!(cx.update(|_, cx| stub.read(cx).tick_count()), 60);
        assert_eq!(
            cx.update(|_, cx| sheet.read(cx).paint_count()),
            initial_sheet_paints,
            "INV11: SheetView must not repaint while InstrumentStub animates at 60 Hz"
        );
        assert_eq!(
            cx.update(|_, cx| sheet.read(cx).render_count()),
            initial_sheet_renders
        );
        assert_eq!(sheet_notifies.get(), 0);

        // Switch to 30 Hz: only every second frame notifies and paints the stub.
        cx.update(|_, cx| {
            stub.update(cx, |s, cx| s.set_rate(InstrumentRate::Hz30, cx));
        });
        cx.run_until_parked();
        let paints_before_30 = cx.update(|_, cx| stub.read(cx).paint_count());
        let ticks_before_30 = cx.update(|_, cx| stub.read(cx).tick_count());
        for step in 0..60 {
            let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
            if step == 0 {
                // One superseded 60 Hz callback may still be queued alongside the new 30 Hz one.
                assert!((1..=2).contains(&ran));
            } else {
                assert_eq!(ran, 1);
            }
            cx.run_until_parked();
        }
        assert_eq!(
            cx.update(|_, cx| stub.read(cx).paint_count()),
            paints_before_30 + 30
        );
        assert_eq!(
            cx.update(|_, cx| stub.read(cx).tick_count()),
            ticks_before_30 + 30
        );
        assert_eq!(
            cx.update(|_, cx| sheet.read(cx).paint_count()),
            initial_sheet_paints,
            "INV11: SheetView must not repaint while InstrumentStub animates at 30 Hz"
        );
        assert_eq!(sheet_notifies.get(), 0);

        // Theme update on the stub alone repaints only the stub.
        let paints_before_theme = cx.update(|_, cx| stub.read(cx).paint_count());
        cx.update(|_, cx| {
            stub.update(cx, |s, cx| s.set_theme(&GRAY, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| stub.read(cx).paint_count()),
            paints_before_theme + 1
        );
        assert_eq!(
            cx.update(|_, cx| sheet.read(cx).paint_count()),
            initial_sheet_paints
        );
        assert_eq!(sheet_notifies.get(), 0);

        // B8: stopping the stub drops any queued callback and requests 0 further frames.
        let paints_at_stop = cx.update(|_, cx| stub.read(cx).paint_count());
        let ticks_at_stop = cx.update(|_, cx| stub.read(cx).tick_count());
        cx.update(|_, cx| {
            stub.update(cx, |s, cx| s.set_running(false, cx));
        });
        cx.run_until_parked();
        // At most one already-queued callback runs and ignores the stale generation.
        let _ = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| stub.read(cx).paint_count()),
            paints_at_stop
        );
        assert_eq!(cx.update(|_, cx| stub.read(cx).tick_count()), ticks_at_stop);
        assert_eq!(
            cx.update(|window, cx| window.simulate_next_frame(cx)),
            0,
            "B8: stopped stub must schedule 0 frames"
        );
    }

    struct OverlayAndStubHost {
        overlay: Entity<OverlayView>,
        stub: Entity<InstrumentStub>,
    }

    impl Render for OverlayAndStubHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(
                    self.overlay
                        .clone()
                        .cached(StyleRefinement::default().size_full()),
                )
                .child(
                    self.stub.clone().cached(
                        StyleRefinement::default()
                            .absolute()
                            .right(px(20.0))
                            .top(px(20.0))
                            .w(px(DEFAULT_INSTRUMENT_SIZE.0))
                            .h(px(DEFAULT_INSTRUMENT_SIZE.1)),
                    ),
                )
        }
    }

    #[gpui::test]
    fn inv11_stub_over_overlay_view_never_repaints_sheet(cx: &mut TestAppContext) {
        let doc = cx.new(|_| sample_doc());
        let mut overlay_handle = None;
        let mut stub_handle = None;
        let (_host, cx) = cx.add_window_view(|window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let overlay = cx.new(|cx| OverlayView::new(doc, camera, &GRAPHITE, cx));
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            let stub = cx.new(|_| InstrumentStub::new(&GRAPHITE));
            overlay_handle = Some(overlay.clone());
            stub_handle = Some(stub.clone());
            OverlayAndStubHost { overlay, stub }
        });
        let overlay = overlay_handle.expect("overlay created");
        let stub = stub_handle.expect("stub created");

        let sheet_notifies = Rc::new(Cell::new(0u32));
        let sn = sheet_notifies.clone();
        cx.update(|_, cx| {
            let sheet = overlay.read(cx).sheet_entity().clone();
            cx.observe(&sheet, move |_, _| sn.set(sn.get() + 1))
                .detach();
        });

        let initial_sheet_paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let initial_overlay_paints = cx.update(|_, cx| overlay.read(cx).overlay_paint_count());
        let initial_stub_paints = cx.update(|_, cx| stub.read(cx).paint_count());
        assert_eq!(initial_sheet_paints, 1);

        for _ in 0..30 {
            let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
            assert_eq!(ran, 1);
            cx.run_until_parked();
        }

        assert_eq!(
            cx.update(|_, cx| stub.read(cx).paint_count()),
            initial_stub_paints + 30
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints,
            "INV11: SheetView behind OverlayView must stay at 0 additional paints"
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).overlay_paint_count()),
            initial_overlay_paints,
            "OverlayView must replay from cache while only InstrumentStub animates"
        );
        assert_eq!(sheet_notifies.get(), 0);
    }
}
