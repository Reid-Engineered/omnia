//! T4.3: item selection, hover tracking, and WD8 rubber-band logic.
//!
//! Pure state machine over [`Doc`] and [`Camera`], separated from GPUI painting so every
//! click, Shift-click, hover, and rubber-band rule is tested directly as well as through
//! simulated GPUI input in [`crate::overlay`].
//!
//! Rules implemented:
//! - K14: hit tolerance is [`HIT_TOLERANCE_PX`] (4 logical px), converted to nanometres via
//!   the camera scale with `ceil` for inclusive Euclidean acceptance.
//! - WD8 / WQ3: rubber-band rectangles are normalised so drag direction never changes the
//!   result; both [`SelectMode::Intersecting`] (default, via [`Doc::query_touching`]) and
//!   [`SelectMode::Enclosed`] (via [`Doc::query_within`]) are supported; Shift adds to the
//!   existing selection instead of replacing it.
//! - Determinism: [`Selection`] stores live items in canonical `(kind, id)` order with no
//!   duplicates, independent of spatial-index iteration order.

use crate::camera::Camera;
use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, point};
use omnia_doc::{
    Doc, ItemId, Pt, Rect, hit_test,
    prefs::{SelectMode, WiringPrefs},
};

/// K14: screen-space hit tolerance in logical pixels.
pub const HIT_TOLERANCE_PX: f64 = 4.0;

/// Canonical sort key matching `omnia_doc::hit` and `SheetView` (Part < Wire < Label, then ID).
#[inline]
pub fn item_sort_key(item: ItemId) -> (u8, u32) {
    match item {
        ItemId::Part(id) => (0, id.0),
        ItemId::Wire(id) => (1, id.0),
        ItemId::Label(id) => (2, id.0),
    }
}

/// Converts [`HIT_TOLERANCE_PX`] (K14) into an inclusive nanometre tolerance at `camera`'s scale.
pub fn hit_tolerance_nm(camera: &Camera) -> i32 {
    let scale = camera.scale();
    debug_assert!(scale.is_finite() && scale > 0.0);
    (HIT_TOLERANCE_PX / scale)
        .ceil()
        .clamp(0.0, f64::from(i32::MAX)) as i32
}

/// Normalises two arbitrary world corners into a valid [`Rect`] (`min <= max`, WD8).
pub fn normalize_world_rect(a: Pt, b: Pt) -> Rect {
    Rect {
        min: Pt {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
        },
        max: Pt {
            x: a.x.max(b.x),
            y: a.y.max(b.y),
        },
    }
}

/// Queries `doc` for items selected by `area` under `mode` (WD8), writing canonical sorted,
/// deduplicated results into `out`.
pub fn query_rubber_band(doc: &Doc, area: Rect, mode: SelectMode, out: &mut Vec<ItemId>) {
    let area = normalize_world_rect(area.min, area.max);
    match mode {
        SelectMode::Intersecting => doc.query_touching(area, out),
        SelectMode::Enclosed => doc.query_within(area, out),
    }
    out.sort_unstable_by_key(|&item| item_sort_key(item));
    out.dedup();
}

/// Returns whether `item` currently exists in `doc`.
pub fn item_exists(doc: &Doc, item: ItemId) -> bool {
    match item {
        ItemId::Part(id) => doc.part(id).is_some(),
        ItemId::Wire(id) => doc.wire(id).is_some(),
        ItemId::Label(id) => doc.label(id).is_some(),
    }
}

/// Canonical, deduplicated set of selected document items.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    items: Vec<ItemId>,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn as_slice(&self) -> &[ItemId] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn contains(&self, item: ItemId) -> bool {
        let key = item_sort_key(item);
        self.items
            .binary_search_by_key(&key, |&existing| item_sort_key(existing))
            .is_ok()
    }

    pub fn clear(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.items.clear();
        true
    }

    /// Replaces the selection with `[item]`. Returns whether the selection changed.
    pub fn select_single(&mut self, item: ItemId) -> bool {
        if self.items.len() == 1 && self.items[0] == item {
            return false;
        }
        self.items.clear();
        self.items.push(item);
        true
    }

    /// Toggles `item` in the selection (Shift-click). Always changes the selection.
    pub fn toggle(&mut self, item: ItemId) -> bool {
        let key = item_sort_key(item);
        match self
            .items
            .binary_search_by_key(&key, |&existing| item_sort_key(existing))
        {
            Ok(idx) => {
                self.items.remove(idx);
            }
            Err(idx) => {
                self.items.insert(idx, item);
            }
        }
        true
    }

    /// Replaces the selection with the canonical deduplicated contents of `items`.
    /// Returns whether the selection changed.
    pub fn set(&mut self, items: impl IntoIterator<Item = ItemId>) -> bool {
        let mut next: Vec<ItemId> = items.into_iter().collect();
        next.sort_unstable_by_key(|&item| item_sort_key(item));
        next.dedup();
        if self.items == next {
            return false;
        }
        self.items = next;
        true
    }

    /// Replaces the selection from a slice that is already sorted and deduplicated in
    /// canonical order. Allocates nothing when capacity suffices.
    pub fn set_canonical(&mut self, items: &[ItemId]) -> bool {
        debug_assert!(
            items
                .windows(2)
                .all(|pair| item_sort_key(pair[0]) < item_sort_key(pair[1]))
        );
        if self.items.as_slice() == items {
            return false;
        }
        self.items.clear();
        self.items.extend_from_slice(items);
        true
    }

    /// Adds every item from `items` into the selection. Returns whether anything was added.
    pub fn add(&mut self, items: impl IntoIterator<Item = ItemId>) -> bool {
        let before_len = self.items.len();
        self.items.extend(items);
        if self.items.len() == before_len {
            return false;
        }
        self.items.sort_unstable_by_key(|&item| item_sort_key(item));
        self.items.dedup();
        self.items.len() != before_len
    }

    /// Selects every live part, wire, and label in `doc` (ui-plan.md section 14).
    /// Returns whether the selection changed.
    pub fn select_all(&mut self, doc: &Doc) -> bool {
        // Doc iterators yield live items in ascending ID order per kind, so Part -> Wire -> Label
        // is already strictly canonical.
        let mut next = Vec::with_capacity(
            doc.parts().size_hint().0 + doc.wires().size_hint().0 + doc.labels().size_hint().0,
        );
        next.extend(doc.parts().map(|(id, _)| ItemId::Part(id)));
        next.extend(doc.wires().map(|(id, _)| ItemId::Wire(id)));
        next.extend(doc.labels().map(|(id, _)| ItemId::Label(id)));
        if self.items == next {
            return false;
        }
        self.items = next;
        true
    }

    /// Drops any selected items that are no longer live in `doc`. Returns whether anything changed.
    pub fn prune(&mut self, doc: &Doc) -> bool {
        let before = self.items.len();
        self.items.retain(|&item| item_exists(doc, item));
        self.items.len() != before
    }
}

/// An active rubber-band drag in world coordinates (WD8).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RubberBand {
    pub anchor: Pt,
    pub current: Pt,
    pub mode: SelectMode,
    pub additive: bool,
    anchor_screen: Point<f64>,
    dragged: bool,
    /// True for a programmatic preview (`set_rubber_band_preview`) rather than a live pointer drag.
    preview: bool,
}

impl RubberBand {
    /// Normalised world rectangle (`min <= max`), independent of drag direction (WD8).
    pub fn world_rect(&self) -> Rect {
        normalize_world_rect(self.anchor, self.current)
    }

    /// Whether the pointer has moved away from the press point so a box should be drawn.
    pub fn is_visible(&self) -> bool {
        self.dragged
    }

    /// Whether this band belongs to an active pointer drag that holds pointer capture.
    pub fn is_pointer_drag(&self) -> bool {
        !self.preview
    }
}

/// What changed after one pointer or state step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionUpdate {
    /// Overlay visuals changed (hover, rubber band, or selection), so `OverlayView` should repaint.
    pub visual_changed: bool,
    /// Selection membership changed, so `SelectionChanged` should be emitted.
    pub selection_changed: bool,
    /// Whether this mouse-down started a rubber-band drag (caller should capture pointer).
    pub started_drag: bool,
    /// Whether this mouse-up ended a rubber-band drag (caller should release pointer).
    pub ended_drag: bool,
}

fn to_f64(p: Point<Pixels>) -> Point<f64> {
    point(f64::from(p.x), f64::from(p.y))
}

/// Pure hover, selection, and rubber-band interaction state machine.
#[derive(Clone, Debug)]
pub struct SelectionState {
    selection: Selection,
    hovered: Option<ItemId>,
    rubber_band: Option<RubberBand>,
    mode: SelectMode,
    scratch: Vec<ItemId>,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self::from_prefs(&WiringPrefs::default())
    }
}

impl SelectionState {
    pub fn new(mode: SelectMode) -> Self {
        Self {
            selection: Selection::new(),
            hovered: None,
            rubber_band: None,
            mode,
            scratch: Vec::new(),
        }
    }

    /// Creates a [`SelectionState`] driven by `prefs.select_mode` (WD8, WD12).
    pub fn from_prefs(prefs: &WiringPrefs) -> Self {
        Self::new(prefs.select_mode)
    }

    pub fn selection(&self) -> &[ItemId] {
        self.selection.as_slice()
    }

    pub fn is_selected(&self, item: ItemId) -> bool {
        self.selection.contains(item)
    }

    pub fn hovered(&self) -> Option<ItemId> {
        self.hovered
    }

    pub fn rubber_band(&self) -> Option<RubberBand> {
        self.rubber_band
    }

    /// Whether an interactive pointer rubber-band drag is currently in progress.
    pub fn is_rubber_banding(&self) -> bool {
        self.rubber_band.is_some_and(|band| band.is_pointer_drag())
    }

    pub fn select_mode(&self) -> SelectMode {
        self.mode
    }

    /// Updates the WD8 rubber-band mode from `prefs`. If a rubber band is already active,
    /// it updates live.
    pub fn apply_prefs(&mut self, prefs: &WiringPrefs) -> bool {
        self.set_select_mode(prefs.select_mode)
    }

    /// Updates the WD8 rubber-band mode. If a rubber band is already active, it updates live.
    pub fn set_select_mode(&mut self, mode: SelectMode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        if let Some(band) = &mut self.rubber_band
            && band.is_pointer_drag()
        {
            band.mode = mode;
            return band.is_visible();
        }
        false
    }

    pub fn set_hovered(&mut self, item: Option<ItemId>) -> bool {
        if self.hovered == item {
            return false;
        }
        self.hovered = item;
        true
    }

    pub fn set_selection(&mut self, items: impl IntoIterator<Item = ItemId>) -> bool {
        self.selection.set(items)
    }

    pub fn clear_selection(&mut self) -> bool {
        self.selection.clear()
    }

    pub fn select_all(&mut self, doc: &Doc) -> bool {
        self.selection.select_all(doc)
    }

    /// Sets a programmatic rubber-band box directly in world coordinates (INV8-clean: no lossy
    /// pixel round-trip, no mutation of `self.mode`, and no pointer capture).
    pub fn set_rubber_band_preview(&mut self, anchor: Pt, current: Pt, mode: SelectMode) -> bool {
        let next = Some(RubberBand {
            anchor,
            current,
            mode,
            additive: false,
            anchor_screen: point(0.0, 0.0),
            dragged: true,
            preview: true,
        });
        if self.rubber_band == next {
            return false;
        }
        self.rubber_band = next;
        true
    }

    /// Cancels an active rubber-band drag or preview without changing the selection.
    pub fn cancel_rubber_band(&mut self) -> bool {
        self.rubber_band
            .take()
            .is_some_and(|band| band.is_visible())
    }

    /// Drops any hovered or selected items that no longer exist in `doc`.
    pub fn prune(&mut self, doc: &Doc) -> SelectionUpdate {
        let mut update = SelectionUpdate::default();
        if let Some(item) = self.hovered
            && !item_exists(doc, item)
        {
            self.hovered = None;
            update.visual_changed = true;
        }
        if self.selection.prune(doc) {
            update.selection_changed = true;
            update.visual_changed = true;
        }
        update
    }

    /// Handles a left mouse press:
    /// - On an item: plain click replaces selection with `[item]`; Shift-click toggles `item`.
    /// - On empty space: begins a rubber-band gesture (resolved on mouse-up).
    pub fn mouse_down(
        &mut self,
        doc: &Doc,
        camera: &Camera,
        event: &MouseDownEvent,
    ) -> SelectionUpdate {
        if event.button != MouseButton::Left {
            return SelectionUpdate::default();
        }
        let screen = to_f64(event.position);
        let world = camera.screen_to_world(screen);
        let tol = hit_tolerance_nm(camera);
        let hit = hit_test(doc, world, tol, &mut self.scratch);
        let mut update = SelectionUpdate::default();
        if let Some(hit) = hit {
            self.rubber_band = None;
            if self.hovered != Some(hit.item) {
                self.hovered = Some(hit.item);
                update.visual_changed = true;
            }
            let changed = if event.modifiers.shift {
                self.selection.toggle(hit.item)
            } else {
                self.selection.select_single(hit.item)
            };
            if changed {
                update.selection_changed = true;
                update.visual_changed = true;
            }
        } else {
            self.rubber_band = Some(RubberBand {
                anchor: world,
                current: world,
                mode: self.mode,
                additive: event.modifiers.shift,
                anchor_screen: screen,
                dragged: false,
                preview: false,
            });
            update.started_drag = true;
            if self.hovered.take().is_some() {
                update.visual_changed = true;
            }
        }
        update
    }

    /// Handles pointer motion over the sheet:
    /// - During a rubber-band drag: updates the band corner (or cancels if button release was lost).
    /// - Otherwise when no button is pressed: updates the hovered item under the cursor.
    pub fn mouse_move(
        &mut self,
        doc: &Doc,
        camera: &Camera,
        event: &MouseMoveEvent,
    ) -> SelectionUpdate {
        let mut update = SelectionUpdate::default();
        let screen = to_f64(event.position);
        if let Some(band) = &mut self.rubber_band
            && band.is_pointer_drag()
        {
            if event.pressed_button != Some(MouseButton::Left) {
                let was_visible = band.is_visible();
                self.rubber_band = None;
                update.ended_drag = true;
                update.visual_changed = was_visible;
                return update;
            }
            let world = camera.screen_to_world(screen);
            let moved = screen != band.anchor_screen || world != band.anchor;
            let changed = band.current != world
                || band.additive != event.modifiers.shift
                || (moved && !band.dragged);
            band.current = world;
            band.additive = event.modifiers.shift;
            if moved {
                band.dragged = true;
            }
            if self.hovered.take().is_some() {
                update.visual_changed = true;
            }
            if changed && band.dragged {
                update.visual_changed = true;
            }
            return update;
        }

        if event.pressed_button.is_some() {
            return update;
        }
        let world = camera.screen_to_world(screen);
        let tol = hit_tolerance_nm(camera);
        let next = hit_test(doc, world, tol, &mut self.scratch).map(|h| h.item);
        if self.hovered != next {
            self.hovered = next;
            update.visual_changed = true;
        }
        update
    }

    /// Clears hover when the pointer leaves the sheet while not dragging.
    pub fn mouse_leave(&mut self) -> SelectionUpdate {
        if self.rubber_band.is_some() {
            return SelectionUpdate::default();
        }
        let changed = self.hovered.take().is_some();
        SelectionUpdate {
            visual_changed: changed,
            ..Default::default()
        }
    }

    /// Completes an active rubber-band gesture (or bare click on empty space).
    /// Shift is read from the release event (consistent with live tracking on mouse-move).
    pub fn mouse_up(
        &mut self,
        doc: &Doc,
        camera: &Camera,
        event: &MouseUpEvent,
    ) -> SelectionUpdate {
        if event.button != MouseButton::Left {
            return SelectionUpdate::default();
        }
        if !self.is_rubber_banding() {
            return SelectionUpdate::default();
        }
        let Some(mut band) = self.rubber_band.take() else {
            return SelectionUpdate::default();
        };
        let screen = to_f64(event.position);
        let world = camera.screen_to_world(screen);
        if screen != band.anchor_screen || world != band.anchor {
            band.current = world;
            band.dragged = true;
        }
        let additive = event.modifiers.shift;
        let mut update = SelectionUpdate {
            visual_changed: band.is_visible(),
            ended_drag: true,
            ..Default::default()
        };

        if !band.dragged {
            if !additive && self.selection.clear() {
                update.selection_changed = true;
                update.visual_changed = true;
            }
        } else {
            query_rubber_band(doc, band.world_rect(), band.mode, &mut self.scratch);
            let changed = if additive {
                self.selection.add(self.scratch.iter().copied())
            } else {
                self.selection.set_canonical(&self.scratch)
            };
            if changed {
                update.selection_changed = true;
                update.visual_changed = true;
            }
        }

        // Refresh hover under the release position so the cursor immediately reflects what is beneath it.
        let next_hover =
            hit_test(doc, world, hit_tolerance_nm(camera), &mut self.scratch).map(|h| h.item);
        if self.hovered != next_hover {
            self.hovered = next_hover;
            update.visual_changed = true;
        }
        update
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, Modifiers, px, size};
    use omnia_doc::{GRID_NM, Label, LabelId, Orient, Part, PartId, SymbolId, Wire, WireId};
    use proptest::prelude::*;

    fn viewport() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(0.0), px(0.0)),
            size: size(px(800.0), px(600.0)),
        }
    }

    fn camera() -> Camera {
        Camera::new(Pt::ZERO, 1.0, viewport())
    }

    fn screen_of(camera: &Camera, pt: Pt) -> Point<Pixels> {
        camera.world_to_pixels(pt)
    }

    fn down(at: Point<Pixels>, shift: bool) -> MouseDownEvent {
        MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: if shift {
                Modifiers::shift()
            } else {
                Modifiers::none()
            },
            click_count: 1,
            ..Default::default()
        }
    }

    fn moved(at: Point<Pixels>, button: Option<MouseButton>, shift: bool) -> MouseMoveEvent {
        MouseMoveEvent {
            position: at,
            pressed_button: button,
            modifiers: if shift {
                Modifiers::shift()
            } else {
                Modifiers::none()
            },
        }
    }

    fn up(at: Point<Pixels>, shift: bool) -> MouseUpEvent {
        MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: if shift {
                Modifiers::shift()
            } else {
                Modifiers::none()
            },
            click_count: 1,
        }
    }

    fn sample_doc() -> Doc {
        let g = GRID_NM;
        // Part 0 at (0, 0), body [-2g, -g]..[2g, g], pins at (-4g, 0) and (4g, 0) (outside body!).
        let part0 = (
            Part {
                symbol: SymbolId(0),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: "R1".into(),
                value: "10k".into(),
            },
            vec![Pt { x: -4 * g, y: 0 }, Pt { x: 4 * g, y: 0 }],
            Rect::new(Pt { x: -2 * g, y: -g }, Pt { x: 2 * g, y: g }).unwrap(),
        );
        // Part 1 at (10g, 0), body [-g, -g]..[g, g].
        let part1 = (
            Part {
                symbol: SymbolId(0),
                at: Pt { x: 10 * g, y: 0 },
                orient: Orient::IDENTITY,
                designator: "R2".into(),
                value: "22k".into(),
            },
            vec![Pt { x: -g, y: 0 }, Pt { x: g, y: 0 }],
            Rect::new(Pt { x: -g, y: -g }, Pt { x: g, y: g }).unwrap(),
        );
        let wire0 = Wire {
            a: Pt { x: 0, y: 5 * g },
            b: Pt { x: 6 * g, y: 5 * g },
        };
        let label0 = Label {
            at: Pt {
                x: -6 * g,
                y: 5 * g,
            },
            text: "NET1".into(),
        };
        Doc::from_items([part0, part1], [wire0], [label0]).unwrap()
    }

    #[test]
    fn default_select_mode_is_intersecting_per_wq3() {
        let state = SelectionState::default();
        assert_eq!(state.select_mode(), SelectMode::Intersecting);
        assert!(state.selection().is_empty());
        assert_eq!(state.hovered(), None);
        assert_eq!(state.rubber_band(), None);
    }

    #[test]
    fn click_shift_click_and_empty_click_selection() {
        let doc = sample_doc();
        let cam = camera();
        let mut state = SelectionState::default();

        let p0 = screen_of(&cam, Pt::ZERO);
        let w0 = screen_of(
            &cam,
            Pt {
                x: 3 * GRID_NM,
                y: 5 * GRID_NM,
            },
        );
        let empty = screen_of(
            &cam,
            Pt {
                x: -10 * GRID_NM,
                y: -10 * GRID_NM,
            },
        );

        // Plain click selects Part(0).
        let u = state.mouse_down(&doc, &cam, &down(p0, false));
        assert!(u.selection_changed && u.visual_changed && !u.started_drag);
        assert_eq!(state.selection(), &[ItemId::Part(PartId(0))]);

        // Clicking the same item again does not change selection.
        let u = state.mouse_down(&doc, &cam, &down(p0, false));
        assert!(!u.selection_changed);

        // Shift-click adds Wire(0).
        let u = state.mouse_down(&doc, &cam, &down(w0, true));
        assert!(u.selection_changed);
        assert_eq!(
            state.selection(),
            &[ItemId::Part(PartId(0)), ItemId::Wire(WireId(0))]
        );

        // Shift-click on Part(0) toggles it off.
        let u = state.mouse_down(&doc, &cam, &down(p0, true));
        assert!(u.selection_changed);
        assert_eq!(state.selection(), &[ItemId::Wire(WireId(0))]);

        // Shift-click on empty space keeps selection intact.
        let u = state.mouse_down(&doc, &cam, &down(empty, true));
        assert!(u.started_drag);
        let u = state.mouse_up(&doc, &cam, &up(empty, true));
        assert!(u.ended_drag && !u.selection_changed);
        assert_eq!(state.selection(), &[ItemId::Wire(WireId(0))]);

        // Plain click on empty space clears selection.
        let u = state.mouse_down(&doc, &cam, &down(empty, false));
        assert!(u.started_drag);
        let u = state.mouse_up(&doc, &cam, &up(empty, false));
        assert!(u.ended_drag && u.selection_changed);
        assert!(state.selection().is_empty());
    }

    #[test]
    fn wd8_rubber_band_all_four_directions_and_both_modes() {
        // Mirrors TC-band-01..08: a box covering half of Part(0) (body x in [-2g, 2g], y in [-g, g]).
        let doc = sample_doc();
        let cam = camera();
        let g = GRID_NM;
        let x0 = 0;
        let x1 = 4 * g;
        let y0 = -2 * g;
        let y1 = 2 * g;
        let corners = [
            (Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }), // NW -> SE
            (Pt { x: x1, y: y1 }, Pt { x: x0, y: y0 }), // SE -> NW
            (Pt { x: x1, y: y0 }, Pt { x: x0, y: y1 }), // NE -> SW
            (Pt { x: x0, y: y1 }, Pt { x: x1, y: y0 }), // SW -> NE
        ];

        for mode in [SelectMode::Intersecting, SelectMode::Enclosed] {
            for (start, end) in corners {
                let mut state = SelectionState::new(mode);
                let s0 = screen_of(&cam, start);
                let s1 = screen_of(&cam, end);
                // Start on empty corner: shift start slightly away if start hits Part(0) at (0, -2g)
                // (2g = 40 px away from y = -g, well beyond 4 px hit tolerance).
                let u_down = state.mouse_down(&doc, &cam, &down(s0, false));
                assert!(u_down.started_drag, "start {start:?} should be empty");
                let u_move =
                    state.mouse_move(&doc, &cam, &moved(s1, Some(MouseButton::Left), false));
                assert!(u_move.visual_changed);
                assert!(state.rubber_band().unwrap().is_visible());
                let u_up = state.mouse_up(&doc, &cam, &up(s1, false));
                assert!(u_up.ended_drag);
                assert_eq!(state.rubber_band(), None);
                match mode {
                    SelectMode::Intersecting => {
                        assert_eq!(state.selection(), &[ItemId::Part(PartId(0))]);
                    }
                    SelectMode::Enclosed => {
                        assert!(state.selection().is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn wd8_rubber_band_shift_adds_and_excludes_pin_only_touch() {
        // Mirrors TC-band-09 + WD8 body-vs-pin rule.
        let doc = sample_doc();
        let cam = camera();
        let g = GRID_NM;
        let mut state = SelectionState::new(SelectMode::Intersecting);

        // Select Label(0) first.
        let lbl = screen_of(
            &cam,
            Pt {
                x: -6 * g,
                y: 5 * g,
            },
        );
        state.mouse_down(&doc, &cam, &down(lbl, false));
        assert_eq!(state.selection(), &[ItemId::Label(LabelId(0))]);

        // Drag a box covering only Part(0)'s external pin at (-4g, 0) without touching body [-2g, 2g].
        let p_a = screen_of(
            &cam,
            Pt {
                x: -5 * g,
                y: -2 * g,
            },
        );
        let p_b = screen_of(
            &cam,
            Pt {
                x: -3 * g,
                y: 2 * g,
            },
        );
        state.mouse_down(&doc, &cam, &down(p_a, true));
        state.mouse_move(&doc, &cam, &moved(p_b, Some(MouseButton::Left), true));
        let u = state.mouse_up(&doc, &cam, &up(p_b, true));
        assert!(
            !u.selection_changed,
            "pin stub outside body must not select"
        );
        assert_eq!(state.selection(), &[ItemId::Label(LabelId(0))]);

        // Shift-drag over Part(1) adds Part(1) to the existing selection (TC-band-09).
        let r_a = screen_of(
            &cam,
            Pt {
                x: 8 * g,
                y: -2 * g,
            },
        );
        let r_b = screen_of(
            &cam,
            Pt {
                x: 12 * g,
                y: 2 * g,
            },
        );
        state.mouse_down(&doc, &cam, &down(r_a, true));
        state.mouse_move(&doc, &cam, &moved(r_b, Some(MouseButton::Left), true));
        let u = state.mouse_up(&doc, &cam, &up(r_b, true));
        assert!(u.selection_changed);
        assert_eq!(
            state.selection(),
            &[ItemId::Part(PartId(1)), ItemId::Label(LabelId(0))]
        );
    }

    #[test]
    fn select_all_and_hover_and_lost_release() {
        let doc = sample_doc();
        let cam = camera();
        let mut state = SelectionState::default();

        assert!(state.select_all(&doc));
        assert_eq!(
            state.selection(),
            &[
                ItemId::Part(PartId(0)),
                ItemId::Part(PartId(1)),
                ItemId::Wire(WireId(0)),
                ItemId::Label(LabelId(0)),
            ]
        );
        assert!(!state.select_all(&doc), "idempotent select_all");

        // Hover over Part(0), then stay on Part(0) (no visual change), then leave.
        let p0 = screen_of(&cam, Pt::ZERO);
        let u = state.mouse_move(&doc, &cam, &moved(p0, None, false));
        assert!(u.visual_changed);
        assert_eq!(state.hovered(), Some(ItemId::Part(PartId(0))));
        let u = state.mouse_move(&doc, &cam, &moved(p0, None, false));
        assert!(!u.visual_changed);
        let u = state.mouse_leave();
        assert!(u.visual_changed);
        assert_eq!(state.hovered(), None);

        // Start a rubber band and lose the release (pressed_button == None on move).
        let empty = screen_of(
            &cam,
            Pt {
                x: -10 * GRID_NM,
                y: -10 * GRID_NM,
            },
        );
        let drag = screen_of(
            &cam,
            Pt {
                x: -8 * GRID_NM,
                y: -8 * GRID_NM,
            },
        );
        state.mouse_down(&doc, &cam, &down(empty, false));
        state.mouse_move(&doc, &cam, &moved(drag, Some(MouseButton::Left), false));
        assert!(state.is_rubber_banding());
        let u = state.mouse_move(&doc, &cam, &moved(drag, None, false));
        assert!(u.ended_drag && u.visual_changed);
        assert!(!state.is_rubber_banding());
    }

    #[test]
    fn rubber_band_shift_follows_release_event_and_preview_keeps_mode_and_world_coords() {
        let doc = sample_doc();
        let cam = camera();
        let g = GRID_NM;
        let mut state = SelectionState::new(SelectMode::Intersecting);

        // Select Label(0).
        let lbl = screen_of(
            &cam,
            Pt {
                x: -6 * g,
                y: 5 * g,
            },
        );
        state.mouse_down(&doc, &cam, &down(lbl, false));
        assert_eq!(state.selection(), &[ItemId::Label(LabelId(0))]);

        // Press with Shift, release without Shift -> release event wins (replaces selection).
        let r_a = screen_of(
            &cam,
            Pt {
                x: 8 * g,
                y: -2 * g,
            },
        );
        let r_b = screen_of(
            &cam,
            Pt {
                x: 12 * g,
                y: 2 * g,
            },
        );
        state.mouse_down(&doc, &cam, &down(r_a, true));
        state.mouse_move(&doc, &cam, &moved(r_b, Some(MouseButton::Left), false));
        state.mouse_up(&doc, &cam, &up(r_b, false));
        assert_eq!(state.selection(), &[ItemId::Part(PartId(1))]);

        // Press without Shift, hold Shift on release -> release event wins (adds Label(0)).
        let l_a = screen_of(
            &cam,
            Pt {
                x: -8 * g,
                y: 3 * g,
            },
        );
        let l_b = screen_of(
            &cam,
            Pt {
                x: -4 * g,
                y: 7 * g,
            },
        );
        state.mouse_down(&doc, &cam, &down(l_a, false));
        state.mouse_move(&doc, &cam, &moved(l_b, Some(MouseButton::Left), true));
        state.mouse_up(&doc, &cam, &up(l_b, true));
        assert_eq!(
            state.selection(),
            &[ItemId::Part(PartId(1)), ItemId::Label(LabelId(0))]
        );

        // Programmatic preview keeps exact world nm, does not mutate select_mode, and is not a pointer drag.
        let a = Pt {
            x: 123_456,
            y: -789_012,
        };
        let b = Pt {
            x: 654_321,
            y: 987_654,
        };
        assert!(state.set_rubber_band_preview(a, b, SelectMode::Enclosed));
        assert_eq!(state.select_mode(), SelectMode::Intersecting);
        assert!(!state.is_rubber_banding());
        let band = state.rubber_band().unwrap();
        assert_eq!(band.anchor, a);
        assert_eq!(band.current, b);
        assert_eq!(band.mode, SelectMode::Enclosed);
        assert!(band.is_visible());
        assert!(!band.is_pointer_drag());
        assert!(!state.set_rubber_band_preview(a, b, SelectMode::Enclosed));
    }

    #[test]
    fn wd8_wiring_prefs_drives_mode_and_excludes_label_text_extents() {
        let doc = sample_doc();
        let cam = camera();
        let g = GRID_NM;

        // Default WiringPrefs picks Intersecting (WQ3).
        let default_prefs = WiringPrefs::default();
        assert_eq!(default_prefs.select_mode, SelectMode::Intersecting);
        let mut state = SelectionState::from_prefs(&default_prefs);
        assert_eq!(state.select_mode(), SelectMode::Intersecting);

        // Switching WiringPrefs to Enclosed updates state and any active pointer rubber band live.
        let enclosed_prefs = WiringPrefs::new(true, 10, SelectMode::Enclosed, false).unwrap();
        let s0 = screen_of(&cam, Pt { x: 0, y: -2 * g });
        let s1 = screen_of(&cam, Pt { x: 4 * g, y: 2 * g });
        state.mouse_down(&doc, &cam, &down(s0, false));
        state.mouse_move(&doc, &cam, &moved(s1, Some(MouseButton::Left), false));
        assert!(state.apply_prefs(&enclosed_prefs));
        assert_eq!(state.select_mode(), SelectMode::Enclosed);
        assert_eq!(state.rubber_band().unwrap().mode, SelectMode::Enclosed);
        state.mouse_up(&doc, &cam, &up(s1, false));
        assert!(
            state.selection().is_empty(),
            "Enclosed mode must not select half-covered Part(0)"
        );

        // WD8: label counts by its anchor point (-6g, 5g) only; a box covering adjacent text
        // extents (-5g..-g, 4g..6g) without the anchor point selects nothing in either mode.
        let text_a = screen_of(
            &cam,
            Pt {
                x: -5 * g,
                y: 4 * g,
            },
        );
        let text_b = screen_of(&cam, Pt { x: -g, y: 6 * g });
        for prefs in [default_prefs, enclosed_prefs] {
            state.apply_prefs(&prefs);
            state.mouse_down(&doc, &cam, &down(text_a, false));
            state.mouse_move(&doc, &cam, &moved(text_b, Some(MouseButton::Left), false));
            state.mouse_up(&doc, &cam, &up(text_b, false));
            assert!(
                state.selection().is_empty(),
                "label text extents must not trigger selection in {:?}",
                prefs.select_mode
            );
        }
    }

    proptest! {
        #[test]
        fn wd8_rubber_band_direction_invariant_and_matches_oracle(
            ax in -20i32..=20,
            ay in -20i32..=20,
            bx in -20i32..=20,
            by in -20i32..=20,
        ) {
            let doc = sample_doc();
            let g = GRID_NM;
            let a = Pt { x: ax * g, y: ay * g };
            let b = Pt { x: bx * g, y: by * g };
            let c = Pt { x: ax * g, y: by * g };
            let d = Pt { x: bx * g, y: ay * g };

            for mode in [SelectMode::Intersecting, SelectMode::Enclosed] {
                let mut r1 = Vec::new();
                let mut r2 = Vec::new();
                let mut r3 = Vec::new();
                let mut r4 = Vec::new();
                query_rubber_band(&doc, Rect { min: a, max: b }, mode, &mut r1);
                query_rubber_band(&doc, Rect { min: b, max: a }, mode, &mut r2);
                query_rubber_band(&doc, Rect { min: c, max: d }, mode, &mut r3);
                query_rubber_band(&doc, Rect { min: d, max: c }, mode, &mut r4);
                prop_assert_eq!(&r1, &r2);
                prop_assert_eq!(&r1, &r3);
                prop_assert_eq!(&r1, &r4);

                let area = normalize_world_rect(a, b);
                let mut oracle = Vec::new();
                for (id, _) in doc.parts() {
                    let body = doc.part_body(id).unwrap();
                    let ok = match mode {
                        SelectMode::Intersecting => body.intersects(area),
                        SelectMode::Enclosed => area.contains_rect(body),
                    };
                    if ok {
                        oracle.push(ItemId::Part(id));
                    }
                }
                for (id, wire) in doc.wires() {
                    let seg = normalize_world_rect(wire.a, wire.b);
                    let ok = match mode {
                        SelectMode::Intersecting => seg.intersects(area),
                        SelectMode::Enclosed => area.contains_rect(seg),
                    };
                    if ok {
                        oracle.push(ItemId::Wire(id));
                    }
                }
                for (id, label) in doc.labels() {
                    let pt = Rect { min: label.at, max: label.at };
                    let ok = match mode {
                        SelectMode::Intersecting => pt.intersects(area),
                        SelectMode::Enclosed => area.contains_rect(pt),
                    };
                    if ok {
                        oracle.push(ItemId::Label(id));
                    }
                }
                prop_assert_eq!(r1, oracle);
            }
        }
    }
}
