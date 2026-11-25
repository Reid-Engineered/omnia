//! T5.3: move, rotate, mirror and delete command planning for selections and drags
//! (D2, K2, INV5, WD3, WD10, T5.5).
//!
//! Pure geometry and command builders used by [`crate::tool::ToolMachine`] and
//! [`crate::OverlayView`]:
//! - Anchor-pin grid snapping for dragged parts (K2), keeping zero-movement clicks as clicks.
//! - Deterministic, lattice-safe pivot selection for in-place and mid-drag reorientation.
//! - Single-command commits via `Command::Move`, `Command::Transform` (T5.5), `Command::Reorient`,
//!   and `Command::Delete` so one undo reverts the whole gesture (INV5).

use crate::{camera_anim::selection_bounds, selection::item_exists};
use omnia_doc::{
    Command, Doc, GRID_NM, Isometry, ItemId, Orient, Pt, Rect, SHEET_LIMIT_NM, prefs::FollowPolicy,
    snap_to,
};

/// Largest multiple of [`GRID_NM`] that lies within K1 (`±SHEET_LIMIT_NM`).
const MAX_GRID_NM: i64 = (SHEET_LIMIT_NM / GRID_NM) as i64 * GRID_NM as i64;

/// Applies `orient` to an `i64` offset `(dx, dy)` without risking `i32` overflow.
#[inline]
fn apply_orient_i64(orient: Orient, dx: i64, dy: i64) -> (i64, i64) {
    match orient.to_raw() {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        3 => (dy, -dx),
        4 => (-dx, dy),
        5 => (-dy, -dx),
        6 => (dx, -dy),
        _ => (dy, dx),
    }
}

/// Deterministic, lattice-safe pivot for reorienting `items` (in place or during a drag).
///
/// - A single live `Part` pivots around `part.at` (centre for standard symbols, pin for ground),
///   preserving half-grid pin parity for symbols like `IC-COM-OpAmp`.
/// - A single live `Label` pivots around `label.at`.
/// - A single live `Wire` or multi-item selection pivots around the grid-snapped centre of the
///   live selection bounds (always on `GRID_NM` ⊂ `PIN_LATTICE_NM`, avoiding `OffLattice`).
pub fn selection_pivot(doc: &Doc, items: &[ItemId]) -> Option<Pt> {
    let mut single: Option<ItemId> = None;
    let mut count = 0usize;
    for &item in items {
        if item_exists(doc, item) {
            count += 1;
            if count == 1 {
                single = Some(item);
            } else {
                break;
            }
        }
    }
    if count == 0 {
        return None;
    }
    if count == 1 {
        match single.expect("count == 1") {
            ItemId::Part(id) => return doc.part(id).map(|part| part.at),
            ItemId::Label(id) => return doc.label(id).map(|label| label.at),
            ItemId::Wire(id) => {
                let wire = doc.wire(id)?;
                let rect = Rect {
                    min: Pt {
                        x: wire.a.x.min(wire.b.x),
                        y: wire.a.y.min(wire.b.y),
                    },
                    max: Pt {
                        x: wire.a.x.max(wire.b.x),
                        y: wire.a.y.max(wire.b.y),
                    },
                };
                return Some(snap_to(rect.center(), GRID_NM));
            }
        }
    }
    selection_bounds(doc, items).map(|bounds| snap_to(bounds.center(), GRID_NM))
}

/// Picks the world-space anchor pin used to snap a drag to [`GRID_NM`] (K2).
///
/// Uses pin 0 of `pressed` when it is a live `Part`; otherwise pin 0 of the first live `Part` in
/// canonical `items` order; otherwise `None` (wire/label-only drags snap their raw delta to
/// [`GRID_NM`]).
pub fn drag_anchor_pin(doc: &Doc, pressed: ItemId, items: &[ItemId]) -> Option<Pt> {
    if let ItemId::Part(id) = pressed
        && let Some(pin) = doc.pins(id).and_then(|pins| pins.first().copied())
    {
        return Some(pin);
    }
    for &item in items {
        if let ItemId::Part(id) = item
            && let Some(pin) = doc.pins(id).and_then(|pins| pins.first().copied())
        {
            return Some(pin);
        }
    }
    None
}

/// Computes the snapped translation delta for a drag from `anchor` to `current` with mid-drag
/// reorientation `by` about `pivot` (K2).
///
/// When `current == anchor && by == Orient::IDENTITY`, returns [`Pt::ZERO`] so clicking an
/// off-grid part without moving remains a zero-delta click rather than jumping.
///
/// Otherwise, if `anchor_pin` is `Some(p0)`, transforms `p0` by `by` around `pivot`, adds
/// `current - anchor`, snaps that world pin position onto [`GRID_NM`], and returns the exact
/// delta from the rotated pin to the snapped grid point so the anchor pin lands on [`GRID_NM`].
pub fn drag_delta(anchor: Pt, current: Pt, pivot: Pt, by: Orient, anchor_pin: Option<Pt>) -> Pt {
    if current == anchor && by == Orient::IDENTITY {
        return Pt::ZERO;
    }
    let raw_dx = i64::from(current.x) - i64::from(anchor.x);
    let raw_dy = i64::from(current.y) - i64::from(anchor.y);
    if let Some(p0) = anchor_pin {
        let dx = i64::from(p0.x) - i64::from(pivot.x);
        let dy = i64::from(p0.y) - i64::from(pivot.y);
        let (rx, ry) = apply_orient_i64(by, dx, dy);
        let p_rot_x = i64::from(pivot.x) + rx;
        let p_rot_y = i64::from(pivot.y) + ry;
        let target = Pt {
            x: (p_rot_x + raw_dx).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
            y: (p_rot_y + raw_dy).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
        };
        let snapped = snap_to(target, GRID_NM);
        let del_x = i64::from(snapped.x) - p_rot_x;
        let del_y = i64::from(snapped.y) - p_rot_y;
        Pt {
            x: del_x.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: del_y.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        }
    } else {
        let raw = Pt {
            x: i32::try_from(raw_dx).expect("K1 keeps a drag delta inside i32"),
            y: i32::try_from(raw_dy).expect("K1 keeps a drag delta inside i32"),
        };
        snap_to(raw, GRID_NM)
    }
}

/// The [`Isometry`] describing a drag preview and its matching commit (`Command::Move` or
/// `Command::Transform`).
#[inline]
pub fn drag_isometry(delta: Pt, pivot: Pt, by: Orient) -> Isometry {
    Isometry::Transform { delta, pivot, by }
}

/// Builds the single commit command for a drag (INV5, T5.5), or `None` when nothing changed.
///
/// - `by == Orient::IDENTITY && delta == Pt::ZERO`: `None` (a click, no edit).
/// - `by == Orient::IDENTITY`: `Command::Move`.
/// - `by != Orient::IDENTITY`: `Command::Transform` (even when `delta == Pt::ZERO`), planned and
///   normalised once against the original sheet so one undo reverts the whole gesture.
pub fn drag_command(
    items: &[ItemId],
    delta: Pt,
    pivot: Pt,
    by: Orient,
    follow: FollowPolicy,
) -> Option<Command> {
    if items.is_empty() || (by == Orient::IDENTITY && delta == Pt::ZERO) {
        return None;
    }
    if by == Orient::IDENTITY {
        Some(Command::Move {
            items: items.to_vec(),
            delta,
            follow,
        })
    } else {
        Some(Command::Transform {
            items: items.to_vec(),
            delta,
            pivot,
            by,
            follow,
        })
    }
}

/// Builds a direct `Command::Reorient` for idle R, X or Y on `selection` (WD3).
pub fn reorient_selection_command(
    doc: &Doc,
    selection: &[ItemId],
    by: Orient,
    follow: FollowPolicy,
) -> Option<Command> {
    if by == Orient::IDENTITY || selection.is_empty() {
        return None;
    }
    let items: Vec<ItemId> = selection
        .iter()
        .copied()
        .filter(|&item| item_exists(doc, item))
        .collect();
    if items.is_empty() {
        return None;
    }
    let pivot = selection_pivot(doc, &items)?;
    Some(Command::Reorient {
        items,
        pivot,
        by,
        follow,
    })
}

/// Builds `Command::Delete` for the live items in `selection` (WD10, WQ4).
pub fn delete_selection_command(
    doc: &Doc,
    selection: &[ItemId],
    with_wires: bool,
) -> Option<Command> {
    let items: Vec<ItemId> = selection
        .iter()
        .copied()
        .filter(|&item| item_exists(doc, item))
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(Command::Delete { items, with_wires })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{
        Label, LabelId, PIN_LATTICE_NM, Part, PartId, Wire, WireId, prefs::WiringPrefs,
    };
    use omnia_symbols::SymbolLibrary;

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    #[test]
    fn t5_3_selection_pivot_rules_are_deterministic_and_lattice_safe() {
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let def = lib.get(opamp);
        let opamp_at = Pt {
            x: -PIN_LATTICE_NM,
            y: -PIN_LATTICE_NM,
        };
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: opamp,
                    at: opamp_at,
                    orient: Orient::IDENTITY,
                    designator: "U1".into(),
                    value: "".into(),
                },
                def.pins.iter().map(|p| p.at).collect(),
                def.bounds,
            )],
            [Wire {
                a: Pt { x: 0, y: 0 },
                b: Pt { x: 3 * G, y: 0 },
            }],
            [Label {
                at: Pt {
                    x: PIN_LATTICE_NM,
                    y: 2 * G,
                },
                text: "N1".into(),
            }],
        )
        .unwrap();

        // Single part pivots on part.at (preserves half-grid parity for OpAmp).
        assert_eq!(
            selection_pivot(&doc, &[ItemId::Part(PartId(0))]),
            Some(opamp_at)
        );
        // Single label pivots on label.at.
        assert_eq!(
            selection_pivot(&doc, &[ItemId::Label(LabelId(0))]),
            Some(Pt {
                x: PIN_LATTICE_NM,
                y: 2 * G,
            })
        );
        // Single wire snaps midpoint (1.5G, 0) onto GRID_NM.
        let wire_pivot = selection_pivot(&doc, &[ItemId::Wire(WireId(0))]).unwrap();
        assert_eq!(wire_pivot.x % G, 0);
        assert_eq!(wire_pivot.y % G, 0);

        // Multi-item selection snaps bounding centre onto GRID_NM, independent of input order.
        let p_ab =
            selection_pivot(&doc, &[ItemId::Part(PartId(0)), ItemId::Wire(WireId(0))]).unwrap();
        let p_ba =
            selection_pivot(&doc, &[ItemId::Wire(WireId(0)), ItemId::Part(PartId(0))]).unwrap();
        assert_eq!(p_ab, p_ba);
        assert_eq!(p_ab.x % G, 0);
        assert_eq!(p_ab.y % G, 0);
    }

    #[test]
    fn t5_3_drag_delta_snaps_anchor_pin_to_grid_and_keeps_click_at_zero() {
        // Off-grid anchor pin: clicking without moving stays zero delta.
        let off_pin = Pt {
            x: PIN_LATTICE_NM,
            y: PIN_LATTICE_NM,
        };
        assert_eq!(
            drag_delta(
                pt(0, 0),
                pt(0, 0),
                pt(0, 0),
                Orient::IDENTITY,
                Some(off_pin)
            ),
            Pt::ZERO
        );

        // Moving an off-grid part snaps its anchor pin onto GRID_NM.
        let delta = drag_delta(
            pt(0, 0),
            Pt {
                x: 2 * G + 200_000,
                y: -G - 100_000,
            },
            pt(0, 0),
            Orient::IDENTITY,
            Some(off_pin),
        );
        let final_pin = off_pin + delta;
        assert_eq!(final_pin.x % G, 0);
        assert_eq!(final_pin.y % G, 0);

        // Rotating during drag keeps the transformed anchor pin on GRID_NM.
        let delta_rot = drag_delta(
            pt(0, 0),
            Pt {
                x: G + 300_000,
                y: -2 * G,
            },
            pt(0, 0),
            Orient::ROT_90,
            Some(off_pin),
        );
        let iso = drag_isometry(delta_rot, pt(0, 0), Orient::ROT_90);
        let final_rot_pin = iso.apply(off_pin).unwrap();
        assert_eq!(final_rot_pin.x % G, 0);
        assert_eq!(final_rot_pin.y % G, 0);

        // K1 boundary: dragging to ±SHEET_LIMIT_NM snaps anchor pin on GRID_NM inside K1.
        for edge in [
            Pt {
                x: SHEET_LIMIT_NM,
                y: SHEET_LIMIT_NM,
            },
            Pt {
                x: -SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
        ] {
            let d = drag_delta(pt(0, 0), edge, pt(0, 0), Orient::IDENTITY, Some(off_pin));
            let snapped_pin = off_pin + d;
            assert!(
                snapped_pin.is_in_bounds(),
                "edge {edge:?} -> pin {snapped_pin:?} out of K1"
            );
            assert_eq!(snapped_pin.x % G, 0);
            assert_eq!(snapped_pin.y % G, 0);
        }
    }

    #[test]
    fn t5_3_drag_command_uses_move_or_transform() {
        let follow = WiringPrefs::default().follow_policy(false);
        let items = [ItemId::Part(PartId(0))];
        assert_eq!(
            drag_command(&items, Pt::ZERO, Pt::ZERO, Orient::IDENTITY, follow),
            None
        );
        assert_eq!(
            drag_command(&items, pt(1, 0), Pt::ZERO, Orient::IDENTITY, follow),
            Some(Command::Move {
                items: items.to_vec(),
                delta: pt(1, 0),
                follow,
            })
        );
        assert_eq!(
            drag_command(&items, Pt::ZERO, pt(2, 3), Orient::ROT_90, follow),
            Some(Command::Transform {
                items: items.to_vec(),
                delta: Pt::ZERO,
                pivot: pt(2, 3),
                by: Orient::ROT_90,
                follow,
            })
        );
    }
}
