//! T1.5: pan and zoom input, and the status the canvas publishes.
//!
//! Input mapping (G9, G10, T0.5):
//!
//! - `ScrollDelta::Lines` is a mouse wheel: zoom about the cursor, one notch ([`LINES_PER_NOTCH`]
//!   lines) multiplying the zoom by 2^(1/4) (K4). Merged notches (T0.5 saw -6 lines) zoom by
//!   whole steps; fractional lines from high-resolution wheels zoom by the matching fraction.
//! - `ScrollDelta::Pixels` is a touchpad (or touch) pan: the sheet follows the fingers by
//!   exactly the delta.
//! - `PinchEvent` zooms about the pinch position by `1 + delta` per event. A touchpad gesture
//!   is either a pan or a pinch, never both (G10), so nothing here mixes them.
//! - Middle-drag, and left-drag while Space is held, pan by the pointer movement. The drag
//!   captures the pointer, so it keeps panning when the cursor leaves the sheet.
//! - A gesture that ends `Cancelled` puts the camera back where the gesture started.
//!
//! [`Navigator`] holds this as a pure state machine over a [`Camera`], so it is tested
//! without a window. [`SheetCanvas`] is the entity that owns the camera, renders
//! [`SheetView`] and feeds it events. It is notified only when the camera changes (D7);
//! cursor movement only emits [`CanvasEvent::Status`], which the status bar view observes.

use crate::camera::Camera;
use crate::camera_anim::{
    CameraAnimation, ViewAnimation, fit_rect_camera, fit_sheet_camera, viewport_center,
    zoom_to_selection_camera,
};
use crate::sheet::{SheetDocument, SheetView, grid_level};
use gpui::{
    App, Bounds, Context, DispatchPhase, Element, ElementId, Entity, EventEmitter, FocusHandle,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, KeyDownEvent,
    KeyUpEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement,
    PinchEvent, Pixels, Point, Render, ScrollDelta, ScrollEvent, ScrollSource, Styled, TouchPhase,
    WeakEntity, Window, div, point, prelude::*,
};
use omnia_doc::{Doc, ItemId, Pt, Rect};
use omnia_theme::Theme;

/// Lines per wheel notch: Windows' default `SPI_GETWHEELSCROLLLINES`, which GPUI multiplies
/// into `ScrollDelta::Lines` (`gpui_windows/src/events.rs`); T0.5 logged ±3 per notch.
pub const LINES_PER_NOTCH: f64 = 3.0;

/// K4: the zoom factor for `notches` wheel notches, 2^(notches / 4). Positive zooms in.
pub fn wheel_factor(notches: f64) -> f64 {
    (notches / 4.0).exp2()
}

/// What the status bar shows (plan section 9).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasStatus {
    /// World point under the pointer, or `None` before the pointer has been over the sheet.
    pub cursor: Option<Pt>,
    /// The grid step currently drawn (K10), in nm.
    pub grid: i32,
    /// Zoom multiplier (1 = 100%).
    pub zoom: f32,
}

impl CanvasStatus {
    pub fn new(camera: &Camera, cursor: Option<Pt>) -> Self {
        Self {
            cursor,
            grid: grid_level(camera.scale()).step_nm,
            zoom: camera.zoom() as f32,
        }
    }
}

/// Events [`SheetCanvas`] emits. Selection arrives with T4.3.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasEvent {
    Status(CanvasStatus),
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    button: MouseButton,
    last: Point<f64>,
}

/// Pan and zoom state machine. Every method returns whether the camera changed.
#[derive(Clone, Debug, Default)]
pub struct Navigator {
    space_held: bool,
    drag: Option<Drag>,
    /// The camera when the current touch gesture started, restored if it is cancelled.
    gesture_start: Option<Camera>,
}

fn to_f64(p: Point<Pixels>) -> Point<f64> {
    point(f64::from(p.x), f64::from(p.y))
}

fn zoom_about(camera: &mut Camera, anchor: Point<f64>, factor: f64) -> bool {
    let before = camera.zoom();
    camera.zoom_about(anchor, factor);
    camera.zoom() != before
}

impl Navigator {
    pub fn space_held(&self) -> bool {
        self.space_held
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Space down or up. Releasing Space does not end a drag already under way.
    pub fn set_space(&mut self, held: bool) {
        self.space_held = held;
    }

    fn gesture_phase(&mut self, camera: &mut Camera, phase: TouchPhase) -> Option<bool> {
        match phase {
            TouchPhase::Started => {
                self.gesture_start = Some(camera.clone());
                None
            }
            TouchPhase::Moved => None,
            TouchPhase::Ended => {
                self.gesture_start = None;
                None
            }
            TouchPhase::Cancelled => {
                let changed = match self.gesture_start.take() {
                    Some(start) => {
                        let changed = start.zoom() != camera.zoom()
                            || start.world_to_screen(Pt::ZERO) != camera.world_to_screen(Pt::ZERO);
                        *camera = start;
                        changed
                    }
                    None => false,
                };
                Some(changed)
            }
        }
    }

    pub fn scroll(&mut self, camera: &mut Camera, event: &ScrollEvent) -> bool {
        match event.delta {
            ScrollDelta::Lines(lines) => {
                let notches = f64::from(lines.y) / LINES_PER_NOTCH;
                notches != 0.0 && zoom_about(camera, to_f64(event.position), wheel_factor(notches))
            }
            ScrollDelta::Pixels(delta) => {
                if let Some(changed) = self.gesture_phase(camera, event.touch_phase) {
                    return changed;
                }
                let delta = to_f64(delta);
                if delta.x == 0.0 && delta.y == 0.0 || !delta.x.is_finite() || !delta.y.is_finite()
                {
                    return false;
                }
                camera.pan_by(delta);
                true
            }
        }
    }

    pub fn pinch(&mut self, camera: &mut Camera, event: &PinchEvent) -> bool {
        if let Some(changed) = self.gesture_phase(camera, event.phase) {
            return changed;
        }
        let factor = 1.0 + f64::from(event.delta);
        factor != 1.0 && zoom_about(camera, to_f64(event.position), factor)
    }

    /// Starts a pan for a middle press, or a left press while Space is held. Returns
    /// whether a pan started; the camera does not move until the pointer does.
    pub fn mouse_down(&mut self, event: &MouseDownEvent) -> bool {
        let pans = match event.button {
            MouseButton::Middle => true,
            MouseButton::Left => self.space_held,
            _ => false,
        };
        if !pans || self.drag.is_some() {
            return false;
        }
        self.drag = Some(Drag {
            button: event.button,
            last: to_f64(event.position),
        });
        true
    }

    pub fn mouse_move(&mut self, camera: &mut Camera, event: &MouseMoveEvent) -> bool {
        let Some(drag) = &mut self.drag else {
            return false;
        };
        if event.pressed_button.is_none() {
            // The release was not delivered to us; stop rather than pan with no button held.
            self.drag = None;
            return false;
        }
        let position = to_f64(event.position);
        let delta = point(position.x - drag.last.x, position.y - drag.last.y);
        drag.last = position;
        if delta.x == 0.0 && delta.y == 0.0 {
            return false;
        }
        camera.pan_by(delta);
        true
    }

    /// Ends the pan if `button` started it. Returns whether a pan ended.
    pub fn mouse_up(&mut self, event: &MouseUpEvent) -> bool {
        if self.drag.is_some_and(|drag| drag.button == event.button) {
            self.drag = None;
            true
        } else {
            false
        }
    }
}

/// The sheet canvas entity: camera, input, and the [`SheetView`] element. Embed it with
/// `.cached(..)` (D7, G6) so status-only updates never repaint the sheet.
pub struct SheetCanvas {
    camera: Camera,
    theme: &'static Theme,
    doc: Option<Entity<Doc>>,
    sheet_doc: Option<SheetDocument>,
    nav: Navigator,
    focus: FocusHandle,
    /// Last pointer position over the sheet, window logical px.
    pointer: Option<Point<f64>>,
    status: CanvasStatus,
    viewport: Bounds<Pixels>,
    anim: Option<ViewAnimation>,
    renders: u64,
}

impl EventEmitter<CanvasEvent> for SheetCanvas {}

impl SheetCanvas {
    /// The camera's viewport is replaced by the element's bounds at prepaint and on every
    /// event.
    pub fn new(camera: Camera, theme: &'static Theme, cx: &mut Context<Self>) -> Self {
        Self {
            status: CanvasStatus::new(&camera, None),
            camera,
            theme,
            doc: None,
            sheet_doc: None,
            nav: Navigator::default(),
            focus: cx.focus_handle(),
            pointer: None,
            viewport: Bounds::default(),
            anim: None,
            renders: 0,
        }
    }

    /// Attaches a document entity to this canvas for rendering and `fit_sheet` (T4.5).
    pub fn with_doc(mut self, doc: Entity<Doc>) -> Self {
        self.sheet_doc = Some(SheetDocument::new(doc.clone()));
        self.doc = Some(doc);
        self
    }

    pub fn doc(&self) -> Option<&Entity<Doc>> {
        self.doc.as_ref()
    }

    /// Replaces or clears the attached document, cancelling any active view animation.
    pub fn set_doc(&mut self, doc: Option<Entity<Doc>>, cx: &mut Context<Self>) {
        self.anim = None;
        self.sheet_doc = doc.as_ref().map(|d| SheetDocument::new(d.clone()));
        self.doc = doc;
        cx.notify();
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn viewport(&self) -> Bounds<Pixels> {
        self.viewport
    }

    pub fn status(&self) -> CanvasStatus {
        self.status
    }

    pub fn navigator(&self) -> &Navigator {
        &self.nav
    }

    pub fn is_animating(&self) -> bool {
        self.anim.is_some()
    }

    /// Cancels any active view animation immediately from the current camera position (K13).
    pub fn cancel_animation(&mut self) -> bool {
        self.anim.take().is_some()
    }

    /// How many times this view has rendered (tests check D7 with it).
    pub fn render_count(&self) -> u64 {
        self.renders
    }

    fn emit_status_if_changed(&mut self, cx: &mut Context<Self>) {
        let cursor = self.pointer.map(|p| self.camera.screen_to_world(p));
        let status = CanvasStatus::new(&self.camera, cursor);
        if status != self.status {
            self.status = status;
            cx.emit(CanvasEvent::Status(status));
        }
    }

    /// Starts a 150 ms ease-out view animation (K13) from the current camera to `target`.
    pub fn animate_to(&mut self, target: Camera, cx: &mut Context<Self>) {
        let anim = CameraAnimation::new(self.camera.clone(), target, self.viewport);
        if anim.is_noop() {
            self.anim = None;
            return;
        }
        self.anim = Some(ViewAnimation::start(anim, cx));
        cx.notify();
    }

    /// Animates the camera to fit `rect` in the current viewport with default margins.
    pub fn fit_rect(&mut self, rect: Rect, cx: &mut Context<Self>) {
        self.animate_to(fit_rect_camera(rect, self.viewport), cx);
    }

    /// Animates the camera to fit all items in `doc` (or origin at 100% if empty).
    pub fn fit_doc(&mut self, doc: &Doc, cx: &mut Context<Self>) {
        self.animate_to(fit_sheet_camera(Some(doc), self.viewport), cx);
    }

    /// Animates the camera to fit the attached document (or origin at 100% if none/empty).
    pub fn fit_sheet(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let target = fit_sheet_camera(self.doc.as_ref().map(|d| d.read(cx)), self.viewport);
        self.animate_to(target, cx);
    }

    /// Animates the camera to fit `selection` in `doc`. Returns `false` if nothing live is selected.
    pub fn zoom_to_selection_in(
        &mut self,
        doc: &Doc,
        selection: &[ItemId],
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = zoom_to_selection_camera(doc, selection, self.viewport) else {
            return false;
        };
        self.animate_to(target, cx);
        true
    }

    /// Animates the camera to fit the current selection when selection is available on the canvas.
    pub fn zoom_to_selection(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    /// Animates a K4 zoom of `steps` notches about the viewport centre (`ui-plan.md` section 14).
    pub fn zoom_step(&mut self, steps: i32, _window: &mut Window, cx: &mut Context<Self>) {
        if steps == 0 {
            return;
        }
        let mut target = self
            .anim
            .as_ref()
            .map_or_else(|| self.camera.clone(), |a| a.target().clone());
        target.set_viewport(self.viewport);
        target.zoom_about(
            viewport_center(self.viewport),
            wheel_factor(f64::from(steps)),
        );
        self.animate_to(target, cx);
    }

    /// Runs one input step: notifies (repaints the sheet) only if the camera changed, and
    /// emits a status only if the status changed.
    fn step<R>(
        &mut self,
        viewport: Bounds<Pixels>,
        pointer: Option<Point<f64>>,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Navigator, &mut Camera, &mut Option<ViewAnimation>) -> (bool, R),
    ) -> R {
        self.viewport = viewport;
        self.camera.set_viewport(viewport);
        if pointer.is_some() {
            self.pointer = pointer;
        }
        let (changed, result) = f(&mut self.nav, &mut self.camera, &mut self.anim);
        if changed {
            cx.notify();
        }
        self.emit_status_if_changed(cx);
        result
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        viewport: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.step(
            viewport,
            Some(to_f64(event.position)),
            cx,
            |nav, _, anim| {
                let started = nav.mouse_down(event);
                if started {
                    *anim = None;
                }
                (false, started)
            },
        )
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        viewport: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.step(
            viewport,
            Some(to_f64(event.position)),
            cx,
            |nav, camera, anim| {
                let panned = nav.mouse_move(camera, event);
                if panned {
                    *anim = None;
                }
                (panned, ())
            },
        )
    }

    /// Ending a pan moves nothing, so it neither notifies nor emits.
    fn on_mouse_up(&mut self, event: &MouseUpEvent) -> bool {
        self.nav.mouse_up(event)
    }

    fn on_scroll(&mut self, event: &ScrollEvent, viewport: Bounds<Pixels>, cx: &mut Context<Self>) {
        // A touch pan reports where it started, not the pointer (G9).
        let pointer = (event.source == ScrollSource::Pointer).then(|| to_f64(event.position));
        self.step(viewport, pointer, cx, |nav, camera, anim| {
            match event.delta {
                ScrollDelta::Lines(lines) if lines.y != 0.0 => *anim = None,
                ScrollDelta::Pixels(delta) => {
                    let d = to_f64(delta);
                    if event.touch_phase == TouchPhase::Started
                        || (d.x.is_finite() && d.y.is_finite() && (d.x != 0.0 || d.y != 0.0))
                    {
                        *anim = None;
                    }
                }
                _ => {}
            }
            (nav.scroll(camera, event), ())
        })
    }

    fn on_pinch(&mut self, event: &PinchEvent, viewport: Bounds<Pixels>, cx: &mut Context<Self>) {
        self.step(
            viewport,
            Some(to_f64(event.position)),
            cx,
            |nav, camera, anim| {
                if event.phase == TouchPhase::Started
                    || (event.phase != TouchPhase::Cancelled
                        && event.phase != TouchPhase::Ended
                        && event.delta != 0.0)
                {
                    *anim = None;
                }
                (nav.pinch(camera, event), ())
            },
        )
    }
}

impl Render for SheetCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        if let Some(anim) = &self.anim {
            let (next_cam, finished) = anim.step(self.viewport, cx);
            self.camera = next_cam;
            self.emit_status_if_changed(cx);
            if finished {
                self.anim = None;
            } else {
                window.request_animation_frame();
            }
        }
        let sheet = match &self.sheet_doc {
            Some(document) => {
                SheetView::with_document(self.camera.clone(), self.theme, document.clone())
            }
            None => SheetView::new(self.camera.clone(), self.theme),
        };
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "space" {
                    this.nav.set_space(true);
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" {
                    this.nav.set_space(false);
                    cx.stop_propagation();
                }
            }))
            .child(CanvasInput {
                sheet,
                canvas: cx.entity().downgrade(),
                focus: self.focus.clone(),
                dragging: self.nav.is_dragging(),
            })
    }
}

/// [`SheetView`] plus one hitbox and window-level mouse listeners (D7: one hitbox for the
/// whole sheet). Window-level listeners let a captured drag follow the pointer anywhere.
struct CanvasInput {
    sheet: SheetView,
    canvas: WeakEntity<SheetCanvas>,
    focus: FocusHandle,
    /// A pan is under way: hold the pointer capture on this frame's hitbox.
    dragging: bool,
}

impl Element for CanvasInput {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.sheet.request_layout(id, inspector_id, window, cx)
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
        // Keep the entity's camera on the painted bounds, so `camera()` maps what is shown.
        // No notify: the bounds are already the ones being painted.
        self.canvas
            .update(cx, |canvas, _| {
                canvas.viewport = bounds;
                canvas.camera.set_viewport(bounds);
            })
            .ok();
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.sheet.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            &mut (),
            window,
            cx,
        );
        if self.dragging {
            // Hitbox ids are never reused, so a pan step that repaints the sheet must move
            // the capture to the new hitbox or the drag stops at the sheet's edge. A cached
            // replay keeps the old hitbox and its id, so it needs nothing.
            window.capture_pointer(hitbox.id);
        }

        let (canvas, hit, focus) = (self.canvas.clone(), hitbox.clone(), self.focus.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hit.is_hovered(window) {
                return;
            }
            window.focus(&focus, cx);
            let started = canvas
                .update(cx, |canvas, cx| canvas.on_mouse_down(event, bounds, cx))
                .unwrap_or(false);
            if started {
                window.capture_pointer(hit.id);
                cx.stop_propagation();
            }
        });

        let (canvas, hit) = (self.canvas.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            // While a pan holds the capture, `is_hovered` is true wherever the pointer is.
            if phase == DispatchPhase::Bubble && hit.is_hovered(window) {
                canvas
                    .update(cx, |canvas, cx| canvas.on_mouse_move(event, bounds, cx))
                    .ok();
            }
        });

        let (canvas, hit) = (self.canvas.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let ended = canvas
                .update(cx, |canvas, _| canvas.on_mouse_up(event))
                .unwrap_or(false);
            // GPUI also releases any capture after a mouse up; this keeps it explicit.
            if ended && window.captured_hitbox() == Some(hit.id) {
                window.release_pointer();
            }
        });

        let (canvas, hit) = (self.canvas.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hit.should_handle_scroll(window) {
                canvas
                    .update(cx, |canvas, cx| canvas.on_scroll(event, bounds, cx))
                    .ok();
                cx.stop_propagation();
            }
        });

        let (canvas, hit) = (self.canvas.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &PinchEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && hit.is_hovered(window) {
                canvas
                    .update(cx, |canvas, cx| canvas.on_pinch(event, bounds, cx))
                    .ok();
                cx.stop_propagation();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{MAX_ZOOM, MIN_ZOOM};
    use gpui::{Modifiers, px, size};

    fn viewport() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(0.0), px(64.0)),
            size: size(px(1100.0), px(700.0)),
        }
    }

    fn make_camera(zoom: f64) -> Camera {
        let mut camera = Camera::new(
            Pt {
                x: 1_234_567,
                y: -7_654_321,
            },
            zoom,
            viewport(),
        );
        camera.pan_by(point(0.375, -0.125));
        camera
    }

    fn wheel(at: Point<f64>, lines: f32) -> ScrollEvent {
        ScrollEvent {
            position: point(px(at.x as f32), px(at.y as f32)),
            delta: ScrollDelta::Lines(point(0.0, lines)),
            ..Default::default()
        }
    }

    fn pixels(at: Point<f64>, dx: f32, dy: f32, phase: TouchPhase) -> ScrollEvent {
        ScrollEvent {
            position: point(px(at.x as f32), px(at.y as f32)),
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            touch_phase: phase,
            ..Default::default()
        }
    }

    fn pinch(at: Point<f64>, delta: f32, phase: TouchPhase) -> PinchEvent {
        PinchEvent {
            position: point(px(at.x as f32), px(at.y as f32)),
            delta,
            modifiers: Modifiers::default(),
            phase,
        }
    }

    /// Unrounded world point under `p`, in nm, through the public API only: the world point
    /// that maps back to `p` within a fraction of a pixel.
    fn world(camera: &Camera, p: Point<f64>) -> Pt {
        camera.screen_to_world(p)
    }

    fn assert_fixed(before: Pt, after: Pt) {
        assert!(
            (i64::from(before.x) - i64::from(after.x)).abs() <= 1
                && (i64::from(before.y) - i64::from(after.y)).abs() <= 1,
            "{before:?} moved to {after:?}"
        );
    }

    #[test]
    fn wheel_zoom_keeps_the_point_under_the_cursor() {
        // GPUI rounds positions to f32 Pixels; use positions exact in f32.
        let at = point(731.25, 402.5);
        for zoom in [MIN_ZOOM, 0.37, 1.0, 6.1, MAX_ZOOM] {
            for lines in [3.0, -3.0, -6.0, 1.0, 9.0] {
                let mut camera = make_camera(zoom);
                let mut nav = Navigator::default();
                let before = world(&camera, at);
                let changed = nav.scroll(&mut camera, &wheel(at, lines));
                assert_fixed(before, world(&camera, at));
                let expected =
                    (zoom * wheel_factor(f64::from(lines) / 3.0)).clamp(MIN_ZOOM, MAX_ZOOM);
                assert!((camera.zoom() / expected - 1.0).abs() < 1e-12);
                assert_eq!(changed, expected != zoom);
            }
        }
    }

    #[test]
    fn one_notch_is_k4_and_round_trips() {
        let at = point(100.0, 200.0);
        let mut camera = make_camera(1.0);
        let mut nav = Navigator::default();
        assert!(nav.scroll(&mut camera, &wheel(at, 3.0)));
        assert!((camera.zoom() - 2f64.powf(0.25)).abs() < 1e-12);
        for _ in 0..4 {
            nav.scroll(&mut camera, &wheel(at, 3.0));
        }
        for _ in 0..5 {
            nav.scroll(&mut camera, &wheel(at, -3.0));
        }
        assert!((camera.zoom() - 1.0).abs() < 1e-12);
        // At the limits the wheel does nothing, so the sheet is not repainted.
        let mut camera = make_camera(MAX_ZOOM);
        assert!(!nav.scroll(&mut camera, &wheel(at, 3.0)));
        let mut camera = make_camera(MIN_ZOOM);
        assert!(!nav.scroll(&mut camera, &wheel(at, -3.0)));
        assert!(!nav.scroll(&mut camera, &wheel(at, 0.0)));
    }

    #[test]
    fn pixel_scroll_pans_by_exactly_the_delta() {
        let mut camera = make_camera(1.0);
        let mut nav = Navigator::default();
        let p = Pt {
            x: 5_080_000,
            y: 2_540_000,
        };
        let before = camera.world_to_screen(p);
        let at = point(10.0, 10.0);
        assert!(!nav.scroll(&mut camera, &pixels(at, 0.0, 0.0, TouchPhase::Started)));
        assert!(nav.scroll(&mut camera, &pixels(at, 1.5, -64.25, TouchPhase::Moved)));
        assert!(nav.scroll(&mut camera, &pixels(at, -0.5, 0.25, TouchPhase::Moved)));
        assert!(!nav.scroll(&mut camera, &pixels(at, 0.0, 0.0, TouchPhase::Ended)));
        assert_eq!(camera.zoom(), 1.0);
        assert_eq!(
            camera.world_to_screen(p),
            point(before.x + 1.0, before.y - 64.0)
        );
    }

    #[test]
    fn pinch_zooms_about_the_pinch_position() {
        let at = point(512.5, 300.25);
        for zoom in [MIN_ZOOM, 1.0, 9.5, MAX_ZOOM] {
            let mut camera = make_camera(zoom);
            let mut nav = Navigator::default();
            let before = world(&camera, at);
            // T0.5: Started with delta 0, Moved with delta = ratio - 1, Ended with delta 0.
            nav.pinch(&mut camera, &pinch(at, 0.0, TouchPhase::Started));
            let mut expected = zoom;
            for delta in [0.014f32, 0.067, -0.03, 0.2, -0.5] {
                nav.pinch(&mut camera, &pinch(at, delta, TouchPhase::Moved));
                expected = (expected * (1.0 + f64::from(delta))).clamp(MIN_ZOOM, MAX_ZOOM);
                assert_fixed(before, world(&camera, at));
            }
            nav.pinch(&mut camera, &pinch(at, 0.0, TouchPhase::Ended));
            assert!((camera.zoom() / expected - 1.0).abs() < 1e-9);
        }
    }

    #[test]
    fn cancelled_gestures_restore_the_camera() {
        let at = point(400.0, 300.0);
        let mut camera = make_camera(1.0);
        let mut nav = Navigator::default();
        let start = camera.world_to_screen(Pt::ZERO);
        nav.pinch(&mut camera, &pinch(at, 0.0, TouchPhase::Started));
        nav.pinch(&mut camera, &pinch(at, 0.5, TouchPhase::Moved));
        assert!(nav.pinch(&mut camera, &pinch(at, 0.0, TouchPhase::Cancelled)));
        assert_eq!(camera.zoom(), 1.0);
        assert_eq!(camera.world_to_screen(Pt::ZERO), start);

        nav.scroll(&mut camera, &pixels(at, 0.0, 0.0, TouchPhase::Started));
        nav.scroll(&mut camera, &pixels(at, 30.0, 40.0, TouchPhase::Moved));
        assert!(nav.scroll(&mut camera, &pixels(at, 0.0, 0.0, TouchPhase::Cancelled)));
        assert_eq!(camera.world_to_screen(Pt::ZERO), start);
        // A cancel with nothing started changes nothing.
        assert!(!nav.scroll(&mut camera, &pixels(at, 0.0, 0.0, TouchPhase::Cancelled)));
    }

    fn down(button: MouseButton, x: f32, y: f32) -> MouseDownEvent {
        MouseDownEvent {
            button,
            position: point(px(x), px(y)),
            click_count: 1,
            ..Default::default()
        }
    }

    fn moved(button: Option<MouseButton>, x: f32, y: f32) -> MouseMoveEvent {
        MouseMoveEvent {
            position: point(px(x), px(y)),
            pressed_button: button,
            modifiers: Modifiers::default(),
        }
    }

    fn up(button: MouseButton, x: f32, y: f32) -> MouseUpEvent {
        MouseUpEvent {
            button,
            position: point(px(x), px(y)),
            click_count: 1,
            ..Default::default()
        }
    }

    #[test]
    fn middle_drag_pans_by_the_pointer_movement() {
        let mut camera = make_camera(2.0);
        let mut nav = Navigator::default();
        let p = Pt::ZERO;
        let before = camera.world_to_screen(p);
        assert!(nav.mouse_down(&down(MouseButton::Middle, 100.0, 100.0)));
        assert!(nav.mouse_move(&mut camera, &moved(Some(MouseButton::Middle), 130.5, 90.0)));
        assert!(nav.mouse_move(
            &mut camera,
            &moved(Some(MouseButton::Middle), -20.0, 5000.0)
        ));
        assert!(!nav.mouse_move(
            &mut camera,
            &moved(Some(MouseButton::Middle), -20.0, 5000.0)
        ));
        assert!(!nav.mouse_up(&up(MouseButton::Left, 0.0, 0.0)));
        assert!(nav.mouse_up(&up(MouseButton::Middle, 0.0, 0.0)));
        assert!(!nav.mouse_move(&mut camera, &moved(None, 0.0, 0.0)));
        assert_eq!(
            camera.world_to_screen(p),
            point(before.x - 120.0, before.y + 4900.0)
        );
        assert_eq!(camera.zoom(), 2.0);
    }

    #[test]
    fn left_drag_pans_only_while_space_is_held() {
        let mut camera = make_camera(1.0);
        let mut nav = Navigator::default();
        assert!(!nav.mouse_down(&down(MouseButton::Left, 0.0, 0.0)));
        assert!(!nav.mouse_move(&mut camera, &moved(Some(MouseButton::Left), 50.0, 0.0)));
        nav.set_space(true);
        let before = camera.world_to_screen(Pt::ZERO);
        assert!(nav.mouse_down(&down(MouseButton::Left, 50.0, 0.0)));
        // A second button does not restart the drag.
        assert!(!nav.mouse_down(&down(MouseButton::Middle, 70.0, 0.0)));
        nav.set_space(false);
        assert!(nav.mouse_move(&mut camera, &moved(Some(MouseButton::Left), 60.0, 7.0)));
        assert!(nav.mouse_up(&up(MouseButton::Left, 60.0, 7.0)));
        assert_eq!(
            camera.world_to_screen(Pt::ZERO),
            point(before.x + 10.0, before.y + 7.0)
        );
        assert!(!nav.mouse_down(&down(MouseButton::Left, 0.0, 0.0)));
        assert!(!nav.mouse_down(&down(MouseButton::Right, 0.0, 0.0)));
    }

    #[test]
    fn a_lost_release_ends_the_drag() {
        let mut camera = make_camera(1.0);
        let mut nav = Navigator::default();
        nav.mouse_down(&down(MouseButton::Middle, 0.0, 0.0));
        assert!(!nav.mouse_move(&mut camera, &moved(None, 40.0, 0.0)));
        assert!(!nav.is_dragging());
    }

    #[test]
    fn status_reports_drawn_grid_and_zoom() {
        let camera = make_camera(1.0);
        let status = CanvasStatus::new(&camera, Some(Pt::ZERO));
        assert_eq!(status.grid, omnia_doc::GRID_NM);
        assert_eq!(status.zoom, 1.0);
        let camera = Camera::new(Pt::ZERO, MAX_ZOOM, viewport());
        assert_eq!(
            CanvasStatus::new(&camera, None).grid,
            omnia_doc::GRID_NM >> 5
        );
    }
}
