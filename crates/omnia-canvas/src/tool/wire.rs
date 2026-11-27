//! T6.2: interactive wire tool state, K14/WD5 snapping, and WD6 first-corner routing.
//!
//! Pure state and geometry helpers for [`crate::tool::Tool::Wire`]:
//! - Snap radius is [`SNAP_RADIUS_PX`] (10 logical px, K14, WK4), converted to world nanometres
//!   from the live [`Camera`] scale at each event so snapping stays invariant in screen pixels
//!   across zooms (`TC-snap-09`).
//! - Snapping delegates to [`omnia_doc::snap`] with [`GRID_NM`] (WK1, WD5), marking `Pin`,
//!   `WireEnd`, `Junction`, `WireBody` and `Grid` targets before any click.
//! - First-corner selection follows WD6: when the snapped cursor first leaves `start`, the first
//!   leg is vertical if `|dy| > |dx|`, else horizontal (ties go horizontal, M10). The choice stays
//!   until Space flips it or the segment is committed; each new segment chooses again.
//! - Committing a click emits one [`Command::AddWires`] carrying the 1- or 2-segment L-route from
//!   [`omnia_doc::l_route`] (T6.1, INV2, INV5). Clicking a `Grid` target continues the wire from
//!   its end; clicking a pin or wire target (`Pin`, `WireEnd`, `Junction`, `WireBody`) ends the
//!   wire while staying in `Tool::Wire`.

use crate::{
    camera::Camera,
    tool::{Commit, DocStamp},
};
use omnia_doc::{
    Command, Doc, GRID_NM, ItemId, Pt, SHEET_LIMIT_NM, Snap, Wire, l_route, route::Axis, snap,
};

/// K14 / WK4: screen-space snap radius in logical pixels.
pub const SNAP_RADIUS_PX: f64 = 10.0;

/// Converts [`SNAP_RADIUS_PX`] (K14, WK4) into an inclusive nanometre snap radius at `camera`'s
/// current scale (matching `hit_tolerance_nm`'s ceiling rounding while staying exact at rational
/// zoom steps such as 25%, 100%, and 800%).
pub fn snap_radius_nm(camera: &Camera) -> i32 {
    let zoom = camera.zoom();
    debug_assert!(zoom.is_finite() && zoom > 0.0);
    // K3: 20 logical px per GRID_NM at 100% zoom, so 10 px / scale == (10 * GRID_NM) / (20 * zoom).
    ((SNAP_RADIUS_PX * f64::from(GRID_NM)) / (20.0 * zoom))
        .ceil()
        .clamp(0.0, f64::from(i32::MAX)) as i32
}

/// Clamps `p` inside K1 sheet limits (`±SHEET_LIMIT_NM`) so [`omnia_doc::snap`] never panics on
/// synthetic out-of-range coordinates.
#[inline]
fn clamp_in_bounds(p: Pt) -> Pt {
    Pt {
        x: p.x.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
        y: p.y.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
    }
}

/// Flips a WD6 first-leg [`Axis`].
#[inline]
fn flip_axis(axis: Axis) -> Axis {
    match axis {
        Axis::Horizontal => Axis::Vertical,
        Axis::Vertical => Axis::Horizontal,
    }
}

/// WD6 / M10: chooses the first leg of an L-route when the snapped cursor first leaves `start`.
///
/// Returns `None` when `to == start`, [`Axis::Vertical`] when `|dy| > |dx|`, and
/// [`Axis::Horizontal`] otherwise (including diagonal ties `|dx| == |dy|`).
pub fn choose_first_leg(start: Pt, to: Pt) -> Option<Axis> {
    if start == to {
        return None;
    }
    let dx = (i64::from(to.x) - i64::from(start.x)).abs();
    let dy = (i64::from(to.y) - i64::from(start.y)).abs();
    if dy > dx {
        Some(Axis::Vertical)
    } else {
        Some(Axis::Horizontal)
    }
}

/// Whether clicking `target` terminates the current wire chain (T6.2: a click on a pin or wire
/// ends the wire; a click on empty grid continues from its end).
#[inline]
pub fn is_terminating_snap(target: Snap) -> bool {
    matches!(
        target,
        Snap::Pin { .. } | Snap::WireEnd { .. } | Snap::Junction { .. } | Snap::WireBody { .. }
    )
}

/// `Tool::Wire` state: active chain origin, live WD5 snap target, and WD6 first-leg choice (T6.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireGesture {
    /// Start point of the segment currently being drawn, once the first click has been made.
    pub start: Option<Pt>,
    /// Current snapped world point under the cursor (or raw world point when doc-less).
    pub cursor: Option<Pt>,
    /// Live WD5 snap target under the cursor, marked in `stage/snap` before clicking.
    pub snap: Option<Snap>,
    /// WD6 first-leg axis, locked when the snapped cursor first leaves `start`, flipped by Space,
    /// and cleared on commit or cancel.
    pub first_leg: Option<Axis>,
}

impl WireGesture {
    /// Constructs a wire-tool preview directly for golden rendering or programmatic inspection.
    ///
    /// When `start` is `Some(s)` and `first_leg` is `None`, derives the WD6 first leg from
    /// `choose_first_leg(s, snap.at())`.
    pub fn for_preview(start: Option<Pt>, snap: Snap, first_leg: Option<Axis>) -> Self {
        let end = snap.at();
        let first_leg = match (start, first_leg) {
            (Some(_), Some(axis)) => Some(axis),
            (Some(s), None) => choose_first_leg(s, end),
            (None, _) => None,
        };
        Self {
            start,
            cursor: Some(end),
            snap: Some(snap),
            first_leg,
        }
    }

    /// Current snapped target point, falling back to `cursor` when no `Snap` is recorded.
    #[inline]
    pub fn target_point(&self) -> Option<Pt> {
        self.snap.map(Snap::at).or(self.cursor)
    }

    /// L-route preview segments from `start` to the current snapped target (T6.1, WD6).
    ///
    /// Returns `[None, None]` when `start` is unset or equals the target point. Allocates nothing.
    pub fn preview_wires(&self) -> [Option<Wire>; 2] {
        let (Some(start), Some(end)) = (self.start, self.target_point()) else {
            return [None, None];
        };
        let flip = matches!(self.first_leg, Some(Axis::Vertical));
        l_route(start, end, flip)
    }

    /// Updates cursor and WD5 snap target on pointer motion, locking the WD6 first leg the first
    /// time the snapped cursor leaves `start`. Allocates nothing once `scratch` has capacity (B10).
    ///
    /// Returns whether visible preview state changed.
    pub fn update_cursor(
        &mut self,
        doc: Option<&Doc>,
        raw_at: Pt,
        snap_radius: i32,
        scratch: &mut Vec<ItemId>,
    ) -> bool {
        let prev = *self;
        let (next_snap, next_pt) = match doc {
            Some(doc) => {
                let s = snap(
                    doc,
                    clamp_in_bounds(raw_at),
                    GRID_NM,
                    snap_radius.max(0),
                    scratch,
                );
                (Some(s), s.at())
            }
            None => (None, raw_at),
        };
        self.snap = next_snap;
        self.cursor = Some(next_pt);
        if let Some(start) = self.start
            && self.first_leg.is_none()
        {
            self.first_leg = choose_first_leg(start, next_pt);
        }
        *self != prev
    }

    /// Space (T6.2, WD6): flips the first-leg orientation of the active L-preview.
    ///
    /// Returns `true` if a live wire start was active and the corner orientation changed.
    pub fn flip_corner(&mut self) -> bool {
        let Some(start) = self.start else {
            return false;
        };
        if let Some(axis) = self.first_leg {
            self.first_leg = Some(flip_axis(axis));
            return true;
        }
        if let Some(end) = self.target_point()
            && let Some(chosen) = choose_first_leg(start, end)
        {
            self.first_leg = Some(flip_axis(chosen));
            return true;
        }
        false
    }

    /// Esc (first press while wiring): drops the active wire start and first-leg choice while
    /// keeping the tool active and the cursor snap target intact (INV5).
    pub fn drop_start(&mut self) -> bool {
        if self.start.take().is_some() {
            self.first_leg = None;
            true
        } else {
            false
        }
    }

    /// Left click in `Tool::Wire`:
    /// - When `start` is `None`, anchors `start` at the WD5 snap target (no edit, INV5).
    /// - When `start` is `Some(s)`, emits one `Command::AddWires` for `l_route(s, target, flip)`
    ///   (when `target != s`). Clicking a `Grid` target continues the wire from `target` with a
    ///   fresh WD6 first-leg choice; clicking a pin or wire target (`Pin`, `WireEnd`, `Junction`,
    ///   `WireBody`) ends the wire (`start = None`) while remaining in `Tool::Wire`.
    pub fn click(
        &mut self,
        doc: &Doc,
        doc_key: u64,
        raw_at: Pt,
        snap_radius: i32,
        scratch: &mut Vec<ItemId>,
    ) -> (Option<Commit>, bool) {
        let prev = *self;
        let target = snap(
            doc,
            clamp_in_bounds(raw_at),
            GRID_NM,
            snap_radius.max(0),
            scratch,
        );
        let target_pt = target.at();
        self.snap = Some(target);
        self.cursor = Some(target_pt);

        let Some(start) = self.start else {
            self.start = Some(target_pt);
            self.first_leg = None;
            return (None, *self != prev);
        };

        if self.first_leg.is_none() {
            self.first_leg = choose_first_leg(start, target_pt);
        }

        let mut wires = Vec::new();
        for wire in self.preview_wires().into_iter().flatten() {
            wires.push(wire);
        }
        let commit = (!wires.is_empty()).then(|| Commit {
            command: Command::AddWires(wires),
            base: DocStamp {
                key: doc_key,
                revision: doc.revision(),
            },
        });

        if is_terminating_snap(target) {
            self.start = None;
            self.first_leg = None;
        } else if target_pt != start {
            self.start = Some(target_pt);
            self.first_leg = None;
        }

        let changed = *self != prev || commit.is_some();
        (commit, changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Bounds, point, px, size};
    use omnia_doc::{Orient, PIN_LATTICE_NM, Part, PartId, Rect, SymbolId, WireId};

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    fn viewport() -> Bounds<gpui::Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)))
    }

    #[test]
    fn k14_wk4_snap_radius_tracks_screen_pixels_across_zooms() {
        let cam_100 = Camera::new(Pt::ZERO, 1.0, viewport());
        assert_eq!(snap_radius_nm(&cam_100), PIN_LATTICE_NM);
        let cam_25 = Camera::new(Pt::ZERO, 0.25, viewport());
        assert_eq!(snap_radius_nm(&cam_25), 2 * GRID_NM);
        let cam_800 = Camera::new(Pt::ZERO, 8.0, viewport());
        assert_eq!(snap_radius_nm(&cam_800), PIN_LATTICE_NM / 8);
    }

    #[test]
    fn tc_corner_01_to_04_wd6_first_leg_and_space_flip() {
        let doc = Doc::new();
        let mut scratch = Vec::new();
        let r = PIN_LATTICE_NM;

        // TC-corner-01: leave start to the right -> horizontal first.
        let mut g = WireGesture::default();
        g.click(&doc, 1, pt(0, 0), r, &mut scratch);
        g.update_cursor(Some(&doc), pt(2, 0), r, &mut scratch);
        assert_eq!(g.first_leg, Some(Axis::Horizontal));
        // TC-corner-04: moving so vertical dominates does not flip.
        g.update_cursor(Some(&doc), pt(2, 6), r, &mut scratch);
        assert_eq!(g.first_leg, Some(Axis::Horizontal));
        assert_eq!(
            g.preview_wires(),
            [
                Some(Wire {
                    a: pt(0, 0),
                    b: pt(2, 0),
                }),
                Some(Wire {
                    a: pt(2, 0),
                    b: pt(2, 6),
                }),
            ]
        );
        // Space flips the corner to vertical first.
        assert!(g.flip_corner());
        assert_eq!(g.first_leg, Some(Axis::Vertical));
        assert_eq!(
            g.preview_wires(),
            [
                Some(Wire {
                    a: pt(0, 0),
                    b: pt(0, 6),
                }),
                Some(Wire {
                    a: pt(0, 6),
                    b: pt(2, 6),
                }),
            ]
        );

        // TC-corner-02: leave start upward -> vertical first.
        let mut g = WireGesture::default();
        g.click(&doc, 1, pt(0, 0), r, &mut scratch);
        g.update_cursor(Some(&doc), pt(0, -2), r, &mut scratch);
        assert_eq!(g.first_leg, Some(Axis::Vertical));
        // Moving horizontally keeps vertical first until Space flips it.
        g.update_cursor(Some(&doc), pt(6, -2), r, &mut scratch);
        assert_eq!(g.first_leg, Some(Axis::Vertical));
        assert!(g.flip_corner());
        assert_eq!(g.first_leg, Some(Axis::Horizontal));

        // TC-corner-03: leave start diagonally (|dx| == |dy|) -> tie goes horizontal (M10).
        let mut g = WireGesture::default();
        g.click(&doc, 1, pt(0, 0), r, &mut scratch);
        g.update_cursor(Some(&doc), pt(3, -3), r, &mut scratch);
        assert_eq!(g.first_leg, Some(Axis::Horizontal));
    }

    #[test]
    fn tc_snap_09_same_screen_distance_wins_at_25_and_800_zoom() {
        // Place a pin at (0, 0). At 8 px screen distance from the pin, Pin must win at both
        // 25% and 800% zoom, while at 12 px screen distance Grid must win at both zooms.
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "".into(),
                },
                vec![Pt::ZERO],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )],
            [],
            [],
        )
        .unwrap();
        let mut scratch = Vec::new();

        for zoom in [0.25, 8.0] {
            let cam = Camera::new(Pt::ZERO, zoom, viewport());
            let radius = snap_radius_nm(&cam);
            let origin = cam.world_to_screen(Pt::ZERO);

            let inside = cam.screen_to_world(point(origin.x + 8.0, origin.y));
            let mut g = WireGesture::default();
            g.update_cursor(Some(&doc), inside, radius, &mut scratch);
            assert_eq!(
                g.snap,
                Some(Snap::Pin {
                    part: PartId(0),
                    pin: 0,
                    at: Pt::ZERO,
                }),
                "8 px screen distance must snap to Pin at zoom {zoom}"
            );

            let outside = cam.screen_to_world(point(origin.x + 12.0, origin.y));
            g.update_cursor(Some(&doc), outside, radius, &mut scratch);
            assert!(
                matches!(g.snap, Some(Snap::Grid(_))),
                "12 px screen distance must fall back to Grid at zoom {zoom}, got {:?}",
                g.snap
            );
        }
    }

    #[test]
    fn terminating_snap_classification_matches_t6_2() {
        assert!(is_terminating_snap(Snap::Pin {
            part: PartId(0),
            pin: 0,
            at: Pt::ZERO,
        }));
        assert!(is_terminating_snap(Snap::WireEnd {
            wire: WireId(0),
            at: Pt::ZERO,
        }));
        assert!(is_terminating_snap(Snap::Junction { at: Pt::ZERO }));
        assert!(is_terminating_snap(Snap::WireBody {
            wire: WireId(0),
            at: Pt::ZERO,
        }));
        assert!(!is_terminating_snap(Snap::Grid(Pt::ZERO)));
    }

    #[test]
    fn b10_warm_wire_update_cursor_makes_zero_heap_allocations() {
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "".into(),
                },
                vec![Pt::ZERO, pt(4, 0)],
                Rect::new(Pt::ZERO, pt(4, 0)).unwrap(),
            )],
            [Wire {
                a: pt(0, 4),
                b: pt(8, 4),
            }],
            [],
        )
        .unwrap();
        let mut scratch = Vec::with_capacity(32);
        let mut g = WireGesture::default();
        // Warm once.
        g.click(&doc, 1, Pt::ZERO, PIN_LATTICE_NM, &mut scratch);
        g.update_cursor(Some(&doc), pt(2, 2), PIN_LATTICE_NM, &mut scratch);

        let prev = crate::sheet::replace_alloc_counter(Some(0));
        for step in 0..20 {
            let p = Pt {
                x: step * PIN_LATTICE_NM,
                y: (step % 5) * GRID_NM,
            };
            g.update_cursor(Some(&doc), p, PIN_LATTICE_NM, &mut scratch);
            let _ = g.preview_wires();
        }
        let used = crate::sheet::replace_alloc_counter(prev).unwrap_or(0);
        assert_eq!(used, 0, "B10: warm wire preview update must not allocate");
    }

    fn open_wire_overlay(
        cx: &mut gpui::TestAppContext,
        doc: Doc,
    ) -> (
        gpui::Entity<crate::OverlayView>,
        gpui::Entity<Doc>,
        &mut gpui::VisualTestContext,
    ) {
        use gpui::AppContext;
        cx.update(|cx| cx.bind_keys(crate::keymap::standalone_bindings()));
        let doc = cx.new(|_| doc);
        let doc_clone = doc.clone();
        let (overlay, cx) = cx.add_window_view(move |window, cx| {
            let camera = Camera::new(Pt::ZERO, 1.0, Bounds::default());
            let view = crate::OverlayView::new(doc_clone, camera, &omnia_theme::GRAPHITE, cx);
            window.activate_window();
            window.focus(view.focus_handle(), cx);
            view
        });
        cx.run_until_parked();
        (overlay, doc, cx)
    }

    fn overlay_screen(
        overlay: &gpui::Entity<crate::OverlayView>,
        cx: &mut gpui::VisualTestContext,
        world: Pt,
    ) -> gpui::Point<gpui::Pixels> {
        cx.update(|_, cx| overlay.read(cx).camera().world_to_pixels(world))
    }

    fn move_overlay_world(
        overlay: &gpui::Entity<crate::OverlayView>,
        cx: &mut gpui::VisualTestContext,
        world: Pt,
    ) {
        let p = overlay_screen(overlay, cx, world);
        cx.simulate_mouse_move(p, None, gpui::Modifiers::none());
        cx.run_until_parked();
    }

    fn click_overlay_world(
        overlay: &gpui::Entity<crate::OverlayView>,
        cx: &mut gpui::VisualTestContext,
        world: Pt,
    ) {
        let p = overlay_screen(overlay, cx, world);
        cx.simulate_mouse_down(p, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.simulate_mouse_up(p, gpui::MouseButton::Left, gpui::Modifiers::none());
        cx.run_until_parked();
    }

    /// W3.1 done-when (`TC-corner-01`..`TC-corner-04`, `WD6`, `M10`, `INV5`, `INV11`):
    /// simulated GPUI pointer/keyboard input chooses first leg once when leaving `start`
    /// (right -> Horizontal, up -> Vertical, diagonal tie -> Horizontal), keeps it even when the
    /// other axis dominates, flips only on Space, resets on each new segment, and causes zero
    /// `SheetView` paints during preview/flip/cancel.
    #[gpui::test]
    fn tc_corner_01_to_04_simulated_input_and_zero_sheet_paints(cx: &mut gpui::TestAppContext) {
        use crate::tool::{Tool, ToolState};
        use std::{cell::Cell, rc::Rc};

        let (overlay, doc, cx) = open_wire_overlay(cx, Doc::new());
        let sheet_notifies = Rc::new(Cell::new(0u32));
        let sn = sheet_notifies.clone();
        cx.update(|_, cx| {
            let sheet = overlay.read(cx).sheet_entity().clone();
            cx.observe(&sheet, move |_, _| sn.set(sn.get() + 1))
                .detach();
            overlay.update(cx, |o, cx| o.set_tool(Tool::Wire, cx));
        });
        cx.run_until_parked();

        let sp0 = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        let sn0 = sheet_notifies.get();
        let rev0 = cx.update(|_, cx| doc.read(cx).revision());

        // TC-corner-01: anchor start at (0, 0) and leave right to (3, 0) -> Horizontal first.
        click_overlay_world(&overlay, cx, pt(0, 0));
        move_overlay_world(&overlay, cx, pt(3, 0));
        match cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
            ToolState::Wiring(w) => {
                assert_eq!(w.first_leg, Some(Axis::Horizontal));
                assert_eq!(
                    w.preview_wires(),
                    [
                        Some(Wire {
                            a: pt(0, 0),
                            b: pt(3, 0)
                        }),
                        None
                    ]
                );
            }
            other => panic!("expected Wiring state, got {other:?}"),
        }

        // TC-corner-04 (H->V): move so vertical dominates (3, 8) -> Horizontal stays locked.
        move_overlay_world(&overlay, cx, pt(3, 8));
        match cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
            ToolState::Wiring(w) => {
                assert_eq!(w.first_leg, Some(Axis::Horizontal));
                assert_eq!(
                    w.preview_wires(),
                    [
                        Some(Wire {
                            a: pt(0, 0),
                            b: pt(3, 0)
                        }),
                        Some(Wire {
                            a: pt(3, 0),
                            b: pt(3, 8)
                        }),
                    ]
                );
            }
            other => panic!("expected Wiring state, got {other:?}"),
        }
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            sp0,
            "INV11: pointer preview moves must cost 0 SheetView paints"
        );
        // Space flips the corner to Vertical (0 CachedSheet notifies; at most 1 G17 pointer->key
        // modality-switch paint).
        let sp_before_space = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        cx.simulate_keystrokes("space");
        cx.run_until_parked();
        match cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
            ToolState::Wiring(w) => {
                assert_eq!(w.first_leg, Some(Axis::Vertical));
                assert_eq!(
                    w.preview_wires(),
                    [
                        Some(Wire {
                            a: pt(0, 0),
                            b: pt(0, 8)
                        }),
                        Some(Wire {
                            a: pt(0, 8),
                            b: pt(3, 8)
                        }),
                    ]
                );
            }
            other => panic!("expected flipped Wiring state, got {other:?}"),
        }
        assert_eq!(
            cx.update(|_, cx| doc.read(cx).revision()),
            rev0,
            "INV5: preview and Space flip must not modify Doc"
        );
        assert_eq!(
            sheet_notifies.get(),
            sn0,
            "INV11: preview and Space flip must never notify CachedSheet"
        );
        assert!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)) <= sp_before_space + 1,
            "INV11/G17: Space flip causes at most 1 GPUI modality-switch paint"
        );

        // Commit on empty Grid (3, 8): continues from (3, 8) with a fresh WD6 choice.
        click_overlay_world(&overlay, cx, pt(3, 8));
        assert_eq!(cx.update(|_, cx| doc.read(cx).revision()), rev0 + 1);
        let sn_after_commit = sheet_notifies.get();

        // TC-corner-02: leave upward from (3, 8) to (3, 5) -> Vertical first.
        move_overlay_world(&overlay, cx, pt(3, 5));
        let sp_seg2 = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        assert!(matches!(
            cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Wiring(WireGesture {
                first_leg: Some(Axis::Vertical),
                ..
            })
        ));
        // TC-corner-04 (V->H): move right to (10, 5) -> Vertical stays locked until Space flips.
        move_overlay_world(&overlay, cx, pt(10, 5));
        assert!(matches!(
            cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Wiring(WireGesture {
                first_leg: Some(Axis::Vertical),
                ..
            })
        ));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            sp_seg2,
            "INV11: second-segment pointer preview must cost 0 SheetView paints"
        );
        cx.simulate_keystrokes("space");
        cx.run_until_parked();
        assert!(matches!(
            cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Wiring(WireGesture {
                first_leg: Some(Axis::Horizontal),
                ..
            })
        ));
        assert_eq!(
            sheet_notifies.get(),
            sn_after_commit,
            "INV11: second-segment preview and flip must never notify CachedSheet"
        );

        // Esc drops active start without editing Doc, staying in Tool::Wire.
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| doc.read(cx).revision()), rev0 + 1);

        // TC-corner-03: start at (-4, -4) and leave diagonally to (-1, -7) (|dx| == |dy|).
        click_overlay_world(&overlay, cx, pt(-4, -4));
        let sp_diag = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
        move_overlay_world(&overlay, cx, pt(-1, -7));
        assert!(matches!(
            cx.update(|_, cx| overlay.read(cx).tool_state().clone()),
            ToolState::Wiring(WireGesture {
                first_leg: Some(Axis::Horizontal),
                ..
            })
        ));
        assert_eq!(
            cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
            sp_diag,
            "INV11: diagonal preview must cost 0 SheetView paints"
        );
        assert_eq!(sheet_notifies.get(), sn_after_commit);
    }

    /// W3.1 done-when (`TC-snap-09`, `WD5`, `WK4`, `INV11`): simulated pointer motion at 25% and
    /// 800% zoom selects the exact same snap class (`Pin`, `WireEnd`, `Junction`, `WireBody`) at
    /// 8 px screen distance and falls back to `Grid` at 12 px screen distance, with 0 `SheetView`
    /// paints.
    #[gpui::test]
    fn tc_snap_09_simulated_input_25_and_800_zoom_zero_sheet_paints(cx: &mut gpui::TestAppContext) {
        use crate::tool::{Tool, ToolState};

        let doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: pt(-10, 0),
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "".into(),
                },
                vec![Pt::ZERO],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )],
            [
                // WireEnd at (0, -6) with vertical body extending upward.
                Wire {
                    a: pt(0, -6),
                    b: pt(0, -10),
                },
                // 3-wire Junction at (10, 0).
                Wire {
                    a: pt(6, 0),
                    b: pt(10, 0),
                },
                Wire {
                    a: pt(10, 0),
                    b: pt(14, 0),
                },
                Wire {
                    a: pt(10, 0),
                    b: pt(10, 4),
                },
                // Horizontal WireBody passing through (0, 6).
                Wire {
                    a: pt(-4, 6),
                    b: pt(4, 6),
                },
            ],
            [],
        )
        .unwrap();

        let (overlay, _, cx) = open_wire_overlay(cx, doc);
        cx.update(|_, cx| overlay.update(cx, |o, cx| o.set_tool(Tool::Wire, cx)));
        cx.run_until_parked();

        let cases = [
            ("Pin", pt(-10, 0), (8.0_f32, 0.0_f32), (12.0_f32, 0.0_f32)),
            ("WireEnd", pt(0, -6), (0.0, 8.0), (0.0, 12.0)),
            ("Junction", pt(10, 0), (0.0, -8.0), (0.0, -12.0)),
            ("WireBody", pt(0, 6), (0.0, 8.0), (0.0, 12.0)),
        ];

        for zoom in [0.25, 8.0] {
            for &(label, target, inside_px, outside_px) in &cases {
                cx.update(|_, cx| {
                    overlay.update(cx, |o, cx| {
                        let vp = o.viewport();
                        o.set_camera(Camera::new(target, zoom, vp), cx);
                    });
                });
                cx.run_until_parked();
                let sp0 = cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx));
                let target_px = overlay_screen(&overlay, cx, target);

                let inside = gpui::point(
                    target_px.x + gpui::px(inside_px.0),
                    target_px.y + gpui::px(inside_px.1),
                );
                cx.simulate_mouse_move(inside, None, gpui::Modifiers::none());
                cx.run_until_parked();
                let inside_snap = match cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
                    ToolState::Wiring(w) => w.snap,
                    other => panic!("expected Wiring state, got {other:?}"),
                };
                match label {
                    "Pin" => assert_eq!(
                        inside_snap,
                        Some(Snap::Pin {
                            part: PartId(0),
                            pin: 0,
                            at: target,
                        }),
                        "8 px inside must snap to Pin at zoom {zoom}"
                    ),
                    "WireEnd" => assert_eq!(
                        inside_snap,
                        Some(Snap::WireEnd {
                            wire: WireId(0),
                            at: target,
                        }),
                        "8 px inside must snap to WireEnd at zoom {zoom}"
                    ),
                    "Junction" => assert_eq!(
                        inside_snap,
                        Some(Snap::Junction { at: target }),
                        "8 px inside must snap to Junction at zoom {zoom}"
                    ),
                    "WireBody" => assert_eq!(
                        inside_snap,
                        Some(Snap::WireBody {
                            wire: WireId(4),
                            at: target,
                        }),
                        "8 px inside must snap to WireBody at zoom {zoom}"
                    ),
                    _ => unreachable!(),
                }

                let outside = gpui::point(
                    target_px.x + gpui::px(outside_px.0),
                    target_px.y + gpui::px(outside_px.1),
                );
                cx.simulate_mouse_move(outside, None, gpui::Modifiers::none());
                cx.run_until_parked();
                let outside_snap = match cx.update(|_, cx| overlay.read(cx).tool_state().clone()) {
                    ToolState::Wiring(w) => w.snap,
                    other => panic!("expected Wiring state, got {other:?}"),
                };
                assert!(
                    matches!(outside_snap, Some(Snap::Grid(_))),
                    "12 px outside {label} must fall back to Grid at zoom {zoom}, got {outside_snap:?}"
                );
                assert_eq!(
                    cx.update(|_, cx| overlay.read(cx).sheet_paint_count(cx)),
                    sp0,
                    "INV11: TC-snap-09 pointer moves must cost 0 SheetView paints"
                );
            }
        }
    }
}
