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

mod ghosts;
mod net;

pub use net::NetHighlight;

use std::{cell::Cell, rc::Rc};

use crate::{
    camera::Camera,
    camera_anim::{
        CameraAnimation, ViewAnimation, fit_area_camera, fit_rect_camera, fit_sheet_camera,
        viewport_center, zoom_to_selection_camera,
    },
    input::wheel_factor,
    selection::{RubberBand, SelectionState, SelectionUpdate, hit_tolerance_nm, item_exists},
    sheet::{SheetDocument, SheetView},
    snap_px::{centered_square, device_rect, round_device, stroke_device_px, to_device},
    tool::{self, ToolHost, ToolMachine, ToolRequest},
};
use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, ContentMask, Context, DispatchPhase, Edges, Element,
    ElementId, Entity, EventEmitter, FocusHandle, GlobalElementId, Hitbox, HitboxBehavior,
    HitboxId, Hsla, InspectorElementId, InteractiveElement, IntoElement, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Position, Render, Style,
    StyleRefinement, Styled, Subscription, WeakEntity, Window, div, fill, point, px, relative,
};
use omnia_doc::{
    Change, Doc, ItemId, NetId, Pt, Rect, Wire, WireId,
    prefs::{SelectMode, WiringPrefs},
};
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
    /// T5.3: dragged items hidden from `SheetView` while rendered as overlay ghosts (D7).
    hidden: Option<Rc<[ItemId]>>,
    paints: Rc<Cell<u64>>,
    renders: u64,
}

impl CachedSheet {
    pub fn new(camera: Camera, theme: &'static Theme, document: Option<SheetDocument>) -> Self {
        Self {
            camera,
            theme,
            document,
            hidden: None,
            paints: Rc::new(Cell::new(0)),
            renders: 0,
        }
    }

    /// Shared symbol masks from the cached document for overlay ghosts (T5.3).
    pub(crate) fn masks(&self) -> Option<&crate::sheet::symbols::Masks> {
        self.document.as_ref().map(SheetDocument::masks)
    }

    /// Currently hidden dragged items, if any (T5.3 tests).
    #[cfg(test)]
    pub(crate) fn hidden_items(&self) -> &[ItemId] {
        self.hidden.as_deref().unwrap_or(&[])
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
                    .with_hidden(self.hidden.clone())
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

/// Lets a caller attribute overlay hover/paint work separately from GPUI calls (B10, Q5).
pub trait OverlayPaintScope {
    fn omnia(&self, work: &mut dyn FnMut()) {
        work();
    }

    fn gpui(&self, work: &mut dyn FnMut()) {
        work();
    }
}

fn run_omnia<R>(scope: Option<&dyn OverlayPaintScope>, f: impl FnOnce() -> R) -> R {
    match scope {
        Some(s) => {
            let mut out = None;
            let mut f = Some(f);
            s.omnia(&mut || {
                if let Some(f) = f.take() {
                    out = Some(f());
                }
            });
            out.expect("OverlayPaintScope::omnia must execute its closure")
        }
        None => f(),
    }
}

fn run_gpui<R>(scope: Option<&dyn OverlayPaintScope>, f: impl FnOnce() -> R) -> R {
    match scope {
        Some(s) => {
            let mut out = None;
            let mut f = Some(f);
            s.gpui(&mut || {
                if let Some(f) = f.take() {
                    out = Some(f());
                }
            });
            out.expect("OverlayPaintScope::gpui must execute its closure")
        }
        None => f(),
    }
}

pub(super) fn paint_quad_scoped(
    window: &mut Window,
    quad: gpui::PaintQuad,
    scope: Option<&dyn OverlayPaintScope>,
) {
    run_gpui(scope, || window.paint_quad(quad));
}

/// T5.2: pointer capture taken by an [`OverlayView`] gesture (contract C4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CaptureRecord {
    window: AnyWindowHandle,
    hitbox: HitboxId,
    generation: u64,
}

/// T5.2: every path that ends a drag, rubber band, or pending press routes through
/// [`OverlayView::end_gesture`] with one of these reasons (contract C4).
#[derive(Clone, Copy, Debug)]
pub(crate) enum GestureEndReason<'a> {
    Release {
        event: &'a MouseUpEvent,
        owns_capture: bool,
    },
    Cancel,
    FreshPress,
    LostCapture,
    MoveOffSheet,
    ToolChange(tool::Tool),
    WindowDeactivated,
    DocumentSwitch,
    Attach,
    Detach,
    Drop,
    CancelRubberBand,
    HistoryStep,
}

/// Transient interactive overlay above a `.cached()` [`SheetView`] (D7, INV11).
///
/// Owns hover, selection, net-highlight, and rubber-band state, registers one sheet-wide hitbox
/// for pointer picking and rubber-band drags, and repaints only itself on transient updates.
///
/// Precondition (contract C1): an `OverlayView` belongs to exactly one window and is mounted in
/// that window for as long as it is attached to a [`crate::SheetCanvas`]. Remounting it in
/// another window is unsupported.
pub struct OverlayView {
    doc: Entity<Doc>,
    sheet: Entity<CachedSheet>,
    camera: Camera,
    theme: &'static Theme,
    state: SelectionState,
    /// T5.2: the tool state machine; it sees the pointer before selection does.
    tools: ToolMachine,
    /// T5.3: optional shell-supplied placement provider (`parts-plan.md` section 14).
    place_provider: Option<tool::PlaceProvider>,
    net_highlight: NetHighlight,
    focus: FocusHandle,
    viewport: Bounds<Pixels>,
    anim: Option<ViewAnimation>,
    renders: u64,
    paints: Rc<Cell<u64>>,
    scope: Option<Rc<dyn OverlayPaintScope>>,
    /// T5.2: the capture record set when a gesture takes capture, cleared only in `end_gesture`.
    capture: Rc<Cell<Option<CaptureRecord>>>,
    /// T5.2: counts the gestures that took a capture. GPUI reuses a hitbox id within a frame,
    /// so a deferred release compares this, not the id, to know no newer gesture captured.
    capture_gen: Rc<Cell<u64>>,
    /// T5.2: ends a pointer-held gesture when the window deactivates (set at first render).
    activation: Option<Subscription>,
    /// T5.2: the one window this overlay belongs to (contract C1), recorded at first render.
    window_handle: Option<AnyWindowHandle>,
}

impl EventEmitter<OverlayEvent> for OverlayView {}

impl OverlayView {
    pub fn new(
        doc: Entity<Doc>,
        camera: Camera,
        theme: &'static Theme,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_prefs(doc, camera, theme, &WiringPrefs::default(), cx)
    }

    /// Creates an [`OverlayView`] whose initial rubber-band mode comes from `prefs` (WD8, WD12).
    pub fn with_prefs(
        doc: Entity<Doc>,
        camera: Camera,
        theme: &'static Theme,
        prefs: &WiringPrefs,
        cx: &mut Context<Self>,
    ) -> Self {
        let document = SheetDocument::new(doc.clone());
        let sheet = cx.new(|_| CachedSheet::new(camera.clone(), theme, Some(document)));
        Self {
            doc,
            sheet,
            camera,
            theme,
            state: SelectionState::from_prefs(prefs),
            tools: ToolMachine::with_prefs(*prefs),
            place_provider: None,
            net_highlight: NetHighlight::new(),
            focus: cx.focus_handle(),
            viewport: Bounds::default(),
            anim: None,
            renders: 0,
            paints: Rc::new(Cell::new(0)),
            scope: None,
            capture: Rc::new(Cell::new(None)),
            capture_gen: Rc::new(Cell::new(0)),
            activation: None,
            window_handle: None,
        }
    }

    /// Installs an [`OverlayPaintScope`] to separate Omnia hover/paint allocations from GPUI (B10, Q5).
    pub fn with_paint_scope(mut self, scope: Rc<dyn OverlayPaintScope>) -> Self {
        self.scope = Some(scope);
        self
    }

    /// Replaces or clears the [`OverlayPaintScope`] used for B10 allocation accounting.
    pub fn set_paint_scope(&mut self, scope: Option<Rc<dyn OverlayPaintScope>>) {
        self.scope = scope;
    }

    /// T5.3: installs or clears the placement provider used when `Tool::Place` commits
    /// (`parts-plan.md` section 14). Defaults to [`omnia_symbols::SymbolLibrary::place_command`]
    /// when `None`.
    pub fn set_place_provider(&mut self, provider: Option<tool::PlaceProvider>) {
        self.place_provider = provider;
    }

    /// Syncs `CachedSheet.hidden` with the active drag items so `SheetView` hides dragged items
    /// once the drag lifts off (paint 1) and restores them on drag end (paint 2) (D7, INV11, B10).
    /// Plain clicks that never lift leave `hidden == None` (0 `SheetView` paints, INV11).
    fn sync_hidden_items(&mut self, cx: &mut Context<Self>) {
        let lifted_items: Option<&[ItemId]> = match self.tools.state() {
            tool::ToolState::Dragging(drag) if drag.is_lifted() && !drag.items.is_empty() => {
                Some(drag.items.as_slice())
            }
            tool::ToolState::DraggingSegment(seg)
                if seg.is_lifted() && !seg.hidden_items().is_empty() =>
            {
                Some(seg.hidden_items())
            }
            _ => None,
        };
        if self.sheet.read(cx).hidden.as_deref() == lifted_items {
            return;
        }
        debug_assert!(
            lifted_items.is_none_or(crate::sheet::is_canonically_sorted),
            "sync_hidden_items requires canonically sorted items"
        );
        let next = lifted_items.map(Rc::from);
        self.sheet.update(cx, |sheet, cx| {
            sheet.hidden = next;
            cx.notify();
        });
    }

    pub fn doc(&self) -> &Entity<Doc> {
        &self.doc
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    pub fn viewport(&self) -> Bounds<Pixels> {
        self.viewport
    }

    pub fn is_animating(&self) -> bool {
        self.anim.is_some()
    }

    /// Cancels any active view animation immediately from the current camera position (K13).
    pub fn cancel_animation(&mut self) -> bool {
        self.anim.take().is_some()
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

    pub(crate) fn is_rubber_banding(&self) -> bool {
        self.state.is_rubber_banding()
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

    /// Applies wiring preferences to the overlay (WD8, WD12; never notifies `SheetView`):
    /// `select_mode` at once; the follow policy at the next commit; and
    /// `delete_wires_with_part` at the next Delete, whichever path requests it (WD10, WQ4).
    pub fn apply_prefs(&mut self, prefs: &WiringPrefs, cx: &mut Context<Self>) {
        self.tools.set_prefs(*prefs);
        if self.state.apply_prefs(prefs) {
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
        self.end_gesture(GestureEndReason::CancelRubberBand, None, Some(cx));
    }

    /// Sets a visible rubber band directly in world coordinates for golden rendering or
    /// programmatic preview (never notifies `SheetView`, keeps `select_mode()` unchanged, and
    /// does not capture the pointer). If a pointer-driven rubber band is currently live, it is
    /// ended through [`Self::end_gesture`] first so its capture is released (C4).
    pub fn set_rubber_band_preview(
        &mut self,
        anchor: Pt,
        current: Pt,
        mode: SelectMode,
        cx: &mut Context<Self>,
    ) {
        if self.state.is_rubber_banding() {
            self.end_gesture(GestureEndReason::CancelRubberBand, None, Some(cx));
        }
        if self.state.set_rubber_band_preview(anchor, current, mode) {
            cx.notify();
        }
    }

    /// Sets a lifted segment-drag preview directly for golden rendering or programmatic
    /// inspection (WD7, WD11, W3.3). Does not capture the pointer; ends any active gesture first
    /// through [`Self::end_gesture`] (C4). Returns `false` if `wire` does not exist.
    pub fn set_segment_drag_preview(
        &mut self,
        wire: WireId,
        offset: i32,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.tools.holds_capture()
            || self.state.is_rubber_banding()
            || matches!(self.tools.state(), tool::ToolState::DraggingSegment(_))
        {
            self.end_gesture(GestureEndReason::Cancel, None, Some(cx));
        }
        let base = tool::DocStamp {
            key: tool::doc_key(&self.doc),
            revision: self.doc.read(cx).revision(),
        };
        let Some(gesture) =
            tool::SegmentDragGesture::for_preview(self.doc.read(cx), wire, offset, base)
        else {
            return false;
        };
        self.tools.set_segment_preview(gesture);
        self.sync_hidden_items(cx);
        cx.notify();
        true
    }

    /// Starts a 150 ms ease-out view animation (K13) from the current camera to `target`.
    pub fn animate_to(&mut self, target: Camera, cx: &mut Context<Self>) {
        let anim = CameraAnimation::new(self.camera.clone(), target, self.viewport);
        if anim.is_noop() {
            self.anim = None;
            return;
        }
        self.anim = Some(ViewAnimation::start(anim, cx));
        self.sheet.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Animates the camera to fit `rect` in the current viewport with default margins.
    pub fn fit_rect(&mut self, rect: Rect, cx: &mut Context<Self>) {
        self.animate_to(fit_rect_camera(rect, self.viewport), cx);
    }

    /// Animates the camera to fit `rect` edge-to-edge in the current viewport (K13, T5.6).
    pub fn fit_area(&mut self, rect: Rect, cx: &mut Context<Self>) {
        self.animate_to(fit_area_camera(rect, self.viewport), cx);
    }

    /// Animates the camera to fit all items in the sheet (or origin at 100% if empty).
    pub fn fit_sheet(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let target = fit_sheet_camera(Some(self.doc.read(cx)), self.viewport);
        self.animate_to(target, cx);
    }

    /// Animates the camera to fit the current selection. Returns `false` if nothing live is
    /// selected.
    pub fn zoom_to_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(target) =
            zoom_to_selection_camera(self.doc.read(cx), self.state.selection(), self.viewport)
        else {
            return false;
        };
        self.animate_to(target, cx);
        true
    }

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

    /// Updates the camera and invalidates both the cached sheet and the overlay when changed (D7).
    pub fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>) {
        self.anim = None;
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
        self.anim = None;
        // T5.2: no gesture survives a document swap; end_gesture releases its capture (C4).
        self.end_gesture(GestureEndReason::DocumentSwitch, None, Some(cx));
        debug_assert!(self.state.rubber_band().is_none());
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
        // W3.5: an edit can remove the hovered item without a pointer move (Delete with
        // `delete_wires_with_part` takes unselected runs). Drop the tool's hover mirror of a
        // removed item; a live one is left for the next pointer move, as before.
        if let tool::ToolState::Hovering(item) = *self.tools.state()
            && !item_exists(self.doc.read(cx), item)
        {
            self.tools.hover(None);
        }
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

    fn take_capture(&self, hitbox: HitboxId, window: &mut Window) {
        if let Some(recorded) = self.window_handle
            && recorded != window.window_handle()
        {
            debug_assert_eq!(
                recorded,
                window.window_handle(),
                "C1: OverlayView captured in a window other than its recorded one"
            );
            return;
        }
        let generation = self.capture_gen.get() + 1;
        self.capture_gen.set(generation);
        window.capture_pointer(hitbox);
        self.capture.set(Some(CaptureRecord {
            window: window.window_handle(),
            hitbox,
            generation,
        }));
    }

    /// T5.2: the single gesture-end and capture-release path (contract C4).
    ///
    /// Every path that ends a drag, rubber band, or pending press routes here; no other code
    /// clears those fields or releases capture. Releases immediately when `window` is the
    /// capture's window, or otherwise via a deferred update against the recorded window handle
    /// guarded by the capture generation so a newer capture is never released.
    pub(crate) fn end_gesture(
        &mut self,
        reason: GestureEndReason<'_>,
        window: Option<&mut Window>,
        mut cx: Option<&mut Context<Self>>,
    ) -> bool {
        let handled = match reason {
            GestureEndReason::Release {
                event,
                owns_capture,
            } => {
                if self.tools.routes_pointer() {
                    let world = self.camera.screen_to_world(to_f64(event.position));
                    let outcome = if self.tools.holds_capture() && !owns_capture {
                        self.tools.pointer_lost()
                    } else if let Some(cx) = cx.as_deref() {
                        let doc = self.doc.read(cx);
                        self.tools
                            .pointer_up_with_doc(doc, world, event.modifiers.alt)
                    } else {
                        self.tools.pointer_up(world, event.modifiers.alt)
                    };
                    let release = outcome.release;
                    if let Some(cx) = cx.as_deref_mut() {
                        tool::finish(self, outcome, cx);
                    }
                    release
                } else if self.state.is_rubber_banding() {
                    if owns_capture {
                        if let Some(cx) = cx.as_deref_mut() {
                            let update =
                                self.state.mouse_up(self.doc.read(cx), &self.camera, event);
                            self.apply_update(update, cx);
                        } else {
                            self.state.cancel_rubber_band();
                        }
                    } else {
                        let was_visible = self.state.cancel_rubber_band();
                        if was_visible && let Some(cx) = cx.as_deref_mut() {
                            cx.notify();
                        }
                    }
                    true
                } else {
                    false
                }
            }
            GestureEndReason::Cancel => {
                let mut outcome = self.tools.cancel();
                if !outcome.handled {
                    let had_band = self.state.is_rubber_banding();
                    let was_visible = self.state.cancel_rubber_band();
                    if had_band || was_visible {
                        outcome.handled = true;
                        outcome.preview_changed = true;
                    }
                }
                let handled = outcome.handled;
                if let Some(cx) = cx.as_deref_mut() {
                    tool::finish(self, outcome, cx);
                }
                handled
            }
            GestureEndReason::CancelRubberBand => {
                let held = self.state.is_rubber_banding();
                let was_visible = self.state.cancel_rubber_band();
                if (was_visible || held)
                    && let Some(cx) = cx.as_deref_mut()
                {
                    cx.notify();
                }
                was_visible || held
            }
            GestureEndReason::FreshPress => {
                let had_band = self.state.is_rubber_banding();
                let band_vis = if had_band {
                    self.state.cancel_rubber_band()
                } else {
                    false
                };
                let outcome = self.tools.pointer_lost();
                if (outcome.preview_changed || band_vis || had_band)
                    && let Some(cx) = cx.as_deref_mut()
                {
                    cx.notify();
                }
                outcome.handled || had_band || band_vis
            }
            GestureEndReason::LostCapture
            | GestureEndReason::MoveOffSheet
            | GestureEndReason::WindowDeactivated => {
                let had_band = self.state.is_rubber_banding();
                let band_vis = if had_band {
                    self.state.cancel_rubber_band()
                } else {
                    false
                };
                let mut outcome = self.tools.pointer_lost();
                outcome.preview_changed |= band_vis;
                let handled = outcome.handled || had_band || band_vis;
                if let Some(cx) = cx.as_deref_mut() {
                    tool::finish(self, outcome, cx);
                }
                handled
            }
            GestureEndReason::ToolChange(tool) => {
                let had_band = self.state.is_rubber_banding();
                let band_vis = self.state.cancel_rubber_band();
                let mut outcome = self.tools.set_tool(tool);
                outcome.preview_changed |= band_vis || had_band;
                let handled = outcome.handled;
                if let Some(cx) = cx.as_deref_mut() {
                    tool::finish(self, outcome, cx);
                }
                handled
            }
            GestureEndReason::DocumentSwitch
            | GestureEndReason::Attach
            | GestureEndReason::Detach => {
                let had_band = self.state.is_rubber_banding();
                let band_vis = self.state.cancel_rubber_band();
                let mut outcome = self.tools.document_replaced();
                outcome.preview_changed |= band_vis || had_band;
                let handled = outcome.handled || had_band || band_vis;
                if let Some(cx) = cx.as_deref_mut() {
                    tool::finish(self, outcome, cx);
                }
                handled
            }
            GestureEndReason::Drop => {
                self.state.cancel_rubber_band();
                self.tools.document_replaced();
                true
            }
            GestureEndReason::HistoryStep => {
                let had_band = self.state.is_rubber_banding();
                let band_vis = if had_band {
                    self.state.cancel_rubber_band()
                } else {
                    false
                };
                let mut outcome = self.tools.cancel();
                outcome.preview_changed |= band_vis || had_band;
                let handled = outcome.handled || had_band || band_vis;
                if let Some(cx) = cx.as_deref_mut() {
                    tool::finish(self, outcome, cx);
                }
                handled
            }
        };

        if let Some(cx) = cx.as_deref_mut() {
            self.sync_hidden_items(cx);
        }

        if !self.tools.holds_capture()
            && !self.state.is_rubber_banding()
            && let Some(record) = self.capture.take()
        {
            match window {
                Some(window) if window.window_handle() == record.window => {
                    if window.captured_hitbox() == Some(record.hitbox) {
                        window.release_pointer();
                    }
                }
                _ => {
                    if let Some(cx) = cx {
                        let capture_gen = self.capture_gen.clone();
                        cx.defer(move |cx| {
                            if capture_gen.get() != record.generation {
                                return;
                            }
                            record
                                .window
                                .update(cx, |_, window, _| {
                                    if window.captured_hitbox() == Some(record.hitbox) {
                                        window.release_pointer();
                                    }
                                })
                                .ok();
                        });
                    }
                }
            }
        }

        handled
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        bounds: Bounds<Pixels>,
        hitbox: HitboxId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.viewport = bounds;
        self.camera.set_viewport(bounds);
        // T5.2: a drag, band or pending press still live at a fresh press lost its release.
        // End it immediately through `end_gesture` so its capture is released before the press
        // runs (including a Shift-click while a band is live).
        if self.tools.holds_capture()
            || self.state.is_rubber_banding()
            || matches!(self.tools.state(), tool::ToolState::Placing(p) if p.pressed)
        {
            self.end_gesture(GestureEndReason::FreshPress, Some(window), Some(cx));
        }
        // T5.2: the tool sees the press first; a press it leaves goes to selection.
        let world = self.camera.screen_to_world(to_f64(event.position));
        let outcome = self.tools.pointer_down(
            self.doc.read(cx),
            tool::doc_key(&self.doc),
            world,
            hit_tolerance_nm(&self.camera),
            self.state.selection(),
            event.modifiers.shift,
        );
        if outcome.handled {
            let pressed = match self.tools.state() {
                tool::ToolState::Dragging(drag) => Some(drag.pressed),
                tool::ToolState::DraggingSegment(seg) => Some(ItemId::Wire(seg.wire)),
                _ => None,
            };
            if let Some(pressed) = pressed {
                // A press on an unselected item selects it; the drag then moves it alone.
                let mut update = SelectionUpdate {
                    visual_changed: self.state.set_hovered(Some(pressed)),
                    ..SelectionUpdate::default()
                };
                if !self.state.is_selected(pressed) && self.state.set_selection([pressed]) {
                    update.selection_changed = true;
                    update.visual_changed = true;
                }
                self.apply_update(update, cx);
            } else if matches!(
                self.tools.state(),
                tool::ToolState::ZoomingArea(zoom) if zoom.is_dragging()
            ) {
                // Cancel any active animation so the zoom rectangle stays anchored to the sheet.
                self.cancel_animation();
                cx.emit(tool::CanvasZoomArea::Cancel {
                    doc: tool::doc_key(&self.doc),
                });
            }
            let capture = tool::finish(self, outcome, cx).capture;
            if capture {
                self.take_capture(hitbox, window);
            }
            return capture;
        }
        let update = self
            .state
            .mouse_down(self.doc.read(cx), &self.camera, event);
        self.apply_update(update, cx);
        if update.started_drag {
            self.take_capture(hitbox, window);
        }
        update.started_drag
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        bounds: Bounds<Pixels>,
        owns_capture: bool,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scope = self.scope.clone();
        if self.tools.routes_pointer() {
            // T5.2: a gesture holds the pointer; hover and selection wait for it to end.
            self.viewport = bounds;
            self.camera.set_viewport(bounds);
            let world = self.camera.screen_to_world(to_f64(event.position));
            let left_held = event.pressed_button == Some(MouseButton::Left);
            if self.tools.holds_capture() && (!owns_capture || !left_held) {
                let reason = if hovered {
                    GestureEndReason::LostCapture
                } else {
                    GestureEndReason::MoveOffSheet
                };
                run_gpui(scope.as_deref(), || {
                    self.end_gesture(reason, Some(window), Some(cx));
                });
                return;
            }
            if !left_held && matches!(self.tools.state(), tool::ToolState::Placing(p) if p.pressed)
            {
                let reason = if hovered {
                    GestureEndReason::LostCapture
                } else {
                    GestureEndReason::MoveOffSheet
                };
                run_gpui(scope.as_deref(), || {
                    self.end_gesture(reason, Some(window), Some(cx));
                });
                if !hovered {
                    return;
                }
            }
            let doc = self.doc.read(cx);
            let outcome = self.tools.pointer_move_with_doc(doc, world, left_held);
            run_gpui(scope.as_deref(), || {
                tool::finish(self, outcome, cx);
            });
            return;
        }
        if self.state.is_rubber_banding() {
            let left_held = event.pressed_button == Some(MouseButton::Left);
            if !owns_capture || !left_held {
                let reason = if hovered {
                    GestureEndReason::LostCapture
                } else {
                    GestureEndReason::MoveOffSheet
                };
                run_gpui(scope.as_deref(), || {
                    self.end_gesture(reason, Some(window), Some(cx));
                });
                return;
            }
        }
        let update = run_omnia(scope.as_deref(), || {
            self.viewport = bounds;
            self.camera.set_viewport(bounds);
            let doc = self.doc.read(cx);
            let update = self.state.mouse_move(doc, &self.camera, event);
            if update.visual_changed || update.selection_changed {
                self.net_highlight
                    .sync(doc, self.state.hovered(), self.state.selection());
            }
            // Allocation-free: the tool mirrors hover as Idle/Hovering (T5.2).
            self.tools.hover(self.state.hovered());
            update
        });
        run_gpui(scope.as_deref(), || self.emit_and_notify(update, cx));
    }

    fn on_mouse_leave(&mut self, cx: &mut Context<Self>) {
        if self.tools.routes_pointer() {
            return;
        }
        let update = self.state.mouse_leave();
        self.tools.hover(self.state.hovered());
        self.apply_update(update, cx);
    }

    fn on_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        bounds: Bounds<Pixels>,
        owns_capture: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.viewport = bounds;
        self.camera.set_viewport(bounds);
        self.end_gesture(
            GestureEndReason::Release {
                event,
                owns_capture,
            },
            Some(window),
            Some(cx),
        );
    }
}

/// T5.2: the tool machine's view of the overlay (`tool::sheet_key_context`).
impl ToolHost for OverlayView {
    fn tools(&self) -> &ToolMachine {
        &self.tools
    }

    fn tools_mut(&mut self) -> &mut ToolMachine {
        &mut self.tools
    }

    fn tool_doc(&self) -> Entity<Doc> {
        self.doc.clone()
    }

    fn preview_changed(&mut self, cx: &mut Context<Self>) {
        self.sync_hidden_items(cx);
        cx.notify();
    }

    fn doc_changed(&mut self, change: &Change, cx: &mut Context<Self>) {
        self.on_doc_change(change, cx);
        // T5.2: each event names the document it came from, checked at delivery (C2).
        let key = tool::doc_key(&self.doc);
        let history = tool::CanvasHistory::of(key, self.doc.read(cx));
        cx.emit(tool::CanvasHistoryEvent(history));
        cx.emit(tool::CanvasEdit {
            doc: key,
            dirty: change.dirty,
        });
    }

    fn request(&mut self, request: ToolRequest, cx: &mut Context<Self>) {
        match request {
            ToolRequest::Click { item, base } => {
                // T5.2: a click selects its item only on the document it was made on, and only if
                // the item still exists there (another command may have deleted it mid-press).
                if tool::doc_key(&self.doc) == base.key && item_exists(self.doc.read(cx), item) {
                    self.set_selection([item], cx);
                }
            }
            ToolRequest::Place { symbol, at, orient } => {
                let base = tool::DocStamp {
                    key: tool::doc_key(&self.doc),
                    revision: self.doc.read(cx).revision(),
                };
                let command = tool::place::build_place_command(
                    self.doc.read(cx),
                    symbol,
                    at,
                    orient,
                    self.place_provider.as_ref(),
                );
                if let Some(command) = command {
                    tool::apply(self, tool::Commit { command, base }, cx);
                }
            }
            ToolRequest::ReorientSelection(by) => {
                let base = tool::DocStamp {
                    key: tool::doc_key(&self.doc),
                    revision: self.doc.read(cx).revision(),
                };
                let follow = self.tools.prefs().follow_policy(false);
                let command = tool::move_::reorient_selection_command(
                    self.doc.read(cx),
                    self.state.selection(),
                    by,
                    follow,
                );
                if let Some(command) = command {
                    tool::apply(self, tool::Commit { command, base }, cx);
                }
            }
            ToolRequest::DeleteSelection => {
                // W3.5: never delete mid-gesture (Dragging/Placing/Wiring) or during a live
                // pointer rubber-band drag, even if called directly through ToolHost::request (INV5).
                if !matches!(
                    self.tools.state(),
                    tool::ToolState::Idle | tool::ToolState::Hovering(_)
                ) || self.state.is_rubber_banding()
                {
                    return;
                }
                let base = tool::DocStamp {
                    key: tool::doc_key(&self.doc),
                    revision: self.doc.read(cx).revision(),
                };
                // W3.5: the Del key, both `delete_selection` methods and the shell's Delete
                // all arrive here, so the preference is read once, now (WD10, WQ4).
                let with_wires = self.tools.prefs().delete_wires_with_part;
                let command = tool::move_::delete_selection_command(
                    self.doc.read(cx),
                    self.state.selection(),
                    with_wires,
                );
                if let Some(command) = command {
                    tool::apply(self, tool::Commit { command, base }, cx);
                }
            }
            ToolRequest::ZoomArea(rect) => {
                self.fit_area(rect, cx);
                cx.emit(tool::CanvasZoomArea::Fit {
                    doc: tool::doc_key(&self.doc),
                    area: rect,
                });
            }
        }
    }
}

fn to_f64(p: gpui::Point<Pixels>) -> gpui::Point<f64> {
    point(f64::from(p.x), f64::from(p.y))
}

impl Render for OverlayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current_window = window.window_handle();
        if let Some(recorded) = self.window_handle {
            if recorded != current_window {
                debug_assert_eq!(
                    recorded, current_window,
                    "C1: OverlayView belongs to one window; remounting in another window is unsupported"
                );
                return div();
            }
        } else {
            self.window_handle = Some(current_window);
            // T5.2: a deactivated window may never deliver the release; end the gesture (C4).
            self.activation = Some(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.end_gesture(GestureEndReason::WindowDeactivated, Some(window), Some(cx));
                }
            }));
            // T5.2: a dropped overlay releases the capture it still holds (C4).
            cx.on_release_in(window, |this, window, _| {
                this.end_gesture(GestureEndReason::Drop, Some(window), None);
            })
            .detach();
        }
        self.renders += 1;
        if let Some(anim) = &self.anim {
            let (next_cam, finished) = anim.step(self.viewport, cx);
            self.camera = next_cam.clone();
            self.sheet.update(cx, |sheet, _| {
                sheet.camera = next_cam;
            });
            if finished {
                self.anim = None;
            } else {
                window.request_animation_frame();
                let overlay = cx.entity().downgrade();
                let sheet = self.sheet.clone();
                window.on_next_frame(move |_, cx| {
                    if overlay
                        .read_with(cx, |o, _| o.is_animating())
                        .unwrap_or(false)
                    {
                        sheet.update(cx, |_, cx| cx.notify());
                    }
                });
            }
        }
        tool::sheet_key_context(div(), cx)
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
                dragging: self.state.is_rubber_banding() || self.tools.holds_capture(),
                paints: self.paints.clone(),
                scope: self.scope.clone(),
                capture: self.capture.clone(),
                window_handle: current_window,
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
    scope: Option<Rc<dyn OverlayPaintScope>>,
    capture: Rc<Cell<Option<CaptureRecord>>>,
    window_handle: AnyWindowHandle,
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
            .update(cx, |overlay, _| {
                overlay.viewport = bounds;
                overlay.camera.set_viewport(bounds);
            })
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
        if window.window_handle() != self.window_handle {
            debug_assert_eq!(
                window.window_handle(),
                self.window_handle,
                "C1: OverlayView painted in a window other than its recorded one"
            );
            return;
        }
        self.paints.set(self.paints.get() + 1);

        if let Some(overlay) = self.overlay.upgrade() {
            let scope = self.scope.as_deref();
            run_omnia(scope, || {
                let mut camera = self.camera.clone();
                camera.set_viewport(bounds);
                let sf = window.scale_factor();
                let view = overlay.read(cx);
                let doc = self.doc.read(cx);
                let sheet = view.sheet.read(cx);
                let ghost_preview = match view.tools.state() {
                    tool::ToolState::Placing(place) => match place.at {
                        Some(at) => ghosts::GhostPreview::Place {
                            symbol: place.symbol,
                            at,
                            orient: place.orient,
                        },
                        None => ghosts::GhostPreview::None,
                    },
                    tool::ToolState::Dragging(drag)
                        if drag.is_lifted() && !drag.items.is_empty() =>
                    {
                        ghosts::GhostPreview::Drag {
                            items: &drag.items,
                            transform: drag.isometry(),
                        }
                    }
                    tool::ToolState::DraggingSegment(seg) if seg.is_lifted() => {
                        ghosts::GhostPreview::Segment {
                            wires: seg.preview_wires(),
                            blocker: seg.blocker(),
                        }
                    }
                    _ => ghosts::GhostPreview::None,
                };
                let zoom_area = match view.tools.state() {
                    tool::ToolState::ZoomingArea(zoom) => {
                        zoom.world_rect().filter(|_| zoom.is_visible())
                    }
                    _ => None,
                };
                let ctx = OverlayPaintContext {
                    doc,
                    camera: &camera,
                    bounds,
                    sf,
                    theme: self.theme,
                    masks: sheet.masks(),
                    scope,
                };
                paint_overlay(
                    &view.state,
                    &view.net_highlight,
                    ghost_preview,
                    zoom_area,
                    &ctx,
                    window,
                    cx,
                );
            });
        }

        if self.dragging
            && let Some(record) = self.capture.get()
            && record.window == window.window_handle()
            && window.captured_hitbox() == Some(record.hitbox)
        {
            // T5.2: move a capture this overlay still holds to the new hitbox. A capture GPUI
            // dropped (any mouse-up clears it) is not reclaimed; the next event ends the gesture.
            window.capture_pointer(hitbox.id);
            self.capture.set(Some(CaptureRecord {
                hitbox: hitbox.id,
                ..record
            }));
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
                .update(cx, |overlay, cx| {
                    overlay.on_mouse_down(event, bounds, hit.id, window, cx)
                })
                .unwrap_or(false);
            if started {
                cx.stop_propagation();
            }
        });

        let (overlay, hit) = (self.overlay.clone(), hitbox.clone());
        let capture = self.capture.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let owns = capture.get().is_some_and(|rec| {
                rec.window == window.window_handle() && window.captured_hitbox() == Some(rec.hitbox)
            });
            let hovered = hit.is_hovered(window);
            let left_held = event.pressed_button == Some(MouseButton::Left);
            // T5.2: a drag or band that lost its capture, or a pending Place press whose release
            // was lost, ends wherever the pointer is, before hover routing could treat it as
            // leaving the sheet.
            let lost = overlay
                .read_with(cx, |overlay, _| {
                    (!owns && (overlay.tools.holds_capture() || overlay.state.is_rubber_banding()))
                        || (!left_held
                            && matches!(
                                overlay.tools.state(),
                                tool::ToolState::Placing(p) if p.pressed
                            ))
                })
                .unwrap_or(false);
            if hovered || lost {
                overlay
                    .update(cx, |overlay, cx| {
                        overlay.on_mouse_move(event, bounds, owns, hovered, window, cx);
                    })
                    .ok();
            } else {
                overlay
                    .update(cx, |overlay, cx| overlay.on_mouse_leave(cx))
                    .ok();
            }
        });

        let overlay = self.overlay.clone();
        let capture = self.capture.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            // GPUI clears the capture only after the listeners run, so ownership is current.
            let owns = capture.get().is_some_and(|rec| {
                rec.window == window.window_handle() && window.captured_hitbox() == Some(rec.hitbox)
            });
            overlay
                .update(cx, |overlay, cx| {
                    overlay.on_mouse_up(event, bounds, owns, window, cx);
                })
                .ok();
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
fn paint_solid_outline(
    rect: DevRect,
    thickness: u32,
    color: Hsla,
    sf: f32,
    scope: Option<&dyn OverlayPaintScope>,
    window: &mut Window,
) {
    let t = thickness.max(1);
    let w = rect.width;
    let h = rect.height;
    let th = t.min(h);
    let tw = t.min(w);
    // Top and bottom bars.
    paint_quad_scoped(
        window,
        fill(device_rect(rect.left, rect.top, w, th, sf), color),
        scope,
    );
    if h > th {
        paint_quad_scoped(
            window,
            fill(
                device_rect(rect.left, rect.top + f64::from(h - th), w, th, sf),
                color,
            ),
            scope,
        );
    }
    // Left and right inner bars.
    if h > th.saturating_mul(2) {
        let inner_h = h - th * 2;
        let inner_top = rect.top + f64::from(th);
        paint_quad_scoped(
            window,
            fill(device_rect(rect.left, inner_top, tw, inner_h, sf), color),
            scope,
        );
        if w > tw {
            paint_quad_scoped(
                window,
                fill(
                    device_rect(rect.left + f64::from(w - tw), inner_top, tw, inner_h, sf),
                    color,
                ),
                scope,
            );
        }
    }
}

/// Paints a dashed four-edge rectangle outline (used for Intersecting rubber-band mode).
fn paint_dashed_outline(
    rect: DevRect,
    thickness: u32,
    color: Hsla,
    sf: f32,
    scope: Option<&dyn OverlayPaintScope>,
    window: &mut Window,
) {
    let dash = stroke_device_px(BAND_DASH_PX, sf);
    let gap = stroke_device_px(BAND_GAP_PX, sf);
    let step = dash.saturating_add(gap).max(1);
    let t = thickness.max(1);
    if rect.width <= dash || rect.height <= dash {
        paint_solid_outline(rect, t, color, sf, scope, window);
        return;
    }
    let bottom_y = rect.top + f64::from(rect.height.saturating_sub(t));
    let right_x = rect.left + f64::from(rect.width.saturating_sub(t));
    // Horizontal edges (top & bottom).
    let mut x = 0u32;
    while x < rect.width {
        let seg = dash.min(rect.width - x);
        paint_quad_scoped(
            window,
            fill(
                device_rect(rect.left + f64::from(x), rect.top, seg, t, sf),
                color,
            ),
            scope,
        );
        paint_quad_scoped(
            window,
            fill(
                device_rect(rect.left + f64::from(x), bottom_y, seg, t, sf),
                color,
            ),
            scope,
        );
        x = x.saturating_add(step);
    }
    // Vertical edges (left & right).
    let mut y = 0u32;
    while y < rect.height {
        let seg = dash.min(rect.height - y);
        paint_quad_scoped(
            window,
            fill(
                device_rect(rect.left, rect.top + f64::from(y), t, seg, sf),
                color,
            ),
            scope,
        );
        paint_quad_scoped(
            window,
            fill(
                device_rect(right_x, rect.top + f64::from(y), t, seg, sf),
                color,
            ),
            scope,
        );
        y = y.saturating_add(step);
    }
}

/// Paints four crisp corner handle squares on a selected part frame.
fn paint_corner_handles(
    rect: DevRect,
    side: u32,
    color: Hsla,
    sf: f32,
    scope: Option<&dyn OverlayPaintScope>,
    window: &mut Window,
) {
    let right = rect.left + f64::from(rect.width);
    let bottom = rect.top + f64::from(rect.height);
    let scale = f64::from(sf);
    for (x, y) in [
        (rect.left, rect.top),
        (right, rect.top),
        (rect.left, bottom),
        (right, bottom),
    ] {
        paint_quad_scoped(
            window,
            fill(
                centered_square(point(x / scale, y / scale), side, sf),
                color,
            ),
            scope,
        );
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
    masks: Option<&'a crate::sheet::symbols::Masks>,
    scope: Option<&'a dyn OverlayPaintScope>,
}

fn paint_overlay(
    state: &SelectionState,
    net_highlight: &NetHighlight,
    ghost_preview: ghosts::GhostPreview<'_>,
    zoom_area: Option<Rect>,
    ctx: &OverlayPaintContext<'_>,
    window: &mut Window,
    cx: &App,
) {
    let OverlayPaintContext {
        doc,
        camera,
        bounds,
        sf,
        theme,
        masks,
        scope,
    } = *ctx;
    let is_dragging_ghosts = matches!(
        ghost_preview,
        ghosts::GhostPreview::Drag { .. } | ghosts::GhostPreview::Segment { .. }
    );
    let hovered = if is_dragging_ghosts {
        None
    } else {
        state.hovered()
    };
    let selection = if is_dragging_ghosts {
        &[][..]
    } else {
        state.selection()
    };
    let band = state.rubber_band().filter(|b| b.is_visible());
    if hovered.is_none()
        && selection.is_empty()
        && band.is_none()
        && zoom_area.is_none()
        && (is_dragging_ghosts || net_highlight.is_empty())
        && !ghost_preview.is_active()
    {
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

    run_gpui(scope, || {
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // Layer 1: translucent fills and halos (always underneath crisp outlines).
            window.paint_layer(|window| {
                run_omnia(scope, || {
                    // Hovered item halo (skip when already selected so selection remains visually dominant).
                    if let Some(item) = hovered
                        && !state.is_selected(item)
                        && item_world_bounds(doc, item)
                            .is_some_and(|r| r.intersects(visible_padded))
                    {
                        match item {
                            ItemId::Part(id) => {
                                if let Some(body) = doc.part_body(id)
                                    && let Some(rect) =
                                        padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                                {
                                    paint_quad_scoped(
                                        window,
                                        fill(rect.bounds(sf), hover_color.opacity(0.18)),
                                        scope,
                                    );
                                }
                            }
                            ItemId::Wire(id) => {
                                if let Some(&wire) = doc.wire(id)
                                    && let Some(rect) =
                                        wire_rect_dev(wire, camera, sf, wire_halo_dev, clip)
                                {
                                    paint_quad_scoped(
                                        window,
                                        fill(rect.bounds(sf), hover_color.opacity(0.55)),
                                        scope,
                                    );
                                }
                            }
                            ItemId::Label(id) => {
                                if let Some(label) = doc.label(id)
                                    && let Some(rect) =
                                        label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                                {
                                    paint_quad_scoped(
                                        window,
                                        fill(rect.bounds(sf), hover_color.opacity(0.25)),
                                        scope,
                                    );
                                }
                            }
                        }
                    }

                    // Selected item fills / halos.
                    for &item in selection {
                        if !item_world_bounds(doc, item)
                            .is_some_and(|r| r.intersects(visible_padded))
                        {
                            continue;
                        }
                        match item {
                            ItemId::Part(id) => {
                                if let Some(body) = doc.part_body(id)
                                    && let Some(rect) =
                                        padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                                {
                                    paint_quad_scoped(
                                        window,
                                        fill(rect.bounds(sf), halo_color.opacity(0.22)),
                                        scope,
                                    );
                                }
                            }
                            ItemId::Wire(id) => {
                                if let Some(&wire) = doc.wire(id)
                                    && let Some(rect) =
                                        wire_rect_dev(wire, camera, sf, wire_halo_dev, clip)
                                {
                                    paint_quad_scoped(
                                        window,
                                        fill(rect.bounds(sf), halo_color.opacity(0.50)),
                                        scope,
                                    );
                                }
                            }
                            ItemId::Label(id) => {
                                if let Some(label) = doc.label(id)
                                    && let Some(rect) =
                                        label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                                {
                                    paint_quad_scoped(
                                        window,
                                        fill(rect.bounds(sf), halo_color.opacity(0.32)),
                                        scope,
                                    );
                                }
                            }
                        }
                    }

                    // Rubber-band translucent fill.
                    if let Some(band) = band
                        && let Some(rect) =
                            padded_rect_dev(band.world_rect(), 0.0, camera, sf, clip)
                    {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(sf), halo_color.opacity(0.22)),
                            scope,
                        );
                    }

                    // Zoom-area translucent fill (T5.6).
                    if let Some(area) = zoom_area
                        && let Some(rect) = padded_rect_dev(area, 0.0, camera, sf, clip)
                    {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(sf), halo_color.opacity(0.22)),
                            scope,
                        );
                    }
                });
            });

            // Layer 2: crisp net highlights, outlines, wire highlights, corner handles, and rubber-band border.
            window.paint_layer(|window| {
                run_omnia(scope, || {
                    // T4.4: whole-net wire highlight in `stage/net-highlight` (underneath selection handles).
                    // Skip while dragging ghosts so hidden/dragged wires do not leave stationary highlights behind.
                    if !is_dragging_ghosts {
                        let wire_paint = net::WirePaintParams {
                            camera,
                            visible_padded,
                            sf,
                            width_dev: wire_select_dev,
                            clip,
                            theme,
                            scope,
                        };
                        net::paint_wires(doc, net_highlight.wires(), &wire_paint, window);
                    }

                    // Hovered item outline (hovered wires are painted by `net::paint_wires` above, with
                    // their Layer 1 hover halo indicating the hovered segment).
                    if let Some(item) = hovered
                        && !state.is_selected(item)
                        && item_world_bounds(doc, item)
                            .is_some_and(|r| r.intersects(visible_padded))
                    {
                        match item {
                            ItemId::Part(id) => {
                                if let Some(body) = doc.part_body(id)
                                    && let Some(rect) =
                                        padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                                {
                                    paint_solid_outline(
                                        rect,
                                        hover_outline_dev,
                                        hover_color,
                                        sf,
                                        scope,
                                        window,
                                    );
                                }
                            }
                            ItemId::Wire(_) => {}
                            ItemId::Label(id) => {
                                if let Some(label) = doc.label(id)
                                    && let Some(rect) =
                                        label_rect_dev(label.at, LABEL_HALF_PX, camera, sf, clip)
                                {
                                    paint_solid_outline(
                                        rect,
                                        hover_outline_dev,
                                        hover_color,
                                        sf,
                                        scope,
                                        window,
                                    );
                                }
                            }
                        }
                    }

                    // Selected item outlines, wire strokes, and handles.
                    for &item in selection {
                        if !item_world_bounds(doc, item)
                            .is_some_and(|r| r.intersects(visible_padded))
                        {
                            continue;
                        }
                        match item {
                            ItemId::Part(id) => {
                                if let Some(body) = doc.part_body(id)
                                    && let Some(rect) =
                                        padded_rect_dev(body, PART_FRAME_PAD_PX, camera, sf, clip)
                                {
                                    paint_solid_outline(
                                        rect,
                                        select_outline_dev,
                                        select_color,
                                        sf,
                                        scope,
                                        window,
                                    );
                                    paint_corner_handles(
                                        rect,
                                        handle_dev,
                                        select_color,
                                        sf,
                                        scope,
                                        window,
                                    );
                                }
                            }
                            ItemId::Wire(id) => {
                                if let Some(&wire) = doc.wire(id) {
                                    if let Some(rect) =
                                        wire_rect_dev(wire, camera, sf, wire_select_dev, clip)
                                    {
                                        paint_quad_scoped(
                                            window,
                                            fill(rect.bounds(sf), select_color),
                                            scope,
                                        );
                                    }
                                    for end in [wire.a, wire.b] {
                                        let screen = camera.world_to_screen(end);
                                        paint_quad_scoped(
                                            window,
                                            fill(
                                                centered_square(screen, handle_dev, sf),
                                                select_color,
                                            ),
                                            scope,
                                        );
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
                                            scope,
                                            window,
                                        );
                                    }
                                    let screen = camera.world_to_screen(label.at);
                                    let dot = stroke_device_px(3.0, sf);
                                    paint_quad_scoped(
                                        window,
                                        fill(centered_square(screen, dot, sf), select_color),
                                        scope,
                                    );
                                }
                            }
                        }
                    }

                    // Rubber-band border: dashed for Intersecting (WD8/WQ3 default), solid for Enclosed.
                    if let Some(band) = band
                        && let Some(rect) =
                            padded_rect_dev(band.world_rect(), 0.0, camera, sf, clip)
                    {
                        match band.mode {
                            SelectMode::Intersecting => {
                                paint_dashed_outline(
                                    rect,
                                    band_border_dev,
                                    select_color,
                                    sf,
                                    scope,
                                    window,
                                );
                            }
                            SelectMode::Enclosed => {
                                paint_solid_outline(
                                    rect,
                                    band_border_dev,
                                    select_color,
                                    sf,
                                    scope,
                                    window,
                                );
                            }
                        }
                    }

                    // Zoom-area solid border (T5.6).
                    if let Some(area) = zoom_area
                        && let Some(rect) = padded_rect_dev(area, 0.0, camera, sf, clip)
                    {
                        paint_solid_outline(rect, band_border_dev, select_color, sf, scope, window);
                    }
                });
            });

            // T5.3: placement preview and drag-move ghosts (D7, INV6, INV11).
            if ghost_preview.is_active()
                && let Some(masks) = masks
            {
                let ghost_params = ghosts::GhostPaintParams {
                    doc,
                    camera,
                    bounds,
                    visible_padded,
                    sf,
                    clip,
                    theme,
                    masks,
                    scope,
                };
                ghosts::paint_ghosts(ghost_preview, &ghost_params, window, cx);
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
        open_overlay_with_prefs(cx, WiringPrefs::default())
    }

    fn open_overlay_with_prefs(
        cx: &mut TestAppContext,
        prefs: WiringPrefs,
    ) -> (OverlayFixture, &mut VisualTestContext) {
        let doc = cx.new(|_| test_doc());
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::with_prefs(doc_clone, camera, &GRAPHITE, &prefs, cx);
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

    fn drag_rubber_band(
        overlay: &Entity<OverlayView>,
        cx: &mut VisualTestContext,
        start: Pt,
        end: Pt,
        shift: bool,
    ) {
        let mods = if shift {
            Modifiers::shift()
        } else {
            Modifiers::none()
        };
        let s0 = screen_of(overlay, cx, start);
        let s1 = screen_of(overlay, cx, end);
        let mid = point((s0.x + s1.x) * 0.5, (s0.y + s1.y) * 0.5);
        cx.simulate_mouse_down(s0, MouseButton::Left, mods);
        cx.simulate_mouse_move(mid, MouseButton::Left, mods);
        cx.simulate_mouse_move(s1, MouseButton::Left, mods);
        assert!(
            cx.update(|_, cx| overlay.read(cx).rubber_band().unwrap().is_visible()),
            "rubber band must become visible after drag move"
        );
        cx.simulate_mouse_up(s1, MouseButton::Left, mods);
        assert_eq!(cx.update(|_, cx| overlay.read(cx).rubber_band()), None);
    }

    fn run_tc_band_half_part(
        cx: &mut TestAppContext,
        mode: SelectMode,
        start_world: Pt,
        end_world: Pt,
    ) {
        let prefs = WiringPrefs::new(true, 10, mode, false).unwrap();
        let (
            OverlayFixture {
                overlay,
                sheet_notifies,
                selection_events,
                ..
            },
            cx,
        ) = open_overlay_with_prefs(cx, prefs);
        assert_eq!(cx.update(|_, cx| overlay.read(cx).select_mode()), mode);
        let initial_paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));

        drag_rubber_band(&overlay, cx, start_world, end_world, false);

        match mode {
            SelectMode::Intersecting => {
                assert_eq!(
                    cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
                    vec![ItemId::Part(PartId(0))]
                );
                assert_eq!(selection_events.get(), 1);
            }
            SelectMode::Enclosed => {
                assert!(cx.update(|_, cx| overlay.read(cx).selection().is_empty()));
                assert_eq!(selection_events.get(), 0);
            }
        }
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_paints
        );
        assert_eq!(sheet_notifies.get(), 0);
    }

    // Half-covering box over R1 (body x in [-8g, -4g], y in [-2g, 2g]): x in [-10g, -6g], y in [-4g, 4g].
    #[gpui::test]
    fn tc_band_01_nw_to_se_intersecting(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Intersecting,
            Pt {
                x: -10 * g,
                y: -4 * g,
            },
            Pt {
                x: -6 * g,
                y: 4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_02_se_to_nw_intersecting(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Intersecting,
            Pt {
                x: -6 * g,
                y: 4 * g,
            },
            Pt {
                x: -10 * g,
                y: -4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_03_ne_to_sw_intersecting(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Intersecting,
            Pt {
                x: -6 * g,
                y: -4 * g,
            },
            Pt {
                x: -10 * g,
                y: 4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_04_sw_to_ne_intersecting(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Intersecting,
            Pt {
                x: -10 * g,
                y: 4 * g,
            },
            Pt {
                x: -6 * g,
                y: -4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_05_nw_to_se_enclosed(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Enclosed,
            Pt {
                x: -10 * g,
                y: -4 * g,
            },
            Pt {
                x: -6 * g,
                y: 4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_06_se_to_nw_enclosed(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Enclosed,
            Pt {
                x: -6 * g,
                y: 4 * g,
            },
            Pt {
                x: -10 * g,
                y: -4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_07_ne_to_sw_enclosed(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Enclosed,
            Pt {
                x: -6 * g,
                y: -4 * g,
            },
            Pt {
                x: -10 * g,
                y: 4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_08_sw_to_ne_enclosed(cx: &mut TestAppContext) {
        let g = GRID_NM;
        run_tc_band_half_part(
            cx,
            SelectMode::Enclosed,
            Pt {
                x: -10 * g,
                y: 4 * g,
            },
            Pt {
                x: -6 * g,
                y: -4 * g,
            },
        );
    }

    #[gpui::test]
    fn tc_band_09_shift_adds_to_existing_selection(cx: &mut TestAppContext) {
        let g = GRID_NM;
        let (
            OverlayFixture {
                overlay,
                sheet_notifies,
                selection_events,
                ..
            },
            cx,
        ) = open_overlay(cx);
        let initial_paints = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).select_mode()),
            SelectMode::Intersecting
        );

        // Existing selection: Label(0) at (0, 4g).
        let label_pos = screen_of(&overlay, cx, Pt { x: 0, y: 4 * g });
        cx.simulate_mouse_down(label_pos, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(label_pos, MouseButton::Left, Modifiers::none());
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Label(LabelId(0))]
        );

        // Shift-drag half-box over R1 in default Intersecting mode -> adds Part(0).
        drag_rubber_band(
            &overlay,
            cx,
            Pt {
                x: -10 * g,
                y: -4 * g,
            },
            Pt {
                x: -6 * g,
                y: 4 * g,
            },
            true,
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0)), ItemId::Label(LabelId(0))]
        );

        // Switch preference to Enclosed via apply_prefs; Shift-drag half-box over C1 -> keeps existing selection.
        let enclosed_prefs = WiringPrefs::new(true, 10, SelectMode::Enclosed, false).unwrap();
        cx.update(|_, cx| {
            overlay.update(cx, |v, cx| v.apply_prefs(&enclosed_prefs, cx));
        });
        drag_rubber_band(
            &overlay,
            cx,
            Pt {
                x: 6 * g,
                y: -4 * g,
            },
            Pt {
                x: 10 * g,
                y: 4 * g,
            },
            true,
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![ItemId::Part(PartId(0)), ItemId::Label(LabelId(0))]
        );

        // Shift-drag full box enclosing C1 ([3g..10g] x [-4g..4g], avoiding Wire(0) wholly) -> adds Part(1).
        drag_rubber_band(
            &overlay,
            cx,
            Pt {
                x: 10 * g,
                y: 4 * g,
            },
            Pt {
                x: 3 * g,
                y: -4 * g,
            },
            true,
        );
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).selection().to_vec()),
            vec![
                ItemId::Part(PartId(0)),
                ItemId::Part(PartId(1)),
                ItemId::Label(LabelId(0)),
            ]
        );
        assert_eq!(selection_events.get(), 3);
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            initial_paints
        );
        assert_eq!(sheet_notifies.get(), 0);
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

    #[gpui::test]
    fn overlay_paint_scope_attributes_mouse_move_and_paint(cx: &mut TestAppContext) {
        struct CountingScope {
            omnia: Rc<Cell<u64>>,
            gpui: Rc<Cell<u64>>,
        }
        impl OverlayPaintScope for CountingScope {
            fn omnia(&self, work: &mut dyn FnMut()) {
                self.omnia.set(self.omnia.get() + 1);
                work();
            }
            fn gpui(&self, work: &mut dyn FnMut()) {
                self.gpui.set(self.gpui.get() + 1);
                work();
            }
        }

        let omnia_calls = Rc::new(Cell::new(0u64));
        let gpui_calls = Rc::new(Cell::new(0u64));
        let scope: Rc<dyn OverlayPaintScope> = Rc::new(CountingScope {
            omnia: omnia_calls.clone(),
            gpui: gpui_calls.clone(),
        });

        let doc = cx.new(|_| test_doc());
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = OverlayView::new(doc, camera, &GRAPHITE, cx).with_paint_scope(scope);
            window.focus(view.focus_handle(), cx);
            view
        });

        let omnia_before = omnia_calls.get();
        let gpui_before = gpui_calls.get();
        let wire_pos = screen_of(&overlay, cx, Pt { x: 0, y: 0 });
        cx.simulate_mouse_move(wire_pos, None, Modifiers::none());
        assert!(
            omnia_calls.get() > omnia_before,
            "B10/T4.6: OverlayPaintScope::omnia must wrap hover and paint work"
        );
        assert!(
            gpui_calls.get() > gpui_before,
            "B10/T4.6: OverlayPaintScope::gpui must wrap GPUI notify/paint calls"
        );
    }
}
