//! Runs (WD1) and the boundary of a selection (WD2).
//!
//! The document stores single segments (D2). A run is derived, never stored: a chain of
//! segments joined at corners, where a corner is a point holding exactly two wire ends and no
//! pin. A run ends at an [`Anchor`]. Everything here reads the point map (wire ends and pins)
//! and the labels the connectivity point store keeps per point; nothing allocates beyond the
//! output buffers.
//!
//! Results never depend on the order in which ends were added at a point (an undo or a load
//! can change it): lists of runs are sorted by geometry, then by id (see [`Doc::runs_at`]).

use smallvec::SmallVec;

use crate::{Doc, ItemId, LabelId, PartId, Pt, WireId, points::End};

/// Where a run ends (WD1).
///
/// The kinds overlap in WD1's wording (a pin can also sit on a junction), so one point gets
/// one kind in this order: `Boundary`, then `Pin`, then `Junction`, then `FreeEnd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchor {
    /// A part's pin. When several pins share the point, the lowest `(part, pin)`.
    Pin { part: PartId, pin: usize },
    /// Three or more wire ends and no pin.
    Junction,
    /// A wire end with nothing else at its point.
    FreeEnd,
    /// A boundary point of the selection being planned (WD2). Only [`Doc::affected_runs`]
    /// reports it.
    Boundary,
    /// The run is a closed loop of corners with no anchor anywhere on it. Both ends are the
    /// loop's least point by `(x, y)`; only [`Doc::run_through`] can report it.
    Closed,
}

/// A chain of wire segments joined at corners (WD1).
///
/// `wires` go from `a` to `b`: the first has an end at `a.0`, consecutive wires share a
/// corner, and the last has an end at `b.0`. `labels` are the labels on the run's corners in
/// ascending id order; labels on its two anchors are not included (they stay put, WD1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub wires: SmallVec<[WireId; 4]>,
    pub a: (Pt, Anchor),
    pub b: (Pt, Anchor),
    pub labels: SmallVec<[LabelId; 1]>,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            wires: SmallVec::new(),
            a: (Pt::ZERO, Anchor::FreeEnd),
            b: (Pt::ZERO, Anchor::FreeEnd),
            labels: SmallVec::new(),
        }
    }
}

impl Run {
    fn clear(&mut self) {
        self.wires.clear();
        self.labels.clear();
        self.a = (Pt::ZERO, Anchor::FreeEnd);
        self.b = (Pt::ZERO, Anchor::FreeEnd);
    }

    fn reverse(&mut self) {
        self.wires.reverse();
        std::mem::swap(&mut self.a, &mut self.b);
    }
}

/// The boundary of a selection (WD2): every point where a pin of a selected part, or an end of
/// a selected wire, meets an end of an unselected wire. Built by [`Doc::boundary_points`].
///
/// The points are sorted by `(x, y)` without repeats, so nothing here depends on the order of
/// the selection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Boundary {
    points: Vec<Pt>,
    selected_wires: Vec<WireId>,
}

impl Boundary {
    /// Boundary points, sorted by `(x, y)`, each once.
    pub fn points(&self) -> &[Pt] {
        &self.points
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn contains(&self, at: Pt) -> bool {
        self.points
            .binary_search_by_key(&key(at), |&p| key(p))
            .is_ok()
    }

    /// Whether `wire` is in the selection the boundary was built from.
    pub fn is_selected(&self, wire: WireId) -> bool {
        self.selected_wires
            .binary_search_by_key(&wire.0, |w| w.0)
            .is_ok()
    }
}

fn key(p: Pt) -> (i32, i32) {
    (p.x, p.y)
}

impl Doc {
    /// Counts the part's pins whose point holds anything besides that pin: a wire end, another
    /// pin (of any part, this one included) or a label. Each pin counts once, however much is
    /// on it (WD3, M2). Saturates at `u16::MAX`; a missing part counts 0.
    pub fn connected_pins(&self, part: PartId) -> u16 {
        let Some(stored) = self.parts.get(part.0) else {
            return 0;
        };
        let mut count = 0u16;
        for &at in &stored.pins {
            if self.points.at(at).len() > 1 || !self.connectivity.labels_at(at).is_empty() {
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Replaces `out` with the run `wire` is in, with no selection in play. The run is oriented
    /// so that `a` is the lesser end by `(x, y)` (either way round when both ends share a
    /// point). A closed loop of corners comes back with both ends [`Anchor::Closed`]. A missing
    /// wire leaves `out.wires` empty.
    pub fn run_through(&self, wire: WireId, out: &mut Run) {
        out.clear();
        let Some(&w) = self.wires.get(wire.0) else {
            return;
        };
        // First find the end reached through `w.a`, then walk the whole run from there.
        let Some(end) = self.walk(w.b, wire, None, out) else {
            self.close_loop(w.b, out);
            return;
        };
        let last = *out.wires.last().expect("a walk takes at least one wire");
        out.clear();
        let far = self
            .walk(end.0, last, None, out)
            .expect("a walk from an anchor ends at an anchor");
        out.a = end;
        out.b = far;
        if key(out.b.0) < key(out.a.0) {
            out.reverse();
        }
        out.labels.sort_unstable_by_key(|l| l.0);
    }

    /// Replaces `out` with the runs that start at `at`, with no selection in play, each oriented
    /// with `a` at `at`. A run whose two ends are both `at` appears once. Empty when `at` is a
    /// corner (it lies inside a run, not at its end) or holds no wire end.
    ///
    /// The runs are sorted by `a`, then `b`, then the point their first wire leads to (all by
    /// `(x, y)`), then the first wire's id. The order depends only on the geometry and ids,
    /// never on the order the wire ends were added at a point (which an undo or a load changes).
    pub fn runs_at(&self, at: Pt, out: &mut Vec<Run>) {
        out.clear();
        let Some(anchor) = self.anchor_at(at, None) else {
            return;
        };
        for end in self.points.at(at) {
            let End::Wire(first) = end else { continue };
            let mut run = Run::default();
            let far = self
                .walk(at, first, None, &mut run)
                .expect("a walk from an anchor ends at an anchor");
            let last = *run.wires.last().expect("a walk takes at least one wire");
            // A loop back to `at` is met from both of its wires; keep the direction that leaves
            // along the lesser first step.
            if far.0 == at && self.leg_key(at, last) < self.leg_key(at, first) {
                continue;
            }
            run.a = (at, anchor);
            run.b = far;
            run.labels.sort_unstable_by_key(|l| l.0);
            out.push(run);
        }
        self.sort_runs(out);
    }

    /// Replaces `out` with the boundary of `items` (WD2). Labels have no ends and add nothing;
    /// missing ids and repeats are ignored, and the order of `items` does not matter.
    pub fn boundary_points(&self, items: &[ItemId], out: &mut Boundary) {
        let Boundary {
            points,
            selected_wires,
        } = out;
        points.clear();
        selected_wires.clear();
        selected_wires.extend(items.iter().filter_map(|&item| match item {
            ItemId::Wire(id) if self.wires.get(id.0).is_some() => Some(id),
            _ => None,
        }));
        selected_wires.sort_unstable_by_key(|w| w.0);
        selected_wires.dedup();
        let selected = |w: WireId| selected_wires.binary_search_by_key(&w.0, |s| s.0).is_ok();
        let meets_unselected = |at: Pt| {
            self.points
                .at(at)
                .iter()
                .any(|end| matches!(end, End::Wire(w) if !selected(w)))
        };
        for &item in items {
            match item {
                ItemId::Part(id) => {
                    if let Some(stored) = self.parts.get(id.0) {
                        points.extend(stored.pins.iter().copied().filter(|&p| meets_unselected(p)));
                    }
                }
                ItemId::Wire(id) => {
                    if let Some(w) = self.wires.get(id.0) {
                        points.extend([w.a, w.b].into_iter().filter(|&p| meets_unselected(p)));
                    }
                }
                ItemId::Label(_) => {}
            }
        }
        points.sort_unstable_by_key(|&p| key(p));
        points.dedup();
    }

    /// Replaces `out` with the affected runs of `boundary` (WD2): the runs of unselected wires
    /// that start at a boundary point, each found once even when both of its ends are boundary
    /// points (WINV8). A run stops at any boundary point as well as at WD1's anchors, and starts
    /// with `a` at a boundary point. The result depends only on the document and the set of
    /// selected items, never on their order. The runs are sorted as in [`Doc::runs_at`]: by
    /// boundary point, then far end, then first step, then the first wire's id (WD3's order).
    pub fn affected_runs(&self, boundary: &Boundary, out: &mut Vec<Run>) {
        out.clear();
        for &start in &boundary.points {
            for end in self.points.at(start) {
                let End::Wire(first) = end else { continue };
                if boundary.is_selected(first) {
                    continue;
                }
                let mut run = Run::default();
                let far = self
                    .walk(start, first, Some(boundary), &mut run)
                    .expect("a walk from a boundary point ends at an anchor");
                let last = *run.wires.last().expect("a walk takes at least one wire");
                // A run between two boundary points is met from both ends; keep the walk that
                // starts from the lesser point, then along the lesser first step.
                if boundary.contains(far.0)
                    && (key(far.0), self.leg_key(far.0, last))
                        < (key(start), self.leg_key(start, first))
                {
                    continue;
                }
                run.a = (start, Anchor::Boundary);
                run.b = far;
                run.labels.sort_unstable_by_key(|l| l.0);
                out.push(run);
            }
        }
        self.sort_runs(out);
    }

    /// Where `wire` leads from its end at `from` by `(x, y)`, then its id: the key that orders
    /// runs leaving the same point.
    fn leg_key(&self, from: Pt, wire: WireId) -> ((i32, i32), u32) {
        let w = self.wires.get(wire.0).expect("walked wires are live");
        (key(if w.a == from { w.b } else { w.a }), wire.0)
    }

    /// Sorts runs by `a`, `b`, then the first wire's leg from `a`. Distinct runs in one result
    /// have distinct first wires, so the key is unique and the unstable sort is deterministic.
    fn sort_runs(&self, runs: &mut [Run]) {
        runs.sort_unstable_by_key(|run| {
            (
                key(run.a.0),
                key(run.b.0),
                self.leg_key(run.a.0, run.wires[0]),
            )
        });
    }

    /// The anchor at `at`, or `None` when it is a corner.
    fn anchor_at(&self, at: Pt, boundary: Option<&Boundary>) -> Option<Anchor> {
        if boundary.is_some_and(|b| b.contains(at)) {
            return Some(Anchor::Boundary);
        }
        let ends = self.points.at(at);
        let pin = ends
            .iter()
            .filter_map(|end| match end {
                End::Pin { part, pin } => Some((part.0, pin)),
                End::Wire(_) => None,
            })
            .min();
        if let Some((part, pin)) = pin {
            return Some(Anchor::Pin {
                part: PartId(part),
                pin,
            });
        }
        match ends.len() {
            2 => None,
            n if n >= 3 => Some(Anchor::Junction),
            _ => Some(Anchor::FreeEnd),
        }
    }

    /// Walks from `start` along `first` and on through corners, pushing wires and corner labels
    /// onto `out`. Returns the anchor it stops at, or `None` when it comes back round to
    /// `first` (a closed loop of corners, possible only when `start` is itself a corner).
    fn walk(
        &self,
        start: Pt,
        first: WireId,
        boundary: Option<&Boundary>,
        out: &mut Run,
    ) -> Option<(Pt, Anchor)> {
        let (mut at, mut current) = (start, first);
        loop {
            out.wires.push(current);
            let wire = self
                .wires
                .get(current.0)
                .expect("the point map lists live wires only");
            let next = if wire.a == at { wire.b } else { wire.a };
            if let Some(anchor) = self.anchor_at(next, boundary) {
                return Some((next, anchor));
            }
            let Some((x, y)) = self.points.at(next).two_wires() else {
                unreachable!("a corner holds exactly two wire ends");
            };
            let other = if x == current { y } else { x };
            // A selected wire beyond a corner makes that corner a boundary point; this guards a
            // boundary built from a different selection.
            if boundary.is_some_and(|b| b.is_selected(other)) {
                return Some((next, Anchor::Boundary));
            }
            out.labels
                .extend_from_slice(self.connectivity.labels_at(next));
            if other == first {
                return None;
            }
            at = next;
            current = other;
        }
    }

    /// Finishes a closed loop walked from `start`: rotates it to begin at its least point.
    fn close_loop(&self, start: Pt, out: &mut Run) {
        let mut at = start;
        let mut best = (start, 0);
        for (i, wire) in out.wires.iter().enumerate() {
            if key(at) < key(best.0) {
                best = (at, i);
            }
            let w = self.wires.get(wire.0).expect("walked wires are live");
            at = if w.a == at { w.b } else { w.a };
        }
        out.wires.rotate_left(best.1);
        out.a = (best.0, Anchor::Closed);
        out.b = (best.0, Anchor::Closed);
        out.labels.sort_unstable_by_key(|l| l.0);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use proptest::prelude::*;

    use super::*;
    use crate::{Command, Label, Orient, Part, Rect, SymbolId, Wire};

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x, y }
    }

    fn add_part(doc: &mut Doc, at: Pt, local_pins: &[Pt]) -> PartId {
        doc.insert_part(
            Part {
                symbol: SymbolId(0),
                at,
                orient: Orient::IDENTITY,
                designator: "U".into(),
                value: "v".into(),
            },
            local_pins,
            Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
        )
        .unwrap()
    }

    fn add_wire(doc: &mut Doc, a: Pt, b: Pt) -> WireId {
        doc.insert_wire(Wire { a, b }).unwrap()
    }

    fn add_label(doc: &mut Doc, at: Pt) -> LabelId {
        doc.insert_label(Label {
            at,
            text: "N".into(),
        })
        .unwrap()
    }

    // ---- brute-force references: scans of the arenas, independent of the point map ----

    fn wire_ends_at(doc: &Doc, at: Pt) -> Vec<WireId> {
        doc.wires()
            .flat_map(|(id, w)| [(id, w.a), (id, w.b)])
            .filter_map(|(id, p)| (p == at).then_some(id))
            .collect()
    }

    fn pins_at(doc: &Doc, at: Pt) -> Vec<(PartId, usize)> {
        doc.parts()
            .flat_map(|(id, _)| {
                doc.pins(id)
                    .unwrap()
                    .iter()
                    .enumerate()
                    .filter_map(move |(i, &p)| (p == at).then_some((id, i)))
            })
            .collect()
    }

    fn labels_at(doc: &Doc, at: Pt) -> Vec<LabelId> {
        doc.labels()
            .filter_map(|(id, l)| (l.at == at).then_some(id))
            .collect()
    }

    fn is_corner(doc: &Doc, at: Pt) -> bool {
        wire_ends_at(doc, at).len() == 2 && pins_at(doc, at).is_empty()
    }

    /// WD1's anchor at `at`, or `None` for a corner.
    fn brute_anchor(doc: &Doc, at: Pt) -> Option<Anchor> {
        let pins = pins_at(doc, at);
        if let Some(&(part, pin)) = pins.iter().min_by_key(|(p, i)| (p.0, *i)) {
            return Some(Anchor::Pin { part, pin });
        }
        match wire_ends_at(doc, at).len() {
            2 => None,
            n if n >= 3 => Some(Anchor::Junction),
            _ => Some(Anchor::FreeEnd),
        }
    }

    /// The points of a run from `a` to `b`, checking that consecutive wires share them.
    fn run_points(doc: &Doc, run: &Run) -> Vec<Pt> {
        let mut at = run.a.0;
        let mut pts = vec![at];
        for w in &run.wires {
            let wire = doc.wire(*w).unwrap();
            assert!(
                wire.a == at || wire.b == at,
                "wire {w:?} does not touch {at:?}"
            );
            at = if wire.a == at { wire.b } else { wire.a };
            pts.push(at);
        }
        assert_eq!(at, run.b.0, "the last wire must end at b");
        pts
    }

    /// The run's wires as a set (sorted, each once).
    fn wire_set(run: &Run) -> Vec<u32> {
        let mut v: Vec<u32> = run.wires.iter().map(|w| w.0).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    /// Asserts that no wire occurs twice in `run.wires`, closed runs included.
    fn assert_wires_once(run: &Run) {
        let set = wire_set(run);
        assert_eq!(set.len(), run.wires.len(), "a wire repeats in {run:?}");
    }

    fn sorted_labels(mut v: Vec<LabelId>) -> Vec<LabelId> {
        v.sort_unstable_by_key(|l| l.0);
        v
    }

    // ---- generated sheets ----

    #[derive(Debug, Clone)]
    enum Op {
        Wire { at: Pt, len: i32, horizontal: bool },
        Part { at: Pt, pins: u8 },
        Label { at: Pt },
        RemoveWire { pick: usize },
    }

    fn op() -> impl Strategy<Value = Op> {
        let at = (-3i32..=3, -3i32..=3).prop_map(|(x, y)| pt(x, y));
        prop_oneof![
            6 => (at.clone(), prop_oneof![-3i32..=-1, 1i32..=3], any::<bool>())
                .prop_map(|(at, len, horizontal)| Op::Wire { at, len, horizontal }),
            2 => (at.clone(), 1u8..=4).prop_map(|(at, pins)| Op::Part { at, pins }),
            1 => at.prop_map(|at| Op::Label { at }),
            1 => any::<usize>().prop_map(|pick| Op::RemoveWire { pick }),
        ]
    }

    /// Builds a sheet through the crate-private inserts. Small coordinates make corners,
    /// junctions, closed loops, coincident pins and overlapping wires common.
    fn build(ops: &[Op]) -> Doc {
        const LOCAL_PINS: [Pt; 4] = [
            Pt { x: 0, y: 0 },
            Pt { x: 1, y: 0 },
            Pt { x: 0, y: 1 },
            Pt { x: 1, y: 1 },
        ];
        let mut doc = Doc::new();
        for op in ops {
            match *op {
                Op::Wire {
                    at,
                    len,
                    horizontal,
                } => {
                    let b = if horizontal {
                        pt(at.x + len, at.y)
                    } else {
                        pt(at.x, at.y + len)
                    };
                    add_wire(&mut doc, at, b);
                }
                Op::Part { at, pins } => {
                    add_part(&mut doc, at, &LOCAL_PINS[..pins as usize]);
                }
                Op::Label { at } => {
                    add_label(&mut doc, at);
                }
                Op::RemoveWire { pick } => {
                    let ids: Vec<_> = doc.wires().map(|(id, _)| id).collect();
                    if !ids.is_empty() {
                        doc.remove_wire(ids[pick % ids.len()]).unwrap();
                    }
                }
            }
        }
        doc
    }

    fn all_items(doc: &Doc) -> Vec<ItemId> {
        doc.parts()
            .map(|(id, _)| ItemId::Part(id))
            .chain(doc.wires().map(|(id, _)| ItemId::Wire(id)))
            .chain(doc.labels().map(|(id, _)| ItemId::Label(id)))
            .collect()
    }

    fn shuffle(items: &mut [ItemId], mut seed: u64) {
        for i in (1..items.len()).rev() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            items.swap(i, (seed >> 33) as usize % (i + 1));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Every wire belongs to exactly one run; interior points are corners; ends are anchors.
        #[test]
        fn runs_partition_wires(ops in prop::collection::vec(op(), 1..60)) {
            let doc = build(&ops);
            let mut run = Run::default();
            let mut owner: HashMap<u32, Vec<u32>> = HashMap::new();
            let mut closed = HashSet::new();
            for (id, _) in doc.wires() {
                doc.run_through(id, &mut run);
                let set = wire_set(&run);
                prop_assert!(set.contains(&id.0));
                prop_assert_eq!(set.len(), run.wires.len(), "a wire repeats in its run");
                let pts = run_points(&doc, &run);
                let interior = if run.a.1 == Anchor::Closed {
                    prop_assert_eq!(run.b.1, Anchor::Closed);
                    prop_assert_eq!(run.a.0, run.b.0);
                    let least = pts.iter().min_by_key(|p| (p.x, p.y)).unwrap();
                    prop_assert_eq!(*least, run.a.0);
                    closed.insert(id.0);
                    &pts[..pts.len() - 1]
                } else {
                    prop_assert_eq!(Some(run.a.1), brute_anchor(&doc, run.a.0));
                    prop_assert_eq!(Some(run.b.1), brute_anchor(&doc, run.b.0));
                    prop_assert!((run.a.0.x, run.a.0.y) <= (run.b.0.x, run.b.0.y));
                    &pts[1..pts.len() - 1]
                };
                for &p in interior {
                    prop_assert!(is_corner(&doc, p), "interior point {:?} is not a corner", p);
                }
                let expected: Vec<_> = interior.iter().flat_map(|&p| labels_at(&doc, p)).collect();
                prop_assert_eq!(run.labels.to_vec(), sorted_labels(expected));
                owner.insert(id.0, set);
            }
            // Every member of a run reports the same run.
            for set in owner.values() {
                for w in set {
                    prop_assert_eq!(&owner[w], set);
                }
            }
            // Walking from every anchor finds every open run: once from each end, or once when
            // both ends share a point. Closed loops have no anchor and are never found.
            let mut points: Vec<Pt> = doc.wires().flat_map(|(_, w)| [w.a, w.b]).collect();
            points.sort_unstable_by_key(|p| (p.x, p.y));
            points.dedup();
            let mut seen: HashMap<u32, usize> = HashMap::new();
            let mut out = Vec::new();
            for p in points {
                doc.runs_at(p, &mut out);
                let anchor = brute_anchor(&doc, p);
                if anchor.is_none() {
                    prop_assert!(out.is_empty(), "runs start at a corner {:?}", p);
                }
                for r in &out {
                    prop_assert_eq!(r.a, (p, anchor.unwrap()));
                    prop_assert_eq!(Some(r.b.1), brute_anchor(&doc, r.b.0));
                    run_points(&doc, r);
                    let set = wire_set(r);
                    prop_assert_eq!(set.len(), r.wires.len(), "a wire repeats in a run");
                    prop_assert_eq!(&owner[&r.wires[0].0], &set);
                    doc.run_through(r.wires[0], &mut run);
                    prop_assert_eq!(&r.labels, &run.labels);
                    for w in set {
                        *seen.entry(w).or_default() += 1;
                    }
                }
            }
            for (id, _) in doc.wires() {
                doc.run_through(id, &mut run);
                let expected = if closed.contains(&id.0) {
                    0
                } else if run.a.0 == run.b.0 {
                    1
                } else {
                    2
                };
                prop_assert_eq!(seen.get(&id.0).copied().unwrap_or(0), expected);
            }
        }

        /// `connected_pins` equals a brute-force count (TC-count-01, TC-count-02).
        #[test]
        fn connected_pins_matches_brute_force(ops in prop::collection::vec(op(), 1..60)) {
            let doc = build(&ops);
            for (id, _) in doc.parts() {
                let pins = doc.pins(id).unwrap();
                let expected = pins
                    .iter()
                    .filter(|&&p| {
                        !wire_ends_at(&doc, p).is_empty()
                            || pins_at(&doc, p).len() > 1
                            || !labels_at(&doc, p).is_empty()
                    })
                    .count();
                prop_assert_eq!(usize::from(doc.connected_pins(id)), expected);
            }
        }

        /// WINV8: the affected runs are the same whatever the order of the selection, and each
        /// appears once.
        #[test]
        fn affected_runs_ignore_selection_order(
            ops in prop::collection::vec(op(), 1..60),
            mask in prop::collection::vec(any::<bool>(), 1..64),
            repeats in prop::collection::vec(any::<usize>(), 0..4),
            seed in any::<u64>(),
        ) {
            let doc = build(&ops);
            let items = all_items(&doc);
            let mut selection: Vec<ItemId> = items
                .iter()
                .enumerate()
                .filter_map(|(i, &item)| mask[i % mask.len()].then_some(item))
                .collect();
            if !selection.is_empty() {
                let extra: Vec<_> = repeats.iter().map(|r| selection[r % selection.len()]).collect();
                selection.extend(extra);
            }
            let mut reordered = selection.clone();
            shuffle(&mut reordered, seed);

            let selected_wires: HashSet<u32> = selection
                .iter()
                .filter_map(|i| match i { ItemId::Wire(w) => Some(w.0), _ => None })
                .collect();
            let unselected_end = |p: Pt| wire_ends_at(&doc, p).iter().any(|w| !selected_wires.contains(&w.0));
            let mut expected_points: Vec<Pt> = selection
                .iter()
                .flat_map(|&item| match item {
                    ItemId::Part(id) => doc.pins(id).unwrap().to_vec(),
                    ItemId::Wire(id) => { let w = doc.wire(id).unwrap(); vec![w.a, w.b] }
                    ItemId::Label(_) => vec![],
                })
                .filter(|&p| unselected_end(p))
                .collect();
            expected_points.sort_unstable_by_key(|p| (p.x, p.y));
            expected_points.dedup();

            let (mut b1, mut b2) = (Boundary::default(), Boundary::default());
            doc.boundary_points(&selection, &mut b1);
            doc.boundary_points(&reordered, &mut b2);
            prop_assert_eq!(b1.points(), expected_points.as_slice());
            prop_assert_eq!(&b1, &b2);

            let (mut r1, mut r2) = (Vec::new(), Vec::new());
            doc.affected_runs(&b1, &mut r1);
            doc.affected_runs(&b2, &mut r2);
            prop_assert_eq!(&r1, &r2);
            let mut s1: Vec<_> = r1.iter().map(wire_set).collect();
            let mut s2: Vec<_> = r2.iter().map(wire_set).collect();
            s1.sort();
            s2.sort();
            prop_assert_eq!(s1, s2);

            let mut claimed = HashSet::new();
            for run in &r1 {
                assert_wires_once(run);
                prop_assert!(b1.contains(run.a.0));
                prop_assert_eq!(run.a.1, Anchor::Boundary);
                let pts = run_points(&doc, run);
                for w in &run.wires {
                    prop_assert!(!selected_wires.contains(&w.0), "a selected wire is in a run");
                    prop_assert!(claimed.insert(w.0), "a wire is in two affected runs");
                }
                for &p in &pts[1..pts.len() - 1] {
                    prop_assert!(is_corner(&doc, p) && !b1.contains(p));
                }
                let expected_b = if b1.contains(run.b.0) {
                    Anchor::Boundary
                } else {
                    brute_anchor(&doc, run.b.0).expect("a run ends at an anchor")
                };
                prop_assert_eq!(run.b.1, expected_b);
                let labels: Vec<_> = pts[1..pts.len() - 1].iter().flat_map(|&p| labels_at(&doc, p)).collect();
                prop_assert_eq!(run.labels.to_vec(), sorted_labels(labels));
            }
            // Every unselected wire with an end on a boundary point is in some affected run.
            for (id, w) in doc.wires() {
                if !selected_wires.contains(&id.0) && (b1.contains(w.a) || b1.contains(w.b)) {
                    prop_assert!(claimed.contains(&id.0));
                }
            }
        }

        /// Determinism: rebuilding the same document (same geometry, same ids) by deleting and
        /// undoing every item in a shuffled order, or by saving and loading it, changes the
        /// order in which ends were added at each point but not `runs_at` or `affected_runs`,
        /// which must come back identical, order included.
        #[test]
        fn reconstruction_keeps_run_order(
            ops in prop::collection::vec(op(), 1..60),
            mask in prop::collection::vec(any::<bool>(), 1..64),
            seed in any::<u64>(),
        ) {
            let doc = build(&ops);
            let mut items = all_items(&doc);
            let selection: Vec<ItemId> = items
                .iter()
                .enumerate()
                .filter_map(|(i, &item)| mask[i % mask.len()].then_some(item))
                .collect();
            let mut points: Vec<Pt> = doc
                .wires()
                .flat_map(|(_, w)| [w.a, w.b])
                .chain(doc.parts().flat_map(|(id, _)| doc.pins(id).unwrap().to_vec()))
                .collect();
            points.sort_unstable_by_key(|p| (p.x, p.y));
            points.dedup();
            let snapshot = |d: &Doc| {
                let mut out = Vec::new();
                let at: Vec<Vec<Run>> = points
                    .iter()
                    .map(|&p| {
                        d.runs_at(p, &mut out);
                        out.clone()
                    })
                    .collect();
                let mut boundary = Boundary::default();
                d.boundary_points(&selection, &mut boundary);
                d.affected_runs(&boundary, &mut out);
                (at, boundary, out)
            };
            let before = snapshot(&doc);

            let mut undone = build(&ops);
            shuffle(&mut items, seed);
            for &item in &items {
                undone.apply(Command::Delete { items: vec![item], with_wires: false }).unwrap();
                undone.undo().unwrap();
            }
            prop_assert!(undone.content_eq(&doc));
            prop_assert_eq!(&snapshot(&undone), &before);

            let mut bytes = Vec::new();
            undone.save(&mut bytes).unwrap();
            let loaded = Doc::load(bytes.as_slice()).unwrap();
            prop_assert!(loaded.content_eq(&doc));
            prop_assert_eq!(&snapshot(&loaded), &before);
        }

        /// The labels at a point are read from the connectivity point store and stay correct
        /// through insert, remove, restore, `Move`, `Reorient`, undo, redo and load: at every
        /// step `labels_at`, the corner labels of every run and `connected_pins` match a scan
        /// of the label arena.
        #[test]
        fn labels_at_tracks_every_edit(edits in prop::collection::vec(edit(), 1..40)) {
            let mut doc = Doc::new();
            let mut seen: HashSet<Pt> = HashSet::new();
            for e in edits {
                let items = all_items(&doc);
                let pick = |mask: &[bool]| -> Vec<ItemId> {
                    items
                        .iter()
                        .enumerate()
                        .filter_map(|(i, &item)| mask[i % mask.len()].then_some(item))
                        .collect()
                };
                // Refused commands change nothing; the checks below hold either way.
                match e {
                    Edit::Wire { at, len, horizontal } => {
                        let b = if horizontal { pt(at.x + len, at.y) } else { pt(at.x, at.y + len) };
                        let _ = doc.apply(Command::AddWires(vec![Wire { a: at, b }]));
                    }
                    Edit::Part { at } => {
                        let _ = doc.apply(Command::PlacePart {
                            part: Part {
                                symbol: SymbolId(0),
                                at,
                                orient: Orient::IDENTITY,
                                designator: "U".into(),
                                value: "v".into(),
                            },
                            local_pins: vec![Pt::ZERO, pt(1, 0)],
                            local_bounds: Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
                        });
                    }
                    Edit::Label { at } => {
                        let _ = doc.apply(Command::AddLabel(Label { at, text: "N".into() }));
                    }
                    Edit::Move { mask, delta } => {
                        let _ = doc.apply(Command::Move { follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(), items: pick(&mask), delta });
                    }
                    Edit::Reorient { mask, pivot, by } => {
                        let _ = doc.apply(Command::Reorient { follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                            items: pick(&mask),
                            pivot,
                            by: Orient::ALL[by as usize],
                        });
                    }
                    Edit::Delete { mask } => {
                        let _ = doc.apply(Command::Delete { items: pick(&mask), with_wires: false });
                    }
                    Edit::Undo => {
                        doc.undo();
                    }
                    Edit::Redo => {
                        doc.redo();
                    }
                    Edit::RemoveRestore { pick } => {
                        // The crate-private remove and restore, as a pair so that the undo
                        // history still matches the document afterwards.
                        let ids: Vec<_> = doc.labels().map(|(id, _)| id).collect();
                        if !ids.is_empty() {
                            let id = ids[pick % ids.len()];
                            let label = doc.remove_label(id).unwrap();
                            check_labels(&doc, &seen);
                            doc.restore_label(id, label).unwrap();
                        }
                    }
                    Edit::Load => {
                        let mut bytes = Vec::new();
                        doc.save(&mut bytes).unwrap();
                        doc = Doc::load(bytes.as_slice()).unwrap();
                    }
                }
                seen.extend(doc.labels().map(|(_, l)| l.at));
                seen.extend(doc.wires().flat_map(|(_, w)| [w.a, w.b]));
                check_labels(&doc, &seen);
            }
        }
    }

    /// `labels_at` at every point in `seen`, the corner labels of every run and
    /// `connected_pins` all match scans of the arenas.
    fn check_labels(doc: &Doc, seen: &HashSet<Pt>) {
        let mut out = Vec::new();
        for &p in seen {
            doc.labels_at(p, &mut out);
            assert_eq!(out, sorted_labels(labels_at(doc, p)), "labels at {p:?}");
        }
        let mut run = Run::default();
        for (id, _) in doc.wires() {
            doc.run_through(id, &mut run);
            let pts = run_points(doc, &run);
            let interior = if run.a.1 == Anchor::Closed {
                &pts[..pts.len() - 1]
            } else {
                &pts[1..pts.len() - 1]
            };
            let expected: Vec<_> = interior.iter().flat_map(|&p| labels_at(doc, p)).collect();
            assert_eq!(run.labels.to_vec(), sorted_labels(expected));
        }
        for (id, _) in doc.parts() {
            let expected = doc
                .pins(id)
                .unwrap()
                .iter()
                .filter(|&&p| {
                    !wire_ends_at(doc, p).is_empty()
                        || pins_at(doc, p).len() > 1
                        || !labels_at(doc, p).is_empty()
                })
                .count();
            assert_eq!(usize::from(doc.connected_pins(id)), expected);
        }
    }

    #[derive(Debug, Clone)]
    enum Edit {
        Wire { at: Pt, len: i32, horizontal: bool },
        Part { at: Pt },
        Label { at: Pt },
        Move { mask: Vec<bool>, delta: Pt },
        Reorient { mask: Vec<bool>, pivot: Pt, by: u8 },
        Delete { mask: Vec<bool> },
        Undo,
        Redo,
        RemoveRestore { pick: usize },
        Load,
    }

    fn edit() -> impl Strategy<Value = Edit> {
        let at = (-3i32..=3, -3i32..=3).prop_map(|(x, y)| pt(x, y));
        let mask = prop::collection::vec(any::<bool>(), 1..8);
        prop_oneof![
            4 => (at.clone(), prop_oneof![-3i32..=-1, 1i32..=3], any::<bool>())
                .prop_map(|(at, len, horizontal)| Edit::Wire { at, len, horizontal }),
            1 => at.clone().prop_map(|at| Edit::Part { at }),
            3 => at.clone().prop_map(|at| Edit::Label { at }),
            2 => (mask.clone(), (-2i32..=2, -2i32..=2))
                .prop_map(|(mask, (x, y))| Edit::Move { mask, delta: pt(x, y) }),
            2 => (mask.clone(), at, 0u8..8)
                .prop_map(|(mask, pivot, by)| Edit::Reorient { mask, pivot, by }),
            1 => mask.prop_map(|mask| Edit::Delete { mask }),
            2 => Just(Edit::Undo),
            1 => Just(Edit::Redo),
            1 => any::<usize>().prop_map(|pick| Edit::RemoveRestore { pick }),
            1 => Just(Edit::Load),
        ]
    }

    /// Finding the corner labels along a long chain reads each corner's labels in place: the
    /// labels on a 200-corner run in one index cell are found, and moving or deleting one
    /// updates the run.
    #[test]
    fn long_chain_labels() {
        let mut doc = Doc::new();
        let mut ids = Vec::new();
        // A staircase (0, 0), (1, 0), (1, 1), (2, 1), ...: 199 corners and two free ends.
        let step = |k: i32| pt((k + 1) / 2, k / 2);
        for k in 0..200 {
            ids.push(add_wire(&mut doc, step(k), step(k + 1)));
        }
        let corner = add_label(&mut doc, pt(1, 0));
        let mut run = Run::default();
        doc.run_through(ids[100], &mut run);
        assert_eq!(run.wires.len(), 200);
        assert_wires_once(&run);
        assert_eq!(run.labels.as_slice(), &[corner]);
        doc.apply(Command::Move {
            follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
            items: vec![ItemId::Label(corner)],
            delta: pt(2, 2),
        })
        .unwrap();
        doc.run_through(ids[100], &mut run);
        assert_eq!(run.labels.as_slice(), &[corner], "(3, 2) is also a corner");
        doc.undo().unwrap();
        doc.apply(Command::Delete {
            items: vec![ItemId::Label(corner)],
            with_wires: false,
        })
        .unwrap();
        doc.run_through(ids[100], &mut run);
        assert!(run.labels.is_empty());
    }

    /// Delete and undo of one wire at a three-wire junction moves its end to the back of that
    /// point's incidence list; `runs_at` and `affected_runs` keep their order.
    #[test]
    fn junction_order_survives_delete_undo_and_load() {
        let mut doc = Doc::new();
        let p = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        let w = [
            add_wire(&mut doc, Pt::ZERO, pt(0, 2)),
            add_wire(&mut doc, Pt::ZERO, pt(-2, 0)),
            add_wire(&mut doc, Pt::ZERO, pt(2, 0)),
        ];
        let runs = |d: &Doc| {
            let (mut at, mut affected, mut boundary) =
                (Vec::new(), Vec::new(), Boundary::default());
            d.runs_at(Pt::ZERO, &mut at);
            d.boundary_points(&[ItemId::Part(p)], &mut boundary);
            d.affected_runs(&boundary, &mut affected);
            (at, affected)
        };
        let before = runs(&doc);
        // Sorted by far end: (-2, 0), (0, 2), (2, 0).
        let far: Vec<_> = before.0.iter().map(|r| r.b.0).collect();
        assert_eq!(far, vec![pt(-2, 0), pt(0, 2), pt(2, 0)]);
        assert_eq!(
            before.1.iter().map(|r| r.wires[0]).collect::<Vec<_>>(),
            vec![w[1], w[0], w[2]]
        );
        for &id in &w {
            doc.apply(Command::Delete {
                items: vec![ItemId::Wire(id)],
                with_wires: false,
            })
            .unwrap();
            doc.undo().unwrap();
            assert_eq!(runs(&doc), before);
        }
        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        assert_eq!(runs(&Doc::load(bytes.as_slice()).unwrap()), before);
    }

    /// TC-count-01: two pins on the same net, nine connected pins in all, count 9.
    #[test]
    fn tc_count_01_counts_pins_not_nets() {
        let mut doc = Doc::new();
        let pins: Vec<Pt> = (0..10).map(|x| pt(x * 2, 0)).collect();
        let part = add_part(&mut doc, Pt::ZERO, &pins);
        // Pins 0 and 1 share a net through one wire; pins 2..=8 each get a stub; pin 9 is bare.
        add_wire(&mut doc, pt(0, 0), pt(2, 0));
        for x in 2..=8 {
            add_wire(&mut doc, pt(x * 2, 0), pt(x * 2, -2));
        }
        assert_eq!(doc.net_at(pt(0, 0)), doc.net_at(pt(2, 0)));
        assert_eq!(doc.connected_pins(part), 9);
    }

    /// TC-count-02: one pin with three wire ends on it counts 1.
    #[test]
    fn tc_count_02_one_pin_three_ends() {
        let mut doc = Doc::new();
        let part = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO, pt(0, 5)]);
        add_wire(&mut doc, Pt::ZERO, pt(2, 0));
        add_wire(&mut doc, Pt::ZERO, pt(-2, 0));
        add_wire(&mut doc, Pt::ZERO, pt(0, -2));
        assert_eq!(doc.connected_pins(part), 1);
        assert_eq!(doc.connected_pins(PartId(99)), 0);
    }

    #[test]
    fn connected_pins_counts_labels_and_coincident_pins() {
        let mut doc = Doc::new();
        let part = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO, pt(4, 0), pt(4, 0)]);
        assert_eq!(doc.connected_pins(part), 2, "pins 1 and 2 share a point");
        add_label(&mut doc, Pt::ZERO);
        assert_eq!(doc.connected_pins(part), 3, "a label on a pin counts");
    }

    /// A Z between two pins: one run, the label on its corner rides it, the anchors are pins.
    #[test]
    fn z_run_between_pins() {
        let mut doc = Doc::new();
        let p = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        let q = add_part(&mut doc, pt(6, 4), &[Pt::ZERO]);
        let w0 = add_wire(&mut doc, pt(0, 0), pt(3, 0));
        let w1 = add_wire(&mut doc, pt(3, 4), pt(3, 0));
        let w2 = add_wire(&mut doc, pt(3, 4), pt(6, 4));
        let on_corner = add_label(&mut doc, pt(3, 4));
        add_label(&mut doc, Pt::ZERO); // on an anchor: not part of the run
        let expected = Run {
            wires: [w0, w1, w2].into_iter().collect(),
            a: (Pt::ZERO, Anchor::Pin { part: p, pin: 0 }),
            b: (pt(6, 4), Anchor::Pin { part: q, pin: 0 }),
            labels: [on_corner].into_iter().collect(),
        };
        let mut run = Run::default();
        for w in [w0, w1, w2] {
            doc.run_through(w, &mut run);
            assert_eq!(run, expected);
        }
        let mut out = vec![Run::default()];
        doc.runs_at(Pt::ZERO, &mut out);
        assert_eq!(out, vec![expected.clone()]);
        doc.runs_at(pt(3, 0), &mut out);
        assert!(out.is_empty(), "a corner starts no run");
        doc.runs_at(pt(9, 9), &mut out);
        assert!(out.is_empty());
        doc.runs_at(pt(6, 4), &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].wires.as_slice(), &[w2, w1, w0]);
        doc.run_through(WireId(99), &mut run);
        assert!(run.wires.is_empty());
    }

    /// A T: three runs meet at a junction; each ends at the junction and a free end.
    #[test]
    fn junction_splits_runs() {
        let mut doc = Doc::new();
        for b in [pt(-2, 0), pt(2, 0), pt(0, 2)] {
            add_wire(&mut doc, Pt::ZERO, b);
        }
        let mut out = Vec::new();
        doc.runs_at(Pt::ZERO, &mut out);
        assert_eq!(out.len(), 3);
        for run in &out {
            assert_eq!(run.wires.len(), 1);
            assert_eq!(run.a, (Pt::ZERO, Anchor::Junction));
            assert_eq!(run.b.1, Anchor::FreeEnd);
        }
    }

    /// A ring of corners has no anchor: one closed run starting at its least point.
    #[test]
    fn closed_loop() {
        let mut doc = Doc::new();
        let ring = [pt(2, 2), pt(4, 2), pt(4, 4), pt(2, 4)];
        let mut ids = Vec::new();
        for i in 0..4 {
            ids.push(add_wire(&mut doc, ring[(i + 1) % 4], ring[i]));
        }
        let label = add_label(&mut doc, pt(4, 4));
        let mut run = Run::default();
        doc.run_through(ids[2], &mut run);
        assert_eq!(run.a, (pt(2, 2), Anchor::Closed));
        assert_eq!(run.b, run.a);
        assert_eq!(wire_set(&run), vec![0, 1, 2, 3]);
        assert_wires_once(&run);
        assert_eq!(run.labels.as_slice(), &[label]);
        run_points(&doc, &run);
        let mut out = Vec::new();
        doc.runs_at(pt(2, 2), &mut out);
        assert!(out.is_empty());
    }

    /// A loop leaving a pin and coming back to it is one run, found once from that pin, in the
    /// direction whose first step is the lesser point: towards (0, 2), not (2, 0).
    #[test]
    fn loop_back_to_an_anchor_is_found_once() {
        let mut doc = Doc::new();
        let p = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        add_wire(&mut doc, Pt::ZERO, pt(2, 0));
        add_wire(&mut doc, pt(2, 0), pt(2, 2));
        add_wire(&mut doc, pt(2, 2), pt(0, 2));
        add_wire(&mut doc, pt(0, 2), Pt::ZERO);
        let mut out = Vec::new();
        doc.runs_at(Pt::ZERO, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].a, (Pt::ZERO, Anchor::Pin { part: p, pin: 0 }));
        assert_eq!(out[0].b, out[0].a);
        assert_eq!(
            out[0].wires.as_slice(),
            &[WireId(3), WireId(2), WireId(1), WireId(0)]
        );
    }

    /// TC-move-17's setup: a part and the first segment of its run are selected, so the rest
    /// of the run starts at the boundary point where the selected segment ends.
    #[test]
    fn selected_segment_moves_the_boundary() {
        let mut doc = Doc::new();
        let p = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        let q = add_part(&mut doc, pt(4, 4), &[Pt::ZERO]);
        let w0 = add_wire(&mut doc, Pt::ZERO, pt(2, 0));
        let w1 = add_wire(&mut doc, pt(2, 0), pt(2, 4));
        let w2 = add_wire(&mut doc, pt(2, 4), pt(4, 4));
        let mut boundary = Boundary::default();
        doc.boundary_points(&[ItemId::Wire(w0), ItemId::Part(p)], &mut boundary);
        assert_eq!(boundary.points(), &[pt(2, 0)]);
        let mut out = Vec::new();
        doc.affected_runs(&boundary, &mut out);
        assert_eq!(
            out,
            vec![Run {
                wires: [w1, w2].into_iter().collect(),
                a: (pt(2, 0), Anchor::Boundary),
                b: (pt(4, 4), Anchor::Pin { part: q, pin: 0 }),
                labels: SmallVec::new(),
            }]
        );
        // Selecting only the part gives the whole run.
        doc.boundary_points(&[ItemId::Part(p)], &mut boundary);
        doc.affected_runs(&boundary, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].wires.as_slice(), &[w0, w1, w2]);
        // No selection, no boundary, nothing affected.
        doc.boundary_points(&[], &mut boundary);
        assert!(boundary.is_empty());
        doc.affected_runs(&boundary, &mut out);
        assert!(out.is_empty());
    }

    /// TC-move-06/09's setup: one run shared by two selected parts is found once, from the
    /// lesser end, whichever part is listed first.
    #[test]
    fn run_between_two_selected_parts_is_found_once() {
        let mut doc = Doc::new();
        let p = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        let q = add_part(&mut doc, pt(6, 2), &[Pt::ZERO]);
        add_wire(&mut doc, Pt::ZERO, pt(3, 0));
        add_wire(&mut doc, pt(3, 0), pt(3, 2));
        add_wire(&mut doc, pt(3, 2), pt(6, 2));
        let mut boundary = Boundary::default();
        let mut out = Vec::new();
        for items in [
            [ItemId::Part(p), ItemId::Part(q)],
            [ItemId::Part(q), ItemId::Part(p)],
        ] {
            doc.boundary_points(&items, &mut boundary);
            assert_eq!(boundary.points(), &[Pt::ZERO, pt(6, 2)]);
            doc.affected_runs(&boundary, &mut out);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].a, (Pt::ZERO, Anchor::Boundary));
            assert_eq!(out[0].b, (pt(6, 2), Anchor::Boundary));
            assert_eq!(out[0].wires.len(), 3);
        }
        // Two pins touching with no wire are not a boundary (WQ6).
        let mut doc = Doc::new();
        let a = add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        add_part(&mut doc, Pt::ZERO, &[Pt::ZERO]);
        doc.boundary_points(&[ItemId::Part(a)], &mut boundary);
        assert!(boundary.is_empty());
    }
}
