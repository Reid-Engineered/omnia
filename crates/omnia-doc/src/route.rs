//! Wire routing rules: the L-route and the split/merge normal form applied by
//! `Command::AddWires` (D2, INV2; wiring-plan WD4, WD9, M12, M18).
//!
//! Normal form after an `AddWires` command (WINV3 restricted to wires):
//! no wire end lies in another wire's interior; no two wires share a stretch of
//! non-zero length; and no two collinear segments meet at a point that holds
//! nothing else. A meeting point holds something else when a pin lies there, a
//! label sits there (WD9: a labelled point is never merged away), or a third
//! wire end meets there (a junction). Crossing interiors never split and never
//! connect (M18). The planner is pure and deterministic: its result depends
//! only on the geometry and the existing ids, never on hash-map iteration order
//! or on the order of the added wires. Ties are broken by geometry, then by the
//! lowest existing id, which also survives a split or merge.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::{Pt, Wire, WireId};

/// The L-route between `a` and `b`: zero, one, or two axis-aligned segments.
///
/// Equal points route to nothing and collinear points to a single segment.
/// Otherwise the route has one corner. `flip == false` leaves `a` horizontally
/// (corner at `(b.x, a.y)`); `flip == true` leaves `a` vertically (corner at
/// `(a.x, b.y)`). Segment direction follows `a` to corner to `b`.
pub fn l_route(a: Pt, b: Pt, flip: bool) -> Vec<Wire> {
    if a == b {
        return Vec::new();
    }
    if a.x == b.x || a.y == b.y {
        return vec![Wire { a, b }];
    }
    let corner = if flip {
        Pt { x: a.x, y: b.y }
    } else {
        Pt { x: b.x, y: a.y }
    };
    vec![Wire { a, b: corner }, Wire { a: corner, b }]
}

fn pt_key(p: Pt) -> (i32, i32) {
    (p.x, p.y)
}

/// Normalised (low, high, horizontal) view of a valid wire.
fn normalised(wire: Wire) -> (Pt, Pt, bool) {
    if wire.a.y == wire.b.y {
        if wire.a.x <= wire.b.x {
            (wire.a, wire.b, true)
        } else {
            (wire.b, wire.a, true)
        }
    } else if wire.a.y <= wire.b.y {
        (wire.a, wire.b, false)
    } else {
        (wire.b, wire.a, false)
    }
}

struct SourceSeg {
    lo: Pt,
    hi: Pt,
    horizontal: bool,
    existing: Option<WireId>,
    descending: bool,
}

#[derive(Clone)]
struct Atomic {
    lo: Pt,
    hi: Pt,
    horizontal: bool,
    existing: Vec<WireId>,
    descending: Option<bool>,
}

struct FinalSeg {
    lo: Pt,
    hi: Pt,
    horizontal: bool,
    existing: Vec<WireId>,
    descending: bool,
}

/// The wire edits an `AddWires` command must perform, in deterministic order.
///
/// `remove` holds every existing wire that execution must take out (wires that
/// disappear and wires whose geometry changes), sorted by id. `insert` holds
/// the final wires to put in, in geometry order; `Some(id)` restores a kept
/// existing id with new geometry, `None` allocates a fresh id. Existing wires
/// absent from both are untouched and keep their ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AddWiresPlan {
    pub remove: Vec<(WireId, Wire)>,
    pub insert: Vec<(Option<WireId>, Wire)>,
}

/// Plans the split and merge normalisation for `AddWires`.
///
/// `existing` is the live wire set, `pins` every part pin point and `labels`
/// every label point. All inputs are treated as sets: their order does not
/// affect the plan.
pub(crate) fn plan_add_wires(
    existing: &[(WireId, Wire)],
    pins: &[Pt],
    labels: &[Pt],
    added: &[Wire],
) -> AddWiresPlan {
    if added.is_empty() {
        return AddWiresPlan {
            remove: Vec::new(),
            insert: Vec::new(),
        };
    }

    let mut existing_sorted: Vec<(WireId, Wire)> = existing.to_vec();
    existing_sorted.sort_by_key(|(id, _)| id.0);
    // Canonical added order: geometry first, so fresh ids do not depend on the
    // caller's wire order.
    let mut added_sorted: Vec<Wire> = added.to_vec();
    added_sorted.sort_by(|lhs, rhs| {
        let (llo, lhi, _) = normalised(*lhs);
        let (rlo, rhi, _) = normalised(*rhs);
        (pt_key(llo), pt_key(lhi), pt_key(lhs.a), pt_key(lhs.b)).cmp(&(
            pt_key(rlo),
            pt_key(rhi),
            pt_key(rhs.a),
            pt_key(rhs.b),
        ))
    });

    let mut sources: Vec<SourceSeg> =
        Vec::with_capacity(existing_sorted.len() + added_sorted.len());
    for &(id, wire) in &existing_sorted {
        let (lo, hi, horizontal) = normalised(wire);
        sources.push(SourceSeg {
            lo,
            hi,
            horizontal,
            existing: Some(id),
            descending: wire.a != lo,
        });
    }
    for &wire in added_sorted.iter() {
        let (lo, hi, horizontal) = normalised(wire);
        sources.push(SourceSeg {
            lo,
            hi,
            horizontal,
            existing: None,
            descending: wire.a != lo,
        });
    }

    // Feature points: every segment end, pin and label point. A segment is
    // split at every feature point in its interior (INV2, WD9).
    let mut feature: Vec<Pt> = Vec::new();
    for source in &sources {
        feature.push(source.lo);
        feature.push(source.hi);
    }
    feature.extend_from_slice(pins);
    feature.extend_from_slice(labels);
    feature.sort_by_key(|&p| pt_key(p));
    feature.dedup();

    let mut rows: FxHashMap<i32, Vec<i32>> = FxHashMap::default();
    let mut cols: FxHashMap<i32, Vec<i32>> = FxHashMap::default();
    for p in &feature {
        rows.entry(p.y).or_default().push(p.x);
        cols.entry(p.x).or_default().push(p.y);
    }
    for xs in rows.values_mut() {
        xs.sort_unstable();
        xs.dedup();
    }
    for ys in cols.values_mut() {
        ys.sort_unstable();
        ys.dedup();
    }

    // Atomic pieces: maximal sub-segments free of interior feature points.
    // Identical pieces from overlapping wires are deduplicated here, so no two
    // final wires can share a stretch.
    let mut atomic_index: FxHashMap<(u8, i32, i32, i32), usize> = FxHashMap::default();
    let mut atomics: Vec<Atomic> = Vec::new();
    for source in &sources {
        let mut cuts: Vec<i32> = Vec::new();
        if source.horizontal {
            if let Some(xs) = rows.get(&source.lo.y) {
                let start = xs.partition_point(|&x| x <= source.lo.x);
                let end = xs.partition_point(|&x| x < source.hi.x);
                cuts.extend_from_slice(&xs[start..end]);
            }
        } else if let Some(ys) = cols.get(&source.lo.x) {
            let start = ys.partition_point(|&y| y <= source.lo.y);
            let end = ys.partition_point(|&y| y < source.hi.y);
            cuts.extend_from_slice(&ys[start..end]);
        }
        let mut bounds: Vec<Pt> = Vec::with_capacity(cuts.len() + 2);
        bounds.push(source.lo);
        bounds.extend(cuts.iter().map(|&c| {
            if source.horizontal {
                Pt {
                    x: c,
                    y: source.lo.y,
                }
            } else {
                Pt {
                    x: source.lo.x,
                    y: c,
                }
            }
        }));
        bounds.push(source.hi);
        for pair in bounds.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            let key = (
                u8::from(!source.horizontal),
                if source.horizontal { lo.y } else { lo.x },
                if source.horizontal { lo.x } else { lo.y },
                if source.horizontal { hi.x } else { hi.y },
            );
            let idx = *atomic_index.entry(key).or_insert_with(|| {
                atomics.push(Atomic {
                    lo,
                    hi,
                    horizontal: source.horizontal,
                    existing: Vec::new(),
                    descending: Some(source.descending),
                });
                atomics.len() - 1
            });
            let atomic = &mut atomics[idx];
            if let Some(id) = source.existing
                && !atomic.existing.contains(&id)
            {
                atomic.existing.push(id);
            }
            if atomic.descending != Some(source.descending) {
                atomic.descending = None;
            }
        }
    }

    // Ends per point over the deduplicated pieces: a merge point must hold
    // exactly the two ends being joined, and no pin or label.
    let mut incidence: FxHashMap<(i32, i32), u32> = FxHashMap::default();
    for atomic in &atomics {
        *incidence.entry(pt_key(atomic.lo)).or_default() += 1;
        *incidence.entry(pt_key(atomic.hi)).or_default() += 1;
    }
    let mut pin_keys: Vec<(i32, i32)> = pins.iter().map(|&p| pt_key(p)).collect();
    pin_keys.sort_unstable();
    pin_keys.dedup();
    let mut label_keys: Vec<(i32, i32)> = labels.iter().map(|&p| pt_key(p)).collect();
    label_keys.sort_unstable();
    label_keys.dedup();
    let mergeable = |at: Pt| {
        incidence.get(&pt_key(at)).copied().unwrap_or(0) == 2
            && pin_keys.binary_search(&pt_key(at)).is_err()
            && label_keys.binary_search(&pt_key(at)).is_err()
    };

    // Merge collinear neighbours along each line, farthest first by geometry.
    let mut lines: BTreeMap<(u8, i32), Vec<usize>> = BTreeMap::new();
    for (idx, atomic) in atomics.iter().enumerate() {
        let fixed = if atomic.horizontal {
            atomic.lo.y
        } else {
            atomic.lo.x
        };
        lines
            .entry((u8::from(!atomic.horizontal), fixed))
            .or_default()
            .push(idx);
    }
    let mut finals: Vec<FinalSeg> = Vec::new();
    for indices in lines.values_mut() {
        indices.sort_by_key(|&idx| {
            let a = &atomics[idx];
            if a.horizontal { a.lo.x } else { a.lo.y }
        });
        let mut run: Vec<usize> = Vec::new();
        let finish_run = |run: &mut Vec<usize>, finals: &mut Vec<FinalSeg>| {
            if run.is_empty() {
                return;
            }
            let first = &atomics[run[0]];
            let last = &atomics[*run.last().unwrap()];
            let mut seg_existing: Vec<WireId> = Vec::new();
            let mut descending = atomics[run[0]].descending;
            for &idx in run.iter() {
                for &id in &atomics[idx].existing {
                    if !seg_existing.contains(&id) {
                        seg_existing.push(id);
                    }
                }
                if atomics[idx].descending != descending {
                    descending = None;
                }
            }
            seg_existing.sort_by_key(|id| id.0);
            finals.push(FinalSeg {
                lo: first.lo,
                hi: last.hi,
                horizontal: first.horizontal,
                existing: seg_existing,
                descending: descending.unwrap_or(false),
            });
            run.clear();
        };
        for &idx in indices.iter() {
            if let Some(&last_idx) = run.last() {
                let junction = atomics[last_idx].hi;
                if atomics[idx].lo != junction || !mergeable(junction) {
                    finish_run(&mut run, &mut finals);
                }
            }
            run.push(idx);
        }
        finish_run(&mut run, &mut finals);
    }

    // Keep the lowest surviving existing id per final segment; the first final
    // segment (in geometry order) wins a split wire's id.
    finals.sort_by(|lhs, rhs| {
        (pt_key(lhs.lo), pt_key(lhs.hi), lhs.horizontal).cmp(&(
            pt_key(rhs.lo),
            pt_key(rhs.hi),
            rhs.horizontal,
        ))
    });
    let existing_wire: BTreeMap<u32, Wire> = existing_sorted
        .iter()
        .map(|(id, wire)| (id.0, *wire))
        .collect();
    let mut used: Vec<WireId> = Vec::new();
    let mut remove: Vec<(WireId, Wire)> = Vec::new();
    let mut insert: Vec<(Option<WireId>, Wire)> = Vec::new();
    for seg in &finals {
        let wire = if seg.descending {
            Wire {
                a: seg.hi,
                b: seg.lo,
            }
        } else {
            Wire {
                a: seg.lo,
                b: seg.hi,
            }
        };
        let keep = seg.existing.iter().copied().find(|id| !used.contains(id));
        match keep {
            Some(id) => {
                used.push(id);
                let old = existing_wire[&id.0];
                let (old_lo, old_hi, _) = normalised(old);
                if old_lo != seg.lo || old_hi != seg.hi {
                    remove.push((id, old));
                    insert.push((Some(id), wire));
                }
            }
            None => insert.push((None, wire)),
        }
    }
    for &(id, wire) in &existing_sorted {
        if !used.contains(&id) {
            remove.push((id, wire));
        }
    }
    remove.sort_by_key(|(id, _)| id.0);
    AddWiresPlan { remove, insert }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Connectivity, Doc, Label, Orient, Part, Rect, SymbolId};
    use proptest::prelude::*;

    fn w(x1: i32, y1: i32, x2: i32, y2: i32) -> Wire {
        Wire {
            a: Pt { x: x1, y: y1 },
            b: Pt { x: x2, y: y2 },
        }
    }

    fn live_wires(doc: &Doc) -> Vec<(WireId, Wire)> {
        doc.wires().map(|(id, &wire)| (id, wire)).collect()
    }

    fn interior_contains(wire: Wire, p: Pt) -> bool {
        if wire.a.y == wire.b.y {
            p.y == wire.a.y && p.x > wire.a.x.min(wire.b.x) && p.x < wire.a.x.max(wire.b.x)
        } else {
            p.x == wire.a.x && p.y > wire.a.y.min(wire.b.y) && p.y < wire.a.y.max(wire.b.y)
        }
    }

    /// INV2: no wire end lies in another wire's interior.
    fn assert_inv2(doc: &Doc) {
        let wires = live_wires(doc);
        for &(id, wire) in &wires {
            for &(other_id, other) in &wires {
                if id != other_id {
                    assert!(
                        !interior_contains(other, wire.a) && !interior_contains(other, wire.b),
                        "INV2: end of {id:?} {wire:?} lies in interior of {other_id:?} {other:?}"
                    );
                }
            }
        }
    }

    /// WINV3 wire half: no shared stretch; no mergeable collinear meeting.
    fn assert_wire_normal_form(doc: &Doc) {
        let wires = live_wires(doc);
        for (i, &(_, a)) in wires.iter().enumerate() {
            for &(_, b) in &wires[i + 1..] {
                let (alo, ahi, ah) = normalised(a);
                let (blo, bhi, bh) = normalised(b);
                if ah == bh {
                    if ah && alo.y == blo.y {
                        assert!(
                            ahi.x <= blo.x || bhi.x <= alo.x,
                            "shared stretch: {a:?} {b:?}"
                        );
                    }
                    if !ah && alo.x == blo.x {
                        assert!(
                            ahi.y <= blo.y || bhi.y <= alo.y,
                            "shared stretch: {a:?} {b:?}"
                        );
                    }
                }
            }
        }
        // A point with exactly two wire ends, no pin and no label, must not
        // join two collinear segments (they should have merged).
        let mut ends: Vec<(Pt, WireId, Wire)> = Vec::new();
        for &(id, wire) in &wires {
            ends.push((wire.a, id, wire));
            ends.push((wire.b, id, wire));
        }
        let pins: Vec<Pt> = doc
            .parts()
            .flat_map(|(id, _)| doc.pins(id).unwrap().to_vec())
            .collect();
        let labels: Vec<Pt> = doc.labels().map(|(_, label)| label.at).collect();
        for &(at, _, _) in &ends {
            let here: Vec<&(Pt, WireId, Wire)> = ends.iter().filter(|(p, _, _)| *p == at).collect();
            if here.len() == 2 && !pins.contains(&at) && !labels.contains(&at) {
                let first = here[0].2;
                let second = here[1].2;
                let collinear = (first.a.y == first.b.y && second.a.y == second.b.y)
                    || (first.a.x == first.b.x && second.a.x == second.b.x);
                assert!(
                    !collinear,
                    "mergeable collinear segments meet at {at:?}: {first:?} {second:?}"
                );
            }
        }
    }

    fn assert_inv10(doc: &Doc) {
        let oracle = Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &wire)| (id, wire)),
            doc.labels(),
            doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
        );
        crate::connectivity::tests::assert_partitions(&doc.connectivity, &oracle);
    }

    fn assert_all(doc: &Doc) {
        assert_inv2(doc);
        assert_wire_normal_form(doc);
        assert_inv10(doc);
    }

    #[test]
    fn l_route_corners_and_degenerate_cases() {
        let a = Pt { x: 0, y: 0 };
        let b = Pt { x: 10, y: 20 };
        assert_eq!(l_route(a, a, false), Vec::new());
        assert_eq!(l_route(a, Pt { x: 10, y: 0 }, true), vec![w(0, 0, 10, 0)]);
        assert_eq!(
            l_route(a, b, false),
            vec![w(0, 0, 10, 0), w(10, 0, 10, 20)],
            "flip=false leaves a horizontally"
        );
        assert_eq!(
            l_route(a, b, true),
            vec![w(0, 0, 0, 20), w(0, 20, 10, 20)],
            "flip=true leaves a vertically"
        );
    }

    #[test]
    fn inv2_wire_ending_on_body_splits_and_joins_net() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        let before = doc
            .wires()
            .map(|(id, &wire)| (id, wire))
            .collect::<Vec<_>>();
        assert_eq!(before, vec![(WireId(0), w(0, 0, 10, 0))]);
        doc.apply(Command::AddWires(vec![w(5, 0, 5, 10)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 3);
        // The split keeps the original id on the first piece (INV4 helper).
        assert_eq!(doc.wire(WireId(0)), Some(&w(0, 0, 5, 0)));
        let net = doc.net_of(WireId(0)).unwrap();
        let mut net_wires = Vec::new();
        doc.net_wires(net, &mut net_wires);
        assert_eq!(net_wires.len(), 3, "a wire ending on a body joins its net");
        // INV4: undo restores the pre-split wire with its id; redo re-splits.
        doc.undo().unwrap();
        assert_eq!(live_wires(&doc), before);
        assert_all(&doc);
        doc.redo().unwrap();
        assert_eq!(doc.wires().count(), 3);
        assert_all(&doc);
    }

    #[test]
    fn inv2_crossing_interiors_never_connect() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        doc.apply(Command::AddWires(vec![w(5, -5, 5, 5)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 2, "crossing interiors are not split");
        assert_ne!(
            doc.net_of(WireId(0)),
            doc.net_of(WireId(1)),
            "two wires crossing at interiors are in different nets (INV2)"
        );
    }

    #[test]
    fn added_wire_splits_over_existing_end_and_pin() {
        // Over an existing end.
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(5, 0, 5, 10)])).unwrap();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 3);
        assert_eq!(
            doc.net_of(WireId(0)),
            doc.net_of(WireId(1)),
            "passing over an existing end connects there"
        );

        // Over a pin.
        let mut doc = Doc::new();
        doc.apply(Command::PlacePart {
            part: Part {
                symbol: SymbolId(0),
                at: Pt { x: 5, y: 0 },
                orient: Orient::IDENTITY,
                designator: "R1".into(),
                value: "1k".into(),
            },
            local_pins: vec![Pt::ZERO],
            local_bounds: Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
        })
        .unwrap();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 2, "the added wire splits at the pin");
        assert!(doc.net_at(Pt { x: 5, y: 0 }).is_some());
        let first = doc.wires().next().unwrap().0;
        assert_eq!(doc.net_of(first), doc.net_at(Pt { x: 5, y: 0 }));
    }

    #[test]
    fn collinear_segments_merge_and_undo_restores_ids() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 5, 0)])).unwrap();
        doc.apply(Command::AddWires(vec![w(5, 0, 10, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(
            live_wires(&doc),
            vec![(WireId(0), w(0, 0, 10, 0))],
            "merged segment keeps the lowest id"
        );
        doc.undo().unwrap();
        assert_eq!(live_wires(&doc), vec![(WireId(0), w(0, 0, 5, 0))]);
        doc.undo().unwrap();
        assert!(doc.wires().next().is_none());

        // No merge across a junction (a third end at the meeting point).
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0), w(5, 0, 5, 10)]))
            .unwrap();
        assert_all(&doc);
        assert_eq!(doc.wires().count(), 3);

        // No merge across a labelled point (WD9; the W1.4 predicate).
        let mut doc = Doc::from_items(
            [],
            [w(0, 0, 5, 0), w(5, 0, 10, 0)],
            [Label {
                at: Pt { x: 5, y: 0 },
                text: "SIG".into(),
            }],
        )
        .unwrap();
        doc.apply(Command::AddWires(vec![w(20, 0, 25, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(doc.wire(WireId(0)), Some(&w(0, 0, 5, 0)));
        assert_eq!(doc.wire(WireId(1)), Some(&w(5, 0, 10, 0)));
    }

    #[test]
    fn overlapping_wires_unite_without_shared_stretch() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![w(0, 0, 10, 0)])).unwrap();
        doc.apply(Command::AddWires(vec![w(5, 0, 15, 0)])).unwrap();
        assert_all(&doc);
        assert_eq!(live_wires(&doc), vec![(WireId(0), w(0, 0, 15, 0))]);
        // Re-adding an existing wire is a validated no-op.
        let revision = doc.revision();
        let change = doc.apply(Command::AddWires(vec![w(0, 0, 15, 0)])).unwrap();
        assert_eq!(change.dirty, None);
        assert_eq!(doc.revision(), revision);
    }

    #[test]
    fn add_wires_result_does_not_depend_on_wire_order() {
        let wires = vec![w(0, 0, 10, 0), w(5, 0, 5, 10), w(5, 10, 15, 10)];
        let mut first = Doc::new();
        first.apply(Command::AddWires(wires.clone())).unwrap();
        let mut second = Doc::new();
        second
            .apply(Command::AddWires(vec![wires[2], wires[0], wires[1]]))
            .unwrap();
        assert!(first.content_eq(&second));
        assert_eq!(live_wires(&first), live_wires(&second));
        assert_all(&first);
    }

    fn wire_strategy() -> impl Strategy<Value = Wire> {
        (-4i32..5, -4i32..5, any::<bool>(), 1i32..5).prop_map(|(x, y, horiz, len)| {
            if horiz {
                w(x, y, x + len, y)
            } else {
                w(x, y, x, y + len)
            }
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        /// INV2 + INV10 hold after every AddWires step, and undoing every step
        /// restores the empty document (INV4, ids included via content_eq).
        #[test]
        fn inv2_inv10_and_undo_over_random_add_wires(
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..4), 1..24),
        ) {
            let mut doc = Doc::new();
            let empty = Doc::new();
            let mut applied = 0usize;
            for wires in &batches {
                let before_bytes = {
                    let mut bytes = Vec::new();
                    doc.save(&mut bytes).unwrap();
                    bytes
                };
                let before = Doc::load(before_bytes.as_slice()).unwrap();
                let change = doc.apply(Command::AddWires(wires.clone())).unwrap();
                assert_all(&doc);
                if change.dirty.is_some() {
                    prop_assert!(doc.undo().is_some());
                    prop_assert!(doc.content_eq(&before), "INV4: undo restores pre-AddWires ids");
                    prop_assert!(doc.redo().is_some());
                    assert_all(&doc);
                    applied += 1;
                } else {
                    prop_assert!(doc.content_eq(&before), "no-op AddWires changes nothing");
                }
            }
            for _ in 0..applied {
                if doc.undo().is_none() {
                    break;
                }
            }
            // Undo steps that were no-ops pushed nothing; drain any remainder.
            while doc.undo().is_some() {}
            prop_assert!(doc.content_eq(&empty), "undoing every AddWires restores the empty doc");
        }
    }
}
