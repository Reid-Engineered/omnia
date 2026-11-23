//! T4.3 / T4.4: `OverlayView` — hover outline, selection halos, WD8 rubber band, and net
//! highlight over a cached [`SheetView`] (D7, G6, INV6, INV11).
//!
//! Architecture (D7, G6):
//! - [`CachedSheet`] is a dedicated entity wrapping [`SheetView`], embedded inside
//!   [`OverlayView`] with `.cached(StyleRefinement::default().size_full())`.
//! - [`OverlayView`] itself is NOT embedded with `.cached()` inside a parent that notifies on
//!   hover: when hover, selection, net highlight, or a rubber-band drag updates, only
//!   [`OverlayView`] is notified, so GPUI replays [`CachedSheet`]'s cached prepaint and paint
//!   without calling [`SheetView`]'s paint code (INV11).
//! - [`CachedSheet`] is notified only by a camera change, a theme change, a document swap, or a
//!   [`Change`] whose dirty rectangle meets the viewport (D7).
//! - All overlay colours come exclusively from `omnia-theme` tokens (`stage/select`,
//!   `stage/select-halo`, `stage/hover`, `stage/net-highlight`, INV6).

mod net;

pub use net::NetHighlight;

use std::{cell::Cell, rc::Rc};

use crate::{
    camera::Camera,
    selection::{RubberBand, SelectionState, SelectionUpdate},
    sheet::{SheetDocument, SheetView},
    snap_px::{centered_square, device_rect, round_device, stroke_device_px, to_device},
};
use gpui::{
    App, AppContext, Bounds, ContentMask, Context, DispatchPhase, Edges, Element, ElementId,
    Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, InteractiveElement, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Position, Render, Style, StyleRefinement,
    Styled, WeakEntity, Window, div, fill, point, px, relative,
};
use omnia_doc::{Change, Doc, ItemId, NetId, Pt, Rect, Wire, WireId, prefs::SelectMode};
use omnia_theme::Theme;

/// K5 wire stroke width in world nanometres, matching `sheet/wires.rs`.
const WIRE_WIDTH_NM: f64 = 228_600.0;
/// Logical padding around a part body for hover/selection frames.
const PART_FRAME_PAD_PX: f64 = 4.0;
/// Logical outline thickness for selected items.
const SELECT_OUTLINE_PX: f64 = 1.5;
/// Logical outline thickness for hovered items.
const HOVER_OUTLINE_PX: f64 = 1.5;
/// Extra logical thickness for the translucent halo around selected/hovered wires.
const WIRE_HALO_EXTRA_PX: f64 = 6.0;
/// Extra logical thickness for the crisp highlight over selected wires.
const WIRE_SELECT_EXTRA_PX: f64 = 1.0;
/// Logical side of corner handles on selected parts and endpoint markers on selected wires.
const HANDLE_PX: f64 = 5.0;
/// Half-extent in logical px of the selection/hover box around a label anchor (WD8).
const LABEL_HALF_PX: f64 = 6.0;
/// Logical dash and gap lengths for Intersecting-mode rubber-band borders.
const BAND_DASH_PX: f64 = 5.0;
const BAND_GAP_PX: f64 = 4.0;

/// Converts an `omnia-theme` token into GPUI's colour type (INV6).
fn gpui_color(c: omnia_theme::Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

/// Events emitted by [`OverlayView`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayEvent {
    SelectionChanged,
}

/// Cached view wrapping [`SheetView`] with a real paint counter (D7, G6, INV11).
///
/// Never notified by hover, selection, or rubber-band drag updates.
pub struct CachedSheet {
    camera: Camera,
    theme: &'static Theme,
    document: Option<SheetDocument>,
    paints: Rc<Cell<u64>>,
    renders: u64,
}

impl CachedSheet {
    pub fn new(camera: Camera, theme: &'static Theme, document: Option<SheetDocument>) -> Self {
        Self {
            camera,
            theme,
            document,
            paints: Rc::new(Cell::new(0)),
            renders: 0,
        }
    }

    /// Number of times [`SheetView::paint`] actually executed (not replayed from cache).
    pub fn paint_count(&self) -> u64 {
        self.paints.get()
    }

    /// Number of times [`CachedSheet::render`] executed.
    pub fn render_count(&self) -> u64 {
        self.renders
    }
}

impl Render for CachedSheet {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let view = match &self.document {
            Some(document) => {
                SheetView::with_document(self.camera.clone(), self.theme, document.clone())
            }
            None => SheetView::new(self.camera.clone(), self.theme),
        };
        CountedSheet {
            view,
            paints: self.paints.clone(),
        }
    }
}

/// Thin [`Element`] wrapper around [`SheetView`] that counts actual `Element::paint` executions.
struct CountedSheet {
    view: SheetView,
    paints: Rc<Cell<u64>>,
}

impl Element for CountedSheet {
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
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.view.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.view
            .prepaint(id, inspector_id, bounds, request_layout, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paints.set(self.paints.get() + 1);
        self.view.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

/// Transient interactive overlay above a `.cached()` [`SheetView`] (D7, INV11).
///
/// Owns hover, selection, net-highlight, and rubber-band state, registers one sheet-wide hitbox
/// for pointer picking and rubber-band drags, and repaints only itself on transient updates.
pub struct OverlayView {
    doc: Entity<Doc>,
    sheet: Entity<CachedSheet>,
    camera: Camera,
    theme: &'static Theme,
    state: SelectionState,
    net_highlight: NetHighlight,
    focus: FocusHandle,
    renders: u64,
    paints: Rc<Cell<u64>>,
}

impl EventEmitter<OverlayEvent> for OverlayView {}

impl OverlayView {
    pub fn new(
        doc: Entity<Doc>,
        camera: Camera,
        theme: &'static Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        let document = SheetDocument::new(doc.clone());
        let sheet = cx.new(|_| CachedSheet::new(camera.clone(), theme, Some(document)));
        Self {
            doc,
            sheet,
            camera,
            theme,
            state: SelectionState::default(),
            net_highlight: NetHighlight::new(),
            focus: cx.focus_handle(),
            renders: 0,
            paints: Rc::new(Cell::new(0)),
        }
    }

    pub fn doc(&self) -> &Entity<Doc> {
        &self.doc
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn theme(&self) -> &'static Theme {
        self.theme
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn selection(&self) -> &[ItemId] {
        self.state.selection()
    }

    pub fn hovered(&self) -> Option<ItemId> {
        self.state.hovered()
    }

    pub fn highlighted_nets(&self) -> &[NetId] {
        self.net_highlight.nets()
    }

    pub fn highlighted_wires(&self) -> &[WireId] {
        self.net_highlight.wires()
    }

    pub fn rubber_band(&self) -> Option<RubberBand> {
        self.state.rubber_band()
    }

    pub fn select_mode(&self) -> SelectMode {
        self.state.select_mode()
    }

    /// The underlying cached sheet entity (for INV11 tests and observation).
    pub fn sheet_entity(&self) -> &Entity<CachedSheet> {
        &self.sheet
    }

    /// Number of times [`SheetView::paint`] executed (cached replays do not increment this).
    pub fn sheet_paint_count(&self, cx: &App) -> u64 {
        self.sheet.read(cx).paint_count()
    }

    /// Number of times [`CachedSheet::render`] executed.
    pub fn sheet_render_count(&self, cx: &App) -> u64 {
        self.sheet.read(cx).render_count()
    }

    /// Number of times the overlay element painted.
    pub fn overlay_paint_count(&self) -> u64 {
        self.paints.get()
    }

    /// Number of times [`OverlayView::render`] executed.
    pub fn overlay_render_count(&self) -> u64 {
        self.renders
    }

    /// Sets the WD8 rubber-band mode (never notifies `SheetView`).
    pub fn set_select_mode(&mut self, mode: SelectMode, cx: &mut Context<Self>) {
        if self.state.set_select_mode(mode) {
            cx.notify();
        }
    }

    fn sync_net_highlight(&mut self, cx: &App) -> bool {
        self.net_highlight.sync(
            self.doc.read(cx),
            self.state.hovered(),
            self.state.selection(),
        )
    }

    /// Sets the hovered item directly (never notifies `SheetView`).
    pub fn set_hovered(&mut self, item: Option<ItemId>, cx: &mut Context<Self>) {
        if self.state.set_hovered(item) {
            self.sync_net_highlight(cx);
            cx.notify();
        }
    }

    /// Sets the selected items directly (never notifies `SheetView`).
    pub fn set_selection(
        &mut self,
        items: impl IntoIterator<Item = ItemId>,
        cx: &mut Context<Self>,
    ) {
        if self.state.set_selection(items) {
            self.sync_net_highlight(cx);
            cx.emit(OverlayEvent::SelectionChanged);
            cx.notify();
        }
    }

    /// Clears the selection (never notifies `SheetView`).
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.state.clear_selection() {
            self.sync_net_highlight(cx);
            cx.emit(OverlayEvent::SelectionChanged);
            cx.notify();
        }
    }

    /// Selects every live item in the document (ui-plan.md section 14; never notifies `SheetView`).
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let changed = self.state.select_all(self.doc.read(cx));
        if changed {
            self.sync_net_highlight(cx);
            cx.emit(OverlayEvent::SelectionChanged);
            cx.notify();
        }
    }

    /// Cancels an active rubber-band drag (never notifies `SheetView`).
    pub fn cancel_rubber_band(&mut self, cx: &mut Context<Self>) {
        if self.state.cancel_rubber_band() {
            cx.notify();
        }
    }

    /// Sets a visible rubber band directly in world coordinates for golden rendering or
    /// programmatic preview (never notifies `SheetView`, keeps `select_mode()` unchanged, and
    /// does not capture the pointer).
    pub fn set_rubber_band_preview(
        &mut self,
        anchor: Pt,
        current: Pt,
        mode: SelectMode,
        cx: &mut Context<Self>,
    ) {
        if self.state.set_rubber_band_preview(anchor, current, mode) {
            cx.notify();
        }
    }

    /// Updates the camera and invalidates both the cached sheet and the overlay when changed (D7).
    pub fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        if self.camera.zoom() == camera.zoom()
            && self.camera.world_to_screen(Pt::ZERO) == camera.world_to_screen(Pt::ZERO)
            && self.camera.visible() == camera.visible()
        {
            return;
        }
        self.camera = camera.clone();
        self.sheet.update(cx, |sheet, cx| {
            sheet.camera = camera;
            cx.notify();
        });
        cx.notify();
    }

    /// Updates the theme and invalidates both the cached sheet and the overlay when it changes (D7).
    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>) {
        if std::ptr::eq(self.theme, theme) {
            return;
        }
        self.theme = theme;
        self.sheet.update(cx, |sheet, cx| {
            sheet.theme = theme;
            cx.notify();
        });
        cx.notify();
    }

    /// Replaces the displayed document, clearing transient state and invalidating both views.
    pub fn set_document(&mut self, doc: Entity<Doc>, cx: &mut Context<Self>) {
        let document = SheetDocument::new(doc.clone());
        self.doc = doc;
        let had_selection = !self.state.selection().is_empty();
        self.state = SelectionState::new(self.state.select_mode());
        self.net_highlight.clear();
        self.sheet.update(cx, |sheet, cx| {
            sheet.document = Some(document);
            cx.notify();
        });
        if had_selection {
            cx.emit(OverlayEvent::SelectionChanged);
        }
        cx.notify();
    }

    /// Applies D7's dirty-rectangle invalidation after a document command:
    /// notifies `SheetView` only when `change.dirty` intersects the visible viewport, prunes
    /// any removed items from hover/selection, and re-resolves net highlights (T4.4).
    pub fn on_doc_change(&mut self, change: &Change, cx: &mut Context<Self>) {
        let visible = self.camera.visible();
        if change.dirty.is_some_and(|dirty| dirty.intersects(visible)) {
            self.sheet.update(cx, |_, cx| cx.notify());
        }
        let mut update = self.state.prune(self.doc.read(cx));
        if self.sync_net_highlight(cx) {
            update.visual_changed = true;
        }
        self.emit_and_notify(update, cx);
    }

    fn apply_update(&mut self, update: SelectionUpdate, cx: &mut Context<Self>) {
        if update.visual_changed || update.selection_changed {
            self.sync_net_highlight(cx);
        }
        self.emit_and_notify(update, cx);
    }

    fn emit_and_notify(&mut self, update: SelectionUpdate, cx: &mut Context<Self>) {
        if update.selection_changed {
            cx.emit(OverlayEvent::SelectionChanged);
        }
        if update.visual_changed {
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.camera.set_viewport(bounds);
        let update = self
            .state
            .mouse_down(self.doc.read(cx), &self.camera, event);
        self.apply_update(update, cx);
        update.started_drag
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.camera.set_viewport(bounds);
        let update = self
            .state
            .mouse_move(self.doc.read(cx), &self.camera, event);
        self.apply_update(update, cx);
        update.ended_drag
    }

    fn on_mouse_leave(&mut self, cx: &mut Context<Self>) {
        let update = self.state.mouse_leave();
        self.apply_update(update, cx);
    }

    fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.camera.set_viewport(bounds);
        let update = self.state.mouse_up(self.doc.read(cx), &self.camera, event);
        self.apply_update(update, cx);
        update.ended_drag
    }
}

impl Render for OverlayView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        div()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .child(
                self.sheet
                    .clone()
                    .cached(StyleRefinement::default().size_full()),
            )
            .child(OverlayLayer {
                overlay: cx.entity().downgrade(),
                doc: self.doc.clone(),
                camera: self.camera.clone(),
                theme: self.theme,
                focus: self.focus.clone(),
                dragging: self.state.is_rubber_banding(),
                paints: self.paints.clone(),
            })
    }
}

/// Absolutely-positioned overlay element stacked above the `.cached()` sheet.
/// Registers one sheet-wide hitbox (D7) and paints hover, selection, and rubber-band primitives.
struct OverlayLayer {
    overlay: WeakEntity<OverlayView>,
    doc: Entity<Doc>,
    camera: Camera,
    theme: &'static Theme,
    focus: FocusHandle,
    dragging: bool,
    paints: Rc<Cell<u64>>,
}

impl Element for OverlayLayer {
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style {
            position: Position::Absolute,
            inset: Edges::all(px(0.0).into()),
            ..Default::default()
        };
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
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
        // Keep the overlay's camera viewport aligned with layout bounds without notifying.
        self.overlay
            .update(cx, |overlay, _| overlay.camera.set_viewport(bounds))
            .ok();
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paints.set(self.paints.get() + 1);

        if let Some(overlay) = self.overlay.upgrade() {
            let mut camera = self.camera.clone();
            camera.set_viewport(bounds);
            let sf = window.scale_factor();
            let view = overlay.read(cx);
            let doc = self.doc.read(cx);
            let ctx = OverlayPaintContext {
                doc,
                camera: &camera,
                bounds,
                sf,
                theme: self.theme,
            };
            paint_overlay(&view.state, &view.net_highlight, &ctx, window);
        }

        if self.dragging {
            window.capture_pointer(hitbox.id);
        }

        let (overlay, hit, focus) = (self.overlay.clone(), hitbox.clone(), self.focus.clone());
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !hit.is_hovered(window)
                || event.button != MouseButton::Left
            {
                return;
            }
            // Keep explicit focus transfer here because starting a rubber band calls
            // `cx.stop_propagation()`, which would otherwise skip the parent `div().track_focus`
            // bubble listener (`elements/div.rs`). When already focused this is a no-op.
            window.focus(&focus, cx);
            let started = overlay
                .update(cx, |overlay, cx| overlay.on_mouse_down(event, bounds, cx))
                .unwrap_or(false);
            if started {
                window.capture_pointer(hit.id);
                cx.stop_propagation();
            }
        });

        let (overlay, hit) = (self.overlay.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            if hit.is_hovered(window) {
                let ended = overlay
                    .update(cx, |overlay, cx| overlay.on_mouse_move(event, bounds, cx))
                    .unwrap_or(false);
                if ended && window.captured_hitbox() == Some(hit.id) {
                    window.release_pointer();
                }
            } else {
                overlay
                    .update(cx, |overlay, cx| overlay.on_mouse_leave(cx))
                    .ok();
            }
        });

        let (overlay, hit) = (self.overlay.clone(), hitbox.clone());
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            let ended = overlay
                .update(cx, |overlay, cx| overlay.on_mouse_up(event, bounds, cx))
                .unwrap_or(false);
            if ended && window.captured_hitbox() == Some(hit.id) {
                window.release_pointer();
            }
        });
    }
}

/// Device-pixel rectangle `(left, top, width, height)` clipped safely near the viewport.
#[derive(Clone, Copy)]
struct DevRect {
    left: f64,
    top: f64,
    width: u32,
    height: u32,
}

impl DevRect {
    fn from_edges(
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
        clip: (f64, f64, f64, f64),
    ) -> Option<Self> {
        let l = left.max(clip.0);
        let t = top.max(clip.1);
        let r = right.min(clip.2);
        let b = bottom.min(clip.3);
        if r <= l || b <= t {
            return None;
        }
        let w = (r - l).round();
        let h = (b - t).round();
        if w < 1.0 || h < 1.0 || !w.is_finite() || !h.is_finite() {
            return None;
        }
        Some(Self {
            left: l,
            top: t,
            width: w.min(f64::from(u32::MAX)) as u32,
            height: h.min(f64::from(u32::MAX)) as u32,
        })
    }

    fn bounds(self, sf: f32) -> Bounds<Pixels> {
        device_rect(self.left, self.top, self.width, self.height, sf)
    }
}

fn viewport_clip_dev(bounds: Bounds<Pixels>, sf: f32, margin_dev: f64) -> (f64, f64, f64, f64) {
    let l = to_device(f64::from(bounds.left()), sf) - margin_dev;
    let t = to_device(f64::from(bounds.top()), sf) - margin_dev;
    let r = to_device(f64::from(bounds.right()), sf) + margin_dev;
    let b = to_device(f64::from(bounds.bottom()), sf) + margin_dev;
    (l, t, r, b)
}

/// Computes the padded device-pixel frame around a world `Rect`.
fn padded_rect_dev(
    rect: Rect,
    pad_px: f64,
    camera: &Camera,
    sf: f32,
    clip: (f64, f64, f64, f64),
) -> Option<DevRect> {
    let min = camera.world_to_screen(rect.min);
    let max = camera.world_to_screen(rect.max);
    let pad = round_device(to_device(pad_px, sf));
    let left = round_device(to_device(min.x.min(max.x), sf)) - pad;
    let top = round_device(to_device(min.y.min(max.y), sf)) - pad;
    let right = round_device(to_device(min.x.max(max.x), sf)) + pad;
    let bottom = round_device(to_device(min.y.max(max.y), sf)) + pad;
    DevRect::from_edges(
        left,
        top,
        right.max(left + 1.0),
        bottom.max(top + 1.0),
        clip,
    )
}

/// Computes the device-pixel frame around a label anchor point.
fn label_rect_dev(
    at: Pt,
    half_px: f64,
    camera: &Camera,
    sf: f32,
    clip: (f64, f64, f64, f64),
) -> Option<DevRect> {
    let center = camera.world_to_screen(at);
    let cx = round_device(to_device(center.x, sf));
    let cy = round_device(to_device(center.y, sf));
    let half = round_device(to_device(half_px, sf)).max(2.0);
    DevRect::from_edges(cx - half, cy - half, cx + half, cy + half, clip)
}

/// Computes the device-pixel quad covering an axis-aligned wire at `width_dev` device pixels,
/// matching `sheet/wires.rs` alignment and clipped to `clip`.
fn wire_rect_dev(
    wire: Wire,
    camera: &Camera,
    sf: f32,
    width_dev: u32,
    clip: (f64, f64, f64, f64),
) -> Option<DevRect> {
    let vertical = wire.a.x == wire.b.x;
    let (start_pt, end_pt) = if vertical {
        if wire.a.y <= wire.b.y {
            (wire.a, wire.b)
        } else {
            (wire.b, wire.a)
        }
    } else if wire.a.x <= wire.b.x {
        (wire.a, wire.b)
    } else {
        (wire.b, wire.a)
    };
    let start = camera.world_to_screen(start_pt);
    let end = camera.world_to_screen(end_pt);
    let half = f64::from(width_dev) / 2.0;
    let cap = f64::from(width_dev / 2);
    if vertical {
        let left = round_device(to_device(start.x, sf) - half);
        let right = left + f64::from(width_dev);
        let top = round_device(to_device(start.y, sf)) - cap;
        let bottom = round_device(to_device(end.y, sf)) - cap + f64::from(width_dev);
        DevRect::from_edges(left, top, right, bottom, clip)
    } else {
        let top = round_device(to_device(start.y, sf) - half);
        let bottom = top + f64::from(width_dev);
        let left = round_device(to_device(start.x, sf)) - cap;
        let right = round_device(to_device(end.x, sf)) - cap + f64::from(width_dev);
        DevRect::from_edges(left, top, right, bottom, clip)
    }
}

/// Paints a solid four-edge rectangle outline inside `rect` in whole device pixels.
fn paint_solid_outline(rect: DevRect, thickness: u32, color: Hsla, sf: f32, window: &mut Window) {
    let t = thickness.max(1);
    let w = rect.width;
    let h = rect.height;
    let th = t.min(h);
    let tw = t.min(w);
    // Top and bottom bars.
    window.paint_quad(fill(device_rect(rect.left, rect.top, w, th, sf), color));
    if h > th {
        window.paint_quad(fill(
            device_rect(rect.left, rect.top + f64::from(h - th), w, th, sf),
            color,
        ));
    }
    // Left and right inner bars.
    if h > th.saturating_mul(2) {
        let inner_h = h - th * 2;
        let inner_top = rect.top + f64::from(th);
        window.paint_quad(fill(
            device_rect(rect.left, inner_top, tw, inner_h, sf),
            color,
        ));
        if w > tw {
            window.paint_quad(fill(
                device_rect(rect.left + f64::from(w - tw), inner_top, tw, inner_h, sf),
                color,
            ));
        }
    }
}

/// Paints a dashed four-edge rectangle outline (used for Intersecting rubber-band mode).
fn paint_dashed_outline(rect: DevRect, thickness: u32, color: Hsla, sf: f32, window: &mut Window) {
    let dash = stroke_device_px(BAND_DASH_PX, sf);
    let gap = stroke_device_px(BAND_GAP_PX, sf);
    let step = dash.saturating_add(gap).max(1);
    let t = thickness.max(1);
    if rect.width <= dash || rect.height <= dash {
        paint_solid_outline(rect, t, color, sf, window);
        return;
    }
    let bottom_y = rect.top + f64::from(rect.height.saturating_sub(t));
    let right_x = rect.left + f64::from(rect.width.saturating_sub(t));
    // Horizontal edges (top & bottom).
    let mut x = 0u32;
    while x < rect.width {
        let seg = dash.min(rect.width - x);
        window.paint_quad(fill(
            device_rect(rect.left + f64::from(x), rect.top, seg, t, sf),
            color,
        ));
        window.paint_quad(fill(
            device_rect(rect.left + f64::from(x), bottom_y, seg, t, sf),
            color,
        ));
        x = x.saturating_add(step);
    }
    // Vertical edges (left & right).
    let mut y = 0u32;
    while y < rect.height {
        let seg = dash.min(rect.height - y);
        window.paint_quad(fill(
            device_rect(rect.left, rect.top + f64::from(y), t, seg, sf),
            color,
        ));
        window.paint_quad(fill(
            device_rect(right_x, rect.top + f64::from(y), t, seg, sf),
            color,
        ));
        y = y.saturating_add(step);
    }
}

/// Paints four crisp corner handle squares on a selected part frame.
fn paint_corner_handles(rect: DevRect, side: u32, color: Hsla, sf: f32, window: &mut Window) {
    let right = rect.left + f64::from(rect.width);
    let bottom = rect.top + f64::from(rect.height);
    let scale = f64::from(sf);
    for (x, y) in [
        (rect.left, rect.top),
        (right, rect.top),
        (rect.left, bottom),
        (right, bottom),
    ] {
        window.paint_quad(fill(
            centered_square(point(x / scale, y / scale), side, sf),
            color,
        ));
    }
}

fn item_world_bounds(doc: &Doc, item: ItemId) -> Option<Rect> {
    match item {
        ItemId::Part(id) => doc.part_body(id),
        ItemId::Wire(id) => {
            let wire = doc.wire(id)?;
            Some(Rect {
                min: Pt {
                    x: wire.a.x.min(wire.b.x),
                    y: wire.a.y.min(wire.b.y),
                },
                max: Pt {
                    x: wire.a.x.max(wire.b.x),
                    y: wire.a.y.max(wire.b.y),
                },
            })
        }
        ItemId::Label(id) => {
            let at = doc.label(id)?.at;
            Some(Rect { min: at, max: at })
        }
    }
}

struct OverlayPaintContext<'a> {
    doc: &'a Doc,
    camera: &'a Camera,
    bounds: Bounds<Pixels>,
    sf: f32,
    theme: &'static Theme,
}

fn paint_overlay(
    state: &SelectionState,
    net_highlight: &NetHighlight,
    ctx: &OverlayPaintContext<'_>,
    window: &mut Window,
) {
    let OverlayPaintContext {
        doc,
        camera,
        bounds,
        sf,
        theme,
    } = *ctx;
    let hovered = state.hovered();
    let selection = state.selection();
    let band = state.rubber_band().filter(|b| b.is_visible());
    if hovered.is_none() && selection.is_empty() && band.is_none() && net_highlight.is_empty() {
        return;
    }

    let select_color = gpui_color(theme.stage_select);
    let halo_color = gpui_color(theme.stage_select_halo);
    let hover_color = gpui_color(theme.stage_hover);

    let wire_base_px = WIRE_WIDTH_NM * camera.scale();
    let wire_halo_dev = stroke_device_px(wire_base_px + WIRE_HALO_EXTRA_PX, sf);
    let wire_select_dev = stroke_device_px(wire_base_px + WIRE_SELECT_EXTRA_PX, sf);
    let select_outline_dev = stroke_device_px(SELECT_OUTLINE_PX, sf);
    let hover_outline_dev = stroke_device_px(HOVER_OUTLINE_PX, sf);
    let band_border_dev = stroke_device_px(1.0, sf);
    let handle_dev = stroke_device_px(HANDLE_PX, sf);

    let clip_margin =
        f64::from(wire_halo_dev.max(handle_dev)) + to_device(PART_FRAME_PAD_PX + 8.0, sf);
    let clip = viewport_clip_dev(bounds, sf, clip_margin);
    let visible_world = camera.visible();
    let cull_margin_nm =
        ((PART_FRAME_PAD_PX + WIRE_HALO_EXTRA_PX + LABEL_HALF_PX) / camera.scale()).ceil() as i32;
    let visible_padded = Rect {
        min: Pt {
            x: visible_world.min.x.saturating_sub(cull_margin_nm),
            y: visible_world.min.y.saturating_sub(cull_margin_nm),
        },
        max: Pt {
            x: visible_world.max.x.saturating_add(cull_margin_nm),
            y: visible_world.max.y.saturating_add(cull_margin_nm),
        },
    };

    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        // Layer 1: translucent fills and halos (always underneath crisp outlines).
        window.paint_layer(|window| {
            // Hovered item halo (skip when already selected so selection remains visually dominant).
            if let Some(item) = hovered
                && !state.is_selected(item)
                && item_world_bounds(doc, item).is_some_and(|r| r.intersects(visible_padded))
            {
                match item {
                    ItemId::Part(id) => {
                        if let Some(body) = doc.part_body(id)
                            && let Some(rect) =
                                padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                        {
                            window.paint_quad(fill(rect.bounds(sf), hover_color.opacity(0.18)));
                        }
                    }
                    ItemId::Wire(id) => {
                        if let Some(&wire) = doc.wire(id)
                            && let Some(rect) = wire_rect_dev(wire, camera, sf, wire_halo_dev, clip)
                        {
                            window.paint_quad(fill(rect.bounds(sf), hover_color.opacity(0.55)));
                        }
                    }
                    ItemId::Label(id) => {
                        if let Some(label) = doc.label(id)
                            && let Some(rect) =
                                label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                        {
                            window.paint_quad(fill(rect.bounds(sf), hover_color.opacity(0.25)));
                        }
                    }
                }
            }

            // Selected item fills / halos.
            for &item in selection {
                if !item_world_bounds(doc, item).is_some_and(|r| r.intersects(visible_padded)) {
                    continue;
                }
                match item {
                    ItemId::Part(id) => {
                        if let Some(body) = doc.part_body(id)
                            && let Some(rect) =
                                padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                        {
                            window.paint_quad(fill(rect.bounds(sf), halo_color.opacity(0.22)));
                        }
                    }
                    ItemId::Wire(id) => {
                        if let Some(&wire) = doc.wire(id)
                            && let Some(rect) = wire_rect_dev(wire, camera, sf, wire_halo_dev, clip)
                        {
                            window.paint_quad(fill(rect.bounds(sf), halo_color.opacity(0.50)));
                        }
                    }
                    ItemId::Label(id) => {
                        if let Some(label) = doc.label(id)
                            && let Some(rect) =
                                label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                        {
                            window.paint_quad(fill(rect.bounds(sf), halo_color.opacity(0.32)));
                        }
                    }
                }
            }

            // Rubber-band translucent fill.
            if let Some(band) = band
                && let Some(rect) = padded_rect_dev(band.world_rect(), 0.0, camera, sf, clip)
            {
                window.paint_quad(fill(rect.bounds(sf), halo_color.opacity(0.22)));
            }
        });

        // Layer 2: crisp net highlights, outlines, wire highlights, corner handles, and rubber-band border.
        window.paint_layer(|window| {
            // T4.4: whole-net wire highlight in `stage/net-highlight` (underneath selection handles).
            let wire_paint = net::WirePaintParams {
                camera,
                visible_padded,
                sf,
                width_dev: wire_select_dev,
                clip,
                theme,
            };
            net::paint_wires(doc, net_highlight.wires(), &wire_paint, window);

            // Hovered item outline (hovered wires are painted by `net::paint_wires` above, with
            // their Layer 1 hover halo indicating the hovered segment).
            if let Some(item) = hovered
                && !state.is_selected(item)
                && item_world_bounds(doc, item).is_some_and(|r| r.intersects(visible_padded))
            {
                match item {
                    ItemId::Part(id) => {
                        if let Some(body) = doc.part_body(id)
                            && let Some(rect) =
                                padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                        {
                            paint_solid_outline(rect, hover_outline_dev, hover_color, sf, window);
                        }
                    }
                    ItemId::Wire(_) => {}
                    ItemId::Label(id) => {
                        if let Some(label) = doc.label(id)
                            && let Some(rect) =
                                label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                        {
                            paint_solid_outline(rect, hover_outline_dev, hover_color, sf, window);
                        }
                    }
                }
            }

            // Selected item outlines, wire strokes, and handles.
            for &item in selection {
                if !item_world_bounds(doc, item).is_some_and(|r| r.intersects(visible_padded)) {
                    continue;
                }
                match item {
                    ItemId::Part(id) => {
                        if let Some(body) = doc.part_body(id)
                            && let Some(rect) =
                                padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                        {
                            paint_solid_outline(rect, select_outline_dev, select_color, sf, window);
                            paint_corner_handles(rect, handle_dev, select_color, sf, window);
                        }
                    }
                    ItemId::Wire(id) => {
                        if let Some(&wire) = doc.wire(id) {
                            if let Some(rect) =
                                wire_rect_dev(wire, camera, sf, wire_select_dev, clip)
                            {
                                window.paint_quad(fill(rect.bounds(sf), select_color));
                            }
                            for end in [wire.a, wire.b] {
                                let screen = camera.world_to_screen(end);
                                window.paint_quad(fill(
                                    centered_square(screen, handle_dev, sf),
                                    select_color,
                                ));
                            }
                        }
                    }
                    ItemId::Label(id) => {
                        if let Some(label) = doc.label(id) {
                            if let Some(rect) =
                                label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                            {
                                paint_solid_outline(
                                    rect,
                                    select_outline_dev,
                                    select_color,
                                    sf,
                                    window,
                                );
                            }
                            let screen = camera.world_to_screen(label.at);
                            let dot = stroke_device_px(3.0, sf);
                            window.paint_quad(fill(centered_square(screen, dot, sf), select_color));
                        }
                    }
                }
            }

            // Rubber-band border: dashed for Intersecting (WD8/WQ3 default), solid for Enclosed.
            if let Some(band) = band
                && let Some(rect) = padded_rect_dev(band.world_rect(), 0.0, camera, sf, clip)
            {
                match band.mode {
                    SelectMode::Intersecting => {
                        paint_dashed_outline(rect, band_border_dev, select_color, sf, window);
                    }
                    SelectMode::Enclosed => {
                        paint_solid_outline(rect, band_border_dev, select_color, sf, window);
                    }
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Modifiers, TestAppContext, VisualTestContext};
    use omnia_doc::{Command, GRID_NM, Label, LabelId, Orient, Part, PartId, Wire, WireId};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::{GRAPHITE, GRAY};

    fn test_doc() -> Doc {
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
                    at: Pt { x: -6 * g, y: 0 },
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
                    at: Pt { x: 6 * g, y: 0 },
                    orient: Orient::IDENTITY,
                    designator: "C1".into(),
                    value: "100n".into(),
                },
                cap_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                cap_def.bounds,
            ),
        ];
        let wires = [Wire {
            a: Pt { x: -4 * g, y: 0 },
            b: Pt { x: 4 * g, y: 0 },
        }];
        let labels = [Label {
            at: Pt { x: 0, y: 4 * g },
            text: "OUT".into(),
        }];
        Doc::from_items(parts, wires, labels).unwrap()
    }

    struct OverlayFixture {
        overlay: Entity<OverlayView>,
        doc: Entity<Doc>,
        sheet_notifies: Rc<Cell<u32>>,
        selection_events: Rc<Cell<u32>>,
    }

    fn open_overlay(cx: &mut TestAppContext) -> (OverlayFixture, &mut VisualTestContext) {
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc_clone, camera, &GRAPHITE, cx);
            // Focus the sheet once at open so clicks on the active sheet do not trigger GPUI's
            // focus-change `window.refresh()` (which bypasses `.cached()` for that frame).
            window.focus(view.focus_handle(), cx);
            view
        });
        let sheet_notifies = Rc::new(Cell::new(0u32));
        let sn = sheet_notifies.clone();
        let selection_events = Rc::new(Cell::new(0u32));
        let se = selection_events.clone();
        cx.update(|_, cx| {
            let sheet = overlay.read(cx).sheet_entity().clone();
            cx.observe(&sheet, move |_, _| sn.set(sn.get() + 1))
                .detach();
            cx.subscribe(&overlay, move |_, event: &OverlayEvent, _| {
                if *event == OverlayEvent::SelectionChanged {
                    se.set(se.get() + 1);
                }
            })
            .detach();
        });
        (
            OverlayFixture {
                overlay,
                doc,
                sheet_notifies,
                selection_events,
            },
            cx,
        )
    }

    fn screen_of(
        overlay: &Entity<OverlayView>,
        cx: &mut VisualTestContext,
        pt: Pt,
    ) -> gpui::Point<Pixels> {
        cx.update(|_, cx| overlay.read(cx).camera().world_to_pixels(pt))
    }

    #[gpui::test]
    fn inv11_hover_selection_and_rubber_band_never_repaint_sheet(cx: &mut TestAppContext) {
        let (
            OverlayFixture {
                overlay,
                doc,
                sheet_notifies,
                selection_events,
            },
            cx,
        ) = open_overlay(cx);
        let g = GRID_NM;

        let initial_sheet_paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let initial_sheet_renders = cx.update(|_, cx| overlay.read(cx).sheet_render_count(cx));
        let initial_overlay_paints = cx.update(|_, cx| overlay.read(cx).overlay_paint_count());
        assert_eq!(
            initial_sheet_paints, 1,
            "initial frame must paint SheetView once"
        );
        assert_eq!(initial_sheet_renders, 1);

        // 1. 100 simulated hover moves sweeping across R1, wire, empty space, C1, and label.
        let mut hover_transitions = 0u32;
        let mut last_hover = None;
        for step in 0..100 {
            let world = match step % 5 {
                0 => Pt { x: -6 * g, y: 0 }, // R1
                1 => Pt { x: 0, y: 0 },      // Wire(0)
                2 => Pt { x: 6 * g, y: 0 },  // C1
                3 => Pt { x: 0, y: 4 * g },  // Label(0)
                _ => Pt {
                    x: -10 * g + (step % 7) * g / 2,
                    y: -8 * g,
                }, // empty
            };
            let pos = screen_of(&overlay, cx, world);
            cx.simulate_mouse_move(pos, None, Modifiers::none());
            let hovered = cx.update(|_, cx| overlay.read(cx).hovered());
            if hovered != last_hover {
                hover_transitions += 1;
                last_hover = hovered;
            }
        }
        assert!(
            hover_transitions >= 80,
            "hover must actually visit items and empty space"
        );
        assert!(
            cx.update(|_, cx| overlay.read(cx).overlay_paint_count()) > initial_overlay_paints,
            "overlay must have repainted during hover"
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints,
            "INV11: 100 hover moves must not repaint SheetView"
        );
        assert_eq!(
            sheet_notifies.get(),
            0,
            "INV11: hover must not notify SheetView"
        );

        // 2. Click selection, Shift-click add, Shift-click toggle off, and select_all.
        let r1_pos = screen_of(&overlay, cx, Pt { x: -6 * g, y: 0 });
        let wire_pos = screen_of(&overlay, cx, Pt { x: 0, y: 0 });
        cx.simulate_mouse_down(r1_pos, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(r1_pos, MouseButton::Left, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0))]
        );

        cx.simulate_mouse_down(wire_pos, MouseButton::Left, Modifiers::shift());
        cx.simulate_mouse_up(wire_pos, MouseButton::Left, Modifiers::shift());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0)), ItemId::Wire(WireId(0))]
        );

        cx.simulate_mouse_down(r1_pos, MouseButton::Left, Modifiers::shift());
        cx.simulate_mouse_up(r1_pos, MouseButton::Left, Modifiers::shift());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Wire(WireId(0))]
        );
        assert_eq!(selection_events.get(), 3);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints,
            "INV11: selection changes must not repaint SheetView"
        );

        // 3. Rubber-band drag across half of R1 in Intersecting mode (default), then Enclosed mode.
        let band_start = screen_of(
            &overlay,
            cx,
            Pt {
                x: -6 * g,
                y: -5 * g,
            },
        );
        let band_mid = screen_of(&overlay, cx, Pt { x: -4 * g, y: 0 });
        let band_end = screen_of(
            &overlay,
            cx,
            Pt {
                x: -2 * g,
                y: 5 * g,
            },
        );
        cx.simulate_mouse_down(band_start, MouseButton::Left, Modifiers::none());
        for i in 1..=10 {
            let t = i as f32 / 10.0;
            let p = point(
                band_start.x + (band_end.x - band_start.x) * t,
                band_start.y + (band_end.y - band_start.y) * t,
            );
            cx.simulate_mouse_move(p, MouseButton::Left, Modifiers::none());
        }
        assert!(cx.update(|_, cx| overlay.read(cx).rubber_band().unwrap().is_visible()));
        cx.simulate_mouse_up(band_end, MouseButton::Left, Modifiers::none());
        assert_eq!(cx.update(|_, cx| overlay.read(cx).rubber_band()), None);
        // Box x in [-6g, -2g] touches R1 (centred at -6g, half-width 2g) and Wire(0) (starts at -4g).
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0)), ItemId::Wire(WireId(0))]
        );

        // Same drag in Enclosed mode selects neither R1 nor Wire(0) (both only partly inside).
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.set_select_mode(SelectMode::Enclosed, cx));
        });
        cx.simulate_mouse_down(band_end, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(band_mid, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(band_start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(band_start, MouseButton::Left, Modifiers::none());
        assert!(cx.update(|_, cx| overlay.read(cx).selection().is_empty()));

        // Shift rubber-band adds Label(0) on top of a selected R1.
        cx.simulate_mouse_down(r1_pos, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(r1_pos, MouseButton::Left, Modifiers::none());
        let lbl_a = screen_of(
            &overlay,
            cx,
            Pt {
                x: -2 * g,
                y: 3 * g,
            },
        );
        let lbl_b = screen_of(&overlay, cx, Pt { x: 2 * g, y: 5 * g });
        cx.simulate_mouse_down(lbl_a, MouseButton::Left, Modifiers::shift());
        cx.simulate_mouse_move(lbl_b, MouseButton::Left, Modifiers::shift());
        cx.simulate_mouse_up(lbl_b, MouseButton::Left, Modifiers::shift());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0)), ItemId::Label(LabelId(0))]
        );

        // select_all also never repaints SheetView.
        cx.update(|_, cx| overlay.update(cx, |v, cx| v.select_all(cx)));
        assert_eq!(cx.update(|_, cx| overlay.read(cx).selection().len()), 4);

        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints,
            "INV11: SheetView paint count must remain unchanged across hover, selection, and rubber-band drags"
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_render_count(cx)),
            initial_sheet_renders,
            "INV11: CachedSheet render count must remain unchanged"
        );
        assert_eq!(sheet_notifies.get(), 0);

        // 4. Negative control: camera change, theme change, and on-screen doc change DO repaint
        // SheetView, while an off-screen doc change does NOT (D7).
        cx.update(|_, cx| {
            let mut cam = overlay.read(cx).camera().clone();
            cam.pan_by(point(10.0, 0.0));
            overlay.update(cx, |v, cx| v.set_camera(cam, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints + 1,
            "camera change must repaint SheetView"
        );
        assert_eq!(sheet_notifies.get(), 1);

        // Off-screen label add: dirty rect does not meet viewport -> SheetView stays cached.
        let off_screen_change = cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::AddLabel(Label {
                    at: Pt {
                        x: 200 * g,
                        y: 200 * g,
                    },
                    text: "FAR".into(),
                }))
                .unwrap()
            })
        });
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.on_doc_change(&off_screen_change, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints + 1,
            "D7: off-screen dirty rect must not repaint SheetView"
        );

        // On-screen label add: dirty rect meets viewport -> SheetView repaints.
        let on_screen_change = cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::AddLabel(Label {
                    at: Pt { x: 2 * g, y: 2 * g },
                    text: "NEAR".into(),
                }))
                .unwrap()
            })
        });
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.on_doc_change(&on_screen_change, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints + 2,
            "D7: on-screen dirty rect must repaint SheetView"
        );

        // Notifying the non-view Doc entity alone leaves CachedSheet cached (D7: SheetView
        // invalidation goes through `on_doc_change` dirty-rect filtering).
        cx.update(|_, cx| {
            doc.update(cx, |_, cx| cx.notify());
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints + 2,
            "D7: bare Doc notify must not bypass dirty-rect filtering"
        );

        // Theme change repaints SheetView.
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.set_theme(&GRAY, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints + 3,
            "D7: theme change must repaint SheetView"
        );
    }

    #[gpui::test]
    fn unfocused_first_click_triggers_gpui_focus_refresh(cx: &mut TestAppContext) {
        // Documents the pinned-GPUI focus-refresh precondition on INV11 (window.rs:2723-2733):
        // when the sheet is not yet focused, the first click calls `Window::focus` -> `refresh()`,
        // which busts `.cached()` for that single focus-transfer frame. Subsequent clicks while
        // focused do not repaint SheetView.
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |_, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            OverlayView::new(doc_clone, camera, &GRAPHITE, cx)
        });
        let g = GRID_NM;
        let paints_0 = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        assert_eq!(paints_0, 1);

        // First click transfers window focus -> GPUI refresh repaints CachedSheet once.
        let r1_pos = screen_of(&overlay, cx, Pt { x: -6 * g, y: 0 });
        cx.simulate_mouse_down(r1_pos, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(r1_pos, MouseButton::Left, Modifiers::none());
        let paints_1 = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        assert_eq!(
            paints_1,
            paints_0 + 1,
            "unfocused first click transfers focus and triggers GPUI Window::refresh"
        );

        // Once focused, further clicks and selection changes never repaint SheetView (INV11).
        let wire_pos = screen_of(&overlay, cx, Pt { x: 0, y: 0 });
        cx.simulate_mouse_down(wire_pos, MouseButton::Left, Modifiers::shift());
        cx.simulate_mouse_up(wire_pos, MouseButton::Left, Modifiers::shift());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            paints_1,
            "INV11: clicks once focused must not repaint SheetView"
        );
    }

    #[gpui::test]
    fn inv11_net_highlight_never_repaints_sheet(cx: &mut TestAppContext) {
        let g = GRID_NM;
        // Net A: Wire(0) (-8g,0)->(-2g,0) + Wire(1) (-2g,0)->(-2g,4g), plus label-bridged Wire(2) (4g,0)->(8g,0).
        // Net B: Wire(3) (-8g,8g)->(0,8g).
        let net_doc = Doc::from_items(
            [],
            [
                Wire {
                    a: Pt { x: -8 * g, y: 0 },
                    b: Pt { x: -2 * g, y: 0 },
                },
                Wire {
                    a: Pt { x: -2 * g, y: 0 },
                    b: Pt {
                        x: -2 * g,
                        y: 4 * g,
                    },
                },
                Wire {
                    a: Pt { x: 4 * g, y: 0 },
                    b: Pt { x: 8 * g, y: 0 },
                },
                Wire {
                    a: Pt {
                        x: -8 * g,
                        y: 8 * g,
                    },
                    b: Pt { x: 0, y: 8 * g },
                },
            ],
            [
                Label {
                    at: Pt { x: -8 * g, y: 0 },
                    text: "NET_A".into(),
                },
                Label {
                    at: Pt { x: 4 * g, y: 0 },
                    text: "NET_A".into(),
                },
            ],
        )
        .unwrap();

        let doc = cx.new(|_| net_doc);
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc_clone, camera, &GRAPHITE, cx);
            window.focus(view.focus_handle(), cx);
            view
        });
        let sheet_notifies = Rc::new(Cell::new(0u32));
        let sn = sheet_notifies.clone();
        cx.update(|_, cx| {
            let sheet = overlay.read(cx).sheet_entity().clone();
            cx.observe(&sheet, move |_, _| sn.set(sn.get() + 1))
                .detach();
        });

        let initial_sheet_paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let mut prev_overlay_paints = cx.update(|_, cx| overlay.read(cx).overlay_paint_count());
        assert_eq!(initial_sheet_paints, 1);

        let assert_overlay_painted =
            |cx: &mut gpui::VisualTestContext, prev: &mut u64, step: &str| {
                let now = cx.update(|_, cx| overlay.read(cx).overlay_paint_count());
                assert!(
                    now > *prev,
                    "T4.4: expected overlay frame to paint after {step} (prev={}, now={now})",
                    *prev
                );
                *prev = now;
            };

        // Hover Wire(0): whole Net A ([0, 1, 2]) highlights in overlay; SheetView does not repaint.
        let w0_pos = screen_of(&overlay, cx, Pt { x: -5 * g, y: 0 });
        let empty_pos = screen_of(
            &overlay,
            cx,
            Pt {
                x: -10 * g,
                y: -8 * g,
            },
        );
        cx.simulate_mouse_move(w0_pos, None, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).highlighted_wires().to_vec()),
            vec![WireId(0), WireId(1), WireId(2)]
        );
        assert_overlay_painted(cx, &mut prev_overlay_paints, "hover Wire(0)");

        // Hover empty space: highlight clears.
        cx.simulate_mouse_move(empty_pos, None, Modifiers::none());
        assert!(cx.update(|_, cx| overlay.read(cx).highlighted_wires().is_empty()));
        assert_overlay_painted(cx, &mut prev_overlay_paints, "unhover to empty space");

        // Click Wire(1) to select it: Net A highlights.
        let w1_pos = screen_of(
            &overlay,
            cx,
            Pt {
                x: -2 * g,
                y: 2 * g,
            },
        );
        cx.simulate_mouse_down(w1_pos, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(w1_pos, MouseButton::Left, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).highlighted_wires().to_vec()),
            vec![WireId(0), WireId(1), WireId(2)]
        );
        assert_overlay_painted(cx, &mut prev_overlay_paints, "select Wire(1)");

        // Hover Wire(3) while Wire(1) is selected: both Net A and Net B highlight.
        let w3_pos = screen_of(
            &overlay,
            cx,
            Pt {
                x: -4 * g,
                y: 8 * g,
            },
        );
        cx.simulate_mouse_move(w3_pos, None, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).highlighted_wires().to_vec()),
            vec![WireId(0), WireId(1), WireId(2), WireId(3)]
        );
        assert_overlay_painted(cx, &mut prev_overlay_paints, "hover Wire(3)");

        // Off-screen label edit merges `off_wid` into Net A: OverlayView re-resolves without
        // repainting SheetView (D7).
        let off_wire = cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::AddWires(vec![Wire {
                    a: Pt {
                        x: 200 * g,
                        y: 200 * g,
                    },
                    b: Pt {
                        x: 204 * g,
                        y: 200 * g,
                    },
                }]))
                .unwrap()
            })
        });
        let off_wid = off_wire
            .added
            .iter()
            .find_map(|item| match *item {
                ItemId::Wire(id) => Some(id),
                _ => None,
            })
            .unwrap();
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.on_doc_change(&off_wire, cx));
        });
        let off_label = cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::AddLabel(Label {
                    at: Pt {
                        x: 200 * g,
                        y: 200 * g,
                    },
                    text: "NET_A".into(),
                }))
                .unwrap()
            })
        });
        let off_lid = off_label
            .added
            .iter()
            .find_map(|item| match *item {
                ItemId::Label(id) => Some(id),
                _ => None,
            })
            .unwrap();
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.on_doc_change(&off_label, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).highlighted_wires().to_vec()),
            vec![WireId(0), WireId(1), WireId(2), WireId(3), off_wid]
        );
        assert_overlay_painted(cx, &mut prev_overlay_paints, "off-screen net merge");

        // Deleting the off-screen bridging label splits `off_wid` back out of Net A via
        // `on_doc_change`, updating the overlay without repainting SheetView.
        let split_change = cx.update(|_, cx| {
            doc.update(cx, |doc, _| {
                doc.apply(Command::Delete {
                    items: vec![ItemId::Label(off_lid)],
                    with_wires: false,
                })
                .unwrap()
            })
        });
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.on_doc_change(&split_change, cx));
        });
        cx.run_until_parked();
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).highlighted_wires().to_vec()),
            vec![WireId(0), WireId(1), WireId(2), WireId(3)]
        );
        assert_overlay_painted(cx, &mut prev_overlay_paints, "off-screen net split");

        // Clear selection and move hover away: net highlight clears.
        cx.simulate_mouse_move(empty_pos, None, Modifiers::none());
        assert_overlay_painted(cx, &mut prev_overlay_paints, "hover away before clear");
        cx.update(|_, cx| overlay.update(cx, |v, cx| v.clear_selection(cx)));
        cx.run_until_parked();
        assert!(cx.update(|_, cx| overlay.read(cx).highlighted_wires().is_empty()));
        assert_overlay_painted(cx, &mut prev_overlay_paints, "clear selection");

        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_sheet_paints,
            "INV11/T4.4: net highlight changes must never repaint SheetView"
        );
        assert_eq!(
            sheet_notifies.get(),
            0,
            "INV11/T4.4: net highlight changes must never notify SheetView"
        );
    }
}
