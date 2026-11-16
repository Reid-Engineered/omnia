//! Spatial index (D3): a hand-written uniform grid of K12 cells over the K1 sheet.
//!
//! Every item is registered under its bounding rectangle in each cell that rectangle meets, so
//! a long wire sits in every cell it crosses. A query walks the cells under the area in order
//! and deduplicates with a per-item generation stamp: no set, no sort, no allocation beyond
//! growing `out`.
//!
//! The stamps live in [`Cell`]s so that [`SpatialIndex::query`] takes `&self`, as
//! `Doc::query` does in plan section 9. The index is therefore `Send` but not `Sync`: the UI
//! thread owns it with the document (D8); background jobs read a snapshot instead.

use std::cell::Cell;

use crate::{GRID_NM, GeomError, ItemId, Rect, SHEET_LIMIT_NM};

/// K12: the side of one index cell, 8 grid steps.
pub const INDEX_CELL_NM: i32 = 8 * GRID_NM;

/// Cells per axis: enough to cover `-SHEET_LIMIT_NM..=SHEET_LIMIT_NM`.
const CELLS_PER_AXIS: usize = (2 * SHEET_LIMIT_NM as i64 / INDEX_CELL_NM as i64) as usize + 1;

#[derive(Debug, Default)]
struct Entry {
    bounds: Option<Rect>,
    stamp: Cell<u32>,
}

/// One table per item kind, indexed by the arena id (ids are dense and never reused, D2).
#[derive(Debug, Default)]
struct Entries(Vec<Entry>);

impl Entries {
    fn slot(&self, index: u32) -> Option<&Entry> {
        self.0.get(index as usize)
    }

    fn existing_mut(&mut self, index: u32) -> Option<&mut Entry> {
        self.0.get_mut(index as usize)
    }

    fn slot_mut(&mut self, index: u32) -> &mut Entry {
        let index = index as usize;
        if index >= self.0.len() {
            self.0.resize_with(index + 1, Entry::default);
        }
        &mut self.0[index]
    }
}

/// Uniform-grid spatial index of [`ItemId`]s by bounding rectangle (D3, K12).
#[derive(Debug)]
pub struct SpatialIndex {
    /// Row-major, `CELLS_PER_AXIS` squared; each cell lists the items whose bounds meet it.
    cells: Vec<Vec<ItemId>>,
    parts: Entries,
    wires: Entries,
    labels: Entries,
    len: usize,
    /// Stamp of the latest query. Item stamps never exceed it; 0 means "never visited".
    generation: Cell<u32>,
}

impl Default for SpatialIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialIndex {
    pub fn new() -> Self {
        Self {
            cells: std::iter::repeat_with(Vec::new)
                .take(CELLS_PER_AXIS * CELLS_PER_AXIS)
                .collect(),
            parts: Entries::default(),
            wires: Entries::default(),
            labels: Entries::default(),
            len: 0,
            generation: Cell::new(0),
        }
    }

    /// Number of items in the index.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bounds `item` is registered under, if it is in the index.
    pub fn bounds(&self, item: ItemId) -> Option<Rect> {
        self.entry(item)?.bounds
    }

    /// Registers `item` under `bounds` in every cell they meet. If `item` is already present
    /// it is moved to the new bounds and the old ones are returned. Bounds outside K1 or
    /// inverted are rejected and leave the index unchanged.
    pub fn insert(&mut self, item: ItemId, bounds: Rect) -> Result<Option<Rect>, GeomError> {
        let bounds = Rect::new(bounds.min, bounds.max)?;
        let old = self.remove(item);
        let (x0, y0, x1, y1) = cell_span(bounds);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                self.cells[cy * CELLS_PER_AXIS + cx].push(item);
            }
        }
        let entry = self.entry_mut(item);
        entry.bounds = Some(bounds);
        self.len += 1;
        Ok(old)
    }

    /// Unregisters `item` from every cell it was in and returns its bounds.
    pub fn remove(&mut self, item: ItemId) -> Option<Rect> {
        let table = match item {
            ItemId::Part(_) => &mut self.parts,
            ItemId::Wire(_) => &mut self.wires,
            ItemId::Label(_) => &mut self.labels,
        };
        let bounds = table.existing_mut(item_index(item))?.bounds.take()?;
        let (x0, y0, x1, y1) = cell_span(bounds);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                let cell = &mut self.cells[cy * CELLS_PER_AXIS + cx];
                let at = cell
                    .iter()
                    .position(|&i| i == item)
                    .expect("indexed item missing from a cell it spans");
                cell.swap_remove(at);
            }
        }
        self.len -= 1;
        Some(bounds)
    }

    /// Replaces `out` with every item whose bounds meet `area`, edges and corners included
    /// (as [`Rect::intersects`]). Each item appears once; order is unspecified. Allocates
    /// nothing when `out` already has room for the result.
    pub fn query(&self, area: Rect, out: &mut Vec<ItemId>) {
        out.clear();
        if area.min.x > area.max.x || area.min.y > area.max.y {
            return;
        }
        let generation = self.next_generation();
        let (x0, y0, x1, y1) = cell_span(area);
        for cy in y0..=y1 {
            let row = &self.cells[cy * CELLS_PER_AXIS..][..CELLS_PER_AXIS];
            for cell in &row[x0..=x1] {
                for &item in cell {
                    let entry = self.entry(item).expect("cell lists an item with no entry");
                    if entry.stamp.get() == generation {
                        continue;
                    }
                    entry.stamp.set(generation);
                    if entry.bounds.is_some_and(|b| b.intersects(area)) {
                        out.push(item);
                    }
                }
            }
        }
    }

    fn next_generation(&self) -> u32 {
        let next = self.generation.get().wrapping_add(1);
        let next = if next == 0 {
            // Wrapped after 2^32 queries: forget every stamp so none can equal a new one.
            for table in [&self.parts, &self.wires, &self.labels] {
                for entry in &table.0 {
                    entry.stamp.set(0);
                }
            }
            1
        } else {
            next
        };
        self.generation.set(next);
        next
    }

    fn entry(&self, item: ItemId) -> Option<&Entry> {
        match item {
            ItemId::Part(id) => self.parts.slot(id.0),
            ItemId::Wire(id) => self.wires.slot(id.0),
            ItemId::Label(id) => self.labels.slot(id.0),
        }
    }

    fn entry_mut(&mut self, item: ItemId) -> &mut Entry {
        match item {
            ItemId::Part(id) => self.parts.slot_mut(id.0),
            ItemId::Wire(id) => self.wires.slot_mut(id.0),
            ItemId::Label(id) => self.labels.slot_mut(id.0),
        }
    }

    #[cfg(test)]
    fn set_generation(&self, generation: u32) {
        self.generation.set(generation);
    }
}

fn item_index(item: ItemId) -> u32 {
    match item {
        ItemId::Part(id) => id.0,
        ItemId::Wire(id) => id.0,
        ItemId::Label(id) => id.0,
    }
}

/// Column of `v`, clamped to the grid so an area reaching past K1 still walks valid cells.
fn cell_of(v: i32) -> usize {
    let offset = (i64::from(v) + i64::from(SHEET_LIMIT_NM)).clamp(0, 2 * i64::from(SHEET_LIMIT_NM));
    (offset / i64::from(INDEX_CELL_NM)) as usize
}

/// Inclusive cell range `(x0, y0, x1, y1)` under `r` (`r.min <= r.max`).
fn cell_span(r: Rect) -> (usize, usize, usize, usize) {
    (
        cell_of(r.min.x),
        cell_of(r.min.y),
        cell_of(r.max.x),
        cell_of(r.max.y),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LabelId, PartId, Pt, WireId};
    use proptest::prelude::*;
    use std::collections::{BTreeMap, HashSet};

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect::from_points(Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }).unwrap()
    }

    fn sorted(items: &[ItemId]) -> Vec<(u8, u32)> {
        let mut keys: Vec<_> = items.iter().map(|&i| key(i)).collect();
        keys.sort_unstable();
        keys
    }

    fn key(item: ItemId) -> (u8, u32) {
        match item {
            ItemId::Part(id) => (0, id.0),
            ItemId::Wire(id) => (1, id.0),
            ItemId::Label(id) => (2, id.0),
        }
    }

    fn brute(items: &BTreeMap<(u8, u32), (ItemId, Rect)>, area: Rect) -> Vec<(u8, u32)> {
        items
            .iter()
            .filter(|(_, (_, b))| b.intersects(area))
            .map(|(&k, _)| k)
            .collect()
    }

    #[test]
    fn grid_covers_the_sheet() {
        assert_eq!(INDEX_CELL_NM, 20_320_000);
        assert_eq!(cell_of(-SHEET_LIMIT_NM), 0);
        assert_eq!(cell_of(SHEET_LIMIT_NM), CELLS_PER_AXIS - 1);
        assert_eq!(cell_of(i32::MIN), 0);
        assert_eq!(cell_of(i32::MAX), CELLS_PER_AXIS - 1);
        assert_eq!(cell_of(-SHEET_LIMIT_NM + INDEX_CELL_NM - 1), 0);
        assert_eq!(cell_of(-SHEET_LIMIT_NM + INDEX_CELL_NM), 1);
    }

    #[test]
    fn long_wire_registered_in_every_cell_it_crosses() {
        let mut index = SpatialIndex::new();
        let wire = ItemId::Wire(WireId(7));
        let body = rect(-SHEET_LIMIT_NM, 0, SHEET_LIMIT_NM, 0);
        index.insert(wire, body).unwrap();
        let row = cell_of(0);
        for cx in 0..CELLS_PER_AXIS {
            assert_eq!(index.cells[row * CELLS_PER_AXIS + cx], [wire], "cell {cx}");
        }
        let total: usize = index.cells.iter().map(Vec::len).sum();
        assert_eq!(total, CELLS_PER_AXIS);

        // Seen from many cells, reported once.
        let mut out = Vec::new();
        index.query(rect(-SHEET_LIMIT_NM, -5, SHEET_LIMIT_NM, 5), &mut out);
        assert_eq!(out, [wire]);
        // A small window far along the wire still finds it.
        index.query(rect(900_000_000, -1, 900_000_001, 1), &mut out);
        assert_eq!(out, [wire]);
        // Touching an edge counts; one nanometre off does not.
        index.query(rect(0, 1, 10, 10), &mut out);
        assert!(out.is_empty());
        index.query(rect(0, 0, 10, 10), &mut out);
        assert_eq!(out, [wire]);

        assert_eq!(index.remove(wire), Some(body));
        assert!(index.cells.iter().all(Vec::is_empty));
        assert!(index.is_empty());
        assert_eq!(index.remove(wire), None);
    }

    #[test]
    fn insert_moves_and_rejects_bad_bounds() {
        let mut index = SpatialIndex::new();
        let part = ItemId::Part(PartId(3));
        let a = rect(0, 0, 100, 100);
        let b = rect(500_000_000, 500_000_000, 500_000_100, 500_000_100);
        assert_eq!(index.insert(part, a), Ok(None));
        assert_eq!(index.insert(part, b), Ok(Some(a)));
        assert_eq!(index.len(), 1);
        assert_eq!(index.bounds(part), Some(b));
        let mut out = Vec::new();
        index.query(a, &mut out);
        assert!(out.is_empty());
        index.query(b, &mut out);
        assert_eq!(out, [part]);

        let inverted = Rect {
            min: Pt { x: 10, y: 0 },
            max: Pt { x: 0, y: 0 },
        };
        let outside = Rect {
            min: Pt::ZERO,
            max: Pt {
                x: SHEET_LIMIT_NM + 1,
                y: 0,
            },
        };
        let label = ItemId::Label(LabelId(0));
        assert!(index.insert(label, inverted).is_err());
        assert!(index.insert(part, outside).is_err());
        assert_eq!(index.bounds(part), Some(b));
        assert_eq!(index.bounds(label), None);
        assert_eq!(index.len(), 1);

        // An inverted query area finds nothing; one reaching past K1 is clamped.
        index.query(inverted, &mut out);
        assert!(out.is_empty());
        index.query(
            Rect {
                min: Pt {
                    x: i32::MIN,
                    y: i32::MIN,
                },
                max: Pt {
                    x: i32::MAX,
                    y: i32::MAX,
                },
            },
            &mut out,
        );
        assert_eq!(out, [part]);
    }

    #[test]
    fn generation_wrap_resets_stamps() {
        let mut index = SpatialIndex::new();
        let wire = ItemId::Wire(WireId(0));
        index.insert(wire, rect(0, 0, 50_000_000, 0)).unwrap();
        let mut out = Vec::new();
        index.set_generation(u32::MAX - 1);
        index.query(rect(0, 0, 1, 1), &mut out); // stamps u32::MAX
        assert_eq!(out, [wire]);
        index.query(rect(0, 0, 50_000_000, 1), &mut out); // wraps to 1
        assert_eq!(out, [wire]);
        assert_eq!(index.generation.get(), 1);
        index.query(rect(0, 0, 50_000_000, 1), &mut out);
        assert_eq!(out, [wire]);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert(ItemId, Rect),
        Remove(ItemId),
        Query(Rect),
    }

    fn coord() -> impl Strategy<Value = i32> {
        -SHEET_LIMIT_NM..=SHEET_LIMIT_NM
    }

    /// A rectangle of any size anywhere on the sheet: points, cell-sized boxes and spans
    /// over the whole sheet all occur.
    fn any_rect() -> impl Strategy<Value = Rect> {
        let extent = prop_oneof![
            Just(0i64),
            0..=INDEX_CELL_NM as i64 * 2,
            0..=2 * SHEET_LIMIT_NM as i64,
        ];
        (coord(), coord(), extent.clone(), extent).prop_map(|(x, y, w, h)| {
            let x1 = (i64::from(x) + w).min(i64::from(SHEET_LIMIT_NM)) as i32;
            let y1 = (i64::from(y) + h).min(i64::from(SHEET_LIMIT_NM)) as i32;
            rect(x, y, x1, y1)
        })
    }

    /// An axis-aligned wire body of any length, from a point to the whole sheet.
    fn wire_rect() -> impl Strategy<Value = Rect> {
        (coord(), coord(), coord(), any::<bool>()).prop_map(|(x, y, end, horizontal)| {
            if horizontal {
                rect(x, y, end, y)
            } else {
                rect(x, y, x, end)
            }
        })
    }

    fn any_item() -> impl Strategy<Value = ItemId> {
        prop_oneof![
            (0u32..64).prop_map(|i| ItemId::Part(PartId(i))),
            (0u32..64).prop_map(|i| ItemId::Wire(WireId(i))),
            (0u32..64).prop_map(|i| ItemId::Label(LabelId(i))),
        ]
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            4 => (any_item(), prop_oneof![any_rect(), wire_rect()])
                .prop_map(|(i, r)| Op::Insert(i, r)),
            1 => any_item().prop_map(Op::Remove),
            3 => any_rect().prop_map(Op::Query),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// `query` equals a brute-force scan after any sequence of inserts, moves and removes.
        #[test]
        fn query_matches_brute_force(ops in prop::collection::vec(op(), 1..200)) {
            let mut index = SpatialIndex::new();
            let mut model: BTreeMap<(u8, u32), (ItemId, Rect)> = BTreeMap::new();
            let mut out = Vec::new();
            for op in ops {
                match op {
                    Op::Insert(item, bounds) => {
                        let old = index.insert(item, bounds).unwrap();
                        prop_assert_eq!(old, model.insert(key(item), (item, bounds)).map(|(_, b)| b));
                    }
                    Op::Remove(item) => {
                        prop_assert_eq!(index.remove(item), model.remove(&key(item)).map(|(_, b)| b));
                    }
                    Op::Query(area) => {
                        index.query(area, &mut out);
                        let unique: HashSet<_> = out.iter().map(|&i| key(i)).collect();
                        prop_assert_eq!(unique.len(), out.len(), "duplicate in query result");
                        prop_assert_eq!(sorted(&out), brute(&model, area));
                    }
                }
                prop_assert_eq!(index.len(), model.len());
            }
            // Every cell lists exactly the live items whose bounds meet it.
            let total: usize = index.cells.iter().map(Vec::len).sum();
            let expected: usize = model
                .values()
                .map(|(_, b)| {
                    let (x0, y0, x1, y1) = cell_span(*b);
                    (x1 - x0 + 1) * (y1 - y0 + 1)
                })
                .sum();
            prop_assert_eq!(total, expected);
        }
    }
}
