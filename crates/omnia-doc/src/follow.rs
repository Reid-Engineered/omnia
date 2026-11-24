//! Pure WD2/WD3 follow planning. Commands consume this plan in W2.2.
use rustc_hash::FxHashMap;

use crate::{
    Anchor, Boundary, Doc, ItemId, LabelId, Orient, PartId, Pt, Rect, Run, Wire, WireId,
    index::{CELLS_PER_AXIS, cell_of},
    points::End,
    prefs::FollowPolicy,
    route::{
        Axis, Blocker, LocalObstacles, RouteScratch, RouteSheet, SegmentHit, check_new_wires_on,
        route_on,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isometry {
    Translate(Pt),
    Reorient { pivot: Pt, by: Orient },
}
impl Isometry {
    pub fn apply(self, p: Pt) -> Result<Pt, crate::GeomError> {
        match self {
            Self::Translate(d) => p.checked_add(d),
            Self::Reorient { pivot, by } => by.checked_apply_around(p, pivot),
        }
    }
    fn wire(self, w: Wire) -> Result<Wire, crate::GeomError> {
        Ok(Wire {
            a: self.apply(w.a)?,
            b: self.apply(w.b)?,
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFate {
    Translated,
    Rerouted,
    Detached,
}
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FollowPlan {
    pub remove: Vec<WireId>,
    pub add: Vec<Wire>,
    pub labels: Vec<(LabelId, Pt)>,
    pub fates: Vec<(Pt, RunFate)>,
}
impl FollowPlan {
    fn clear(&mut self) {
        self.remove.clear();
        self.add.clear();
        self.labels.clear();
        self.fates.clear();
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowError {
    NoRoute { from: Pt, to: Pt, blocker: Blocker },
}
impl std::fmt::Display for FollowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "follow refused: {self:?}")
    }
}
impl std::error::Error for FollowError {}

// Scan only selected incidences before allocating Boundary's selected-wire list.
fn has_boundary(doc: &Doc, items: &[ItemId]) -> bool {
    let any_wire = items.iter().any(|item| matches!(item, ItemId::Wire(_)));
    let meets = |p| {
        doc.points
            .at(p)
            .iter()
            .any(|e| matches!(e, End::Wire(w) if !any_wire || !items.contains(&ItemId::Wire(w))))
    };
    items.iter().any(|item| match *item {
        ItemId::Part(id) => doc
            .pins(id)
            .is_some_and(|pins| pins.iter().copied().any(meets)),
        ItemId::Wire(id) => doc.wire(id).is_some_and(|w| meets(w.a) || meets(w.b)),
        ItemId::Label(_) => false,
    })
}

fn allows_on(v: &View<'_>, run: &Run, policy: FollowPolicy) -> bool {
    [run.a, run.b]
        .into_iter()
        .filter(|(_, a)| *a == Anchor::Boundary)
        .all(|(p, _)| {
            let mut part_found = false;
            for e in v.doc.points.at(p) {
                if let End::Pin { part, .. } = e
                    && let Some(moved) = v.moved_part(part)
                {
                    part_found = true;
                    if !policy.part_follows(usize::from(moved.connected_pins)) {
                        return false;
                    }
                }
            }
            // WD3: the wire policy is the fallback for a boundary without a moved part.
            part_found || policy.wire_follows()
        })
}

/// A selected part in its final position.
#[derive(Debug)]
struct MovedPart {
    id: PartId,
    /// Its pins in [`View::pins`].
    pins: std::ops::Range<usize>,
    body: Rect,
    /// Body and pins, as the spatial index registers a part.
    envelope: Rect,
    /// Cached `doc.connected_pins(id)` for O(1) policy checks.
    connected_pins: u16,
}

#[derive(Clone, Copy)]
enum ViewEntry {
    /// Index into `View::parts`.
    MovedPart { idx: u32, envelope: Rect },
    /// Moved selected wire or newly planned wire, stored inline to avoid lookups.
    Wire {
        id: WireId,
        wire: Wire,
        bounds: Rect,
    },
}

#[derive(Clone, Copy)]
struct CellNode {
    entry: ViewEntry,
    next: u32,
}

#[inline]
fn set_bit(bits: &mut [u64], id: u32) {
    let idx = id as usize;
    bits[idx >> 6] |= 1u64 << (idx & 63);
}

#[inline]
fn has_bit(bits: &[u64], id: u32) -> bool {
    let idx = id as usize;
    bits.get(idx >> 6)
        .is_some_and(|&word| (word & (1u64 << (idx & 63))) != 0)
}

/// The sheet in its final geometry, as WD4 sees it while the runs are planned: the document
/// with the selection transformed, the replaced runs removed, the runs' end reservations and
/// the wires planned so far. It is layered over the caller's document, which it never
/// changes, so building it costs the selection and the plan, not the sheet. Labels play no
/// part in WD4 and are left out.
///
/// Identities are those of a copy of the document with every slot kept, holes included:
/// existing items keep their ids, reservations take part ids after the document's part
/// slots and planned wires take wire ids after its wire slots, both in insertion order.
struct View<'a> {
    doc: &'a Doc,
    base_parts: u32,
    base_wires: u32,
    /// Live selected parts in their final position, by id.
    parts: Vec<MovedPart>,
    /// Compact O(1) membership bitset for `parts`.
    moved_part_bits: Vec<u64>,
    pins: Vec<Pt>,
    /// Live selected wires, by id.
    selected: Vec<WireId>,
    /// Selected wires outside the replaced runs, in their final position, by id.
    moved: Vec<(WireId, Wire)>,
    /// Wires of the replaced (not detached) runs, by id.
    hidden: Vec<WireId>,
    /// Compact O(1) membership bitset for `selected ∪ hidden`.
    excluded_wire_bits: Vec<u64>,
    /// Zero-area reservation parts with one pin each, at these points.
    markers: Vec<Pt>,
    /// Reservations sorted by `(y, x, id.0)` when `res_sorted` is true.
    res_yx: Vec<(Pt, PartId)>,
    /// Reservations sorted by `(x, y, id.0)` when `res_sorted` is true.
    res_xy: Vec<(Pt, PartId)>,
    res_sorted: bool,
    /// Planned wires and the first wire of the run each replaces.
    added: Vec<(Wire, WireId)>,
    /// Flat arena for the view's own parts and wires by K12 cell (no per-cell Vecs,
    /// and reservations stay out of cells so body/crossing/overlap checks never see them).
    cell_heads: FxHashMap<u32, u32>,
    cell_nodes: Vec<CellNode>,
}

impl<'a> View<'a> {
    /// The selection's parts in their final position. Every failure is [`Blocker::Sheet`].
    fn new(doc: &'a Doc, items: &[ItemId], t: Isometry) -> Result<Self, Blocker> {
        let mut ids: Vec<PartId> = items
            .iter()
            .filter_map(|item| match *item {
                ItemId::Part(id) if doc.parts.get(id.0).is_some() => Some(id),
                _ => None,
            })
            .collect();
        ids.sort_unstable_by_key(|id| id.0);
        ids.dedup();
        let mut selected: Vec<WireId> = items
            .iter()
            .filter_map(|item| match *item {
                ItemId::Wire(id) if doc.wires.get(id.0).is_some() => Some(id),
                _ => None,
            })
            .collect();
        selected.sort_unstable_by_key(|id| id.0);
        selected.dedup();
        let mut moved_part_bits = ids
            .last()
            .map_or_else(Vec::new, |last| vec![0u64; (last.0 as usize >> 6) + 1]);
        let mut parts = Vec::with_capacity(ids.len());
        let mut pins = Vec::new();
        for id in ids {
            let stored = doc.parts.get(id.0).expect("live selected part");
            let at = t.apply(stored.part.at).map_err(|_| Blocker::Sheet)?;
            let orient = match t {
                Isometry::Reorient { by, .. } => stored.part.orient.then(by),
                Isometry::Translate(_) => stored.part.orient,
            };
            let first = pins.len();
            for &p in &stored.pins {
                pins.push(t.apply(p).map_err(|_| Blocker::Sheet)?);
            }
            let moved = &pins[first..];
            if !at.is_in_bounds() || moved.iter().any(|p| !p.is_in_bounds()) {
                return Err(Blocker::Sheet);
            }
            let envelope = Doc::part_envelope_raw(at, orient, stored.local_bounds, moved)
                .map_err(|_| Blocker::Sheet)?;
            let body =
                Doc::part_bounds_at(at, orient, stored.local_bounds).map_err(|_| Blocker::Sheet)?;
            set_bit(&mut moved_part_bits, id.0);
            parts.push(MovedPart {
                id,
                pins: first..pins.len(),
                body,
                envelope,
                connected_pins: doc.connected_pins(id),
            });
        }
        let cell_cap = parts.len().max(64);
        let node_cap = parts.len().saturating_mul(6).max(256);
        Ok(Self {
            doc,
            base_parts: u32::try_from(doc.parts.slots.len()).expect("part id space"),
            base_wires: u32::try_from(doc.wires.slots.len()).expect("wire id space"),
            parts,
            moved_part_bits,
            pins,
            selected,
            moved: Vec::new(),
            hidden: Vec::new(),
            excluded_wire_bits: Vec::new(),
            markers: Vec::new(),
            res_yx: Vec::new(),
            res_xy: Vec::new(),
            res_sorted: true,
            added: Vec::new(),
            cell_heads: FxHashMap::with_capacity_and_hasher(cell_cap, Default::default()),
            cell_nodes: Vec::with_capacity(node_cap),
        })
    }

    /// Starts a planning pass: hides the wires of the runs that are not detached, moves the
    /// remaining selected wires, and drops earlier reservations and planned wires.
    fn reset(&mut self, t: Isometry, runs: &[Run], detached: &[bool]) -> Result<(), Blocker> {
        self.hidden.clear();
        for (run, _) in runs.iter().zip(detached).filter(|(_, d)| !**d) {
            self.hidden.extend_from_slice(&run.wires);
        }
        self.hidden.sort_unstable_by_key(|id| id.0);
        self.hidden.dedup();
        self.excluded_wire_bits.fill(0);
        let max_excluded = self
            .selected
            .last()
            .map(|w| w.0)
            .max(self.hidden.last().map(|w| w.0));
        if let Some(max_id) = max_excluded {
            let words = (max_id as usize >> 6) + 1;
            if self.excluded_wire_bits.len() < words {
                self.excluded_wire_bits.resize(words, 0);
            }
            for &id in &self.selected {
                set_bit(&mut self.excluded_wire_bits, id.0);
            }
            for &id in &self.hidden {
                set_bit(&mut self.excluded_wire_bits, id.0);
            }
        }
        self.moved.clear();
        self.markers.clear();
        self.res_yx.clear();
        self.res_xy.clear();
        self.res_sorted = true;
        self.added.clear();
        self.cell_heads.clear();
        self.cell_nodes.clear();
        for i in 0..self.selected.len() {
            let id = self.selected[i];
            if self.is_hidden(id) {
                continue;
            }
            let w = t
                .wire(*self.doc.wire(id).expect("live selected wire"))
                .map_err(|_| Blocker::Sheet)?;
            Doc::validate_wire(w).map_err(|_| Blocker::Sheet)?;
            self.moved.push((id, w));
        }
        for i in 0..self.parts.len() {
            let envelope = self.parts[i].envelope;
            self.register_entry(
                ViewEntry::MovedPart {
                    idx: i as u32,
                    envelope,
                },
                envelope,
            );
        }
        for i in 0..self.moved.len() {
            let (id, wire) = self.moved[i];
            let bounds = wire_rect(wire);
            self.register_entry(ViewEntry::Wire { id, wire, bounds }, bounds);
        }
        Ok(())
    }

    fn register_entry(&mut self, entry: ViewEntry, area: Rect) {
        let (x0, x1) = (cell_of(area.min.x), cell_of(area.max.x));
        let (y0, y1) = (cell_of(area.min.y), cell_of(area.max.y));
        for cy in y0..=y1 {
            let row = cy * CELLS_PER_AXIS;
            for cx in x0..=x1 {
                let head = self.cell_heads.entry((row + cx) as u32).or_insert(u32::MAX);
                let node_idx = self.cell_nodes.len() as u32;
                self.cell_nodes.push(CellNode { entry, next: *head });
                *head = node_idx;
            }
        }
    }

    fn is_hidden(&self, id: WireId) -> bool {
        self.hidden.binary_search_by_key(&id.0, |w| w.0).is_ok()
    }

    #[inline]
    fn has_moved_part(&self, id: PartId) -> bool {
        has_bit(&self.moved_part_bits, id.0)
    }

    #[inline]
    fn is_excluded_wire(&self, id: WireId) -> bool {
        has_bit(&self.excluded_wire_bits, id.0)
    }

    fn moved_part(&self, id: PartId) -> Option<&MovedPart> {
        if !self.has_moved_part(id) {
            return None;
        }
        self.parts
            .binary_search_by_key(&id.0, |p| p.id.0)
            .ok()
            .map(|i| &self.parts[i])
    }

    /// Whether the document's own entry for `item` is part of the view.
    #[cfg(test)]
    fn keeps(&self, item: ItemId) -> bool {
        match item {
            ItemId::Part(id) => !self.has_moved_part(id),
            ItemId::Wire(id) => !self.is_excluded_wire(id),
            ItemId::Label(_) => false,
        }
    }

    /// The lowest wire id with an end at `at`, before reservations and planned wires exist.
    fn lowest_wire_end(&self, at: Pt) -> Option<WireId> {
        let kept = self.doc.points.at(at).iter().filter_map(|end| match end {
            End::Wire(id) if !self.is_excluded_wire(id) => Some(id),
            _ => None,
        });
        let moved = self
            .moved
            .iter()
            .filter(|(_, w)| w.a == at || w.b == at)
            .map(|&(id, _)| id);
        kept.chain(moved).min_by_key(|id| id.0)
    }

    /// Adds a reservation: a zero-area part with one pin at `at`.
    fn reserve(&mut self, at: Pt) -> PartId {
        let id = PartId(self.base_parts + u32::try_from(self.markers.len()).expect("part ids"));
        self.markers.push(at);
        self.res_yx.push((at, id));
        self.res_xy.push((at, id));
        self.res_sorted = false;
        id
    }

    /// Sorts reservation indices for O(log R) range queries along segments and boxes.
    fn finish_reservations(&mut self) {
        self.res_yx
            .sort_unstable_by_key(|&(p, id)| (p.y, p.x, id.0));
        self.res_xy
            .sort_unstable_by_key(|&(p, id)| (p.x, p.y, id.0));
        self.res_sorted = true;
    }

    /// Adds a planned wire replacing a run whose first wire is `source`.
    fn insert(&mut self, wire: Wire, source: WireId) {
        let id = WireId(self.base_wires + u32::try_from(self.added.len()).expect("wire ids"));
        self.added.push((wire, source));
        let bounds = wire_rect(wire);
        self.register_entry(ViewEntry::Wire { id, wire, bounds }, bounds);
    }

    fn visit_view_cells(&self, area: Rect, mut on_entry: impl FnMut(ViewEntry)) {
        let (x0, x1) = (cell_of(area.min.x), cell_of(area.max.x));
        let (y0, y1) = (cell_of(area.min.y), cell_of(area.max.y));
        let mut walk_head = |mut cur: u32| {
            while cur != u32::MAX {
                let node = self.cell_nodes[cur as usize];
                let bounds = match node.entry {
                    ViewEntry::MovedPart { envelope, .. } => envelope,
                    ViewEntry::Wire { bounds, .. } => bounds,
                };
                if bounds.intersects(area) {
                    on_entry(node.entry);
                }
                cur = node.next;
            }
        };
        let span = (x1 - x0 + 1).saturating_mul(y1 - y0 + 1);
        if span <= self.cell_heads.len() {
            for cy in y0..=y1 {
                let row = cy * CELLS_PER_AXIS;
                for cx in x0..=x1 {
                    if let Some(&head) = self.cell_heads.get(&((row + cx) as u32)) {
                        walk_head(head);
                    }
                }
            }
        } else {
            for (&key, &head) in &self.cell_heads {
                let k = key as usize;
                let cy = k / CELLS_PER_AXIS;
                let cx = k - cy * CELLS_PER_AXIS;
                if (x0..=x1).contains(&cx) && (y0..=y1).contains(&cy) {
                    walk_head(head);
                }
            }
        }
    }

    fn visit_reservations_segment(
        &self,
        area: Rect,
        horizontal: bool,
        mut on_res: impl FnMut(PartId, Pt),
    ) {
        if self.markers.is_empty() {
            return;
        }
        if self.res_sorted {
            if horizontal {
                let y = area.min.y;
                let lo = self
                    .res_yx
                    .partition_point(|&(p, _)| (p.y, p.x) < (y, area.min.x));
                for &(p, id) in &self.res_yx[lo..] {
                    if p.y != y || p.x > area.max.x {
                        break;
                    }
                    on_res(id, p);
                }
            } else {
                let x = area.min.x;
                let lo = self
                    .res_xy
                    .partition_point(|&(p, _)| (p.x, p.y) < (x, area.min.y));
                for &(p, id) in &self.res_xy[lo..] {
                    if p.x != x || p.y > area.max.y {
                        break;
                    }
                    on_res(id, p);
                }
            }
        } else {
            for (i, &p) in self.markers.iter().enumerate() {
                if area.contains(p) {
                    on_res(PartId(self.base_parts + i as u32), p);
                }
            }
        }
    }

    fn collect_reservations_box(&self, area: Rect, out: &mut LocalObstacles) {
        if self.markers.is_empty() {
            return;
        }
        if self.res_sorted {
            let lo = self.res_yx.partition_point(|&(p, _)| p.y < area.min.y);
            for &(p, id) in &self.res_yx[lo..] {
                if p.y > area.max.y {
                    break;
                }
                if p.x >= area.min.x && p.x <= area.max.x {
                    out.res_yx.push((p, id));
                }
            }
            out.res_yx_sorted = true;
        } else {
            for (i, &p) in self.markers.iter().enumerate() {
                if area.contains(p) {
                    out.res_yx.push((p, PartId(self.base_parts + i as u32)));
                }
            }
            out.res_yx_sorted = false;
        }
    }

    /// Reports a view blocker in the document's terms: a reservation is the replaced wire
    /// it stands for, and a planned wire is the first wire of the run it replaces.
    fn remap(&self, blocker: Blocker, sources: &[WireId]) -> Blocker {
        let wire = |w: WireId| {
            if w.0 >= self.base_wires {
                self.added[(w.0 - self.base_wires) as usize].1
            } else {
                w
            }
        };
        match blocker {
            Blocker::Pin { part, .. } if part.0 >= self.base_parts => {
                Blocker::WireEnd(sources[(part.0 - self.base_parts) as usize])
            }
            Blocker::WireEnd(w) => Blocker::WireEnd(wire(w)),
            Blocker::WireBody(w) => Blocker::WireBody(wire(w)),
            Blocker::Overlap(w) => Blocker::Overlap(wire(w)),
            b => b,
        }
    }
}

fn wire_rect(w: Wire) -> Rect {
    Rect::from_points(w.a, w.b).expect("validated wire")
}

impl RouteSheet for View<'_> {
    #[cfg(test)]
    fn query(&self, area: Rect, out: &mut Vec<ItemId>) {
        if area.min.x > area.max.x || area.min.y > area.max.y {
            out.clear();
            return;
        }
        // The caller's index is read without touching its visit stamps (W2.1: the
        // document, index stamps included, is unchanged by planning).
        self.doc
            .index
            .query_filtered_unstamped(area, out, |item, _| self.keeps(item));
        let start = out.len();
        self.visit_view_cells(area, |entry| match entry {
            ViewEntry::MovedPart { idx, .. } => out.push(ItemId::Part(self.parts[idx as usize].id)),
            ViewEntry::Wire { id, .. } => out.push(ItemId::Wire(id)),
        });
        if self.res_sorted {
            let lo = self.res_yx.partition_point(|&(p, _)| p.y < area.min.y);
            for &(p, id) in &self.res_yx[lo..] {
                if p.y > area.max.y {
                    break;
                }
                if p.x >= area.min.x && p.x <= area.max.x {
                    out.push(ItemId::Part(id));
                }
            }
        } else {
            for (i, &p) in self.markers.iter().enumerate() {
                if area.contains(p) {
                    out.push(ItemId::Part(PartId(self.base_parts + i as u32)));
                }
            }
        }
        let (x0, x1) = (cell_of(area.min.x), cell_of(area.max.x));
        let (y0, y1) = (cell_of(area.min.y), cell_of(area.max.y));
        if x0 == x1 && y0 == y1 {
            return;
        }
        // An item spanning several cells is met once per cell. Sorting makes the result
        // independent of the map's iteration order.
        let key = |item: &ItemId| match *item {
            ItemId::Part(id) => (0u8, id.0),
            ItemId::Wire(id) => (1, id.0),
            ItemId::Label(id) => (2, id.0),
        };
        out[start..].sort_unstable_by_key(key);
        let mut write = start;
        for read in start..out.len() {
            if write == start || out[write - 1] != out[read] {
                out[write] = out[read];
                write += 1;
            }
        }
        out.truncate(write);
    }

    fn pins(&self, id: PartId) -> &[Pt] {
        if id.0 >= self.base_parts {
            return std::slice::from_ref(&self.markers[(id.0 - self.base_parts) as usize]);
        }
        match self.moved_part(id) {
            Some(part) => &self.pins[part.pins.clone()],
            None => self.doc.pins(id).expect("queried part is live"),
        }
    }

    #[cfg(test)]
    fn wire(&self, id: WireId) -> Wire {
        if id.0 >= self.base_wires {
            return self.added[(id.0 - self.base_wires) as usize].0;
        }
        match self.moved.binary_search_by_key(&id.0, |(w, _)| w.0) {
            Ok(i) => self.moved[i].1,
            Err(_) => *self.doc.wire(id).expect("queried wire is live"),
        }
    }

    fn part_body(&self, id: PartId) -> Option<Rect> {
        if id.0 >= self.base_parts {
            // A reservation's zero-area body at an in-sheet point is that point.
            let at = self.markers[(id.0 - self.base_parts) as usize];
            return Some(Rect { min: at, max: at });
        }
        match self.moved_part(id) {
            Some(part) => Some(part.body),
            None => self.doc.part_body(id),
        }
    }

    fn visit_segment(&self, area: Rect, horizontal: bool, mut on_hit: impl FnMut(SegmentHit<'_>)) {
        self.doc
            .index
            .visit_parts_and_wires_unstamped(area, |item, bounds| match item {
                ItemId::Part(id) if !self.has_moved_part(id) => on_hit(SegmentHit::Part {
                    id,
                    envelope: bounds,
                    body: None,
                    pins: self.doc.pins(id).expect("queried part is live"),
                }),
                ItemId::Wire(id) if !self.is_excluded_wire(id) => on_hit(SegmentHit::Wire {
                    id,
                    wire: Wire {
                        a: bounds.min,
                        b: bounds.max,
                    },
                }),
                _ => {}
            });
        self.visit_view_cells(area, |entry| match entry {
            ViewEntry::MovedPart { idx, envelope } => {
                let part = &self.parts[idx as usize];
                on_hit(SegmentHit::Part {
                    id: part.id,
                    envelope,
                    body: Some(part.body),
                    pins: &self.pins[part.pins.clone()],
                });
            }
            ViewEntry::Wire { id, wire, .. } => on_hit(SegmentHit::Wire { id, wire }),
        });
        self.visit_reservations_segment(area, horizontal, |id, at| {
            on_hit(SegmentHit::Res { id, at });
        });
    }

    fn collect_box(&self, area: Rect, out: &mut LocalObstacles) {
        out.clear();
        self.doc
            .index
            .visit_parts_and_wires_unstamped(area, |item, bounds| match item {
                ItemId::Part(id) if !self.has_moved_part(id) => {
                    out.seen_parts.push((id, bounds, None));
                }
                ItemId::Wire(id) if !self.is_excluded_wire(id) => {
                    out.seen_wires.push((
                        id,
                        Wire {
                            a: bounds.min,
                            b: bounds.max,
                        },
                    ));
                }
                _ => {}
            });
        self.visit_view_cells(area, |entry| match entry {
            ViewEntry::MovedPart { idx, envelope } => {
                let part = &self.parts[idx as usize];
                out.seen_parts.push((part.id, envelope, Some(part.body)));
            }
            ViewEntry::Wire { id, wire, .. } => out.seen_wires.push((id, wire)),
        });
        self.collect_reservations_box(area, out);
        out.finish();
    }
}

/// WD3: rigid first, unanimous boundary policy, reroute or detach. On refusal `plan` is empty.
/// Labels on rigid corners move once; rerouted label placement belongs to W3.6.
/// Nonempty plans read the document through a final-geometry view layered over it
/// (selection, reservations and planned wires only), never a copy of the sheet.
pub fn plan_follow(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    policy: FollowPolicy,
    plan: &mut FollowPlan,
) -> Result<(), FollowError> {
    plan.clear();
    if !has_boundary(doc, items) {
        return Ok(());
    }
    let result = build(doc, items, t, policy, plan);
    if result.is_err() {
        plan.clear();
    }
    result
}
fn build(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    policy: FollowPolicy,
    plan: &mut FollowPlan,
) -> Result<(), FollowError> {
    let mut boundary = Boundary::default();
    doc.boundary_points(items, &mut boundary);
    let mut runs = Vec::new();
    doc.affected_runs(&boundary, &mut runs);
    let mut detached = vec![false; runs.len()];
    let mut scratch = RouteScratch::new();
    let sheet_error = |blocker| FollowError::NoRoute {
        from: runs[0].a.0,
        to: runs[0].b.0,
        blocker,
    };
    let mut v = View::new(doc, items, t).map_err(sheet_error)?;
    let mut end_markers = Vec::new();
    let mut reservations = Vec::new();
    let mut wires = Vec::new();
    // A failed rigid attempt under detach policy introduces old geometry. Restart so every
    // earlier run sees it too. Each restart fixes at least one fate, so termination is bounded.
    'restart: loop {
        plan.clear();
        v.reset(t, &runs, &detached).map_err(sheet_error)?;
        // Removing a run also removes its otherwise free far end. Reserve every final
        // endpoint before planning: an earlier corner must not capture a later run's end.
        // Zero-area marker parts affect validity only, not body/crossing scores.
        end_markers.clear();
        reservations.clear();
        for (i, r) in runs.iter().enumerate() {
            if detached[i] {
                continue;
            }
            for (end, source) in [(r.a, r.wires[0]), (r.b, *r.wires.last().unwrap())] {
                let at = if end.1 == Anchor::Boundary {
                    t.apply(end.0).map_err(|_| FollowError::NoRoute {
                        from: r.a.0,
                        to: r.b.0,
                        blocker: Blocker::Sheet,
                    })?
                } else {
                    end.0
                };
                let source = v
                    .lowest_wire_end(at)
                    .map_or(source, |id| if id.0 < source.0 { id } else { source });
                reservations.push((at, source));
            }
        }
        // Marker pin ids must preserve the original lowest-wire-id tie at a shared end.
        reservations.sort_unstable_by_key(|(at, source)| (source.0, at.x, at.y));
        for &(at, source) in &reservations {
            v.reserve(at);
            end_markers.push(source);
        }
        v.finish_reservations();
        for (i, r) in runs.iter().enumerate() {
            if detached[i] {
                plan.fates.push((r.a.0, RunFate::Detached));
                continue;
            }
            let a = t.apply(r.a.0).map_err(|_| FollowError::NoRoute {
                from: r.a.0,
                to: r.b.0,
                blocker: Blocker::Sheet,
            })?;
            let b = if r.b.1 == Anchor::Boundary {
                t.apply(r.b.0).map_err(|_| FollowError::NoRoute {
                    from: r.a.0,
                    to: r.b.0,
                    blocker: Blocker::Sheet,
                })?
            } else {
                r.b.0
            };
            wires.clear();
            let rigid = r.b.1 == Anchor::Boundary
                && r.wires
                    .iter()
                    .try_for_each(|id| {
                        wires.push(
                            t.wire(*doc.wire(*id).unwrap())
                                .map_err(|_| Blocker::Sheet)?,
                        );
                        Ok::<_, Blocker>(())
                    })
                    .is_ok()
                && check_new_wires_on(&v, &wires, [a, b]).is_ok();
            let fate = if rigid {
                RunFate::Translated
            } else {
                wires.clear();
                if !allows_on(&v, r, policy) {
                    detached[i] = true;
                    continue 'restart;
                }
                let old = doc.wire(r.wires[0]).unwrap();
                let first_leg = if old.a.y == old.b.y {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                };
                route_on(&v, a, b, first_leg, &mut scratch, &mut wires).map_err(|blocker| {
                    FollowError::NoRoute {
                        from: r.a.0,
                        to: r.b.0,
                        blocker: v.remap(blocker, &end_markers),
                    }
                })?;
                RunFate::Rerouted
            };
            if rigid {
                for &id in &r.labels {
                    if !items.contains(&ItemId::Label(id)) {
                        let p = t.apply(doc.label(id).unwrap().at).map_err(|_| {
                            FollowError::NoRoute {
                                from: r.a.0,
                                to: r.b.0,
                                blocker: Blocker::Sheet,
                            }
                        })?;
                        plan.labels.push((id, p));
                    }
                }
            }
            // Insertion makes subsequent runs validate against every already planned segment.
            for &w in &wires {
                v.insert(w, r.wires[0]);
            }
            plan.remove.extend_from_slice(&r.wires);
            plan.add.extend_from_slice(&wires);
            plan.fates.push((r.a.0, fate));
        }
        plan.labels.sort_unstable_by_key(|(id, _)| id.0);
        plan.labels.dedup_by_key(|(id, _)| *id);
        return Ok(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GRID_NM, Label, Part, Rect, SymbolId};
    use proptest::prelude::*;
    fn p(x: i32, y: i32) -> Pt {
        Pt {
            x: x * GRID_NM,
            y: y * GRID_NM,
        }
    }
    fn part(d: &mut Doc, pins: &[Pt]) -> PartId {
        d.insert_part(
            Part {
                symbol: SymbolId(0),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: "R".into(),
                value: "".into(),
            },
            pins,
            Rect::from_points(Pt::ZERO, Pt::ZERO).unwrap(),
        )
        .unwrap()
    }
    fn wire(d: &mut Doc, a: Pt, b: Pt) -> WireId {
        d.insert_wire(Wire { a, b }).unwrap()
    }
    fn policy(invert: bool) -> FollowPolicy {
        FollowPolicy::new(true, 10, invert).unwrap()
    }
    fn plan(d: &Doc, items: &[ItemId], t: Isometry, pol: FollowPolicy) -> FollowPlan {
        let before = format!("{d:?}");
        let mut out = FollowPlan::default();
        plan_follow(d, items, t, pol, &mut out).unwrap();
        assert_eq!(before, format!("{d:?}"));
        out
    }
    fn fan(n: i32) -> (Doc, PartId) {
        let mut d = Doc::new();
        let pins: Vec<_> = (0..n).map(|y| p(0, y * 4)).collect();
        let id = part(&mut d, &pins);
        for a in pins {
            let b = Pt {
                x: a.x - 6 * GRID_NM,
                ..a
            };
            part(&mut d, &[b]);
            wire(&mut d, a, b);
        }
        (d, id)
    }
    #[test]
    fn tc_move_01_to_05_threshold_and_modifier() {
        for (n, invert, fate) in [
            (2, false, RunFate::Rerouted),
            (9, false, RunFate::Rerouted),
            (10, false, RunFate::Detached),
            (10, true, RunFate::Rerouted),
            (2, true, RunFate::Detached),
        ] {
            let (d, id) = fan(n);
            let out = plan(
                &d,
                &[ItemId::Part(id)],
                Isometry::Translate(p(1, 0)),
                policy(invert),
            );
            assert_eq!(out.fates.len(), n as usize);
            assert!(out.fates.iter().all(|(_, f)| *f == fate));
            if fate == RunFate::Detached {
                assert!(out.remove.is_empty() && out.add.is_empty());
            } else {
                for y in 0..n {
                    assert!(
                        out.add
                            .iter()
                            .any(|w| w.a == p(1, y * 4) || w.b == p(1, y * 4))
                    );
                    assert!(
                        out.add
                            .iter()
                            .any(|w| w.a == p(-6, y * 4) || w.b == p(-6, y * 4))
                    );
                }
            }
        }
    }
    fn bent() -> (Doc, Vec<ItemId>) {
        let mut d = Doc::new();
        let a = part(&mut d, &[p(0, 0)]);
        let b = part(&mut d, &[p(4, 0)]);
        wire(&mut d, p(0, 0), p(0, 2));
        wire(&mut d, p(0, 2), p(4, 2));
        wire(&mut d, p(4, 2), p(4, 0));
        (d, vec![ItemId::Part(a), ItemId::Part(b)])
    }
    #[test]
    fn tc_move_06_07_09_rigid_and_labels() {
        let (mut d, mut items) = bent();
        let label = d
            .insert_label(Label {
                at: p(0, 2),
                text: "net".into(),
            })
            .unwrap();
        let t = Isometry::Translate(p(1, 0));
        let out = plan(&d, &items, t, policy(false));
        assert_eq!(out.fates, vec![(p(0, 0), RunFate::Translated)]);
        assert_eq!(
            out.add,
            d.wires()
                .map(|(_, w)| t.wire(*w).unwrap())
                .collect::<Vec<_>>()
        );
        assert_eq!(out.labels, vec![(label, p(1, 2))]);
        items.reverse();
        assert_eq!(out, plan(&d, &items, t, policy(true)));
    }
    #[test]
    fn tc_move_08_rigid_pin_blocker_reroutes() {
        let (mut d, items) = bent();
        part(&mut d, &[p(3, 2)]);
        let out = plan(&d, &items, Isometry::Translate(p(1, 0)), policy(false));
        assert_eq!(out.fates, vec![(p(0, 0), RunFate::Rerouted)]);
        assert_eq!(out.remove.len(), 3);
        assert!(d.wires().all(|(id, _)| out.remove.contains(&id)));
        assert_eq!(
            out.add,
            vec![Wire {
                a: p(1, 0),
                b: p(5, 0)
            }]
        );
    }
    #[test]
    fn tc_move_10_unanimous_policy_after_failed_rigid() {
        for reverse_thresholds in [false, true] {
            let (mut d, mut items) = bent();
            let selected: Vec<_> = items
                .iter()
                .map(|item| match *item {
                    ItemId::Part(id) => id,
                    _ => unreachable!(),
                })
                .collect();
            let shared: Vec<_> = d.wires().map(|(id, _)| id).collect();
            for (index, &id) in selected.iter().enumerate() {
                let n = if (index == 0) ^ reverse_thresholds {
                    8
                } else {
                    9
                };
                let (mut stored, _) = d.remove_stored_part_raw(id).unwrap();
                for k in 0..n {
                    let at = p(20 + id.0 as i32 * 5, k * 3);
                    stored.pins.push(at);
                    wire(&mut d, at, p(21 + id.0 as i32 * 5, k * 3));
                }
                d.restore_stored_part_raw(id, stored).unwrap();
            }
            part(&mut d, &[p(3, 2)]);
            let out = plan(&d, &items, Isometry::Translate(p(1, 0)), policy(false));
            assert_eq!(
                out.fates.iter().find(|(at, _)| *at == p(0, 0)).unwrap().1,
                RunFate::Detached
            );
            assert!(shared.iter().all(|id| !out.remove.contains(id)));
            assert!(
                out.add
                    .iter()
                    .all(|w| w.a.x >= 20 * GRID_NM && w.b.x >= 20 * GRID_NM)
            );
            items.reverse();
            assert_eq!(
                out,
                plan(&d, &items, Isometry::Translate(p(1, 0)), policy(false))
            );
        }
    }
    #[test]
    fn wd3_part_policy_at_boundary_shared_with_selected_wire() {
        let mut d = Doc::new();
        let a = part(&mut d, &[p(0, 0)]);
        let selected = wire(&mut d, p(0, 0), p(0, -2));
        let replaced = wire(&mut d, p(0, 0), p(4, 0));
        // WD3 applies the wire policy only to an end belonging to a moved wire,
        // not a part. Here the part allows follow and wire-only policy denies it.
        let pol = FollowPolicy::new(true, 1, true).unwrap();
        assert!(pol.part_follows(1));
        assert!(!pol.wire_follows());
        let out = plan(
            &d,
            &[ItemId::Part(a), ItemId::Wire(selected)],
            Isometry::Translate(p(1, 0)),
            pol,
        );
        assert_eq!(out.remove, vec![replaced]);
        assert_eq!(out.fates, vec![(p(0, 0), RunFate::Rerouted)]);
    }

    #[test]
    fn tc_move_11_rotation_and_mirrors() {
        let (d, id) = fan(2);
        for by in Orient::ALL {
            let t = Isometry::Reorient { pivot: p(1, 2), by };
            let out = plan(&d, &[ItemId::Part(id)], t, policy(false));
            for a in [p(0, 0), p(0, 4)] {
                let a = t.apply(a).unwrap();
                assert!(out.add.iter().any(|w| w.a == a || w.b == a));
            }
        }
    }
    #[test]
    fn tc_move_12_final_moved_body_selects_dogleg() {
        let mut d = Doc::new();
        let id = part(&mut d, &[p(0, 0)]);
        let (mut s, _) = d.remove_stored_part_raw(id).unwrap();
        s.local_bounds = Rect::from_points(p(-4, -2), p(0, 2)).unwrap();
        d.restore_stored_part_raw(id, s).unwrap();
        wire(&mut d, p(0, 0), p(-7, 0));
        wire(&mut d, p(-7, 0), p(-7, 4));
        part(&mut d, &[p(-3, 4)]);
        let out = plan(
            &d,
            &[ItemId::Part(id)],
            Isometry::Translate(p(1, 0)),
            policy(false),
        );
        assert_eq!(out.add.len(), 3);
        assert_eq!(out.add.first().unwrap().a, p(1, 0));
        assert_eq!(out.add.last().unwrap().b, p(-7, 4));
    }
    #[test]
    fn tc_move_13_refusal_clears_plan_and_preserves_doc() {
        let (mut d, id) = fan(1);
        // Block every outgoing direction adjacent to the final pin.
        for at in [p(2, 0), p(0, 0), p(1, 1), p(1, -1)] {
            part(&mut d, &[at]);
        }
        let before = format!("{d:?}");
        let mut out = FollowPlan {
            add: vec![Wire {
                a: p(0, 0),
                b: p(1, 0),
            }],
            ..Default::default()
        };
        assert!(matches!(
            plan_follow(
                &d,
                &[ItemId::Part(id)],
                Isometry::Translate(p(1, 0)),
                policy(false),
                &mut out
            ),
            Err(FollowError::NoRoute {
                blocker: Blocker::Pin { .. },
                ..
            })
        ));
        assert_eq!(out, FollowPlan::default());
        assert_eq!(before, format!("{d:?}"));
    }
    #[test]
    fn tc_move_15_fixed_junction() {
        let mut d = Doc::new();
        let id = part(&mut d, &[p(0, 0)]);
        let w = wire(&mut d, p(0, 0), p(4, 0));
        wire(&mut d, p(4, 0), p(4, 3));
        wire(&mut d, p(4, 0), p(6, 0));
        let out = plan(
            &d,
            &[ItemId::Part(id)],
            Isometry::Translate(p(0, 1)),
            policy(false),
        );
        assert_eq!(out.remove, vec![w]);
        assert_eq!(out.add.last().unwrap().b, p(4, 0));
    }
    #[test]
    fn tc_move_16_direct_pins_empty_and_17_selected_segment() {
        let mut d = Doc::new();
        let id = part(&mut d, &[p(0, 0)]);
        part(&mut d, &[p(0, 0)]);
        assert_eq!(
            plan(
                &d,
                &[ItemId::Part(id)],
                Isometry::Translate(p(1, 0)),
                policy(false)
            ),
            FollowPlan::default()
        );
        let first = wire(&mut d, p(0, 0), p(3, 0));
        let rest = wire(&mut d, p(3, 0), p(3, 4));
        let out = plan(
            &d,
            &[ItemId::Part(id), ItemId::Wire(first)],
            Isometry::Translate(p(1, 0)),
            policy(false),
        );
        assert_eq!(out.remove, vec![rest]);
        assert_eq!(out.add.first().unwrap().a, p(4, 0));
        assert_eq!(out.add.last().unwrap().b, p(3, 4));
    }
    #[test]
    fn wd4_reserves_later_free_end_and_validates_detached_geometry() {
        for detach_second in [false, true] {
            let mut d = Doc::new();
            let a = part(&mut d, &[p(0, 0)]);
            let b = part(&mut d, &[p(0, 4), p(20, 20)]);
            wire(&mut d, p(0, 0), p(-4, 0));
            wire(&mut d, p(0, 4), p(-4, 4));
            wire(&mut d, p(20, 20), p(22, 20));
            let policy =
                FollowPolicy::new(true, if detach_second { 2 } else { 10 }, false).unwrap();
            let out = plan(
                &d,
                &[ItemId::Part(a), ItemId::Part(b)],
                Isometry::Translate(p(0, 4)),
                policy,
            );
            // Horizontal-first would put a corner at the later run's fixed free end.
            // With detach policy it would also overlap that run's unchanged wire.
            assert_eq!(
                out.add[0],
                Wire {
                    a: p(0, 4),
                    b: p(0, 0)
                }
            );
            assert_eq!(
                out.add[1],
                Wire {
                    a: p(0, 0),
                    b: p(-4, 0)
                }
            );
            assert_eq!(
                out.fates[1].1,
                if detach_second {
                    RunFate::Detached
                } else {
                    RunFate::Rerouted
                }
            );
        }
    }
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn winv6_winv8_rigid_reorientation_and_labels(
            orientation in 0usize..8, x in -8i32..9, y in -8i32..9,
            invert in any::<bool>()
        ) {
            let (mut d, mut items) = bent();
            let label = d.insert_label(Label { at: p(0, 2), text: "net".into() }).unwrap();
            let t = Isometry::Reorient { pivot: p(x, y), by: Orient::ALL[orientation] };
            let expected = plan(&d, &items, t, policy(invert));
            prop_assert_eq!(&expected.fates, &vec![(p(0, 0), RunFate::Translated)]);
            prop_assert_eq!(&expected.labels, &vec![(label, t.apply(p(0, 2)).unwrap())]);
            prop_assert_eq!(expected.remove.len(), 3);
            prop_assert_eq!(&expected.add, &d.wires().map(|(_, w)| t.wire(*w).unwrap()).collect::<Vec<_>>());
            items.reverse();
            let wires: Vec<_> = d.wires().map(|(id, w)| (id, *w)).collect();
            for &(id, _) in &wires { d.remove_wire_raw(id).unwrap(); }
            for &(id, w) in wires.iter().rev() { d.restore_wire_raw(id, w).unwrap(); }
            prop_assert_eq!(&expected, &plan(&d, &items, t, policy(!invert)));
        }
        #[test]
        fn winv6_winv8_different_ids_and_multiple_selected_parts(n in 2i32..9, dx in -2i32..3, dy in -2i32..3, invert in any::<bool>()) {
            let (d,id)=fan(n);
            let items=vec![ItemId::Part(id),ItemId::Part(PartId(1))];
            let t=Isometry::Translate(p(dx,dy));
            let expected=plan(&d,&items,t,policy(invert));
            let mut rebuilt=Doc::new();let mut selected=Vec::new();
            for (i,s) in d.parts.slots.iter().enumerate().rev() {
                if let Some(s)=s {
                    let local:Vec<_>=s.pins.iter().map(|&p|s.part.orient.inverse().apply(p-s.part.at)).collect();
                    let (new_id,_)=rebuilt.insert_part_raw(s.part.clone(),&local,s.local_bounds).unwrap();
                    if items.contains(&ItemId::Part(PartId(i as u32))) { selected.push(ItemId::Part(new_id)); }
                }
            }
            for (_,w) in d.wires().collect::<Vec<_>>().into_iter().rev() { rebuilt.insert_wire_raw(*w).unwrap(); }
            let actual=plan(&rebuilt,&selected,t,policy(invert));
            prop_assert_eq!(&expected.add,&actual.add);prop_assert_eq!(&expected.fates,&actual.fates);
            let removed:Vec<_>=expected.remove.iter().map(|id|*d.wire(*id).unwrap()).collect();
            let removed_rebuilt:Vec<_>=actual.remove.iter().map(|id|*rebuilt.wire(*id).unwrap()).collect();
            prop_assert_eq!(removed,removed_rebuilt);prop_assert_eq!(actual.fates.len(),n as usize);
            selected.reverse();prop_assert_eq!(&actual,&plan(&rebuilt,&selected,t,policy(invert)));
        }
        #[test]        fn winv6_winv8_selection_and_rebuilt_incidence(n in 1i32..10,dx in -2i32..3,dy in -2i32..3,invert in any::<bool>()) {
            let (mut d,id)=fan(n);let t=Isometry::Translate(p(dx,dy));let mut items=vec![ItemId::Part(id),ItemId::Part(id)];
            let expected=plan(&d,&items,t,policy(invert));items.reverse();
            prop_assert_eq!(&expected,&plan(&d,&items,t,policy(invert)));
            // Restore identical ids in reverse order, rebuilding index and incidence traversal.
            let wires:Vec<_>=d.wires().map(|(id,w)|(id,*w)).collect();
            for &(id,_) in &wires {d.remove_wire_raw(id).unwrap();}
            for &(id,w) in wires.iter().rev() {d.restore_wire_raw(id,w).unwrap();}
            prop_assert_eq!(&expected,&plan(&d,&items,t,policy(invert)));
            prop_assert_eq!(expected.fates.len(),n as usize);
            let mut removed=expected.remove.clone();removed.sort_unstable_by_key(|w|w.0);removed.dedup();prop_assert_eq!(removed.len(),expected.remove.len());
        }
    }
}

#[cfg(test)]
mod view_tests {
    //! The planner's layered view must read exactly like a full copy of the final
    //! geometry (the view W2.1 built before the WB3 work): the same items for every
    //! query, the same pins, wires and bodies, and the same reservation and
    //! planned-wire ids.
    use super::*;
    use crate::{GRID_NM, Label, Part, SymbolId};
    use proptest::prelude::*;

    fn g(x: i32, y: i32) -> Pt {
        Pt {
            x: x * GRID_NM,
            y: y * GRID_NM,
        }
    }

    /// The full copy: every slot kept, the selection moved, the replaced runs removed,
    /// labels left out, then reservations and planned wires inserted.
    fn materialize(
        doc: &Doc,
        items: &[ItemId],
        t: Isometry,
        runs: &[Run],
        detached: &[bool],
        markers: &[Pt],
        added: &[Wire],
    ) -> Result<Doc, Blocker> {
        let mut v = Doc::new();
        v.parts.slots.resize(doc.parts.slots.len(), None);
        v.wires.slots.resize(doc.wires.slots.len(), None);
        for (i, s) in doc.parts.slots.iter().enumerate() {
            if let Some(s) = s {
                let mut s = s.clone();
                if items.contains(&ItemId::Part(PartId(i as u32))) {
                    s.part.at = t.apply(s.part.at).map_err(|_| Blocker::Sheet)?;
                    if let Isometry::Reorient { by, .. } = t {
                        s.part.orient = s.part.orient.then(by);
                    }
                    for p in &mut s.pins {
                        *p = t.apply(*p).map_err(|_| Blocker::Sheet)?;
                    }
                }
                v.restore_stored_part_raw(PartId(i as u32), s)
                    .map_err(|_| Blocker::Sheet)?;
            }
        }
        for (id, &w) in doc.wires() {
            if runs
                .iter()
                .enumerate()
                .any(|(i, r)| !detached[i] && r.wires.contains(&id))
            {
                continue;
            }
            let w = if items.contains(&ItemId::Wire(id)) {
                t.wire(w).map_err(|_| Blocker::Sheet)?
            } else {
                w
            };
            v.restore_wire_raw(id, w).map_err(|_| Blocker::Sheet)?;
        }
        for &at in markers {
            let part = Part {
                symbol: SymbolId(0),
                at,
                orient: Orient::IDENTITY,
                designator: "".into(),
                value: "".into(),
            };
            let zero = Rect::from_points(Pt::ZERO, Pt::ZERO).unwrap();
            v.insert_part_raw(part, &[Pt::ZERO], zero).unwrap();
        }
        for &w in added {
            v.insert_wire_raw(w).unwrap();
        }
        Ok(v)
    }

    fn sorted(items: &[ItemId]) -> Vec<(u8, u32)> {
        let mut keys: Vec<_> = items
            .iter()
            .map(|item| match *item {
                ItemId::Part(id) => (0, id.0),
                ItemId::Wire(id) => (1, id.0),
                ItemId::Label(id) => (2, id.0),
            })
            .collect();
        keys.sort_unstable();
        keys
    }

    fn segment((x, y, len, horizontal): (i32, i32, i32, bool)) -> Wire {
        let len = if len == 0 { 1 } else { len };
        let a = g(x, y);
        let b = if horizontal {
            g(x + len, y)
        } else {
            g(x, y + len)
        };
        Wire { a, b }
    }

    type PartSpec = ((i32, i32), Vec<(i32, i32)>, (i32, i32), usize);
    type SegSpec = (i32, i32, i32, bool);

    fn part_spec() -> impl Strategy<Value = PartSpec> {
        (
            (-12i32..12, -12i32..12),
            prop::collection::vec((-3i32..4, -3i32..4), 0..4),
            (0i32..3, 0i32..3),
            0usize..8,
        )
    }

    fn seg_spec() -> impl Strategy<Value = SegSpec> {
        (-14i32..14, -14i32..14, -8i32..9, any::<bool>())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        #[test]
        fn w22_view_reads_as_the_full_copy(
            parts in prop::collection::vec(part_spec(), 1..8),
            wires in prop::collection::vec(seg_spec(), 0..30),
            labels in prop::collection::vec((-14i32..14, -14i32..14), 0..4),
            holes in prop::collection::vec(any::<u16>(), 0..4),
            pick in prop::collection::vec(any::<u16>(), 1..5),
            rotate in any::<bool>(),
            orientation in 0usize..8,
            shift in (-3i32..4, -3i32..4),
            pivot in (-6i32..7, -6i32..7),
            detach in prop::collection::vec(any::<bool>(), 0..40),
            planned in prop::collection::vec(seg_spec(), 0..12),
            areas in prop::collection::vec((-18i32..18, -18i32..18, 0i32..12, 0i32..12), 1..24),
        ) {
            let mut d = Doc::new();
            for ((x, y), pins, (w, h), o) in parts {
                let local: Vec<Pt> = pins.iter().map(|&(px, py)| g(px, py)).collect();
                let part = Part {
                    symbol: SymbolId(0),
                    at: g(x, y),
                    orient: Orient::ALL[o],
                    designator: "P".into(),
                    value: "".into(),
                };
                d.insert_part(part, &local, Rect::from_points(g(-w, -h), g(w, h)).unwrap())
                    .unwrap();
            }
            for spec in wires {
                d.insert_wire(segment(spec)).unwrap();
            }
            for (x, y) in labels {
                d.insert_label(Label { at: g(x, y), text: "n".into() }).unwrap();
            }
            // Holes: removed slots keep their ids reserved.
            let live: Vec<WireId> = d.wires().map(|(id, _)| id).collect();
            for h in holes {
                if !live.is_empty() {
                    d.remove_wire_raw(live[h as usize % live.len()]);
                }
            }
            let mut all: Vec<ItemId> = d.parts().map(|(id, _)| ItemId::Part(id)).collect();
            all.extend(d.wires().map(|(id, _)| ItemId::Wire(id)));
            all.extend(d.labels().map(|(id, _)| ItemId::Label(id)));
            let items: Vec<ItemId> = pick.iter().map(|&i| all[i as usize % all.len()]).collect();
            let t = if rotate {
                Isometry::Reorient { pivot: g(pivot.0, pivot.1), by: Orient::ALL[orientation] }
            } else {
                Isometry::Translate(g(shift.0, shift.1))
            };
            let mut boundary = Boundary::default();
            d.boundary_points(&items, &mut boundary);
            let mut runs = Vec::new();
            d.affected_runs(&boundary, &mut runs);
            let detached: Vec<bool> = (0..runs.len())
                .map(|i| detach.get(i).copied().unwrap_or(false))
                .collect();

            let view = View::new(&d, &items, t)
                .and_then(|mut v| v.reset(t, &runs, &detached).map(|()| v));
            let copy = materialize(&d, &items, t, &runs, &detached, &[], &[]);
            prop_assert_eq!(view.is_ok(), copy.is_ok());
            let (Ok(mut view), Ok(copy)) = (view, copy) else {
                return Ok(());
            };

            // The lowest wire end at a point, before reservations and planned wires.
            let mut probes: Vec<Pt> = copy.wires().flat_map(|(_, w)| [w.a, w.b]).collect();
            probes.extend(d.wires().flat_map(|(_, w)| [w.a, w.b]));
            for p in probes {
                let expected = copy
                    .points
                    .at(p)
                    .iter()
                    .filter_map(|e| match e {
                        End::Wire(id) => Some(id),
                        End::Pin { .. } => None,
                    })
                    .min_by_key(|id| id.0);
                prop_assert_eq!(view.lowest_wire_end(p), expected);
            }

            let markers: Vec<Pt> = runs.iter().flat_map(|r| [r.a.0, r.b.0]).collect();
            let added: Vec<Wire> = planned.into_iter().map(segment).collect();
            for &at in &markers {
                view.reserve(at);
            }
            for &w in &added {
                view.insert(w, WireId(0));
            }
            let copy = materialize(&d, &items, t, &runs, &detached, &markers, &added).unwrap();
            let (mut got, mut want) = (Vec::new(), Vec::new());
            for &(x, y, w, h) in &areas {
                let area = Rect::from_points(g(x, y), g(x + w, y + h)).unwrap();
                RouteSheet::query(&view, area, &mut got);
                RouteSheet::query(&copy, area, &mut want);
                prop_assert_eq!(sorted(&got), sorted(&want));
                for &item in &got {
                    match item {
                        ItemId::Part(id) => {
                            prop_assert_eq!(RouteSheet::pins(&view, id), RouteSheet::pins(&copy, id));
                            prop_assert_eq!(
                                RouteSheet::part_body(&view, id),
                                RouteSheet::part_body(&copy, id)
                            );
                        }
                        ItemId::Wire(id) => {
                            prop_assert_eq!(RouteSheet::wire(&view, id), RouteSheet::wire(&copy, id));
                        }
                        ItemId::Label(_) => prop_assert!(false, "the view holds no labels"),
                    }
                }
            }
            // W2.2 (WD4, WINV6): the router's production reads of the view (`visit_segment`,
            // `collect_box`) give the same routes, refusals and validator verdicts as the
            // full copy, with the reservation lists unsorted and then sorted as `build`
            // leaves them. Scale 1 keeps the dogleg box local; scale 6 takes the large-box
            // fallback.
            for sorted_reservations in [false, true] {
                if sorted_reservations {
                    view.finish_reservations();
                    for &(x, y, w, h) in &areas {
                        let area = Rect::from_points(g(x, y), g(x + w, y + h)).unwrap();
                        RouteSheet::query(&view, area, &mut got);
                        RouteSheet::query(&copy, area, &mut want);
                        prop_assert_eq!(sorted(&got), sorted(&want));
                    }
                }
                for &(x, y, w, h) in &areas {
                    for scale in [1, 6] {
                        let a = g(x * scale, y * scale);
                        let b = g((x + w) * scale, (y + h) * scale);
                        for axis in [Axis::Horizontal, Axis::Vertical] {
                            let (mut via_view, mut via_copy) = (Vec::new(), Vec::new());
                            let r_view =
                                route_on(&view, a, b, axis, &mut RouteScratch::new(), &mut via_view);
                            let r_copy =
                                route_on(&copy, a, b, axis, &mut RouteScratch::new(), &mut via_copy);
                            prop_assert_eq!(r_view, r_copy);
                            prop_assert_eq!(&via_view, &via_copy);
                        }
                        let corner = Pt { x: b.x, y: a.y };
                        let l: Vec<Wire> = [Wire { a, b: corner }, Wire { a: corner, b }]
                            .into_iter()
                            .filter(|w| w.a != w.b)
                            .collect();
                        prop_assert_eq!(
                            check_new_wires_on(&view, &l, [a, b]),
                            check_new_wires_on(&copy, &l, [a, b])
                        );
                    }
                }
            }
            // Planning never touches the caller's document, index stamps included.
            let before = format!("{d:?}");
            let mut plan = FollowPlan::default();
            let policy = FollowPolicy::new(true, 10, false).unwrap();
            let _ = plan_follow(&d, &items, t, policy, &mut plan);
            prop_assert_eq!(before, format!("{d:?}"));
        }
    }
}
