//! Wire routing rules: the L-route and the split/merge normal form applied by
//! `Command::AddWires` (D2, INV2; wiring-plan WD4, WD9, M12, M18).
//!
//! Local normal form in the touched neighbourhood of `AddWires` (INV2):
//! no wire end lies in another wire's interior; no two wires share a stretch of
//! non-zero length; and no two collinear segments meet at a point that holds
//! nothing else. A meeting point holds something else when a pin lies there, a
//! label sits there (WD9: a labelled point is never merged away), or a third
//! wire end meets there (a junction). Crossing interiors never split and never
//! connect (M18). The planner is pure and deterministic: its result depends
//! only on the geometry and the existing ids, never on hash-map iteration order
//! or on the order of the added wires. Ties are broken by geometry, then by the
//! lowest existing id on a merge; the first geometry-ordered split piece keeps
//! its source id. Fresh ids follow geometry order. Unrelated pre-existing
//! non-normal geometry is preserved; see `plan_add_wires` for the boundary.

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{Doc, ItemId, Pt, Rect, Wire, WireId, points::End};

/// The L-route between `a` and `b`: zero, one, or two axis-aligned segments.
///
/// Equal points route to nothing and collinear points to a single segment.
/// Otherwise the route has one corner. `flip == false` leaves `a` horizontally
/// (corner at `(b.x, a.y)`); `flip == true` leaves `a` vertically (corner at
/// `(a.x, b.y)`). Segment direction follows `a` to corner to `b`.
pub fn l_route(a: Pt, b: Pt, flip: bool) -> [Option<Wire>; 2] {
    if a == b {
        return [None, None];
    }
    if a.x == b.x || a.y == b.y {
        return [Some(Wire { a, b }), None];
    }
    let corner = if flip {
        Pt { x: a.x, y: b.y }
    } else {
        Pt { x: b.x, y: a.y }
    };
    [Some(Wire { a, b: corner }), Some(Wire { a: corner, b })]
}

/// WD4 refusal. Pin indices match `Doc::pins` without narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocker {
    Pin {
        part: crate::PartId,
        pin: usize,
    },
    WireEnd(WireId),
    Overlap(WireId),
    /// Outside K1 or off the WK1 lattice (also malformed candidate segments).
    Sheet,
}

fn on_lattice(p: Pt) -> bool {
    p.is_in_bounds() && p.x % crate::PIN_LATTICE_NM == 0 && p.y % crate::PIN_LATTICE_NM == 0
}

fn interior_contains(w: Wire, p: Pt) -> bool {
    contains(w, p) && p != w.a && p != w.b
}

fn shared_stretch(a: Wire, b: Wire) -> bool {
    let (al, ah, horizontal) = normalised(a);
    let (bl, bh, other_horizontal) = normalised(b);
    horizontal == other_horizontal
        && if horizontal {
            al.y == bl.y && al.x.max(bl.x) < ah.x.min(bh.x)
        } else {
            al.x == bl.x && al.y.max(bl.y) < ah.y.min(bh.y)
        }
}

/// WD4: rule priority first, then segment order from `a`. Within a segment,
/// choose the first obstruction along its direction, then geometry, kind (pin
/// before end), and lowest id/index. Own ends exempt points, never overlaps.
/// Query buffers contain only items intersecting one segment, not the sheet.
/// Callers remove the old run from the document view before validating it.
pub fn check_new_wires(doc: &Doc, wires: &[Wire], own_ends: [Pt; 2]) -> Result<(), Blocker> {
    for &w in wires {
        if !on_lattice(w.a) || !on_lattice(w.b) || w.a == w.b || (w.a.x != w.b.x && w.a.y != w.b.y)
        {
            return Err(Blocker::Sheet);
        }
    }
    let mut scratch = Vec::new();
    for &w in wires {
        doc.query(Rect::from_points(w.a, w.b).unwrap(), &mut scratch);
        let mut best = None;
        let mut consider = |p: Pt, kind: u8, id: u32, pin: usize, blocker| {
            if interior_contains(w, p) && !own_ends.contains(&p) {
                let key = (w.a.manhattan_distance(p), pt_key(p), kind, id, pin);
                if best.as_ref().is_none_or(|&(old, _)| key < old) {
                    best = Some((key, blocker));
                }
            }
        };
        for &item in &scratch {
            match item {
                ItemId::Part(part) => {
                    for (pin, &p) in doc.pins(part).unwrap().iter().enumerate() {
                        consider(p, 0, part.0, pin, Blocker::Pin { part, pin });
                    }
                }
                ItemId::Wire(id) => {
                    let other = *doc.wire(id).unwrap();
                    for p in [other.a, other.b] {
                        consider(p, 1, id.0, 0, Blocker::WireEnd(id));
                    }
                }
                ItemId::Label(_) => {}
            }
        }
        if let Some((_, blocker)) = best {
            return Err(blocker);
        }
    }
    for &w in wires {
        doc.query(Rect::from_points(w.a, w.b).unwrap(), &mut scratch);
        let mut best = None;
        for &item in &scratch {
            if let ItemId::Wire(id) = item {
                let other = *doc.wire(id).unwrap();
                if shared_stretch(w, other) {
                    let (lo, hi, _) = normalised(other);
                    let first = Pt {
                        x: w.a.x.clamp(lo.x, hi.x),
                        y: w.a.y.clamp(lo.y, hi.y),
                    };
                    let distance = w.a.manhattan_distance(first);
                    let key = (distance, pt_key(lo), pt_key(hi), id.0);
                    if best.as_ref().is_none_or(|&(old, _)| key < old) {
                        best = Some((key, id));
                    }
                }
            }
        }
        if let Some((_, id)) = best {
            return Err(Blocker::Overlap(id));
        }
    }
    Ok(())
}

impl Doc {
    /// Debug oracle for INV1/INV2/WINV3/WD9. Uses local index queries and point
    /// incidence; does not compare every wire with every other wire.
    #[cfg(any(debug_assertions, test))]
    pub fn assert_normal_form(&self) {
        let mut scratch = Vec::new();
        for (id, &w) in self.wires() {
            assert!(
                w.a != w.b && (w.a.x == w.b.x || w.a.y == w.b.y),
                "INV1: {id:?}"
            );
            assert!(on_lattice(w.a) && on_lattice(w.b), "WINV3 lattice: {id:?}");
            self.query(Rect::from_points(w.a, w.b).unwrap(), &mut scratch);
            for &item in &scratch {
                match item {
                    ItemId::Wire(other_id) if other_id != id => {
                        let other = *self.wire(other_id).unwrap();
                        assert!(
                            !interior_contains(w, other.a) && !interior_contains(w, other.b),
                            "INV2: {id:?}, {other_id:?}"
                        );
                        assert!(
                            !shared_stretch(w, other),
                            "WINV3 overlap: {id:?}, {other_id:?}"
                        );
                    }
                    ItemId::Part(part) => {
                        for &p in self.pins(part).unwrap() {
                            assert!(!interior_contains(w, p), "WINV3 pin: {id:?}");
                        }
                    }
                    ItemId::Label(label) => assert!(
                        !interior_contains(w, self.label(label).unwrap().at),
                        "WD9 labelled point: {id:?}"
                    ),
                    _ => {}
                }
            }
            for p in [w.a, w.b] {
                if let Some((a, b)) = merge_pair(self, p) {
                    panic!("WINV3 mergeable at {p:?}: {a:?}, {b:?}");
                }
            }
        }
    }
}

// Shared local merge predicate: exactly two ends, no pin/label, opposite
// collinear segments. It never repairs pre-existing overlapping geometry.
fn merge_pair(doc: &Doc, at: Pt) -> Option<(WireId, WireId)> {
    if !doc.connectivity.labels_at(at).is_empty() {
        return None;
    }
    let [End::Wire(a), End::Wire(b)] = doc.points.at(at) else {
        return None;
    };
    let wa = *doc.wire(*a).unwrap();
    let wb = *doc.wire(*b).unwrap();
    if (wa.a.y == wa.b.y) != (wb.a.y == wb.b.y) || shared_stretch(wa, wb) {
        return None;
    }
    Some((*a, *b))
}

/// W1.4: merge only at the fixed ends vacated by Delete. Union the eligible
/// pairs before editing so multiple removed branches cannot make order matter.
pub(crate) fn plan_delete_merges(doc: &Doc, points: &mut Vec<Pt>) -> AddWiresPlan {
    points.sort_by_key(|&p| pt_key(p));
    points.dedup();
    let mut groups: FxHashMap<WireId, WireId> = FxHashMap::default();
    let mut originals = BTreeMap::new();
    fn root(groups: &mut FxHashMap<WireId, WireId>, id: WireId) -> WireId {
        let mut r = id;
        while groups[&r] != r {
            r = groups[&r];
        }
        let mut current = id;
        while current != r {
            let next = groups[&current];
            groups.insert(current, r);
            current = next;
        }
        r
    }
    for &p in points.iter() {
        if let Some((a, b)) = merge_pair(doc, p) {
            for id in [a, b] {
                groups.entry(id).or_insert(id);
                originals.insert(id.0, *doc.wire(id).unwrap());
            }
            let ra = root(&mut groups, a);
            let rb = root(&mut groups, b);
            groups.insert(WireId(ra.0.max(rb.0)), WireId(ra.0.min(rb.0)));
        }
    }
    let mut merged: BTreeMap<u32, Wire> = BTreeMap::new();
    for (&id, &w) in &originals {
        let r = root(&mut groups, WireId(id));
        let (lo, hi, _) = normalised(w);
        merged
            .entry(r.0)
            .and_modify(|m| {
                m.a = if pt_key(lo) < pt_key(m.a) { lo } else { m.a };
                m.b = if pt_key(hi) > pt_key(m.b) { hi } else { m.b };
            })
            .or_insert(Wire { a: lo, b: hi });
    }
    AddWiresPlan {
        remove: originals
            .into_iter()
            .map(|(id, w)| (WireId(id), w))
            .collect(),
        insert: merged
            .into_iter()
            .map(|(id, w)| (Some(WireId(id)), w))
            .collect(),
    }
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

fn contains(wire: Wire, p: Pt) -> bool {
    Rect::from_points(wire.a, wire.b).unwrap().contains(p)
}

fn touches(a: Wire, b: Wire) -> bool {
    contains(a, b.a) || contains(a, b.b) || contains(b, a.a) || contains(b, a.b)
}

/// Plans only the added wires' local neighbourhood (D8, INV2).
/// Direct contacts have an endpoint on the other segment (inclusive); a pure
/// interior crossing is not a contact. Touched points are the ends of these
/// direct contacts and the added wires, plus pins/labels on added wires.
/// Collinear neighbours incident at those fixed points may join the plan, but
/// their far ends do not expand the touched points. Normalisation never walks
/// an unrelated chain. Ends outside the plan still count as merge blockers.
/// Pre-existing non-normal geometry elsewhere is left unchanged (W2.2 owns
/// normalising moved geometry; this is not a document-wide repair pass).
pub(crate) fn plan_add_wires(doc: &Doc, added: &[Wire]) -> AddWiresPlan {
    let mut selected = FxHashMap::default();
    let mut pins = Vec::new();
    let mut labels = Vec::new();
    let mut scratch = Vec::new();
    let mut touched = FxHashSet::default();
    for &wire in added {
        touched.extend([wire.a, wire.b]);
        doc.query(Rect::from_points(wire.a, wire.b).unwrap(), &mut scratch);
        for &item in &scratch {
            match item {
                ItemId::Wire(id) => {
                    let other = *doc.wire(id).unwrap();
                    if touches(wire, other) {
                        selected.insert(id, other);
                        touched.extend([other.a, other.b]);
                    }
                }
                ItemId::Part(id) => {
                    pins.extend(
                        doc.pins(id)
                            .unwrap()
                            .iter()
                            .copied()
                            .filter(|&p| contains(wire, p)),
                    );
                }
                ItemId::Label(id) => labels.push(doc.label(id).unwrap().at),
            }
        }
    }
    touched.extend(pins.iter().copied());
    touched.extend(labels.iter().copied());
    // One boundary expansion at fixed touched points, using the point store.
    // Only collinear neighbours can merge. Perpendicular untouched ends remain
    // blockers, without being copied or replanned.
    let mut endpoint_axes = FxHashSet::default();
    for &wire in selected.values().chain(added) {
        let horizontal = wire.a.y == wire.b.y;
        endpoint_axes.extend([(wire.a, horizontal), (wire.b, horizontal)]);
    }
    let mut neighbours = Vec::new();
    for &at in &touched {
        for end in doc.points.at(at) {
            if let End::Wire(id) = *end {
                let wire = *doc.wire(id).unwrap();
                if endpoint_axes.contains(&(at, wire.a.y == wire.b.y)) {
                    neighbours.push((id, wire));
                }
            }
        }
    }
    selected.extend(neighbours);
    let mut existing_sorted: Vec<_> = selected.iter().map(|(&id, &wire)| (id, wire)).collect();
    existing_sorted.sort_by_key(|(id, _)| id.0);
    // Pins/labels at retained outer ends also prevent merging, even when those
    // points lie outside the new segments. Query points, never the whole sheet.
    for &at in &touched {
        if doc
            .points
            .at(at)
            .iter()
            .any(|end| matches!(end, End::Pin { .. }))
        {
            pins.push(at);
        }
        doc.query(Rect { min: at, max: at }, &mut scratch);
        if scratch.iter().any(|item| matches!(item, ItemId::Label(_))) {
            labels.push(at);
        }
    }
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
    feature.extend_from_slice(&pins);
    feature.extend_from_slice(&labels);
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
        let (coords, low, high) = if source.horizontal {
            (rows.get(&source.lo.y), source.lo.x, source.hi.x)
        } else {
            (cols.get(&source.lo.x), source.lo.y, source.hi.y)
        };
        let coords = coords.map_or(&[][..], Vec::as_slice);
        let start = coords.partition_point(|&c| c <= low);
        let end = coords.partition_point(|&c| c < high);
        let mut lo = source.lo;
        for coordinate in coords[start..end]
            .iter()
            .copied()
            .chain(std::iter::once(high))
        {
            let hi = if source.horizontal {
                Pt {
                    x: coordinate,
                    y: source.lo.y,
                }
            } else {
                Pt {
                    x: source.lo.x,
                    y: coordinate,
                }
            };
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
            if let Some(id) = source.existing {
                atomic.existing.push(id);
            }
            if atomic.descending != Some(source.descending) {
                atomic.descending = None;
            }
            lo = hi;
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
        touched.contains(&at)
            && incidence.get(&pt_key(at)).copied().unwrap_or(0)
                + doc
                    .points
                    .at(at)
                    .iter()
                    .filter(|end| matches!(end, End::Wire(id) if !selected.contains_key(id)))
                    .count() as u32
                == 2
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
                seg_existing.extend_from_slice(&atomics[idx].existing);
                if atomics[idx].descending != descending {
                    descending = None;
                }
            }
            seg_existing.sort_by_key(|id| id.0);
            seg_existing.dedup();
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
    let mut used = FxHashSet::default();
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
                used.insert(id);
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
        assert_eq!(l_route(a, a, false), [None, None]);
        assert_eq!(
            l_route(a, Pt { x: 10, y: 0 }, true),
            [Some(w(0, 0, 10, 0)), None]
        );
        assert_eq!(
            l_route(a, b, false),
            [Some(w(0, 0, 10, 0)), Some(w(10, 0, 10, 20))],
            "flip=false leaves a horizontally"
        );
        assert_eq!(
            l_route(a, b, true),
            [Some(w(0, 0, 0, 20)), Some(w(0, 20, 10, 20))],
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

    fn snapshot(doc: &Doc) -> Doc {
        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        Doc::load(bytes.as_slice()).unwrap()
    }

    fn net_members(doc: &Doc, id: WireId) -> Vec<WireId> {
        let mut wires = Vec::new();
        doc.net_wires(doc.net_of(id).unwrap(), &mut wires);
        wires.sort_by_key(|id| id.0);
        wires
    }

    // INV4 / INV10: unrelated AddWires must not repair legacy non-normal
    // geometry, change its exact ids/directions, or change its net partition.
    fn assert_unrelated_add_preserves(doc: &mut Doc) {
        let before = snapshot(doc);
        let wires = live_wires(doc);
        let memberships: Vec<_> = wires.iter().map(|&(id, _)| net_members(doc, id)).collect();
        let change = doc
            .apply(Command::AddWires(vec![w(1000, 1000, 1010, 1000)]))
            .unwrap();
        assert!(change.removed.is_empty());
        assert_eq!(change.added.len(), 1);
        for (&(id, wire), members) in wires.iter().zip(memberships) {
            assert_eq!(doc.wire(id), Some(&wire));
            assert_eq!(net_members(doc, id), members);
        }
        assert_inv10(doc);
        let after = snapshot(doc);
        doc.undo().unwrap();
        assert!(doc.content_eq(&before), "INV4: exact undo including ids");
        doc.redo().unwrap();
        assert!(doc.content_eq(&after), "INV4: exact redo including ids");
        doc.undo().unwrap();
    }

    #[test]
    fn unrelated_add_preserves_move_delete_and_duplicate_non_normal_states() {
        let mut moved = Doc::new();
        moved
            .apply(Command::AddWires(vec![w(0, 0, 10, 0), w(5, 5, 5, 10)]))
            .unwrap();
        let id = moved.wires().find(|(_, wire)| wire.a.x == 5).unwrap().0;
        moved
            .apply(Command::Move {
                items: vec![ItemId::Wire(id)],
                delta: Pt { x: 0, y: -5 },
            })
            .unwrap();
        assert_unrelated_add_preserves(&mut moved);

        let mut deleted = Doc::new();
        deleted
            .apply(Command::AddWires(vec![w(0, 0, 10, 0), w(5, 0, 5, 10)]))
            .unwrap();
        let branch = deleted
            .wires()
            .find(|(_, wire)| wire.a.x == wire.b.x)
            .unwrap()
            .0;
        deleted
            .apply(Command::Delete(vec![ItemId::Wire(branch)]))
            .unwrap();
        assert_eq!(deleted.wires().count(), 1);
        assert_unrelated_add_preserves(&mut deleted);

        let mut duplicate = Doc::from_items([], [w(0, 0, 10, 0), w(10, 0, 0, 0)], []).unwrap();
        assert_unrelated_add_preserves(&mut duplicate);
    }

    #[test]
    fn merge_expansion_stops_at_fixed_touched_points_and_keeps_external_junctions() {
        // Only the first wire directly touches the new segment. Its far end is
        // a touched point, so the second can merge; the third's far boundary
        // was never touched and the pre-existing extra chain is left alone.
        let mut doc =
            Doc::from_items([], [w(0, 0, 10, 0), w(10, 0, 20, 0), w(20, 0, 30, 0)], []).unwrap();
        doc.apply(Command::AddWires(vec![w(-10, 0, 0, 0)])).unwrap();
        assert_eq!(
            live_wires(&doc),
            vec![(WireId(0), w(-10, 0, 20, 0)), (WireId(2), w(20, 0, 30, 0))]
        );
        assert_inv10(&doc);
        let mut doc =
            Doc::from_items([], [w(0, 0, 10, 0), w(10, 0, 20, 0), w(10, 0, 10, 10)], []).unwrap();
        doc.apply(Command::AddWires(vec![w(-10, 0, 0, 0)])).unwrap();
        assert_eq!(doc.wire(WireId(0)), Some(&w(-10, 0, 10, 0)));
        assert_eq!(doc.wire(WireId(1)), Some(&w(10, 0, 20, 0)));
        assert_eq!(doc.wire(WireId(2)), Some(&w(10, 0, 10, 10)));
        assert_all(&doc);
    }

    fn with_features(pins: Vec<Pt>, labels: Vec<Pt>) -> Doc {
        Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "1k".into(),
                },
                pins,
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )],
            [],
            labels.into_iter().map(|at| Label {
                at,
                text: "SIG".into(),
            }),
        )
        .unwrap()
    }

    fn point_strategy() -> impl Strategy<Value = Pt> {
        (-4i32..5, -4i32..5).prop_map(|(x, y)| Pt { x, y })
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

    fn lattice(p: Pt) -> Pt {
        Pt {
            x: p.x * crate::PIN_LATTICE_NM,
            y: p.y * crate::PIN_LATTICE_NM,
        }
    }

    fn lw(x1: i32, y1: i32, x2: i32, y2: i32) -> Wire {
        let w = w(x1, y1, x2, y2);
        Wire {
            a: lattice(w.a),
            b: lattice(w.b),
        }
    }

    #[test]
    fn wd4_blockers_rule_priority_direction_and_index_order() {
        let segment = lw(0, 0, 10, 0);
        let check = |doc: &Doc| check_new_wires(doc, &[segment], [segment.a, segment.b]);
        let pins = with_features(vec![lattice(Pt { x: 3, y: 0 })], vec![]);
        assert_eq!(
            check(&pins),
            Err(Blocker::Pin {
                part: crate::PartId(0),
                pin: 0
            })
        );
        let ends = Doc::from_items([], [lw(4, 0, 4, 2)], []).unwrap();
        assert_eq!(check(&ends), Err(Blocker::WireEnd(WireId(0))));
        let overlap = Doc::from_items([], [lw(-1, 0, 11, 0)], []).unwrap();
        assert_eq!(check(&overlap), Err(Blocker::Overlap(WireId(0))));
        let outside = Wire {
            a: Pt::ZERO,
            b: Pt {
                x: crate::SHEET_LIMIT_NM + 1,
                y: 0,
            },
        };
        assert_eq!(
            check_new_wires(&pins, &[segment, outside], [segment.a, outside.b]),
            Err(Blocker::Sheet)
        );
        assert_eq!(
            check_new_wires(&Doc::new(), &[w(0, 0, 1, 0)], [Pt::ZERO; 2]),
            Err(Blocker::Sheet)
        );
        // Second rule outranks third even on a later segment.
        let doc = Doc::from_items([], [lw(-1, 0, 11, 0), lw(10, 4, 12, 4)], []).unwrap();
        assert_eq!(
            check_new_wires(
                &doc,
                &[segment, lw(10, 0, 10, 8)],
                [segment.a, lattice(Pt { x: 10, y: 8 })]
            ),
            Err(Blocker::WireEnd(WireId(1)))
        );
        let mut doc = Doc::from_items([], [lw(8, 0, 8, 2), lw(2, 0, 2, 2)], []).unwrap();
        assert_eq!(check(&doc), Err(Blocker::WireEnd(WireId(1))));
        let reversed = Wire {
            a: segment.b,
            b: segment.a,
        };
        assert_eq!(
            check_new_wires(&doc, &[reversed], [segment.a, segment.b]),
            Err(Blocker::WireEnd(WireId(0)))
        );
        // Undo restore changes index insertion order but not the selected blocker.
        doc.apply(Command::Delete(vec![ItemId::Wire(WireId(0))]))
            .unwrap();
        doc.undo().unwrap();
        assert_eq!(check(&doc), Err(Blocker::WireEnd(WireId(1))));
        let own = lattice(Pt { x: 2, y: 0 });
        assert_eq!(
            check_new_wires(&doc, &[segment], [own, lattice(Pt { x: 8, y: 0 })]),
            Ok(())
        );
        let own_pin = lattice(Pt { x: 3, y: 0 });
        assert_eq!(
            check_new_wires(&pins, &[segment], [own_pin, segment.b]),
            Ok(())
        );
    }

    #[test]
    fn inv2_validated_crossing_stays_in_different_nets() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![lw(0, 0, 10, 0)])).unwrap();
        let candidate = lw(5, -5, 5, 5);
        assert_eq!(
            check_new_wires(&doc, &[candidate], [candidate.a, candidate.b]),
            Ok(())
        );
        doc.apply(Command::AddWires(vec![candidate])).unwrap();
        doc.assert_normal_form();
        assert_eq!(doc.wires().count(), 2);
        assert_ne!(doc.net_of(WireId(0)), doc.net_of(WireId(1)));
    }

    #[test]
    fn winv3_delete_merges_only_vacated_points_and_restores_exact_ids() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![
            lw(0, 0, 10, 0),
            lw(3, 0, 3, 3),
            lw(7, 0, 7, 3),
        ]))
        .unwrap();
        let before = snapshot(&doc);
        let branches: Vec<_> = doc
            .wires()
            .filter(|(_, w)| w.a.x == w.b.x)
            .map(|(id, _)| ItemId::Wire(id))
            .collect();
        let mut reversed = snapshot(&doc);
        reversed
            .apply(Command::Delete(branches.iter().copied().rev().collect()))
            .unwrap();
        doc.apply(Command::Delete(branches)).unwrap();
        doc.assert_normal_form();
        assert_inv10(&doc);
        assert_eq!(live_wires(&doc), vec![(WireId(0), lw(0, 0, 10, 0))]);
        assert!(doc.content_eq(&reversed));
        let after = snapshot(&doc);
        doc.undo().unwrap();
        assert!(doc.content_eq(&before));
        assert_inv10(&doc);
        doc.redo().unwrap();
        assert!(doc.content_eq(&after));
        assert_inv10(&doc);
        // WD9 was already implemented by T6.1: exercise a touched label,
        // not an unrelated AddWires which never visits its point.
        let mut labelled = with_features(vec![], vec![lattice(Pt { x: 5, y: 0 })]);
        labelled
            .apply(Command::AddWires(vec![lw(0, 0, 10, 0), lw(5, 0, 5, 5)]))
            .unwrap();
        let branch = labelled.wires().find(|(_, w)| w.a.x == w.b.x).unwrap().0;
        labelled
            .apply(Command::Delete(vec![ItemId::Wire(branch)]))
            .unwrap();
        labelled.assert_normal_form();
        assert_eq!(labelled.wires().count(), 2);
        assert!(labelled.net_at(lattice(Pt { x: 5, y: 0 })).is_some());
        // A distant legacy pair is outside the deletion neighbourhood.
        let mut local =
            Doc::from_items([], [lw(0, 0, 5, 0), lw(5, 0, 10, 0), lw(20, 0, 21, 0)], []).unwrap();
        local
            .apply(Command::Delete(vec![ItemId::Wire(WireId(2))]))
            .unwrap();
        assert_eq!(local.wire(WireId(0)), Some(&lw(0, 0, 5, 0)));
        assert_eq!(local.wire(WireId(1)), Some(&lw(5, 0, 10, 0)));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn winv3_inv4_inv10_add_delete_sequences(
            steps in prop::collection::vec((any::<bool>(), prop::collection::vec(wire_strategy(), 1..4), any::<usize>()), 1..40),
            pins in prop::collection::vec(point_strategy(), 0..8),
            labels in prop::collection::vec(point_strategy(), 0..8),
        ) {
            let mut doc = with_features(pins.into_iter().map(lattice).collect(), labels.into_iter().map(lattice).collect());
            for (delete, wires, pick) in steps {
                let before = snapshot(&doc);
                let live = live_wires(&doc);
                let cmd = if delete && !live.is_empty() {
                    Command::Delete(vec![ItemId::Wire(live[pick % live.len()].0)])
                } else {
                    Command::AddWires(wires.into_iter().map(|w| Wire { a: lattice(w.a), b: lattice(w.b) }).collect())
                };
                doc.apply(cmd).unwrap();
                doc.assert_normal_form();
                assert_inv10(&doc);
                let after = snapshot(&doc);
                if !before.content_eq(&after) {
                    doc.undo().unwrap();
                    prop_assert!(doc.content_eq(&before));
                    doc.assert_normal_form();
                    assert_inv10(&doc);
                    doc.redo().unwrap();
                    prop_assert!(doc.content_eq(&after));
                    doc.assert_normal_form();
                    assert_inv10(&doc);
                }
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        /// INV2 + INV10 hold after every AddWires step, and undoing every step
        /// restores the empty document (INV4, ids included via content_eq).
        #[test]
        fn inv2_inv10_and_undo_over_random_add_wires(
            batches in prop::collection::vec(prop::collection::vec(wire_strategy(), 1..4), 1..24),
            pins in prop::collection::vec(point_strategy(), 0..8),
            labels in prop::collection::vec(point_strategy(), 0..8),
        ) {
            let mut doc = with_features(pins, labels);
            let empty = snapshot(&doc);
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
                let after = snapshot(&doc);
                if change.dirty.is_some() {
                    prop_assert!(doc.undo().is_some());
                    prop_assert!(doc.content_eq(&before), "INV4: undo restores pre-AddWires ids");
                    prop_assert!(doc.redo().is_some());
                    prop_assert!(doc.content_eq(&after), "INV4: redo equals apply, ids included");
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

        /// INV4 / INV10 through Move/Delete states; local AddWires must not
        /// silently repair any of those states on the rest of the sheet.
        #[test]
        fn local_add_with_features_and_mixed_edits(
            steps in prop::collection::vec((0u8..3, prop::collection::vec(wire_strategy(), 1..4), any::<usize>(), point_strategy()), 1..24),
            pins in prop::collection::vec(point_strategy(), 0..8),
            labels in prop::collection::vec(point_strategy(), 0..8),
        ) {
            let mut doc = with_features(pins, labels);
            let initial = snapshot(&doc);
            for (kind, wires, pick, delta) in steps {
                let before = snapshot(&doc);
                let live = live_wires(&doc);
                let cmd = match (kind, live.is_empty()) {
                    (1, false) => Command::Move { items: vec![ItemId::Wire(live[pick % live.len()].0)], delta },
                    (2, false) => Command::Delete(vec![ItemId::Wire(live[pick % live.len()].0)]),
                    _ => Command::AddWires(wires.clone()),
                };
                // Reversed input order must produce the same ids and geometry,
                // even from a non-normal starting state (D2).
                if matches!(&cmd, Command::AddWires(_)) {
                    let mut reversed = snapshot(&doc);
                    reversed.apply(Command::AddWires(wires.into_iter().rev().collect())).unwrap();
                    doc.apply(cmd).unwrap();
                    prop_assert!(doc.content_eq(&reversed));
                } else {
                    doc.apply(cmd).unwrap();
                }
                assert_inv10(&doc);
                let after = snapshot(&doc);
                if !doc.content_eq(&before) {
                    doc.undo().unwrap();
                    prop_assert!(doc.content_eq(&before));
                    doc.redo().unwrap();
                    prop_assert!(doc.content_eq(&after));
                }
                assert_unrelated_add_preserves(&mut doc);
            }
            while doc.undo().is_some() {}
            prop_assert!(doc.content_eq(&initial));
        }
    }
}
