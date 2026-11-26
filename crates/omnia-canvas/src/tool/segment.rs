//! W3.3: wire-segment drag gesture state and preview planner (`wiring-plan.md` WD7, WD11,
//! WINV3, WINV4, INV5, INV11).
//!
//! Dragging a single wire segment in `Tool::Select` moves it perpendicular to its own axis in
//! [`GRID_NM`] steps while keeping the document untouched until release (INV5). Live previews call
//! [`omnia_doc::route::plan_segment_drag`] only when the snapped perpendicular offset changes
//! (WD11). Accepted drags show the exact planned wires and commit one [`Command::DragSegment`] on
//! release; refused drags show the attempted geometry in `mark/conflict` alongside the blocking
//! item and cancel cleanly on release or Esc without changing the document (WD7, WD11, WINV4).

use crate::{selection::item_sort_key, tool::DocStamp};
use omnia_doc::{
    Command, Doc, GRID_NM, ItemId, LabelId, Pt, Rect, Wire, WireId,
    route::{Blocker, SegmentDragPlan, plan_segment_drag},
};

/// Largest multiple of [`GRID_NM`] representable in `i32`, preserving grid alignment even when
/// clamping an extreme off-sheet cursor displacement.
const MAX_GRID_OFFSET_NM: i64 = (i32::MAX / GRID_NM) as i64 * GRID_NM as i64;

/// Classification of one endpoint of the dragged wire segment, matching
/// `omnia_doc::route::plan_segment_drag`'s local corner/anchor rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SegmentEnd {
    Free,
    Anchor,
    Corner {
        id: WireId,
        far: Pt,
        has_label: bool,
    },
}

/// Preview state of a segment drag at its current snapped perpendicular offset (WD7, WD11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentPreview {
    /// Offset is zero or `plan_segment_drag` accepted the current offset.
    Accepted,
    /// `plan_segment_drag` refused the current offset with this WD4 blocker.
    Refused(Blocker),
}

/// Computes the signed perpendicular grid-snapped offset (nm) for dragging a segment from `anchor`
/// to `current` (WD7).
///
/// Positive offset moves along `+y` for a horizontal segment and `+x` for a vertical segment,
/// matching [`omnia_doc::route::plan_segment_drag`] and [`Command::DragSegment`]. Motion along the
/// segment's own axis is ignored. Sub-half-grid motion rounds to `0` so a click with minor jitter
/// stays a click.
pub fn segment_offset(horizontal: bool, anchor: Pt, current: Pt) -> i32 {
    let raw = if horizontal {
        i64::from(current.y) - i64::from(anchor.y)
    } else {
        i64::from(current.x) - i64::from(anchor.x)
    };
    let step = i64::from(GRID_NM);
    let half = step / 2;
    let snapped = if raw >= 0 {
        ((raw + half) / step) * step
    } else {
        ((raw - half) / step) * step
    };
    snapped.clamp(-MAX_GRID_OFFSET_NM, MAX_GRID_OFFSET_NM) as i32
}

/// Classifies one endpoint of `wire` using only public [`Doc`] queries, matching `plan_segment_drag`.
fn classify_end(
    doc: &Doc,
    wire: WireId,
    horizontal: bool,
    at: Pt,
    query_scratch: &mut Vec<ItemId>,
    label_scratch: &mut Vec<LabelId>,
) -> SegmentEnd {
    doc.query(Rect { min: at, max: at }, query_scratch);
    let mut wire_count = 0usize;
    let mut other_wire: Option<(WireId, Wire)> = None;
    for &item in query_scratch.iter() {
        match item {
            ItemId::Part(part) => {
                if doc.pins(part).is_some_and(|pins| pins.contains(&at)) {
                    return SegmentEnd::Anchor;
                }
            }
            ItemId::Wire(id) => {
                if let Some(&w) = doc.wire(id)
                    && (w.a == at || w.b == at)
                {
                    wire_count += 1;
                    if id != wire {
                        other_wire = Some((id, w));
                    }
                }
            }
            ItemId::Label(_) => {}
        }
    }
    if wire_count >= 3 {
        return SegmentEnd::Anchor;
    }
    doc.labels_at(at, label_scratch);
    let has_label = !label_scratch.is_empty();
    if wire_count == 2
        && let Some((other_id, other)) = other_wire
    {
        let other_horiz = other.a.y == other.b.y;
        if other_horiz != horizontal {
            let far = if other.a == at { other.b } else { other.a };
            return SegmentEnd::Corner {
                id: other_id,
                far,
                has_label,
            };
        }
        return SegmentEnd::Anchor;
    }
    if has_label {
        return SegmentEnd::Anchor;
    }
    SegmentEnd::Free
}

#[inline]
fn segment_contains(a: Pt, b: Pt, p: Pt) -> bool {
    if a.y == b.y {
        p.y == a.y && p.x >= a.x.min(b.x) && p.x <= a.x.max(b.x)
    } else {
        p.x == a.x && p.y >= a.y.min(b.y) && p.y <= a.y.max(b.y)
    }
}

/// Builds the attempted geometry for a segment drag at `offset` (used for refused previews in
/// `mark/conflict` when `plan_segment_drag` leaves `plan.add` empty, and for lifted-zero previews).
fn build_attempted_wires(
    seg: Wire,
    horizontal: bool,
    end_a: SegmentEnd,
    end_b: SegmentEnd,
    offset: i32,
    out: &mut Vec<Wire>,
) {
    out.clear();
    if offset == 0 {
        if let SegmentEnd::Corner { far, .. } = end_a {
            out.push(Wire { a: far, b: seg.a });
        }
        out.push(seg);
        if let SegmentEnd::Corner { far, .. } = end_b {
            out.push(Wire { a: seg.b, b: far });
        }
        return;
    }

    let signed_dist = |from: Pt, to: Pt| -> i64 {
        if horizontal {
            i64::from(to.y) - i64::from(from.y)
        } else {
            i64::from(to.x) - i64::from(from.x)
        }
    };

    // WD7: shrinking a corner neighbour past zero length stops at zero.
    let mut effective = i64::from(offset);
    for end in [end_a, end_b] {
        if let SegmentEnd::Corner { far, .. } = end {
            let d = signed_dist(seg.a, far);
            if (effective > 0 && d > 0 && effective > d)
                || (effective < 0 && d < 0 && effective < d)
            {
                effective = d;
            }
        }
    }

    let shift = |p: Pt| -> Pt {
        let (x, y) = if horizontal {
            (i64::from(p.x), i64::from(p.y) + effective)
        } else {
            (i64::from(p.x) + effective, i64::from(p.y))
        };
        Pt {
            x: x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        }
    };

    let new_a = shift(seg.a);
    let new_b = shift(seg.b);

    match end_a {
        SegmentEnd::Free => {}
        SegmentEnd::Anchor => {
            if seg.a != new_a {
                out.push(Wire { a: seg.a, b: new_a });
            }
        }
        SegmentEnd::Corner { far, has_label, .. } => {
            if far != new_a {
                out.push(Wire { a: far, b: new_a });
            }
            if has_label && !segment_contains(far, new_a, seg.a) && new_a != seg.a {
                out.push(Wire { a: new_a, b: seg.a });
            }
        }
    }

    if new_a != new_b {
        out.push(Wire { a: new_a, b: new_b });
    }

    match end_b {
        SegmentEnd::Free => {}
        SegmentEnd::Anchor => {
            if new_b != seg.b {
                out.push(Wire { a: new_b, b: seg.b });
            }
        }
        SegmentEnd::Corner { far, has_label, .. } => {
            if far != new_b {
                out.push(Wire { a: new_b, b: far });
            }
            if has_label && !segment_contains(far, new_b, seg.b) && new_b != seg.b {
                out.push(Wire { a: new_b, b: seg.b });
            }
        }
    }
}

/// Active segment-drag gesture state held by [`crate::tool::ToolMachine`] (WD7, WD11, INV5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentDragGesture {
    /// The wire segment being dragged.
    pub wire: WireId,
    /// Original segment endpoints at drag start.
    pub original: Wire,
    /// `true` for a horizontal wire (drags along `y`); `false` for a vertical wire (drags along `x`).
    pub horizontal: bool,
    /// World point of the press.
    pub anchor: Pt,
    /// Current world point of the pointer.
    pub current: Pt,
    /// Document identity and revision at drag start.
    pub base: DocStamp,
    end_a: SegmentEnd,
    end_b: SegmentEnd,
    /// Canonically sorted items (`wire` plus any directly attached corner neighbours) hidden on
    /// `SheetView` once the drag lifts and kept fixed until drag end (D7, INV11).
    hidden: Vec<ItemId>,
    /// Current snapped perpendicular offset in nm.
    offset: i32,
    /// Reusable buffer for [`plan_segment_drag`] results.
    plan: SegmentDragPlan,
    /// Reusable buffer for refused and lifted-zero preview segments.
    fallback_wires: Vec<Wire>,
    preview: SegmentPreview,
    /// Latched `true` once `offset` becomes non-zero so `SheetView` paints at most twice per drag
    /// (lift + end, INV11), never on plain clicks.
    lifted: bool,
    /// `true` for a programmatic golden/static preview that does not hold pointer capture.
    preview_only: bool,
}

impl SegmentDragGesture {
    /// Starts a pointer-driven segment drag on `wire` at press point `anchor`.
    /// Returns `None` if `wire` does not exist in `doc`.
    pub fn start(doc: &Doc, wire: WireId, anchor: Pt, base: DocStamp) -> Option<Self> {
        let original = *doc.wire(wire)?;
        let horizontal = original.a.y == original.b.y;
        let mut query_scratch = Vec::new();
        let mut label_scratch = Vec::new();
        let end_a = classify_end(
            doc,
            wire,
            horizontal,
            original.a,
            &mut query_scratch,
            &mut label_scratch,
        );
        let end_b = classify_end(
            doc,
            wire,
            horizontal,
            original.b,
            &mut query_scratch,
            &mut label_scratch,
        );

        let mut hidden = Vec::with_capacity(3);
        hidden.push(ItemId::Wire(wire));
        for end in [end_a, end_b] {
            if let SegmentEnd::Corner { id, .. } = end {
                hidden.push(ItemId::Wire(id));
            }
        }
        hidden.sort_unstable_by_key(|&item| item_sort_key(item));
        hidden.dedup();

        let mut fallback_wires = Vec::with_capacity(4);
        build_attempted_wires(original, horizontal, end_a, end_b, 0, &mut fallback_wires);

        Some(Self {
            wire,
            original,
            horizontal,
            anchor,
            current: anchor,
            base,
            end_a,
            end_b,
            hidden,
            offset: 0,
            plan: SegmentDragPlan {
                remove: Vec::with_capacity(4),
                add: Vec::with_capacity(4),
            },
            fallback_wires,
            preview: SegmentPreview::Accepted,
            lifted: false,
            preview_only: false,
        })
    }

    /// Creates a lifted programmatic preview at `offset` nm without pointer capture (used for
    /// golden rendering of refused/accepted states).
    pub fn for_preview(doc: &Doc, wire: WireId, offset: i32, base: DocStamp) -> Option<Self> {
        let original = *doc.wire(wire)?;
        let anchor = original.a;
        let mut gesture = Self::start(doc, wire, anchor, base)?;
        gesture.preview_only = true;
        gesture.current = if gesture.horizontal {
            Pt {
                x: anchor.x,
                y: anchor.y.saturating_add(offset),
            }
        } else {
            Pt {
                x: anchor.x.saturating_add(offset),
                y: anchor.y,
            }
        };
        gesture.apply_offset(Some(doc), offset);
        Some(gesture)
    }

    /// Whether this gesture is an active pointer drag (holds capture), as opposed to a static
    /// programmatic preview.
    pub fn is_pointer_drag(&self) -> bool {
        !self.preview_only
    }

    /// Whether the drag has visually lifted off `SheetView`.
    pub fn is_lifted(&self) -> bool {
        self.lifted
    }

    /// Current snapped perpendicular offset in nm.
    pub fn offset(&self) -> i32 {
        self.offset
    }

    /// Current preview verdict (`Accepted` or `Refused(Blocker)`).
    pub fn preview(&self) -> SegmentPreview {
        self.preview
    }

    /// The blocking item if the current offset is refused (WD7, WD11).
    pub fn blocker(&self) -> Option<Blocker> {
        match self.preview {
            SegmentPreview::Accepted => None,
            SegmentPreview::Refused(blocker) => Some(blocker),
        }
    }

    /// Returns `true` when the current offset is refused by WD4.
    pub fn is_refused(&self) -> bool {
        matches!(self.preview, SegmentPreview::Refused(_))
    }

    /// Canonically sorted items (`wire` and its stretching corner neighbours) hidden on
    /// `SheetView` once lifted (D7, INV11).
    pub fn hidden_items(&self) -> &[ItemId] {
        &self.hidden
    }

    /// The live wire segments to paint in `OverlayView`: `plan.add` for an accepted non-zero
    /// offset, the attempted segments (in `mark/conflict`) for a refused offset, or the original
    /// affected segments if dragged back to zero after lift-off.
    pub fn preview_wires(&self) -> &[Wire] {
        match self.preview {
            SegmentPreview::Accepted if self.offset != 0 => &self.plan.add,
            _ => &self.fallback_wires,
        }
    }

    /// Updates the pointer position and re-plans against `doc` only when the snapped perpendicular
    /// offset changes (WD11). Returns `true` if the preview changed.
    pub fn update(&mut self, doc: Option<&Doc>, at: Pt) -> bool {
        self.current = at;
        let next_offset = segment_offset(self.horizontal, self.anchor, at);
        if next_offset == self.offset {
            return false;
        }
        self.apply_offset(doc, next_offset);
        true
    }

    fn apply_offset(&mut self, doc: Option<&Doc>, next_offset: i32) {
        self.offset = next_offset;
        if next_offset != 0 {
            self.lifted = true;
        }
        if next_offset == 0 {
            self.plan.remove.clear();
            self.plan.add.clear();
            build_attempted_wires(
                self.original,
                self.horizontal,
                self.end_a,
                self.end_b,
                0,
                &mut self.fallback_wires,
            );
            self.preview = SegmentPreview::Accepted;
            return;
        }
        if let Some(doc) = doc {
            match plan_segment_drag(doc, self.wire, next_offset, &mut self.plan) {
                Ok(()) => {
                    self.preview = SegmentPreview::Accepted;
                }
                Err(blocker) => {
                    build_attempted_wires(
                        self.original,
                        self.horizontal,
                        self.end_a,
                        self.end_b,
                        next_offset,
                        &mut self.fallback_wires,
                    );
                    self.preview = SegmentPreview::Refused(blocker);
                }
            }
        } else {
            self.plan.remove.clear();
            build_attempted_wires(
                self.original,
                self.horizontal,
                self.end_a,
                self.end_b,
                next_offset,
                &mut self.plan.add,
            );
            self.preview = SegmentPreview::Accepted;
        }
    }

    /// Builds the single commit command (`Command::DragSegment`) for release, or `None` when
    /// `offset == 0` or the current offset is refused (WD7, WD11, INV5, WINV4).
    pub fn command(&self) -> Option<Command> {
        if self.offset == 0 || self.is_refused() {
            return None;
        }
        Some(Command::DragSegment {
            wire: self.wire,
            offset: self.offset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{Label, Orient, PIN_LATTICE_NM, Part, PartId, SHEET_LIMIT_NM};
    use omnia_symbols::SymbolLibrary;

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    #[test]
    fn w3_3_segment_offset_moves_perpendicular_in_grid_steps_and_ignores_parallel_axis() {
        // Horizontal segment moves only along y.
        assert_eq!(segment_offset(true, pt(0, 0), pt(5, 0)), 0);
        assert_eq!(
            segment_offset(
                true,
                pt(0, 0),
                Pt {
                    x: 4 * G,
                    y: G / 2 - 1,
                }
            ),
            0
        );
        assert_eq!(segment_offset(true, pt(0, 0), Pt { x: 4 * G, y: G / 2 }), G);
        assert_eq!(
            segment_offset(
                true,
                pt(0, 0),
                Pt {
                    x: -3 * G,
                    y: -G / 2,
                }
            ),
            -G
        );
        assert_eq!(segment_offset(true, pt(1, 2), pt(10, 5)), 3 * G);

        // Vertical segment moves only along x.
        assert_eq!(segment_offset(false, pt(0, 0), pt(0, 7)), 0);
        assert_eq!(segment_offset(false, pt(2, 1), pt(-1, 9)), -3 * G);

        // Extreme coordinates clamp to a grid multiple without overflowing i32.
        let extreme = segment_offset(
            true,
            Pt {
                x: 0,
                y: -SHEET_LIMIT_NM,
            },
            Pt {
                x: 0,
                y: SHEET_LIMIT_NM,
            },
        );
        assert_eq!(extreme % G, 0);
        assert!(extreme > 0);
    }

    #[test]
    fn w3_3_segment_drag_gesture_plans_z_run_and_refuses_foreign_pin() {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(res);
        // R1 at (-6, 0): right pin at (-4, 0).
        // R2 at (0, 2): left pin at (-2, 2) blocks dragging horizontal wire (-4, 0)-(4, 0) down by +2G.
        let doc = Doc::from_items(
            [
                (
                    Part {
                        symbol: res,
                        at: pt(-6, 0),
                        orient: Orient::IDENTITY,
                        designator: "R1".into(),
                        value: "10k".into(),
                    },
                    def.pins.iter().map(|p| p.at).collect(),
                    def.bounds,
                ),
                (
                    Part {
                        symbol: res,
                        at: pt(0, 2),
                        orient: Orient::IDENTITY,
                        designator: "R2".into(),
                        value: "10k".into(),
                    },
                    def.pins.iter().map(|p| p.at).collect(),
                    def.bounds,
                ),
            ],
            [Wire {
                a: pt(-4, 0),
                b: pt(4, 0),
            }],
            [],
        )
        .unwrap();

        let stamp = DocStamp {
            key: 1,
            revision: doc.revision(),
        };
        let mut g = SegmentDragGesture::start(&doc, WireId(0), pt(0, 0), stamp).unwrap();
        assert!(!g.is_lifted());
        assert_eq!(g.hidden_items(), &[ItemId::Wire(WireId(0))]);

        // Moving parallel does not change offset or lift.
        assert!(!g.update(Some(&doc), pt(3, 0)));
        assert!(!g.is_lifted());

        // Moving down by +1G is valid: inserts a jog at R1's right pin (-4, 0).
        assert!(g.update(Some(&doc), pt(0, 1)));
        assert!(g.is_lifted());
        assert_eq!(g.preview(), SegmentPreview::Accepted);
        assert_eq!(g.preview_wires().len(), 2);
        assert_eq!(
            g.command(),
            Some(Command::DragSegment {
                wire: WireId(0),
                offset: G,
            })
        );

        // Moving down by +2G crosses R2's pin 0 at (-2, 2): refused with Blocker::Pin.
        assert!(g.update(Some(&doc), pt(0, 2)));
        assert_eq!(
            g.blocker(),
            Some(Blocker::Pin {
                part: PartId(1),
                pin: 0,
            })
        );
        assert!(g.is_refused());
        assert_eq!(g.command(), None);
        // Attempted wires still populated for `mark/conflict` rendering.
        assert_eq!(
            g.preview_wires(),
            &[
                Wire {
                    a: pt(-4, 0),
                    b: pt(-4, 2),
                },
                Wire {
                    a: pt(-4, 2),
                    b: pt(4, 2),
                },
            ]
        );

        // Dragging back to 0 keeps `lifted == true` (no extra SheetView repaint) and clears refusal.
        assert!(g.update(Some(&doc), pt(0, 0)));
        assert!(g.is_lifted());
        assert!(!g.is_refused());
        assert_eq!(g.command(), None);
        assert_eq!(
            g.preview_wires(),
            &[Wire {
                a: pt(-4, 0),
                b: pt(4, 0),
            }]
        );
    }

    #[test]
    fn w3_3_hidden_items_include_stretching_corner_neighbours_in_canonical_order() {
        let doc = Doc::from_items(
            [],
            [
                Wire {
                    a: pt(0, 0),
                    b: pt(4, 0),
                },
                Wire {
                    a: pt(4, 0),
                    b: pt(4, 4),
                },
                Wire {
                    a: pt(4, 4),
                    b: pt(8, 4),
                },
            ],
            [Label {
                at: Pt { x: 4 * G, y: 4 * G },
                text: "N1".into(),
            }],
        )
        .unwrap();
        let stamp = DocStamp {
            key: 1,
            revision: doc.revision(),
        };
        // Dragging the middle vertical wire (WireId(1)) stretches both WireId(0) and WireId(2),
        // even though corner (4, 4) carries a label (matching `plan_segment_drag`).
        let g = SegmentDragGesture::start(&doc, WireId(1), pt(4, 2), stamp).unwrap();
        assert_eq!(
            g.hidden_items(),
            &[
                ItemId::Wire(WireId(0)),
                ItemId::Wire(WireId(1)),
                ItemId::Wire(WireId(2)),
            ]
        );
        assert!(!g.horizontal);
        assert_eq!(PIN_LATTICE_NM * 2, G);
    }
}
