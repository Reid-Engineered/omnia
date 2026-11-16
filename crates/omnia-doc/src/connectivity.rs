//! Incremental geometric and logical connectivity (D2, INV10).
//!
//! Two layers:
//! 1. **Geometric components** over the point map (`Pt -> wire ends, pins and labels`, D2).
//!    Only wires join two points; a pin or label only occupies one, so two abutting pins (or a
//!    pin and a wire end) at the same point share a component. Adding a wire unions the
//!    components at its endpoints (union-by-size); removing a wire takes an O(1) fast path when
//!    an endpoint vanishes, and otherwise re-floods only the touched component via BFS using a
//!    generation stamp on points. Adding or removing a pin never splits or joins components.
//! 2. **Logical nets** (`NetId`) over `(Component, LabelText)`. Components carrying the same net
//!    label text share a `NetId`. Label changes and component splits take O(1) fast paths in the
//!    common single-label case and fall back to a bipartite re-flood of the touched net only when
//!    a net with multiple distinct shorted label texts splits.
//!
//! [`crate::Doc`] owns one and drives it from every insert and remove; [`Connectivity::rebuild`]
//! serves load and is the INV10 test oracle.
//!
//! A [`NetId`] is a handle into the current state only: ids are reused after nets are freed and
//! nets are renumbered when they merge or split. It is valid until the next edit; a holder (for
//! example a net highlight) must re-resolve it from a wire or point after any change.

use std::{borrow::Borrow, collections::VecDeque, mem, sync::Arc};

use rustc_hash::FxHashMap;
use smallvec::{SmallVec, smallvec};

use crate::{Label, LabelId, PartId, Pt, Wire, WireId};

/// Identifier of an electrical net (one or more connected components linked by net labels).
///
/// Valid only until the next edit: freed ids are reused and merges and splits renumber nets.
/// Re-resolve it with [`Connectivity::net_of`] or [`Connectivity::net_at`] after any change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NetId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CompId(u32);

#[derive(Debug, Clone, Copy)]
struct WireEntry {
    wire: Wire,
    comp: CompId,
    comp_idx: u32,
}

#[derive(Debug, Clone)]
struct LabelEntry {
    at: Pt,
    text: Arc<str>,
}

#[derive(Debug)]
struct PointSlot {
    comp: CompId,
    comp_idx: u32,
    visited: u32,
    wires: SmallVec<[WireId; 4]>,
    labels: SmallVec<[LabelId; 2]>,
    pins: SmallVec<[(PartId, u32); 2]>,
}

impl PointSlot {
    fn new(comp: CompId, comp_idx: u32) -> Self {
        Self {
            comp,
            comp_idx,
            visited: 0,
            wires: SmallVec::new(),
            labels: SmallVec::new(),
            pins: SmallVec::new(),
        }
    }

    /// A point stays in the map while any wire end, label or pin occupies it.
    fn is_empty(&self) -> bool {
        self.wires.is_empty() && self.labels.is_empty() && self.pins.is_empty()
    }
}

type LabelCounts = SmallVec<[(Arc<str>, u32); 1]>;

#[derive(Debug)]
struct Comp {
    net: NetId,
    net_idx: u32,
    points: Vec<Pt>,
    wires: Vec<WireId>,
    label_counts: LabelCounts,
}

#[derive(Debug)]
struct Net {
    comps: Vec<CompId>,
    label_counts: LabelCounts,
}

/// Incremental schematic connectivity over wires, part pins and net labels (D2, INV10).
///
/// Each live wire belongs to a geometric connected component over the points its ends, pins and
/// labels occupy, and geometric components sharing any net label text belong to the same
/// [`NetId`].
#[derive(Debug, Default)]
pub struct Connectivity {
    wires: Vec<Option<WireEntry>>,
    labels: Vec<Option<LabelEntry>>,
    /// World pin positions per part, indexed by part id.
    parts: Vec<Option<Vec<Pt>>>,
    points: FxHashMap<Pt, PointSlot>,
    comps: Vec<Option<Comp>>,
    free_comps: Vec<u32>,
    nets: Vec<Option<Net>>,
    free_nets: Vec<u32>,
    live_nets: usize,
    label_nets: FxHashMap<Arc<str>, NetId>,
    visit_gen: u32,
    bfs_queue: VecDeque<Pt>,
}

impl Connectivity {
    /// Creates an empty connectivity tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes all wires, labels, components, and nets while retaining allocated table capacity.
    pub fn clear(&mut self) {
        self.wires.clear();
        self.labels.clear();
        self.parts.clear();
        self.points.clear();
        self.comps.clear();
        self.free_comps.clear();
        self.nets.clear();
        self.free_nets.clear();
        self.live_nets = 0;
        self.label_nets.clear();
        self.visit_gen = 0;
        self.bfs_queue.clear();
    }

    /// Number of currently live nets (including label-only single-point nets with no wires yet).
    pub fn net_count(&self) -> usize {
        self.live_nets
    }

    /// Returns the [`NetId`] of a live wire, or `None` if `wire` is not registered.
    pub fn net_of(&self, wire: WireId) -> Option<NetId> {
        let entry = self.wires.get(wire.0 as usize)?.as_ref()?;
        let comp = self.comps[entry.comp.0 as usize]
            .as_ref()
            .expect("live wire points to freed component");
        Some(comp.net)
    }

    /// Returns the [`NetId`] at `at` if any wire end, pin or label sits there.
    pub fn net_at(&self, at: Pt) -> Option<NetId> {
        let slot = self.points.get(&at)?;
        let comp = self.comps[slot.comp.0 as usize]
            .as_ref()
            .expect("live point points to freed component");
        Some(comp.net)
    }

    /// Replaces `out` with every live wire in `net`. Order is unspecified.
    ///
    /// Allocates nothing when `out` already has enough capacity for the result.
    pub fn net_wires(&self, net: NetId, out: &mut Vec<WireId>) {
        out.clear();
        let Some(Some(net_data)) = self.nets.get(net.0 as usize) else {
            return;
        };
        for &cid in &net_data.comps {
            let comp = self.comps[cid.0 as usize]
                .as_ref()
                .expect("live net lists freed component");
            out.extend_from_slice(&comp.wires);
        }
    }

    /// Replaces `out` with the distinct net label texts carried by `net`. Order is unspecified.
    pub fn net_labels(&self, net: NetId, out: &mut Vec<Arc<str>>) {
        out.clear();
        let Some(Some(net_data)) = self.nets.get(net.0 as usize) else {
            return;
        };
        for (text, _) in &net_data.label_counts {
            out.push(Arc::clone(text));
        }
    }

    /// Rebuilds connectivity from scratch from live wires, live labels and the world pin
    /// positions of live parts.
    ///
    /// Used after load and as the INV10 property-test oracle. Implemented with an independent
    /// from-scratch BFS rather than calling [`Self::add_wire`]/[`Self::add_label`].
    pub fn rebuild<W, L, LB>(wires: W, labels: L) -> Self
    where
        W: IntoIterator<Item = (WireId, Wire)>,
        L: IntoIterator<Item = (LabelId, LB)>,
        LB: Borrow<Label>,
    {
        Self::rebuild_with_parts(wires, labels, std::iter::empty::<(PartId, Vec<Pt>)>())
    }

    pub fn rebuild_with_parts<W, L, LB, P, PB>(wires: W, labels: L, parts: P) -> Self
    where
        W: IntoIterator<Item = (WireId, Wire)>,
        L: IntoIterator<Item = (LabelId, LB)>,
        LB: Borrow<Label>,
        P: IntoIterator<Item = (PartId, PB)>,
        PB: AsRef<[Pt]>,
    {
        let mut conn = Self::new();
        let blank = || PointSlot::new(CompId(0), 0);

        for (id, wire) in wires {
            assert!(
                wire.a != wire.b && (wire.a.x == wire.b.x || wire.a.y == wire.b.y),
                "invalid wire"
            );
            let idx = id.0 as usize;
            if idx >= conn.wires.len() {
                conn.wires.resize_with(idx + 1, || None);
            }
            assert!(
                conn.wires[idx].is_none(),
                "duplicate WireId {:?} in Connectivity::rebuild",
                id
            );
            conn.wires[idx] = Some(WireEntry {
                wire,
                comp: CompId(0),
                comp_idx: 0,
            });
            conn.points
                .entry(wire.a)
                .or_insert_with(blank)
                .wires
                .push(id);
            if wire.b != wire.a {
                conn.points
                    .entry(wire.b)
                    .or_insert_with(blank)
                    .wires
                    .push(id);
            }
        }

        for (id, label_ref) in labels {
            let label = label_ref.borrow();
            let idx = id.0 as usize;
            if idx >= conn.labels.len() {
                conn.labels.resize_with(idx + 1, || None);
            }
            assert!(
                conn.labels[idx].is_none(),
                "duplicate LabelId {:?} in Connectivity::rebuild",
                id
            );
            conn.labels[idx] = Some(LabelEntry {
                at: label.at,
                text: Arc::clone(&label.text),
            });
            conn.points
                .entry(label.at)
                .or_insert_with(blank)
                .labels
                .push(id);
        }

        for (id, pins) in parts {
            let pins = pins.as_ref();
            let idx = id.0 as usize;
            if idx >= conn.parts.len() {
                conn.parts.resize_with(idx + 1, || None);
            }
            assert!(
                conn.parts[idx].is_none(),
                "duplicate PartId {:?} in Connectivity::rebuild",
                id
            );
            conn.parts[idx] = Some(pins.to_vec());
            for (pin, &at) in pins.iter().enumerate() {
                let pin = u32::try_from(pin).expect("pin index overflow");
                conn.points
                    .entry(at)
                    .or_insert_with(blank)
                    .pins
                    .push((id, pin));
            }
        }

        // Phase 1: from-scratch BFS over the point map to discover geometric components.
        let all_pts: Vec<Pt> = conn.points.keys().copied().collect();
        let stamp = 1u32;
        conn.visit_gen = stamp;
        let mut label_to_comps: FxHashMap<Arc<str>, Vec<CompId>> = FxHashMap::default();

        for start in all_pts {
            if conn.points[&start].visited == stamp {
                continue;
            }
            let comp_id = CompId(u32::try_from(conn.comps.len()).expect("component id overflow"));
            let mut comp_pts = Vec::new();
            let mut comp_wires = Vec::new();
            let mut comp_labels = LabelCounts::new();

            conn.points.get_mut(&start).unwrap().visited = stamp;
            conn.bfs_queue.clear();
            conn.bfs_queue.push_back(start);

            while let Some(p) = conn.bfs_queue.pop_front() {
                let comp_pt_idx = u32::try_from(comp_pts.len()).expect("point count overflow");
                comp_pts.push(p);

                // Temporarily inspect the slot, update its comp/comp_idx, and enqueue neighbors.
                let slot = conn.points.get_mut(&p).unwrap();
                slot.comp = comp_id;
                slot.comp_idx = comp_pt_idx;
                for &lid in &slot.labels {
                    let text = &conn.labels[lid.0 as usize].as_ref().unwrap().text;
                    inc_label_count(&mut comp_labels, text, 1);
                }

                // Collect neighbor endpoints without holding a mutable borrow on `conn.points`.
                let mut neighbors: SmallVec<[Pt; 4]> = SmallVec::new();
                for &wid in &slot.wires {
                    let w_entry = conn.wires[wid.0 as usize].as_mut().unwrap();
                    if w_entry.wire.a == p {
                        let w_idx = u32::try_from(comp_wires.len()).expect("wire count overflow");
                        comp_wires.push(wid);
                        w_entry.comp = comp_id;
                        w_entry.comp_idx = w_idx;
                    }
                    let other = if w_entry.wire.a == p {
                        w_entry.wire.b
                    } else {
                        w_entry.wire.a
                    };
                    if other != p {
                        neighbors.push(other);
                    }
                }
                for other in neighbors {
                    let neighbor_slot = conn.points.get_mut(&other).unwrap();
                    if neighbor_slot.visited != stamp {
                        neighbor_slot.visited = stamp;
                        conn.bfs_queue.push_back(other);
                    }
                }
            }

            for (text, _) in &comp_labels {
                label_to_comps
                    .entry(Arc::clone(text))
                    .or_default()
                    .push(comp_id);
            }

            conn.comps.push(Some(Comp {
                net: NetId(0),
                net_idx: 0,
                points: comp_pts,
                wires: comp_wires,
                label_counts: comp_labels,
            }));
        }

        // Phase 2: from-scratch BFS over (Comp, LabelText) to discover logical nets.
        let num_comps = conn.comps.len();
        let mut comp_visited = vec![false; num_comps];
        let mut label_visited: FxHashMap<Arc<str>, bool> = FxHashMap::default();
        let mut comp_queue = VecDeque::new();

        for start_idx in 0..num_comps {
            if comp_visited[start_idx] {
                continue;
            }
            let net_id = NetId(u32::try_from(conn.nets.len()).expect("net id overflow"));
            let mut net_comps = Vec::new();
            let mut net_labels = LabelCounts::new();

            comp_visited[start_idx] = true;
            comp_queue.push_back(CompId(start_idx as u32));

            while let Some(cid) = comp_queue.pop_front() {
                let net_idx = u32::try_from(net_comps.len()).expect("net comp overflow");
                net_comps.push(cid);
                let comp = conn.comps[cid.0 as usize].as_mut().unwrap();
                comp.net = net_id;
                comp.net_idx = net_idx;

                for (text, count) in &comp.label_counts {
                    inc_label_count(&mut net_labels, text, *count);
                    let seen = label_visited.entry(Arc::clone(text)).or_insert(false);
                    if !*seen {
                        *seen = true;
                        if let Some(adj) = label_to_comps.get(text) {
                            for &other_cid in adj {
                                if !comp_visited[other_cid.0 as usize] {
                                    comp_visited[other_cid.0 as usize] = true;
                                    comp_queue.push_back(other_cid);
                                }
                            }
                        }
                    }
                }
            }

            for (text, _) in &net_labels {
                conn.label_nets.insert(Arc::clone(text), net_id);
            }
            conn.nets.push(Some(Net {
                comps: net_comps,
                label_counts: net_labels,
            }));
            conn.live_nets += 1;
        }

        conn
    }

    /// Registers `id` connecting `wire.a` and `wire.b`.
    ///
    /// If `id` is already present, the previous wire is removed first.
    pub fn add_wire(&mut self, id: WireId, wire: Wire) {
        assert!(
            wire.a != wire.b && (wire.a.x == wire.b.x || wire.a.y == wire.b.y),
            "invalid wire"
        );
        if self
            .wires
            .get(id.0 as usize)
            .and_then(|e| e.as_ref())
            .is_some()
        {
            self.remove_wire(id);
        }

        let idx = id.0 as usize;
        if idx >= self.wires.len() {
            self.wires.resize_with(idx + 1, || None);
        }

        let comp_a = self.points.get(&wire.a).map(|s| s.comp);
        let comp_b = if wire.b == wire.a {
            comp_a
        } else {
            self.points.get(&wire.b).map(|s| s.comp)
        };

        let target_comp = match (comp_a, comp_b) {
            (None, None) => {
                // Brand new component (1 or 2 points, 1 wire, 0 labels) and singleton net.
                let comp_id = self.alloc_comp(Comp {
                    net: NetId(0),
                    net_idx: 0,
                    points: if wire.a == wire.b {
                        vec![wire.a]
                    } else {
                        vec![wire.a, wire.b]
                    },
                    wires: vec![id],
                    label_counts: LabelCounts::new(),
                });
                let net_id = self.alloc_net(Net {
                    comps: vec![comp_id],
                    label_counts: LabelCounts::new(),
                });
                self.comps[comp_id.0 as usize].as_mut().unwrap().net = net_id;

                self.points.insert(
                    wire.a,
                    PointSlot {
                        wires: smallvec![id],
                        ..PointSlot::new(comp_id, 0)
                    },
                );
                if wire.b != wire.a {
                    self.points.insert(
                        wire.b,
                        PointSlot {
                            wires: smallvec![id],
                            ..PointSlot::new(comp_id, 1)
                        },
                    );
                }
                self.wires[idx] = Some(WireEntry {
                    wire,
                    comp: comp_id,
                    comp_idx: 0,
                });
                return;
            }
            (Some(ca), None) => {
                // Extend `ca` with `wire.b` (which had no other wires, pins or labels).
                self.points.get_mut(&wire.a).unwrap().wires.push(id);
                let comp = self.comps[ca.0 as usize].as_mut().unwrap();
                let pt_idx = u32::try_from(comp.points.len()).expect("point count overflow");
                let w_idx = u32::try_from(comp.wires.len()).expect("wire count overflow");
                comp.points.push(wire.b);
                comp.wires.push(id);

                self.points.insert(
                    wire.b,
                    PointSlot {
                        wires: smallvec![id],
                        ..PointSlot::new(ca, pt_idx)
                    },
                );
                self.wires[idx] = Some(WireEntry {
                    wire,
                    comp: ca,
                    comp_idx: w_idx,
                });
                return;
            }
            (None, Some(cb)) => {
                // Extend `cb` with `wire.a`.
                self.points.get_mut(&wire.b).unwrap().wires.push(id);
                let comp = self.comps[cb.0 as usize].as_mut().unwrap();
                let pt_idx = u32::try_from(comp.points.len()).expect("point count overflow");
                let w_idx = u32::try_from(comp.wires.len()).expect("wire count overflow");
                comp.points.push(wire.a);
                comp.wires.push(id);

                self.points.insert(
                    wire.a,
                    PointSlot {
                        wires: smallvec![id],
                        ..PointSlot::new(cb, pt_idx)
                    },
                );
                self.wires[idx] = Some(WireEntry {
                    wire,
                    comp: cb,
                    comp_idx: w_idx,
                });
                return;
            }
            (Some(ca), Some(cb)) => {
                self.points.get_mut(&wire.a).unwrap().wires.push(id);
                if wire.b != wire.a {
                    self.points.get_mut(&wire.b).unwrap().wires.push(id);
                }
                if ca == cb {
                    ca
                } else {
                    self.union_comps(ca, cb)
                }
            }
        };

        let comp = self.comps[target_comp.0 as usize].as_mut().unwrap();
        let w_idx = u32::try_from(comp.wires.len()).expect("wire count overflow");
        comp.wires.push(id);
        self.wires[idx] = Some(WireEntry {
            wire,
            comp: target_comp,
            comp_idx: w_idx,
        });
    }

    /// Removes `id` and repairs the touched component (and logical net if split).
    pub fn remove_wire(&mut self, id: WireId) -> Option<Wire> {
        let entry = self.wires.get_mut(id.0 as usize)?.take()?;
        let wire = entry.wire;
        let old_comp = entry.comp;

        // Remove wire from `old_comp.wires` in O(1).
        self.remove_wire_from_comp(old_comp, entry.comp_idx);

        // Remove wire end(s) from `self.points`, pruning empty points.
        let a_alive = self.remove_wire_end_at(wire.a, id, old_comp);
        let b_alive = if wire.b == wire.a {
            a_alive
        } else {
            self.remove_wire_end_at(wire.b, id, old_comp)
        };

        let old_net = self.comps[old_comp.0 as usize].as_ref().unwrap().net;
        if self.comps[old_comp.0 as usize]
            .as_ref()
            .unwrap()
            .points
            .is_empty()
        {
            debug_assert!(
                self.comps[old_comp.0 as usize]
                    .as_ref()
                    .unwrap()
                    .wires
                    .is_empty()
            );
            debug_assert!(
                self.comps[old_comp.0 as usize]
                    .as_ref()
                    .unwrap()
                    .label_counts
                    .is_empty()
            );
            self.detach_and_free_empty_comp(old_comp, old_net);
            return Some(wire);
        }

        // Leaf fast path: if one endpoint vanished completely (no wires, pins or labels left),
        // removing this wire cannot disconnect any remaining points or alter label_counts.
        if wire.a != wire.b && (!a_alive || !b_alive) {
            return Some(wire);
        }
        if wire.a == wire.b {
            return Some(wire);
        }

        // General path: both endpoints remain alive. Flood from `wire.a` and test whether
        // `wire.b` is still reachable inside `old_comp`.
        let old_points = mem::take(&mut self.comps[old_comp.0 as usize].as_mut().unwrap().points);
        let old_wires_count = self.comps[old_comp.0 as usize]
            .as_ref()
            .unwrap()
            .wires
            .len();
        self.comps[old_comp.0 as usize]
            .as_mut()
            .unwrap()
            .wires
            .clear();
        self.comps[old_comp.0 as usize]
            .as_mut()
            .unwrap()
            .label_counts
            .clear();

        let stamp = self.next_visit_gen();
        let (comp1_pts, comp1_wires, comp1_labels) = self.flood_from(wire.a, stamp, old_comp);

        if self.points[&wire.b].visited == stamp {
            // Component stayed connected (wire was part of a cycle).
            debug_assert_eq!(comp1_pts.len(), old_points.len());
            debug_assert_eq!(comp1_wires.len(), old_wires_count);
            let c1 = self.comps[old_comp.0 as usize].as_mut().unwrap();
            c1.points = comp1_pts;
            c1.wires = comp1_wires;
            c1.label_counts = comp1_labels;
            return Some(wire);
        }

        // Component split into two pieces: `old_comp` (containing `wire.a`) and `new_comp`
        // (containing `wire.b`).
        let c1_has_labels = !comp1_labels.is_empty();
        {
            let c1 = self.comps[old_comp.0 as usize].as_mut().unwrap();
            c1.points = comp1_pts;
            c1.wires = comp1_wires;
            c1.label_counts = comp1_labels;
        }

        let new_comp = self.alloc_comp(Comp {
            net: old_net,
            net_idx: 0,
            points: Vec::new(),
            wires: Vec::new(),
            label_counts: LabelCounts::new(),
        });
        let (comp2_pts, comp2_wires, comp2_labels) = self.flood_from(wire.b, stamp, new_comp);
        let c2_has_labels = !comp2_labels.is_empty();
        {
            let c2 = self.comps[new_comp.0 as usize].as_mut().unwrap();
            c2.points = comp2_pts;
            c2.wires = comp2_wires;
            c2.label_counts = comp2_labels;
        }

        debug_assert_eq!(
            self.comps[old_comp.0 as usize]
                .as_ref()
                .unwrap()
                .points
                .len()
                + self.comps[new_comp.0 as usize]
                    .as_ref()
                    .unwrap()
                    .points
                    .len(),
            old_points.len()
        );
        debug_assert_eq!(
            self.comps[old_comp.0 as usize]
                .as_ref()
                .unwrap()
                .wires
                .len()
                + self.comps[new_comp.0 as usize]
                    .as_ref()
                    .unwrap()
                    .wires
                    .len(),
            old_wires_count
        );

        // Temporarily attach `new_comp` to `old_net`, then repair `old_net` if needed.
        {
            let net = self.nets[old_net.0 as usize].as_mut().unwrap();
            let net_idx = u32::try_from(net.comps.len()).expect("net comp overflow");
            net.comps.push(new_comp);
            self.comps[new_comp.0 as usize].as_mut().unwrap().net_idx = net_idx;
        }

        if !c2_has_labels {
            self.detach_comp_to_new_net(new_comp, old_net);
        } else if !c1_has_labels {
            self.detach_comp_to_new_net(old_comp, old_net);
        } else if self.nets[old_net.0 as usize]
            .as_ref()
            .unwrap()
            .label_counts
            .len()
            >= 2
        {
            self.re_flood_net(old_net);
        }

        Some(wire)
    }

    /// Registers net label `id` at `label.at` with `label.text`.
    ///
    /// If `id` is already present, the previous label is removed first.
    pub fn add_label(&mut self, id: LabelId, label: &Label) {
        if self
            .labels
            .get(id.0 as usize)
            .and_then(|e| e.as_ref())
            .is_some()
        {
            self.remove_label(id);
        }

        let idx = id.0 as usize;
        if idx >= self.labels.len() {
            self.labels.resize_with(idx + 1, || None);
        }
        self.labels[idx] = Some(LabelEntry {
            at: label.at,
            text: Arc::clone(&label.text),
        });

        let comp_id = if let Some(slot) = self.points.get_mut(&label.at) {
            slot.labels.push(id);
            slot.comp
        } else {
            // Brand new point with only this label (no wires yet).
            let comp_id = self.alloc_comp(Comp {
                net: NetId(0),
                net_idx: 0,
                points: vec![label.at],
                wires: Vec::new(),
                label_counts: smallvec![(Arc::clone(&label.text), 1)],
            });
            self.points.insert(
                label.at,
                PointSlot {
                    labels: smallvec![id],
                    ..PointSlot::new(comp_id, 0)
                },
            );
            if let Some(&existing_net) = self.label_nets.get(&label.text) {
                let net = self.nets[existing_net.0 as usize].as_mut().unwrap();
                let net_idx = u32::try_from(net.comps.len()).expect("net comp overflow");
                net.comps.push(comp_id);
                inc_label_count(&mut net.label_counts, &label.text, 1);
                let comp = self.comps[comp_id.0 as usize].as_mut().unwrap();
                comp.net = existing_net;
                comp.net_idx = net_idx;
            } else {
                let net_id = self.alloc_net(Net {
                    comps: vec![comp_id],
                    label_counts: smallvec![(Arc::clone(&label.text), 1)],
                });
                self.comps[comp_id.0 as usize].as_mut().unwrap().net = net_id;
                self.label_nets.insert(Arc::clone(&label.text), net_id);
            }
            return;
        };

        let (comp_gained_text, net_c) = {
            let comp = self.comps[comp_id.0 as usize].as_mut().unwrap();
            let gained = inc_label_count(&mut comp.label_counts, &label.text, 1);
            (gained, comp.net)
        };

        if !comp_gained_text {
            let net = self.nets[net_c.0 as usize].as_mut().unwrap();
            inc_label_count(&mut net.label_counts, &label.text, 1);
            return;
        }

        if let Some(&net_t) = self.label_nets.get(&label.text) {
            if net_t == net_c {
                let net = self.nets[net_c.0 as usize].as_mut().unwrap();
                inc_label_count(&mut net.label_counts, &label.text, 1);
            } else {
                let net = self.nets[net_c.0 as usize].as_mut().unwrap();
                inc_label_count(&mut net.label_counts, &label.text, 1);
                self.merge_nets(net_c, net_t);
            }
        } else {
            let net = self.nets[net_c.0 as usize].as_mut().unwrap();
            inc_label_count(&mut net.label_counts, &label.text, 1);
            self.label_nets.insert(Arc::clone(&label.text), net_c);
        }
    }

    /// Removes net label `id` and splits the logical net if the label was bridging components.
    pub fn remove_label(&mut self, id: LabelId) -> Option<Label> {
        let entry = self.labels.get_mut(id.0 as usize)?.take()?;
        let LabelEntry { at, text } = entry;

        let comp_id = {
            let slot = self.points.get_mut(&at).expect("label missing from points");
            let pos = slot
                .labels
                .iter()
                .position(|&l| l == id)
                .expect("label missing from point slot");
            slot.labels.swap_remove(pos);
            let comp_id = slot.comp;
            if slot.is_empty() {
                let removed = self.points.remove(&at).unwrap();
                self.remove_point_from_comp(comp_id, removed.comp_idx);
            }
            comp_id
        };

        let (comp_lost_text, comp_empty_pts, net_c) = {
            let comp = self.comps[comp_id.0 as usize].as_mut().unwrap();
            let lost = dec_label_count(&mut comp.label_counts, &text);
            (lost, comp.points.is_empty(), comp.net)
        };

        let net_lost_text = {
            let net = self.nets[net_c.0 as usize].as_mut().unwrap();
            dec_label_count(&mut net.label_counts, &text)
        };
        if net_lost_text {
            self.label_nets.remove(&text);
        }

        if comp_empty_pts {
            debug_assert!(
                self.comps[comp_id.0 as usize]
                    .as_ref()
                    .unwrap()
                    .wires
                    .is_empty()
            );
            debug_assert!(
                self.comps[comp_id.0 as usize]
                    .as_ref()
                    .unwrap()
                    .label_counts
                    .is_empty()
            );
            self.detach_and_free_empty_comp(comp_id, net_c);
            return Some(Label { at, text });
        }

        if comp_lost_text && !net_lost_text {
            let net = self.nets[net_c.0 as usize].as_ref().unwrap();
            if net.label_counts.len() == 1 {
                // Fast path: `text` is the only label in `net_c`, so `comp_id` now has no labels
                // and detaches cleanly into its own net in O(1).
                debug_assert!(
                    self.comps[comp_id.0 as usize]
                        .as_ref()
                        .unwrap()
                        .label_counts
                        .is_empty()
                );
                self.detach_comp_to_new_net(comp_id, net_c);
            } else {
                // `net_c` carries >= 2 distinct labels and `text` still connects other components.
                self.re_flood_net(net_c);
            }
        }

        Some(Label { at, text })
    }

    /// Registers the world pin positions of part `id`. Each pin occupies its point like a wire
    /// end without joining it to any other point; coincident pins share that point.
    ///
    /// If `id` is already present, the previous pins are removed first.
    pub fn add_part(&mut self, id: PartId, pins: &[Pt]) {
        if self
            .parts
            .get(id.0 as usize)
            .and_then(|e| e.as_ref())
            .is_some()
        {
            self.remove_part(id);
        }
        let idx = id.0 as usize;
        if idx >= self.parts.len() {
            self.parts.resize_with(idx + 1, || None);
        }
        self.parts[idx] = Some(pins.to_vec());
        for (pin, &at) in pins.iter().enumerate() {
            let pin = (id, u32::try_from(pin).expect("pin index overflow"));
            if let Some(slot) = self.points.get_mut(&at) {
                // An occupied point already belongs to a component; a pin changes nothing else.
                slot.pins.push(pin);
                continue;
            }
            // A new point with only this pin: a one-point component in its own unlabelled net.
            let comp_id = self.alloc_comp(Comp {
                net: NetId(0),
                net_idx: 0,
                points: vec![at],
                wires: Vec::new(),
                label_counts: LabelCounts::new(),
            });
            let net_id = self.alloc_net(Net {
                comps: vec![comp_id],
                label_counts: LabelCounts::new(),
            });
            self.comps[comp_id.0 as usize].as_mut().unwrap().net = net_id;
            self.points.insert(
                at,
                PointSlot {
                    pins: smallvec![pin],
                    ..PointSlot::new(comp_id, 0)
                },
            );
        }
    }

    /// Removes the pins of part `id` and returns their world positions.
    ///
    /// A pin never links two points, so removing one cannot split a component: a point that
    /// keeps a wire end, label or other pin is unchanged, and a point left empty had no wire or
    /// label, so it was a one-point unlabelled component, freed here with its net.
    pub fn remove_part(&mut self, id: PartId) -> Option<Vec<Pt>> {
        let pins = self.parts.get_mut(id.0 as usize)?.take()?;
        for (pin, &at) in pins.iter().enumerate() {
            let pin = (id, u32::try_from(pin).expect("pin index overflow"));
            let slot = self.points.get_mut(&at).expect("pin missing from points");
            let pos = slot
                .pins
                .iter()
                .position(|&p| p == pin)
                .expect("pin missing from point slot");
            slot.pins.swap_remove(pos);
            if !slot.is_empty() {
                continue;
            }
            let removed = self.points.remove(&at).unwrap();
            let comp_id = removed.comp;
            self.remove_point_from_comp(comp_id, removed.comp_idx);
            let comp = self.comps[comp_id.0 as usize].as_ref().unwrap();
            debug_assert!(comp.points.is_empty() && comp.wires.is_empty());
            debug_assert!(comp.label_counts.is_empty());
            let net = comp.net;
            self.detach_and_free_empty_comp(comp_id, net);
        }
        Some(pins)
    }

    fn alloc_comp(&mut self, comp: Comp) -> CompId {
        if let Some(id) = self.free_comps.pop() {
            self.comps[id as usize] = Some(comp);
            CompId(id)
        } else {
            let id = u32::try_from(self.comps.len()).expect("component id space exhausted");
            self.comps.push(Some(comp));
            CompId(id)
        }
    }

    fn free_comp(&mut self, id: CompId) -> Comp {
        let comp = self.comps[id.0 as usize]
            .take()
            .expect("double free of component");
        self.free_comps.push(id.0);
        comp
    }

    fn alloc_net(&mut self, net: Net) -> NetId {
        self.live_nets += 1;
        if let Some(id) = self.free_nets.pop() {
            self.nets[id as usize] = Some(net);
            NetId(id)
        } else {
            let id = u32::try_from(self.nets.len()).expect("net id space exhausted");
            self.nets.push(Some(net));
            NetId(id)
        }
    }

    fn free_net(&mut self, id: NetId) -> Net {
        self.live_nets -= 1;
        let net = self.nets[id.0 as usize].take().expect("double free of net");
        self.free_nets.push(id.0);
        net
    }

    fn remove_wire_from_comp(&mut self, comp_id: CompId, comp_idx: u32) {
        let comp = self.comps[comp_id.0 as usize].as_mut().unwrap();
        let idx = comp_idx as usize;
        comp.wires.swap_remove(idx);
        if let Some(&moved_wire) = comp.wires.get(idx) {
            self.wires[moved_wire.0 as usize].as_mut().unwrap().comp_idx = comp_idx;
        }
    }

    fn remove_point_from_comp(&mut self, comp_id: CompId, comp_idx: u32) {
        let comp = self.comps[comp_id.0 as usize].as_mut().unwrap();
        let idx = comp_idx as usize;
        comp.points.swap_remove(idx);
        if let Some(&moved_pt) = comp.points.get(idx) {
            self.points.get_mut(&moved_pt).unwrap().comp_idx = comp_idx;
        }
    }

    fn remove_wire_end_at(&mut self, at: Pt, id: WireId, comp_id: CompId) -> bool {
        let slot = self.points.get_mut(&at).expect("wire end missing");
        let pos = slot
            .wires
            .iter()
            .position(|&w| w == id)
            .expect("wire missing from endpoint slot");
        slot.wires.swap_remove(pos);
        if slot.is_empty() {
            let removed = self.points.remove(&at).unwrap();
            self.remove_point_from_comp(comp_id, removed.comp_idx);
            false
        } else {
            true
        }
    }

    fn remove_comp_from_net(&mut self, comp_id: CompId, net_id: NetId) {
        let comp_net_idx = self.comps[comp_id.0 as usize].as_ref().unwrap().net_idx;
        let net = self.nets[net_id.0 as usize].as_mut().unwrap();
        let idx = comp_net_idx as usize;
        net.comps.swap_remove(idx);
        if let Some(&moved_comp) = net.comps.get(idx) {
            self.comps[moved_comp.0 as usize].as_mut().unwrap().net_idx = comp_net_idx;
        }
    }

    fn detach_and_free_empty_comp(&mut self, comp_id: CompId, net_id: NetId) {
        self.remove_comp_from_net(comp_id, net_id);
        self.free_comp(comp_id);
        if self.nets[net_id.0 as usize]
            .as_ref()
            .unwrap()
            .comps
            .is_empty()
        {
            debug_assert!(
                self.nets[net_id.0 as usize]
                    .as_ref()
                    .unwrap()
                    .label_counts
                    .is_empty()
            );
            self.free_net(net_id);
        }
    }

    fn detach_comp_to_new_net(&mut self, comp_id: CompId, old_net: NetId) {
        self.remove_comp_from_net(comp_id, old_net);
        let new_net = self.alloc_net(Net {
            comps: vec![comp_id],
            label_counts: LabelCounts::new(),
        });
        let comp = self.comps[comp_id.0 as usize].as_mut().unwrap();
        comp.net = new_net;
        comp.net_idx = 0;
    }

    fn union_comps(&mut self, ca: CompId, cb: CompId) -> CompId {
        let size_a = {
            let a = self.comps[ca.0 as usize].as_ref().unwrap();
            a.points.len() + a.wires.len()
        };
        let size_b = {
            let b = self.comps[cb.0 as usize].as_ref().unwrap();
            b.points.len() + b.wires.len()
        };
        let (keep, drop) = if size_a >= size_b { (ca, cb) } else { (cb, ca) };

        let net_drop = self.comps[drop.0 as usize].as_ref().unwrap().net;
        self.remove_comp_from_net(drop, net_drop);
        let drop_comp = self.free_comp(drop);
        let net_keep = self.comps[keep.0 as usize].as_ref().unwrap().net;

        {
            let keep_comp = self.comps[keep.0 as usize].as_mut().unwrap();
            let base_pts = keep_comp.points.len();
            for (offset, &pt) in drop_comp.points.iter().enumerate() {
                let slot = self.points.get_mut(&pt).unwrap();
                slot.comp = keep;
                slot.comp_idx = u32::try_from(base_pts + offset).expect("point count overflow");
            }
            keep_comp.points.extend(drop_comp.points);

            let base_wires = keep_comp.wires.len();
            for (offset, &wid) in drop_comp.wires.iter().enumerate() {
                let w = self.wires[wid.0 as usize].as_mut().unwrap();
                w.comp = keep;
                w.comp_idx = u32::try_from(base_wires + offset).expect("wire count overflow");
            }
            keep_comp.wires.extend(drop_comp.wires);

            for (text, count) in drop_comp.label_counts {
                inc_label_count(&mut keep_comp.label_counts, &text, count);
            }
        }

        if net_keep != net_drop {
            self.merge_nets(net_keep, net_drop);
        }

        keep
    }

    fn merge_nets(&mut self, na: NetId, nb: NetId) -> NetId {
        if na == nb {
            return na;
        }
        let len_a = self.nets[na.0 as usize].as_ref().unwrap().comps.len();
        let len_b = self.nets[nb.0 as usize].as_ref().unwrap().comps.len();
        let (keep, drop) = if len_a >= len_b { (na, nb) } else { (nb, na) };

        let drop_net = self.free_net(drop);
        let keep_net = self.nets[keep.0 as usize].as_mut().unwrap();
        let base = keep_net.comps.len();
        for (offset, &cid) in drop_net.comps.iter().enumerate() {
            let comp = self.comps[cid.0 as usize].as_mut().unwrap();
            comp.net = keep;
            comp.net_idx = u32::try_from(base + offset).expect("net comp overflow");
        }
        keep_net.comps.extend(drop_net.comps);

        for (text, count) in drop_net.label_counts {
            inc_label_count(&mut keep_net.label_counts, &text, count);
            self.label_nets.insert(text, keep);
        }

        keep
    }

    fn next_visit_gen(&mut self) -> u32 {
        let next = self.visit_gen.wrapping_add(1);
        if next == 0 {
            for slot in self.points.values_mut() {
                slot.visited = 0;
            }
            self.visit_gen = 1;
            1
        } else {
            self.visit_gen = next;
            next
        }
    }

    fn flood_from(
        &mut self,
        start: Pt,
        stamp: u32,
        target_comp: CompId,
    ) -> (Vec<Pt>, Vec<WireId>, LabelCounts) {
        let mut pts = Vec::new();
        let mut wires = Vec::new();
        let mut label_counts = LabelCounts::new();

        self.points.get_mut(&start).unwrap().visited = stamp;
        self.bfs_queue.clear();
        self.bfs_queue.push_back(start);

        while let Some(p) = self.bfs_queue.pop_front() {
            let pt_idx = u32::try_from(pts.len()).expect("point count overflow");
            pts.push(p);

            let slot = self.points.get_mut(&p).unwrap();
            slot.comp = target_comp;
            slot.comp_idx = pt_idx;

            for &lid in &slot.labels {
                let text = &self.labels[lid.0 as usize].as_ref().unwrap().text;
                inc_label_count(&mut label_counts, text, 1);
            }

            let mut neighbors: SmallVec<[Pt; 4]> = SmallVec::new();
            for &wid in &slot.wires {
                let w = self.wires[wid.0 as usize].as_mut().unwrap();
                if w.wire.a == p {
                    let w_idx = u32::try_from(wires.len()).expect("wire count overflow");
                    wires.push(wid);
                    w.comp = target_comp;
                    w.comp_idx = w_idx;
                }
                let other = if w.wire.a == p { w.wire.b } else { w.wire.a };
                if other != p {
                    neighbors.push(other);
                }
            }

            for other in neighbors {
                let neighbor_slot = self.points.get_mut(&other).unwrap();
                if neighbor_slot.visited != stamp {
                    neighbor_slot.visited = stamp;
                    self.bfs_queue.push_back(other);
                }
            }
        }

        (pts, wires, label_counts)
    }

    fn re_flood_net(&mut self, net_id: NetId) {
        let all_comps = mem::take(&mut self.nets[net_id.0 as usize].as_mut().unwrap().comps);
        self.nets[net_id.0 as usize]
            .as_mut()
            .unwrap()
            .label_counts
            .clear();

        if all_comps.len() <= 1 {
            let mut label_counts = LabelCounts::new();
            if let Some(&cid) = all_comps.first() {
                let comp = self.comps[cid.0 as usize].as_mut().unwrap();
                comp.net = net_id;
                comp.net_idx = 0;
                for (text, count) in &comp.label_counts {
                    inc_label_count(&mut label_counts, text, *count);
                    self.label_nets.insert(Arc::clone(text), net_id);
                }
            }
            let net = self.nets[net_id.0 as usize].as_mut().unwrap();
            net.comps = all_comps;
            net.label_counts = label_counts;
            return;
        }

        // Build bipartite adjacency between `0..all_comps.len()` and `0..label_texts.len()`.
        let n = all_comps.len();
        let mut label_ids: FxHashMap<Arc<str>, usize> = FxHashMap::default();
        let mut label_texts: Vec<Arc<str>> = Vec::new();
        let mut label_adj: Vec<Vec<usize>> = Vec::new();
        let mut comp_adj: Vec<SmallVec<[usize; 2]>> = vec![SmallVec::new(); n];

        for (ci, &cid) in all_comps.iter().enumerate() {
            let comp = self.comps[cid.0 as usize].as_ref().unwrap();
            for (text, _) in &comp.label_counts {
                let li = if let Some(&idx) = label_ids.get(text) {
                    idx
                } else {
                    let idx = label_texts.len();
                    label_ids.insert(Arc::clone(text), idx);
                    label_texts.push(Arc::clone(text));
                    label_adj.push(Vec::new());
                    idx
                };
                label_adj[li].push(ci);
                comp_adj[ci].push(li);
            }
        }

        let mut visited_comp = vec![false; n];
        let mut visited_label = vec![false; label_texts.len()];
        let mut queue = VecDeque::new();
        let mut first_group = true;

        for start in 0..n {
            if visited_comp[start] {
                continue;
            }
            visited_comp[start] = true;
            queue.clear();
            queue.push_back(start);

            let mut group_comps = Vec::new();
            let mut group_labels = LabelCounts::new();

            while let Some(ci) = queue.pop_front() {
                let cid = all_comps[ci];
                group_comps.push(cid);
                let comp = self.comps[cid.0 as usize].as_ref().unwrap();
                for (text, count) in &comp.label_counts {
                    inc_label_count(&mut group_labels, text, *count);
                }
                for &li in &comp_adj[ci] {
                    if !visited_label[li] {
                        visited_label[li] = true;
                        for &other_ci in &label_adj[li] {
                            if !visited_comp[other_ci] {
                                visited_comp[other_ci] = true;
                                queue.push_back(other_ci);
                            }
                        }
                    }
                }
            }

            let target_net = if first_group {
                first_group = false;
                net_id
            } else {
                self.alloc_net(Net {
                    comps: Vec::new(),
                    label_counts: LabelCounts::new(),
                })
            };

            for (idx, &cid) in group_comps.iter().enumerate() {
                let comp = self.comps[cid.0 as usize].as_mut().unwrap();
                comp.net = target_net;
                comp.net_idx = u32::try_from(idx).expect("net comp overflow");
            }
            for (text, _) in &group_labels {
                self.label_nets.insert(Arc::clone(text), target_net);
            }

            let net = self.nets[target_net.0 as usize].as_mut().unwrap();
            net.comps = group_comps;
            net.label_counts = group_labels;
        }
    }
}

/// Increments `text` in `counts` by `delta`. Returns `true` if `text` was newly inserted.
fn inc_label_count(counts: &mut LabelCounts, text: &Arc<str>, delta: u32) -> bool {
    for (existing, count) in counts.iter_mut() {
        if existing == text {
            *count += delta;
            return false;
        }
    }
    counts.push((Arc::clone(text), delta));
    true
}

/// Decrements `text` in `counts` by 1. Returns `true` if `text`'s count hit 0 and was removed.
fn dec_label_count(counts: &mut LabelCounts, text: &Arc<str>) -> bool {
    if let Some(pos) = counts.iter().position(|(existing, _)| existing == text) {
        if counts[pos].1 > 1 {
            counts[pos].1 -= 1;
            false
        } else {
            counts.swap_remove(pos);
            true
        }
    } else {
        false
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::{BTreeMap, BTreeSet};

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x, y }
    }

    fn w(ax: i32, ay: i32, bx: i32, by: i32) -> Wire {
        Wire {
            a: pt(ax, ay),
            b: pt(bx, by),
        }
    }

    fn lbl(x: i32, y: i32, text: &str) -> Label {
        Label {
            at: pt(x, y),
            text: Arc::from(text),
        }
    }

    pub(crate) fn assert_invariants(c: &Connectivity) {
        let mut points = BTreeSet::new();
        let mut wires = BTreeSet::new();
        let mut comps = BTreeSet::new();
        for (ni, net) in c
            .nets
            .iter()
            .enumerate()
            .filter_map(|(i, n)| n.as_ref().map(|n| (i, n)))
        {
            let mut counts = LabelCounts::new();
            for (ci, &cid) in net.comps.iter().enumerate() {
                assert!(comps.insert(cid.0));
                let comp = c.comps[cid.0 as usize].as_ref().unwrap();
                assert_eq!((comp.net, comp.net_idx), (NetId(ni as u32), ci as u32));
                let mut local = LabelCounts::new();
                for (pi, &at) in comp.points.iter().enumerate() {
                    assert!(points.insert((at.x, at.y)));
                    let slot = &c.points[&at];
                    assert!(!slot.is_empty());
                    assert_eq!((slot.comp, slot.comp_idx), (cid, pi as u32));
                    for &id in &slot.wires {
                        let w = &c.wires[id.0 as usize].as_ref().unwrap().wire;
                        assert!(at == w.a || at == w.b);
                    }
                    for &id in &slot.labels {
                        let l = c.labels[id.0 as usize].as_ref().unwrap();
                        assert_eq!(at, l.at);
                        inc_label_count(&mut local, &l.text, 1);
                    }
                    for &(id, pin) in &slot.pins {
                        assert_eq!(c.parts[id.0 as usize].as_ref().unwrap()[pin as usize], at);
                    }
                }
                local.sort();
                let mut expected = comp.label_counts.clone();
                expected.sort();
                assert_eq!(local, expected);
                for (text, n) in &local {
                    inc_label_count(&mut counts, text, *n);
                }
                for (wi, &id) in comp.wires.iter().enumerate() {
                    assert!(wires.insert(id.0));
                    let w = c.wires[id.0 as usize].as_ref().unwrap();
                    assert_eq!((w.comp, w.comp_idx), (cid, wi as u32));
                    for at in [w.wire.a, w.wire.b] {
                        assert!(c.points[&at].wires.contains(&id));
                    }
                }
            }
            counts.sort();
            let mut expected = net.label_counts.clone();
            expected.sort();
            assert_eq!(counts, expected);
            for (text, _) in &counts {
                assert_eq!(c.label_nets.get(text), Some(&NetId(ni as u32)));
            }
        }
        assert_eq!(points.len(), c.points.len());
        assert_eq!(wires.len(), c.wires.iter().flatten().count());
        assert_eq!(comps.len(), c.comps.iter().flatten().count());
        assert_eq!(c.live_nets, c.nets.iter().flatten().count());
        for (i, l) in c
            .labels
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.as_ref().map(|l| (i, l)))
        {
            assert!(c.points[&l.at].labels.contains(&LabelId(i as u32)));
        }
        for (i, pins) in c
            .parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().map(|p| (i, p)))
        {
            for (pin, at) in pins.iter().enumerate() {
                assert!(c.points[at].pins.contains(&(PartId(i as u32), pin as u32)));
            }
        }
        assert_eq!(
            c.free_comps.iter().copied().collect::<BTreeSet<_>>(),
            c.comps
                .iter()
                .enumerate()
                .filter_map(|(i, c)| c.is_none().then_some(i as u32))
                .collect()
        );
        assert_eq!(
            c.free_nets.iter().copied().collect::<BTreeSet<_>>(),
            c.nets
                .iter()
                .enumerate()
                .filter_map(|(i, n)| n.is_none().then_some(i as u32))
                .collect()
        );
    }

    pub(crate) fn assert_partitions(a: &Connectivity, b: &Connectivity) {
        assert_invariants(a);
        assert_invariants(b);
        assert_eq!(a.net_count(), b.net_count());
        let mut forward = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        assert_eq!(a.points.len(), b.points.len());
        let mut cf = BTreeMap::new();
        let mut cr = BTreeMap::new();
        for &at in a.points.keys() {
            let ac = a.points[&at].comp.0;
            let bc = b.points[&at].comp.0;
            assert_eq!(*cf.entry(ac).or_insert(bc), bc);
            assert_eq!(*cr.entry(bc).or_insert(ac), ac);
            let x = a.net_at(at).unwrap();
            let y = b.net_at(at).unwrap();
            assert_eq!(*forward.entry(x).or_insert(y), y);
            assert_eq!(*reverse.entry(y).or_insert(x), x);
        }
    }

    /// Compares two `Connectivity` instances up to `NetId` isomorphism:
    /// - every live wire maps to a valid `NetId`, dead wires return `None`
    /// - the partition of live wires into nets and each net's label set match identically
    /// - `net_at` agrees on every live point
    fn assert_connectivity_eq(
        inc: &Connectivity,
        oracle: &Connectivity,
        all_wires: &[WireId],
        live_wires: &BTreeMap<u32, Wire>,
        live_labels: &BTreeMap<u32, Label>,
    ) {
        assert_partitions(inc, oracle);
        assert_eq!(inc.net_count(), oracle.net_count(), "net_count mismatch");

        let mut inc_buf = Vec::new();
        let mut orc_buf = Vec::new();
        let mut inc_lbls = Vec::new();
        let mut orc_lbls = Vec::new();

        for &wid in all_wires {
            let is_live = live_wires.contains_key(&wid.0);
            let inc_net = inc.net_of(wid);
            let orc_net = oracle.net_of(wid);
            assert_eq!(inc_net.is_some(), is_live, "live mismatch for {wid:?}");
            assert_eq!(orc_net.is_some(), is_live, "oracle mismatch for {wid:?}");
            if let (Some(ni), Some(no)) = (inc_net, orc_net) {
                inc.net_wires(ni, &mut inc_buf);
                oracle.net_wires(no, &mut orc_buf);
                inc_buf.sort_unstable_by_key(|id| id.0);
                orc_buf.sort_unstable_by_key(|id| id.0);
                // No duplicates in net_wires.
                let mut dedup_inc = inc_buf.clone();
                dedup_inc.dedup();
                assert_eq!(dedup_inc.len(), inc_buf.len(), "duplicate wires in net");
                assert_eq!(inc_buf, orc_buf, "net_wires mismatch for {wid:?}");

                inc.net_labels(ni, &mut inc_lbls);
                oracle.net_labels(no, &mut orc_lbls);
                inc_lbls.sort_unstable();
                orc_lbls.sort_unstable();
                assert_eq!(inc_lbls, orc_lbls, "net_labels mismatch for {wid:?}");
            }
        }

        // Check `net_at` equivalence at every label and wire endpoint.
        let mut all_pts = BTreeSet::new();
        for wire in live_wires.values() {
            all_pts.insert((wire.a.x, wire.a.y));
            all_pts.insert((wire.b.x, wire.b.y));
        }
        for label in live_labels.values() {
            all_pts.insert((label.at.x, label.at.y));
        }
        for (x, y) in all_pts {
            let p = pt(x, y);
            let ni = inc.net_at(p).expect("live point missing net_at in inc");
            let no = oracle
                .net_at(p)
                .expect("live point missing net_at in oracle");
            inc.net_wires(ni, &mut inc_buf);
            oracle.net_wires(no, &mut orc_buf);
            inc_buf.sort_unstable_by_key(|id| id.0);
            orc_buf.sort_unstable_by_key(|id| id.0);
            assert_eq!(inc_buf, orc_buf, "net_at wires mismatch at {p:?}");

            inc.net_labels(ni, &mut inc_lbls);
            oracle.net_labels(no, &mut orc_lbls);
            inc_lbls.sort_unstable();
            orc_lbls.sort_unstable();
            assert_eq!(inc_lbls, orc_lbls, "net_at labels mismatch at {p:?}");
        }
    }

    #[test]
    #[should_panic(expected = "invalid wire")]
    fn rejects_zero_length_wire() {
        Connectivity::new().add_wire(WireId(0), w(0, 0, 0, 0));
    }

    #[test]
    fn crossing_wires_do_not_connect_and_cycle_remove_stays_connected() {
        let mut c = Connectivity::new();
        let w0 = WireId(0);
        let w1 = WireId(1);
        c.add_wire(w0, w(-100, 0, 100, 0));
        c.add_wire(w1, w(0, -100, 0, 100));
        assert_ne!(c.net_of(w0), c.net_of(w1));

        // Build a 4-wire loop around (0,0)-(10,10); removing one wire keeps all remaining 3 connected.
        let l0 = WireId(2);
        let l1 = WireId(3);
        let l2 = WireId(4);
        let l3 = WireId(5);
        c.add_wire(l0, w(0, 0, 10, 0));
        c.add_wire(l1, w(10, 0, 10, 10));
        c.add_wire(l2, w(10, 10, 0, 10));
        c.add_wire(l3, w(0, 10, 0, 0));
        assert_eq!(c.net_of(l0), c.net_of(l1));
        assert_eq!(c.net_of(l1), c.net_of(l2));
        assert_eq!(c.net_of(l2), c.net_of(l3));
        assert_ne!(c.net_of(l0), c.net_of(w0));
        assert_ne!(c.net_of(l0), c.net_of(w1));

        c.remove_wire(l1);
        assert_eq!(c.net_of(l1), None);
        assert_eq!(c.net_of(l0), c.net_of(l2));
        assert_eq!(c.net_of(l2), c.net_of(l3));

        // Removing the opposite wire splits the chain into (l0) and (l2).
        c.remove_wire(l3);
        assert_ne!(c.net_of(l0), c.net_of(l2));
        let mut buf = Vec::new();
        c.net_wires(c.net_of(l0).unwrap(), &mut buf);
        assert_eq!(buf, vec![l0]);
        c.net_wires(c.net_of(l2).unwrap(), &mut buf);
        assert_eq!(buf, vec![l2]);
    }

    #[test]
    fn missing_ids_duplicate_ids_and_parallel_wires() {
        let mut c = Connectivity::new();
        assert_eq!(c.remove_wire(WireId(99)), None);
        assert_eq!(c.remove_label(LabelId(99)), None);
        assert_eq!(c.net_of(WireId(99)), None);
        assert_eq!(c.net_at(pt(0, 0)), None);

        let mut buf = vec![WireId(1)];
        c.net_wires(NetId(99), &mut buf);
        assert!(buf.is_empty());

        // Parallel wires between same endpoints stay connected when one is removed.
        c.add_wire(WireId(0), w(0, 0, 10, 0));
        c.add_wire(WireId(1), w(0, 0, 10, 0));
        assert_eq!(c.net_of(WireId(0)), c.net_of(WireId(1)));
        c.remove_wire(WireId(0));
        assert_eq!(c.net_of(WireId(0)), None);
        assert!(c.net_of(WireId(1)).is_some());

        // Re-adding an existing WireId replaces it cleanly.
        c.add_wire(WireId(1), w(100, 0, 110, 0));
        assert_eq!(c.net_at(pt(0, 0)), None);
        assert!(c.net_at(pt(100, 0)).is_some());

        c.clear();
        assert_eq!(c.net_count(), 0);
        assert_eq!(c.net_of(WireId(1)), None);
    }

    #[test]
    fn net_labels_join_and_split_nets_including_shorted_multi_labels() {
        let mut c = Connectivity::new();
        let w0 = WireId(0);
        let w1 = WireId(1);
        let w2 = WireId(2);
        c.add_wire(w0, w(0, 0, 10, 0));
        c.add_wire(w1, w(100, 0, 110, 0));
        c.add_wire(w2, w(200, 0, 210, 0));
        assert_ne!(c.net_of(w0), c.net_of(w1));

        // Two unconnected wires given the same label share a NetId.
        let la = LabelId(0);
        let lb = LabelId(1);
        c.add_label(la, &lbl(0, 0, "VCC"));
        c.add_label(lb, &lbl(100, 0, "VCC"));
        assert_eq!(c.net_of(w0), c.net_of(w1));

        // Removing one label separates them again.
        c.remove_label(la);
        assert_ne!(c.net_of(w0), c.net_of(w1));

        // Multi-label bridge: w0 gets "A", w1 gets "B", w2 gets both "A" and "B" via bridge wire.
        c.add_label(LabelId(2), &lbl(0, 0, "A"));
        c.add_label(LabelId(3), &lbl(200, 0, "A"));
        let l_bridge = LabelId(4);
        c.add_label(l_bridge, &lbl(210, 0, "B"));
        c.add_label(LabelId(5), &lbl(100, 0, "B"));
        assert_eq!(c.net_of(w0), c.net_of(w1));
        assert_eq!(c.net_of(w0), c.net_of(w2));

        // Removing l_bridge disconnects w1 ("B") from {w0, w2} ("A").
        c.remove_label(l_bridge);
        assert_eq!(c.net_of(w0), c.net_of(w2));
        assert_ne!(c.net_of(w0), c.net_of(w1));
    }

    #[test]
    fn label_before_wire_and_split_leaving_labels_at_both_ends() {
        let mut c = Connectivity::new();
        c.add_label(LabelId(0), &lbl(0, 0, "N1"));
        c.add_label(LabelId(1), &lbl(10, 0, "N2"));
        c.add_label(LabelId(2), &lbl(50, 0, "N2"));
        c.add_wire(WireId(0), w(0, 0, 10, 0));
        c.add_wire(WireId(1), w(50, 0, 60, 0));
        assert_eq!(c.net_of(WireId(0)), c.net_of(WireId(1)));

        // Removing WireId(0) leaves (0,0) with "N1" and (10,0) with "N2" in separate nets.
        c.remove_wire(WireId(0));
        assert_ne!(c.net_at(pt(0, 0)), c.net_at(pt(10, 0)));
        assert_eq!(c.net_at(pt(10, 0)), c.net_of(WireId(1)));
    }

    #[test]
    fn visit_gen_wraparound_works() {
        let mut c = Connectivity::new();
        c.add_wire(WireId(0), w(0, 0, 10, 0));
        c.add_wire(WireId(1), w(10, 0, 10, 10));
        c.add_wire(WireId(2), w(10, 10, 0, 10));
        c.add_wire(WireId(3), w(0, 10, 0, 0));
        c.visit_gen = u32::MAX - 1;
        // Cycle remove hits BFS with gen = u32::MAX, then next wraps to 1.
        c.remove_wire(WireId(3));
        assert_eq!(c.visit_gen, u32::MAX);
        c.remove_wire(WireId(1));
        assert_eq!(c.visit_gen, 1);
        assert_ne!(c.net_of(WireId(0)), c.net_of(WireId(2)));
    }

    #[test]
    fn net_wires_allocates_nothing_with_capacity() {
        let mut c = Connectivity::new();
        for i in 0..50 {
            c.add_wire(WireId(i), w(i as i32 * 10, 0, (i + 1) as i32 * 10, 0));
        }
        let net = c.net_of(WireId(0)).unwrap();
        let mut out = Vec::with_capacity(64);
        let ptr_before = out.as_ptr();
        let cap_before = out.capacity();
        c.net_wires(net, &mut out);
        assert_eq!(out.len(), 50);
        assert_eq!(out.as_ptr(), ptr_before);
        assert_eq!(out.capacity(), cap_before);
    }

    #[derive(Debug, Clone)]
    enum Edit {
        AddWire {
            ax: i32,
            ay: i32,
            len: i32,
            horiz: bool,
        },
        RemoveWire(usize),
        AddLabel {
            x: i32,
            y: i32,
            name: u8,
        },
        RemoveLabel(usize),
    }

    fn edit_strategy() -> impl Strategy<Value = Edit> {
        prop_oneof![
            4 => (-4i32..=4, -4i32..=4, 1i32..=3, any::<bool>()).prop_map(
                |(ax, ay, len, horiz)| Edit::AddWire { ax, ay, len, horiz }
            ),
            2 => (0usize..128).prop_map(Edit::RemoveWire),
            2 => (-4i32..=4, -4i32..=4, 0u8..5).prop_map(|(x, y, name)| Edit::AddLabel {
                x,
                y,
                name,
            }),
            1 => (0usize..128).prop_map(Edit::RemoveLabel),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// INV10: after every add/remove step, incremental connectivity equals a from-scratch rebuild.
        #[test]
        fn inv10_incremental_equals_rebuild_over_random_sequences(
            edits in prop::collection::vec(edit_strategy(), 1..200)
        ) {
            let mut inc = Connectivity::new();
            let mut live_wires: BTreeMap<u32, Wire> = BTreeMap::new();
            let mut live_wire_ids: Vec<u32> = Vec::new();
            let mut all_wires: Vec<WireId> = Vec::new();
            let mut live_labels: BTreeMap<u32, Label> = BTreeMap::new();
            let mut live_label_ids: Vec<u32> = Vec::new();
            let mut next_wire_id = 0u32;
            let mut next_label_id = 0u32;
            let mut parts = BTreeMap::<u32,Vec<Pt>>::new();
            let names = ["VCC", "GND", "CLK", "SDA", "SCL"];

            for (step, edit) in edits.into_iter().enumerate() {
                let id=PartId((step % 12) as u32);
                if step % 3 == 0 { inc.remove_part(id); parts.remove(&id.0); } else { let pins=vec![pt((step%7) as i32,0),pt(0,(step%5) as i32)]; inc.add_part(id,&pins); parts.insert(id.0,pins); }
                match edit {
                    Edit::AddWire { ax, ay, len, horiz } => {
                        let a = pt(ax, ay);
                        let b = if horiz { pt(ax + len, ay) } else { pt(ax, ay + len) };
                        let wire = Wire { a, b };
                        let id = WireId(next_wire_id % 16);
                        next_wire_id += 1;
                        inc.add_wire(id, wire);
                        live_wires.insert(id.0, wire);
                        if !live_wire_ids.contains(&id.0) {live_wire_ids.push(id.0);}
                        if !all_wires.contains(&id) {all_wires.push(id);}
                    }
                    Edit::RemoveWire(pick) => {
                        if !live_wire_ids.is_empty() {
                            let idx = pick % live_wire_ids.len();
                            let raw = live_wire_ids.swap_remove(idx);
                            let expected = live_wires.remove(&raw);
                            assert_eq!(inc.remove_wire(WireId(raw)), expected);
                        }
                    }
                    Edit::AddLabel { x, y, name } => {
                        let label = lbl(x, y, names[name as usize]);
                        let id = LabelId(next_label_id % 16);
                        next_label_id += 1;
                        inc.add_label(id, &label);
                        live_labels.insert(id.0, label);
                        if !live_label_ids.contains(&id.0) {live_label_ids.push(id.0);}
                    }
                    Edit::RemoveLabel(pick) => {
                        if !live_label_ids.is_empty() {
                            let idx = pick % live_label_ids.len();
                            let raw = live_label_ids.swap_remove(idx);
                            let expected = live_labels.remove(&raw);
                            assert_eq!(inc.remove_label(LabelId(raw)), expected);
                        }
                    }
                }

                let oracle = Connectivity::rebuild_with_parts(
                    live_wires.iter().map(|(&id, &w)| (WireId(id), w)),
                    live_labels.iter().map(|(&id, l)| (LabelId(id), l)),
                    parts.iter().map(|(&id,pins)|(PartId(id),pins)),
                );
                assert_connectivity_eq(&inc, &oracle, &all_wires, &live_wires, &live_labels);
            }
        }
    }
}
