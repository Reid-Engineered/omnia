//! T4.5: view animations — fit-to-sheet, zoom-to-selection, and K4 zoom steps.
//!
//! - K13: 150 ms ease-out, interruptible by any pan or zoom input from the current position.
//! - G16: driven on the active view via `window.request_animation_frame()`, stopping
//!   requests as soon as the animation completes or is cancelled so idle draws stay 0 (B8).
//! - INV8: all world-space coordinates stay in `i32`/`i64` nanometres and `f64` scales; no
//!   `f32` ever holds a world coordinate.

use crate::camera::{BASE_SCALE, Camera, MAX_ZOOM, MIN_ZOOM};
use gpui::{App, BackgroundExecutor, Bounds, Pixels, Point, point};
use omnia_doc::{Doc, ItemId, Pt, Rect, SHEET_LIMIT_NM};
use std::{rc::Rc, time::Duration};

/// K13: view animation duration (150 ms).
pub const VIEW_ANIM_DURATION: Duration = Duration::from_millis(150);

/// Default viewport fill fraction for fit-to-sheet and zoom-to-selection (5% margin per side).
pub const FIT_FILL_FRACTION: f64 = 0.9;

/// Standard cubic ease-out on `[0, 1]`, clamped for out-of-range inputs.
#[inline]
pub fn ease_out(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

fn union_opt(acc: Option<Rect>, rect: Rect) -> Option<Rect> {
    Some(match acc {
        Some(existing) => existing.union(rect),
        None => rect,
    })
}

fn union_pt(acc: Option<Rect>, p: Pt) -> Option<Rect> {
    union_opt(acc, Rect { min: p, max: p })
}

fn item_bounds(doc: &Doc, item: ItemId) -> Option<Rect> {
    match item {
        ItemId::Part(id) => {
            let mut acc = doc.part_body(id);
            if let Some(pins) = doc.pins(id) {
                for &pin in pins {
                    acc = union_pt(acc, pin);
                }
            }
            acc
        }
        ItemId::Wire(id) => doc
            .wire(id)
            .and_then(|wire| Rect::from_points(wire.a, wire.b).ok()),
        ItemId::Label(id) => doc.label(id).map(|label| Rect {
            min: label.at,
            max: label.at,
        }),
    }
}

/// Returns the bounding rectangle of all live items in `doc`, or `None` if `doc` is empty.
pub fn doc_bounds(doc: &Doc) -> Option<Rect> {
    let mut acc = None;
    for (id, _) in doc.parts() {
        if let Some(bounds) = item_bounds(doc, ItemId::Part(id)) {
            acc = union_opt(acc, bounds);
        }
    }
    for (_, wire) in doc.wires() {
        if let Ok(bounds) = Rect::from_points(wire.a, wire.b) {
            acc = union_opt(acc, bounds);
        }
    }
    for (_, label) in doc.labels() {
        acc = union_pt(acc, label.at);
    }
    acc
}

/// Returns the bounding rectangle of the live items in `selection`, or `None` if none are live.
pub fn selection_bounds(doc: &Doc, selection: &[ItemId]) -> Option<Rect> {
    let mut acc = None;
    for &item in selection {
        if let Some(bounds) = item_bounds(doc, item) {
            acc = union_opt(acc, bounds);
        }
    }
    acc
}

fn fit_rect_with_fill(rect: Rect, viewport: Bounds<Pixels>, fill_fraction: f64) -> Camera {
    let min_x = rect.min.x.min(rect.max.x);
    let max_x = rect.min.x.max(rect.max.x);
    let min_y = rect.min.y.min(rect.max.y);
    let max_y = rect.min.y.max(rect.max.y);

    // Midpoint in i64 so opposite extreme sheet limits never overflow i32 (D1/INV8).
    let sum_x = i64::from(min_x) + i64::from(max_x);
    let sum_y = i64::from(min_y) + i64::from(max_y);
    let base_x = (sum_x / 2).clamp(-i64::from(SHEET_LIMIT_NM), i64::from(SHEET_LIMIT_NM)) as i32;
    let base_y = (sum_y / 2).clamp(-i64::from(SHEET_LIMIT_NM), i64::from(SHEET_LIMIT_NM)) as i32;
    let rem_x = (sum_x as f64) * 0.5 - f64::from(base_x);
    let rem_y = (sum_y as f64) * 0.5 - f64::from(base_y);

    let w_nm = (i64::from(max_x) - i64::from(min_x)) as f64;
    let h_nm = (i64::from(max_y) - i64::from(min_y)) as f64;
    let vw = f64::from(viewport.size.width);
    let vh = f64::from(viewport.size.height);

    let zoom = if !vw.is_finite()
        || !vh.is_finite()
        || vw <= 0.0
        || vh <= 0.0
        || !fill_fraction.is_finite()
        || fill_fraction <= 0.0
    {
        1.0
    } else {
        let mut z: Option<f64> = None;
        if w_nm > 0.0 {
            let zx = (vw * fill_fraction) / (w_nm * BASE_SCALE);
            z = Some(z.map_or(zx, |cur| cur.min(zx)));
        }
        if h_nm > 0.0 {
            let zy = (vh * fill_fraction) / (h_nm * BASE_SCALE);
            z = Some(z.map_or(zy, |cur| cur.min(zy)));
        }
        z.unwrap_or(1.0).clamp(MIN_ZOOM, MAX_ZOOM)
    };

    let mut cam = Camera::new(
        Pt {
            x: base_x,
            y: base_y,
        },
        zoom,
        viewport,
    );
    if rem_x != 0.0 || rem_y != 0.0 {
        cam.pan_by(point(-rem_x * cam.scale(), -rem_y * cam.scale()));
    }
    cam
}

/// Computes a target camera fitting `rect` inside `viewport` with [`FIT_FILL_FRACTION`] margin.
pub fn fit_rect_camera(rect: Rect, viewport: Bounds<Pixels>) -> Camera {
    fit_rect_with_fill(rect, viewport, FIT_FILL_FRACTION)
}

/// Computes a target camera fitting `rect` edge-to-edge inside `viewport` (for zoom-to-area).
pub fn fit_area_camera(rect: Rect, viewport: Bounds<Pixels>) -> Camera {
    fit_rect_with_fill(rect, viewport, 1.0)
}

/// Computes the target camera for fit-to-sheet, falling back to `Pt::ZERO` at 100% zoom when
/// `doc` is `None` or empty.
pub fn fit_sheet_camera(doc: Option<&Doc>, viewport: Bounds<Pixels>) -> Camera {
    match doc.and_then(doc_bounds) {
        Some(bounds) => fit_rect_camera(bounds, viewport),
        None => Camera::new(Pt::ZERO, 1.0, viewport),
    }
}

/// Computes the target camera for zoom-to-selection, or `None` if nothing live is selected.
pub fn zoom_to_selection_camera(
    doc: &Doc,
    selection: &[ItemId],
    viewport: Bounds<Pixels>,
) -> Option<Camera> {
    selection_bounds(doc, selection).map(|bounds| fit_rect_camera(bounds, viewport))
}

/// Logical centre of `viewport`, matching `Camera::screen_origin`.
pub(crate) fn viewport_center(viewport: Bounds<Pixels>) -> Point<f64> {
    point(
        f64::from(viewport.origin.x) + f64::from(viewport.size.width) / 2.0,
        f64::from(viewport.origin.y) + f64::from(viewport.size.height) / 2.0,
    )
}

/// Extracts `(ref_pt, sub_nm_offset)` representing the world point at `viewport`'s centre.
fn camera_world_center(camera: &Camera, viewport: Bounds<Pixels>) -> (Pt, Point<f64>) {
    let origin = viewport_center(viewport);
    let ref_pt = camera.screen_to_world(origin);
    let ref_screen = camera.world_to_screen(ref_pt);
    let scale = camera.scale();
    let offset_nm = point(
        (origin.x - ref_screen.x) / scale,
        (origin.y - ref_screen.y) / scale,
    );
    (ref_pt, offset_nm)
}

/// Returns whether two cameras already represent the same zoom and screen placement.
pub fn cameras_equal(a: &Camera, b: &Camera) -> bool {
    a.zoom() == b.zoom()
        && a.world_to_screen(Pt::ZERO) == b.world_to_screen(Pt::ZERO)
        && a.visible() == b.visible()
}

/// Pure, time-parameterised camera transition (K13: 150 ms ease-out).
///
/// Interpolates zoom geometrically (`z0 * (z1 / z0)^u`) and pans along the fixed-pivot path
/// (`(1/z0 - 1/z(u)) / (1/z0 - 1/z1)`) so off-centre targets move along a straight screen-space
/// line without swoop. At completion (`elapsed >= duration`), returns `target` verbatim.
#[derive(Clone, Debug)]
pub struct CameraAnimation {
    start: Camera,
    target: Camera,
    duration: Duration,
    start_ref: Pt,
    start_off_nm: Point<f64>,
    start_zoom: f64,
    target_ref: Pt,
    target_off_nm: Point<f64>,
    target_zoom: f64,
}

impl CameraAnimation {
    /// Creates a 150 ms (K13) ease-out animation from `start` to `target` in `viewport`.
    pub fn new(start: Camera, target: Camera, viewport: Bounds<Pixels>) -> Self {
        Self::with_duration(start, target, viewport, VIEW_ANIM_DURATION)
    }

    /// Creates an ease-out animation with an explicit duration.
    pub fn with_duration(
        start: Camera,
        target: Camera,
        viewport: Bounds<Pixels>,
        duration: Duration,
    ) -> Self {
        let mut start = start;
        let mut target = target;
        start.set_viewport(viewport);
        target.set_viewport(viewport);
        let (start_ref, start_off_nm) = camera_world_center(&start, viewport);
        let (target_ref, target_off_nm) = camera_world_center(&target, viewport);
        let start_zoom = start.zoom();
        let target_zoom = target.zoom();
        Self {
            start,
            target,
            duration,
            start_ref,
            start_off_nm,
            start_zoom,
            target_ref,
            target_off_nm,
            target_zoom,
        }
    }

    pub fn start(&self) -> &Camera {
        &self.start
    }

    pub fn target(&self) -> &Camera {
        &self.target
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Whether `start` and `target` are already identical (no animation needed).
    pub fn is_noop(&self) -> bool {
        cameras_equal(&self.start, &self.target)
    }

    /// Evaluates the animation at `elapsed`, returning `(camera, finished)`.
    ///
    /// When `elapsed >= self.duration`, `finished` is `true` and the returned camera equals
    /// `self.target` (re-bound to `viewport` if resized mid-flight).
    pub fn sample(&self, elapsed: Duration, viewport: Bounds<Pixels>) -> (Camera, bool) {
        if self.duration.is_zero() || elapsed >= self.duration {
            let mut exact = self.target.clone();
            exact.set_viewport(viewport);
            return (exact, true);
        }
        if elapsed.is_zero() {
            let mut exact = self.start.clone();
            exact.set_viewport(viewport);
            return (exact, false);
        }

        let t = (elapsed.as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0);
        let u = ease_out(t);

        let z0 = self.start_zoom;
        let z1 = self.target_zoom;
        let zoom = (z0 * (z1 / z0).powf(u)).clamp(MIN_ZOOM, MAX_ZOOM);

        let pan_t = if (z1 / z0 - 1.0).abs() > 1e-9 {
            let inv_z0 = 1.0 / z0;
            let inv_z1 = 1.0 / z1;
            let inv_zu = 1.0 / zoom;
            ((inv_z0 - inv_zu) / (inv_z0 - inv_z1)).clamp(0.0, 1.0)
        } else {
            u
        };

        let dx_nm = (i64::from(self.target_ref.x) - i64::from(self.start_ref.x)) as f64
            + (self.target_off_nm.x - self.start_off_nm.x);
        let dy_nm = (i64::from(self.target_ref.y) - i64::from(self.start_ref.y)) as f64
            + (self.target_off_nm.y - self.start_off_nm.y);

        let world_x = f64::from(self.start_ref.x) + self.start_off_nm.x + dx_nm * pan_t;
        let world_y = f64::from(self.start_ref.y) + self.start_off_nm.y + dy_nm * pan_t;

        let base = Pt {
            x: world_x
                .round()
                .clamp(-f64::from(SHEET_LIMIT_NM), f64::from(SHEET_LIMIT_NM)) as i32,
            y: world_y
                .round()
                .clamp(-f64::from(SHEET_LIMIT_NM), f64::from(SHEET_LIMIT_NM)) as i32,
        };
        let rem_x = world_x - f64::from(base.x);
        let rem_y = world_y - f64::from(base.y);

        let mut cam = Camera::new(base, zoom, viewport);
        cam.pan_by(point(-rem_x * cam.scale(), -rem_y * cam.scale()));
        (cam, false)
    }
}

/// Active GPUI-clock-bound view animation.
///
/// Uses `cx.background_executor().now()` so real apps read wall-clock time while `#[gpui::test]`
/// tests advance deterministically with `cx.background_executor().advance_clock(...)`.
#[derive(Clone)]
pub struct ViewAnimation {
    anim: CameraAnimation,
    elapsed: Rc<dyn Fn(&BackgroundExecutor) -> Duration>,
}

impl std::fmt::Debug for ViewAnimation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewAnimation")
            .field("anim", &self.anim)
            .finish_non_exhaustive()
    }
}

impl ViewAnimation {
    /// Captures the current executor timestamp and starts `anim`.
    pub fn start(anim: CameraAnimation, cx: &App) -> Self {
        let started = cx.background_executor().now();
        Self {
            anim,
            elapsed: Rc::new(move |exec| {
                let now = exec.now();
                if now > started {
                    now - started
                } else {
                    Duration::ZERO
                }
            }),
        }
    }

    pub fn animation(&self) -> &CameraAnimation {
        &self.anim
    }

    pub fn target(&self) -> &Camera {
        self.anim.target()
    }

    /// Samples the animation at the current executor timestamp.
    pub fn step(&self, viewport: Bounds<Pixels>, cx: &App) -> (Camera, bool) {
        let elapsed = (self.elapsed)(cx.background_executor());
        self.anim.sample(elapsed, viewport)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::SheetCanvas;
    use crate::overlay::OverlayView;
    use gpui::{
        AppContext, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, PinchEvent,
        ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, VisualTestContext, px, size,
    };
    use omnia_doc::{GRID_NM, Label, Orient, Part, Wire};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::GRAPHITE;
    use proptest::prelude::*;

    fn viewport() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(12.0), px(24.0)),
            size: size(px(1200.0), px(800.0)),
        }
    }

    fn sample_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let cap = lib.by_name("Capacitor-IEEE-NonPolarized").unwrap();
        let res_def = lib.get(res);
        let cap_def = lib.get(cap);
        let g = GRID_NM;

        let parts = [
            (
                Part {
                    symbol: res,
                    at: Pt {
                        x: 20 * g,
                        y: -10 * g,
                    },
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                res_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                res_def.bounds,
            ),
            (
                Part {
                    symbol: cap,
                    at: Pt {
                        x: 40 * g,
                        y: 10 * g,
                    },
                    orient: Orient::IDENTITY,
                    designator: "C1".into(),
                    value: "100n".into(),
                },
                cap_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                cap_def.bounds,
            ),
        ];
        let wires = [Wire {
            a: Pt {
                x: 22 * g,
                y: -10 * g,
            },
            b: Pt {
                x: 38 * g,
                y: -10 * g,
            },
        }];
        let labels = [Label {
            at: Pt {
                x: 30 * g,
                y: -12 * g,
            },
            text: "SIG".into(),
        }];
        Doc::from_items(parts, wires, labels).unwrap()
    }

    #[test]
    fn ease_out_endpoints_and_monotonic_deceleration() {
        assert_eq!(ease_out(-0.5), 0.0);
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(1.5), 1.0);

        let mut prev = 0.0;
        let mut prev_delta = f64::INFINITY;
        for step in 1..=10 {
            let t = f64::from(step) / 10.0;
            let u = ease_out(t);
            let delta = u - prev;
            assert!(u > prev, "must strictly increase at step {step}");
            assert!(
                delta < prev_delta,
                "ease-out step size must decrease: {delta} >= {prev_delta}"
            );
            prev = u;
            prev_delta = delta;
        }
    }

    #[test]
    fn fit_rect_centers_bounds_and_respects_margin() {
        let vp = viewport();
        let doc = sample_doc();
        let bounds = doc_bounds(&doc).unwrap();
        let cam = fit_rect_camera(bounds, vp);

        let min_s = cam.world_to_screen(bounds.min);
        let max_s = cam.world_to_screen(bounds.max);
        let center_s = viewport_center(vp);
        assert!(((min_s.x + max_s.x) * 0.5 - center_s.x).abs() < 1e-6);
        assert!(((min_s.y + max_s.y) * 0.5 - center_s.y).abs() < 1e-6);

        let margin_x = f64::from(vp.size.width) * (1.0 - FIT_FILL_FRACTION) * 0.5;
        let margin_y = f64::from(vp.size.height) * (1.0 - FIT_FILL_FRACTION) * 0.5;
        assert!(min_s.x >= f64::from(vp.origin.x) + margin_x - 1e-6);
        assert!(max_s.x <= f64::from(vp.origin.x) + f64::from(vp.size.width) - margin_x + 1e-6);
        assert!(min_s.y >= f64::from(vp.origin.y) + margin_y - 1e-6);
        assert!(max_s.y <= f64::from(vp.origin.y) + f64::from(vp.size.height) - margin_y + 1e-6);
    }

    #[test]
    fn fit_sheet_empty_and_selection_bounds_handle_edge_cases() {
        let vp = viewport();
        let empty = Doc::new();
        let empty_cam = fit_sheet_camera(Some(&empty), vp);
        assert_eq!(empty_cam.zoom(), 1.0);
        assert_eq!(empty_cam.world_to_screen(Pt::ZERO), viewport_center(vp));
        assert!(zoom_to_selection_camera(&empty, &[], vp).is_none());

        // Single-axis wire and single-point label.
        let doc = sample_doc();
        let wire_cam = zoom_to_selection_camera(&doc, &[ItemId::Wire(omnia_doc::WireId(0))], vp)
            .expect("wire selected");
        assert!(wire_cam.zoom() > 1.0 && wire_cam.zoom() <= MAX_ZOOM);

        let label_cam = zoom_to_selection_camera(&doc, &[ItemId::Label(omnia_doc::LabelId(0))], vp)
            .expect("label selected");
        assert_eq!(label_cam.zoom(), 1.0);
        let label_pos = doc.label(omnia_doc::LabelId(0)).unwrap().at;
        assert_eq!(label_cam.world_to_screen(label_pos), viewport_center(vp));

        // Sheet-limit extreme rect must not overflow i32 in midpoint computation.
        let extreme = Rect::new(
            Pt {
                x: -SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
            Pt {
                x: SHEET_LIMIT_NM,
                y: SHEET_LIMIT_NM,
            },
        )
        .unwrap();
        let extreme_cam = fit_rect_camera(extreme, vp);
        assert_eq!(extreme_cam.zoom(), MIN_ZOOM);
        assert_eq!(extreme_cam.world_to_screen(Pt::ZERO), viewport_center(vp));
    }

    #[test]
    fn animation_samples_smoothly_and_ends_exactly_on_target() {
        let vp = viewport();
        let mut start = Camera::new(Pt::ZERO, 1.0, vp);
        start.pan_by(point(43.25, -17.5));
        let doc = sample_doc();
        let target = fit_sheet_camera(Some(&doc), vp);
        let anim = CameraAnimation::new(start.clone(), target.clone(), vp);
        assert!(!anim.is_noop());

        let (cam_0, done_0) = anim.sample(Duration::ZERO, vp);
        assert!(!done_0);
        assert_eq!(cam_0.zoom(), start.zoom());
        assert_eq!(
            cam_0.world_to_screen(Pt::ZERO),
            start.world_to_screen(Pt::ZERO)
        );

        // Verify straight-line screen convergence of target center (no swoop) and monotonic zoom.
        let target_center_world = target.screen_to_world(viewport_center(vp));
        let center_s = viewport_center(vp);
        let mut prev_dist = f64::INFINITY;
        let mut prev_zoom = start.zoom();
        for ms in [15, 30, 50, 75, 100, 125, 149] {
            let (cam, done) = anim.sample(Duration::from_millis(ms), vp);
            assert!(!done);
            let zoom = cam.zoom();
            if target.zoom() > start.zoom() {
                assert!(zoom > prev_zoom);
            } else {
                assert!(zoom < prev_zoom);
            }
            prev_zoom = zoom;
            let p = cam.world_to_screen(target_center_world);
            let dist = ((p.x - center_s.x).powi(2) + (p.y - center_s.y).powi(2)).sqrt();
            assert!(
                dist < prev_dist,
                "distance to target center grew at {ms} ms"
            );
            prev_dist = dist;
        }

        // At and past 150 ms: exact bit-for-bit target.
        for ms in [150, 200, 1000] {
            let (cam, done) = anim.sample(Duration::from_millis(ms), vp);
            assert!(done);
            assert_eq!(cam.zoom(), target.zoom());
            assert_eq!(
                cam.world_to_screen(Pt::ZERO),
                target.world_to_screen(Pt::ZERO)
            );
            assert_eq!(cam.visible(), target.visible());
        }
    }

    fn pump_frame(cx: &mut VisualTestContext, step: Duration) -> usize {
        cx.background_executor.advance_clock(step);
        let count = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        count
    }

    fn open_sheet_canvas(
        cx: &mut TestAppContext,
        doc: Option<Doc>,
    ) -> (gpui::Entity<SheetCanvas>, &mut VisualTestContext) {
        let doc_entity = doc.map(|d| cx.new(|_| d));
        cx.add_window_view(move |_, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let mut canvas = SheetCanvas::new(camera, &GRAPHITE, cx);
            if let Some(doc) = doc_entity {
                canvas = canvas.with_doc(doc);
            }
            canvas
        })
    }

    #[gpui::test]
    fn sheet_canvas_fit_animates_to_exact_target_and_stops_idling(cx: &mut TestAppContext) {
        let doc = sample_doc();
        let (canvas, cx) = open_sheet_canvas(cx, Some(doc));

        let expected_target = cx.update(|_, cx| {
            let c = canvas.read(cx);
            let doc = c.doc().unwrap().read(cx);
            fit_sheet_camera(Some(doc), c.viewport())
        });

        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| c.fit_sheet(window, cx));
        });
        cx.run_until_parked();
        assert!(cx.update(|_, cx| canvas.read(cx).is_animating()));

        // Step across 150 ms in ~16 ms frame intervals.
        let mut frames = 0usize;
        for _ in 0..9 {
            let scheduled = pump_frame(cx, Duration::from_millis(16));
            assert_eq!(
                scheduled, 1,
                "animation must request next frame while active"
            );
            frames += 1;
            assert!(cx.update(|_, cx| canvas.read(cx).is_animating()));
        }
        assert_eq!(frames, 9);

        // Next frame crosses 150 ms (10 * 16 = 160 ms) -> lands on exact target and stops.
        let scheduled = pump_frame(cx, Duration::from_millis(16));
        assert_eq!(scheduled, 1);
        assert!(!cx.update(|_, cx| canvas.read(cx).is_animating()));

        cx.update(|_, cx| {
            let cam = canvas.read(cx).camera();
            assert_eq!(cam.zoom(), expected_target.zoom());
            assert_eq!(
                cam.world_to_screen(Pt::ZERO),
                expected_target.world_to_screen(Pt::ZERO)
            );
        });

        // Immediately after completion, 0 animation frames are requested.
        let renders_after = cx.update(|_, cx| canvas.read(cx).render_count());
        assert_eq!(pump_frame(cx, Duration::from_millis(16)), 0);
        // One full second later: still 0 frames (B8).
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).render_count()),
            renders_after
        );
    }

    #[gpui::test]
    fn pan_and_zoom_inputs_cancel_animation_with_no_jump(cx: &mut TestAppContext) {
        let (canvas, cx) = open_sheet_canvas(cx, Some(sample_doc()));

        // Case 1: wheel zoom mid-way cancels immediately from current position with no jump.
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| c.fit_sheet(window, cx));
        });
        cx.run_until_parked();
        assert_eq!(pump_frame(cx, Duration::from_millis(50)), 1);
        assert!(cx.update(|_, cx| canvas.read(cx).is_animating()));

        let pos = point(px(500.0), px(350.0));
        let before_world = cx.update(|_, cx| {
            canvas
                .read(cx)
                .camera()
                .screen_to_world(point(f64::from(pos.x), f64::from(pos.y)))
        });
        let before_zoom = cx.update(|_, cx| canvas.read(cx).camera().zoom());

        cx.simulate_event(ScrollEvent {
            position: pos,
            delta: ScrollDelta::Lines(point(0.0, 3.0)),
            ..Default::default()
        });
        assert!(!cx.update(|_, cx| canvas.read(cx).is_animating()));

        let after_world = cx.update(|_, cx| {
            canvas
                .read(cx)
                .camera()
                .screen_to_world(point(f64::from(pos.x), f64::from(pos.y)))
        });
        let expected_zoom =
            (before_zoom * crate::input::wheel_factor(1.0)).clamp(MIN_ZOOM, MAX_ZOOM);
        let after_zoom = cx.update(|_, cx| canvas.read(cx).camera().zoom());
        assert!(
            (after_zoom / expected_zoom - 1.0).abs() < 1e-12,
            "wheel zoom must apply from current mid-animation zoom, with no jump"
        );
        assert!(
            (i64::from(before_world.x) - i64::from(after_world.x)).abs() <= 1
                && (i64::from(before_world.y) - i64::from(after_world.y)).abs() <= 1,
            "cursor anchor must stay fixed at cancelled position"
        );

        // Drain the single in-flight callback scheduled before cancel; thereafter 0 frames.
        let _ = pump_frame(cx, Duration::from_millis(16));
        let renders_at_rest = cx.update(|_, cx| canvas.read(cx).render_count());
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).render_count()),
            renders_at_rest
        );
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).camera().zoom()),
            after_zoom
        );

        // Case 2: touchpad pixel scroll mid-way cancels and pans cleanly.
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| c.zoom_step(2, window, cx));
        });
        cx.run_until_parked();
        assert_eq!(pump_frame(cx, Duration::from_millis(40)), 1);
        let mid_pt = cx.update(|_, cx| canvas.read(cx).camera().world_to_screen(Pt::ZERO));
        let mid_zoom = cx.update(|_, cx| canvas.read(cx).camera().zoom());

        cx.simulate_event(ScrollEvent {
            position: pos,
            delta: ScrollDelta::Pixels(point(px(0.0), px(0.0))),
            touch_phase: TouchPhase::Started,
            ..Default::default()
        });
        assert!(!cx.update(|_, cx| canvas.read(cx).is_animating()));
        cx.simulate_event(ScrollEvent {
            position: pos,
            delta: ScrollDelta::Pixels(point(px(12.5), px(-7.25))),
            touch_phase: TouchPhase::Moved,
            ..Default::default()
        });
        cx.simulate_event(ScrollEvent {
            position: pos,
            delta: ScrollDelta::Pixels(point(px(0.0), px(0.0))),
            touch_phase: TouchPhase::Ended,
            ..Default::default()
        });
        let panned_pt = cx.update(|_, cx| canvas.read(cx).camera().world_to_screen(Pt::ZERO));
        assert!((panned_pt.x - (mid_pt.x + 12.5)).abs() < 1e-11);
        assert!((panned_pt.y - (mid_pt.y - 7.25)).abs() < 1e-11);
        assert_eq!(cx.update(|_, cx| canvas.read(cx).camera().zoom()), mid_zoom);
        let _ = pump_frame(cx, Duration::from_millis(16));
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);

        // Case 3: pinch mid-way cancels and zooms from current position.
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| c.fit_sheet(window, cx));
        });
        cx.run_until_parked();
        assert_eq!(pump_frame(cx, Duration::from_millis(40)), 1);
        let mid_zoom = cx.update(|_, cx| canvas.read(cx).camera().zoom());
        cx.simulate_event(PinchEvent {
            position: pos,
            delta: 0.0,
            modifiers: Modifiers::none(),
            phase: TouchPhase::Started,
        });
        assert!(!cx.update(|_, cx| canvas.read(cx).is_animating()));
        cx.simulate_event(PinchEvent {
            position: pos,
            delta: 0.25,
            modifiers: Modifiers::none(),
            phase: TouchPhase::Moved,
        });
        cx.simulate_event(PinchEvent {
            position: pos,
            delta: 0.0,
            modifiers: Modifiers::none(),
            phase: TouchPhase::Ended,
        });
        assert!(
            (cx.update(|_, cx| canvas.read(cx).camera().zoom()) / (mid_zoom * 1.25) - 1.0).abs()
                < 1e-12
        );
        let _ = pump_frame(cx, Duration::from_millis(16));
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);

        // Case 4: middle-drag press mid-way cancels immediately before any mouse move.
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| c.zoom_step(-2, window, cx));
        });
        cx.run_until_parked();
        assert_eq!(pump_frame(cx, Duration::from_millis(40)), 1);
        let frozen_pt = cx.update(|_, cx| canvas.read(cx).camera().world_to_screen(Pt::ZERO));
        let frozen_zoom = cx.update(|_, cx| canvas.read(cx).camera().zoom());
        cx.simulate_mouse_down(pos, MouseButton::Middle, Modifiers::none());
        assert!(!cx.update(|_, cx| canvas.read(cx).is_animating()));
        cx.simulate_mouse_move(
            point(px(520.0), px(330.0)),
            MouseButton::Middle,
            Modifiers::none(),
        );
        cx.simulate_mouse_up(
            point(px(520.0), px(330.0)),
            MouseButton::Middle,
            Modifiers::none(),
        );
        let dragged_pt = cx.update(|_, cx| canvas.read(cx).camera().world_to_screen(Pt::ZERO));
        assert!((dragged_pt.x - (frozen_pt.x + 20.0)).abs() < 1e-11);
        assert!((dragged_pt.y - (frozen_pt.y - 20.0)).abs() < 1e-11);
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).camera().zoom()),
            frozen_zoom
        );
        let _ = pump_frame(cx, Duration::from_millis(16));
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);

        // Case 5: Space + left-drag mid-way cancels immediately from current position.
        // Click once to focus the sheet, then start a Space+left drag during an animation.
        cx.simulate_click(pos, Modifiers::none());
        cx.update(|window, cx| {
            canvas.update(cx, |c, cx| c.fit_sheet(window, cx));
        });
        cx.run_until_parked();
        assert_eq!(pump_frame(cx, Duration::from_millis(40)), 1);
        let space = Keystroke::parse("space").unwrap();
        cx.simulate_event(KeyDownEvent {
            keystroke: space.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        let space_pt = cx.update(|_, cx| canvas.read(cx).camera().world_to_screen(Pt::ZERO));
        let space_zoom = cx.update(|_, cx| canvas.read(cx).camera().zoom());
        cx.simulate_mouse_down(pos, MouseButton::Left, Modifiers::none());
        assert!(!cx.update(|_, cx| canvas.read(cx).is_animating()));
        cx.simulate_mouse_move(
            point(px(485.0), px(365.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_up(
            point(px(485.0), px(365.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_event(KeyUpEvent { keystroke: space });
        let space_after = cx.update(|_, cx| canvas.read(cx).camera().world_to_screen(Pt::ZERO));
        assert!((space_after.x - (space_pt.x - 15.0)).abs() < 1e-11);
        assert!((space_after.y - (space_pt.y + 15.0)).abs() < 1e-11);
        assert_eq!(
            cx.update(|_, cx| canvas.read(cx).camera().zoom()),
            space_zoom
        );
        let _ = pump_frame(cx, Duration::from_millis(16));
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);
    }

    #[gpui::test]
    fn overlay_view_fit_and_zoom_to_selection_animate_cached_sheet_and_stop(
        cx: &mut TestAppContext,
    ) {
        let doc = cx.new(|_| sample_doc());
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc_clone, camera, &GRAPHITE, cx);
            window.focus(view.focus_handle(), cx);
            view
        });

        // Select R1 only, zoom to selection.
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| {
                v.set_selection([ItemId::Part(omnia_doc::PartId(0))], cx);
            });
        });
        cx.run_until_parked();

        let expected = cx.update(|_, cx| {
            let v = overlay.read(cx);
            zoom_to_selection_camera(v.doc().read(cx), v.selection(), v.viewport()).unwrap()
        });

        let paints_before = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        cx.update(|window, cx| {
            let started = overlay.update(cx, |v, cx| v.zoom_to_selection(window, cx));
            assert!(started);
        });
        cx.run_until_parked();
        assert!(cx.update(|_, cx| overlay.read(cx).is_animating()));

        for _ in 0..9 {
            assert_eq!(pump_frame(cx, Duration::from_millis(16)), 2);
        }
        // Final frame at 160 ms >= 150 ms completes and stops requesting frames.
        assert_eq!(pump_frame(cx, Duration::from_millis(16)), 2);
        assert!(!cx.update(|_, cx| overlay.read(cx).is_animating()));
        assert!(cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)) > paints_before);

        cx.update(|_, cx| {
            let cam = overlay.read(cx).camera();
            assert_eq!(cam.zoom(), expected.zoom());
            assert_eq!(
                cam.world_to_screen(Pt::ZERO),
                expected.world_to_screen(Pt::ZERO)
            );
        });

        let paints_done = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let overlay_paints_done = cx.update(|_, cx| overlay.read(cx).overlay_paint_count());
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints_done
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).overlay_paint_count()),
            overlay_paints_done
        );

        // Empty selection zoom_to_selection is a no-op (0 frames, 0 repaints).
        cx.update(|window, cx| {
            overlay.update(cx, |v, cx| {
                v.clear_selection(cx);
                assert!(!v.zoom_to_selection(window, cx));
            });
        });
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| overlay.read(cx).is_animating()));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints_done
        );
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);

        // External set_camera cancels a running OverlayView animation cleanly.
        cx.update(|window, cx| {
            overlay.update(cx, |v, cx| v.fit_sheet(window, cx));
        });
        cx.run_until_parked();
        assert_eq!(pump_frame(cx, Duration::from_millis(40)), 2);
        assert!(cx.update(|_, cx| overlay.read(cx).is_animating()));
        let override_cam = Camera::new(Pt { x: GRID_NM, y: 0 }, 2.0, viewport());
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.set_camera(override_cam.clone(), cx));
        });
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| overlay.read(cx).is_animating()));
        let _ = pump_frame(cx, Duration::from_millis(16));
        assert_eq!(pump_frame(cx, Duration::from_secs(1)), 0);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).camera().zoom()),
            override_cam.zoom()
        );
    }

    proptest! {
        #[test]
        fn animation_always_lands_on_exact_target_across_sheet(
            cx0 in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            cy0 in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            z0 in MIN_ZOOM..=MAX_ZOOM,
            cx1 in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            cy1 in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM,
            z1 in MIN_ZOOM..=MAX_ZOOM,
            elapsed_ms in 0u64..=300,
        ) {
            let vp = viewport();
            let start = Camera::new(Pt { x: cx0, y: cy0 }, z0, vp);
            let target = Camera::new(Pt { x: cx1, y: cy1 }, z1, vp);
            let anim = CameraAnimation::new(start, target.clone(), vp);
            let (cam, done) = anim.sample(Duration::from_millis(elapsed_ms), vp);
            prop_assert!((MIN_ZOOM..=MAX_ZOOM).contains(&cam.zoom()));
            prop_assert!(cam.world_to_screen(Pt::ZERO).x.is_finite());
            prop_assert!(cam.world_to_screen(Pt::ZERO).y.is_finite());
            if elapsed_ms >= 150 {
                prop_assert!(done);
                prop_assert_eq!(cam.zoom(), target.zoom());
                prop_assert_eq!(cam.world_to_screen(Pt::ZERO), target.world_to_screen(Pt::ZERO));
            } else {
                prop_assert!(!done);
            }
        }
    }
}
