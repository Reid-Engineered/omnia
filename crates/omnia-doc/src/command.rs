//! Document commands, transactional validation, and change reporting (D2, INV1, INV4).

use std::{fmt, sync::Arc};

use smallvec::SmallVec;

use crate::{
    Doc, GeomError, ItemId, Label, LabelId, Orient, Part, PartId, Pt, Rect, StoreError, Wire,
    WireId,
    points::End,
    undo::{self, UndoOp},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextField {
    Designator,
    Value,
    Label,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    PlacePart {
        part: Part,
        local_pins: Vec<Pt>,
        local_bounds: Rect,
    },
    AddWires(Vec<Wire>),
    AddLabel(Label),
    Move {
        items: Vec<ItemId>,
        delta: Pt,
    },
    Reorient {
        items: Vec<ItemId>,
        pivot: Pt,
        by: Orient,
    },
    Delete(Vec<ItemId>),
    SetText {
        item: ItemId,
        field: TextField,
        text: Arc<str>,
    },
    Batch(Vec<Command>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandError {
    OutOfBounds,
    InvalidWire,
    MissingItem(ItemId),
    DuplicateItem(ItemId),
    InvalidTextField { item: ItemId, field: TextField },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds => f.write_str("item or pin lies outside K1 sheet limits"),
            Self::InvalidWire => f.write_str("wire must be axis-aligned with distinct ends"),
            Self::MissingItem(item) => write!(f, "item {item:?} does not exist"),
            Self::DuplicateItem(item) => write!(f, "duplicate item {item:?} in command"),
            Self::InvalidTextField { item, field } => {
                write!(f, "text field {field:?} is invalid for item {item:?}")
            }
        }
    }
}

impl std::error::Error for CommandError {}

impl From<StoreError> for CommandError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::OutOfBounds => Self::OutOfBounds,
            StoreError::InvalidWire => Self::InvalidWire,
            StoreError::InvalidId => unreachable!("commands never supply raw arena ids"),
        }
    }
}

impl From<GeomError> for CommandError {
    fn from(_: GeomError) -> Self {
        Self::OutOfBounds
    }
}

/// What one `apply`, `undo` or `redo` changed.
///
/// `added` and `removed` are membership deltas only. An item that keeps its id while its
/// geometry changes is in neither list: a moved or reoriented item, the surviving wire of a
/// merge (lowest id) and the first piece of a split wire (D2). `dirty` always covers the old
/// and new bounds of every such item, and `revision` always advances on a real change, so
/// consumers repaint and invalidate per-item caches from `dirty`/`revision`, not from the id
/// lists. An empty pair of lists does not mean "nothing changed"; `dirty: None` does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub revision: u64,
    /// No region for a validated no-op; callers must not repaint in that case.
    pub dirty: Option<Rect>,
    /// Added and removed IDs are unordered sets, independent of command input order.
    pub added: Vec<ItemId>,
    pub removed: Vec<ItemId>,
    pub nets_changed: bool,
}

#[derive(Default)]
pub(crate) struct ChangeAcc {
    pub(crate) dirty: Option<Rect>,
    pub(crate) added: Vec<ItemId>,
    pub(crate) removed: Vec<ItemId>,
    pub(crate) nets_changed: bool,
}

impl ChangeAcc {
    pub(crate) fn record_dirty(&mut self, rect: Rect) {
        self.dirty = Some(match self.dirty {
            Some(existing) => existing.union(rect),
            None => rect,
        });
    }

    pub(crate) fn record_added(&mut self, id: ItemId) {
        if let Some(pos) = self.removed.iter().position(|&existing| existing == id) {
            self.removed.swap_remove(pos);
        } else {
            self.added.push(id);
        }
    }

    pub(crate) fn record_removed(&mut self, id: ItemId) {
        if let Some(pos) = self.added.iter().position(|&existing| existing == id) {
            self.added.swap_remove(pos);
        } else {
            self.removed.push(id);
        }
    }

    pub(crate) fn into_change(self, revision: u64) -> Change {
        Change {
            revision,
            dirty: self.dirty,
            added: self.added,
            removed: self.removed,
            nets_changed: self.nets_changed,
        }
    }
}

pub(crate) fn apply_command(doc: &mut Doc, cmd: Command) -> Result<Change, CommandError> {
    let mut acc = ChangeAcc::default();
    let undo_op = apply_inner(doc, cmd, &mut acc)?;
    if acc.dirty.is_some() {
        doc.history.push_undo(undo_op);
        doc.bump_revision();
    }
    Ok(acc.into_change(doc.revision()))
}

fn apply_inner(doc: &mut Doc, cmd: Command, acc: &mut ChangeAcc) -> Result<UndoOp, CommandError> {
    match cmd {
        Command::PlacePart {
            part,
            local_pins,
            local_bounds,
        } => {
            let has_pins = !local_pins.is_empty();
            let (id, bounds) = doc.insert_part_raw(part, &local_pins, local_bounds)?;
            acc.record_dirty(bounds);
            acc.record_added(ItemId::Part(id));
            if has_pins {
                acc.nets_changed = true;
            }
            Ok(UndoOp::RemovePart(id))
        }
        Command::AddWires(wires) => {
            for &wire in &wires {
                Doc::validate_wire(wire)?;
            }
            if wires.is_empty() {
                return Ok(UndoOp::Batch(Vec::new()));
            }
            // T6.1 (INV2): split and merge into normal form inside apply, with
            // index and connectivity updated by the raw wire edits below.
            let plan = crate::route::plan_add_wires(doc, &wires);
            Ok(apply_wire_plan(doc, plan, acc))
        }
        Command::AddLabel(label) => {
            let (id, bounds) = doc.insert_label_raw(label)?;
            acc.record_dirty(bounds);
            acc.record_added(ItemId::Label(id));
            acc.nets_changed = true;
            Ok(UndoOp::RemoveLabel(id))
        }
        Command::Move { items, delta } => {
            validate_move(doc, &items, delta)?;
            apply_move_raw(doc, &items, delta, acc);
            let inv_delta = if items.is_empty() { Pt::ZERO } else { -delta };
            Ok(UndoOp::Move {
                items,
                delta: inv_delta,
            })
        }
        Command::Reorient { items, pivot, by } => {
            validate_reorient(doc, &items, pivot, by)?;
            apply_reorient_raw(doc, &items, pivot, by, acc);
            Ok(UndoOp::Reorient {
                items,
                pivot,
                by: by.inverse(),
            })
        }
        Command::Delete(items) => {
            for &item in &items {
                ensure_exists(doc, item)?;
            }
            check_unique(&items)?;

            let mut parts = Vec::new();
            let mut wires = Vec::new();
            let mut labels = Vec::new();
            for item in items {
                match item {
                    ItemId::Part(id) => {
                        let (stored, bounds) = doc
                            .remove_stored_part_raw(id)
                            .expect("validated part exists");
                        acc.record_dirty(bounds);
                        acc.record_removed(item);
                        if !stored.pins.is_empty() {
                            acc.nets_changed = true;
                        }
                        parts.push((id, stored));
                    }
                    ItemId::Wire(id) => {
                        let (wire, bounds) =
                            doc.remove_wire_raw(id).expect("validated wire exists");
                        acc.record_dirty(bounds);
                        acc.record_removed(item);
                        acc.nets_changed = true;
                        wires.push((id, wire));
                    }
                    ItemId::Label(id) => {
                        let (label, bounds) =
                            doc.remove_label_raw(id).expect("validated label exists");
                        acc.record_dirty(bounds);
                        acc.record_removed(item);
                        acc.nets_changed = true;
                        labels.push((id, label));
                    }
                }
            }
            let mut points: Vec<_> = wires.iter().flat_map(|(_, w)| [w.a, w.b]).collect();
            let plan = crate::route::plan_delete_merges(doc, &mut points);
            let merge_inverse = apply_wire_plan(doc, plan, acc);
            // Undo batches execute in reverse: undo merges before restoring items.
            Ok(UndoOp::Batch(vec![
                UndoOp::DeleteRestore {
                    parts,
                    wires,
                    labels,
                },
                merge_inverse,
            ]))
        }
        Command::SetText { item, field, text } => {
            let (old_text, bounds, nets_changed) =
                doc.set_text_raw(item, field, Arc::clone(&text))?;
            if old_text == text {
                return Ok(UndoOp::Batch(Vec::new()));
            }
            acc.record_dirty(bounds);
            if nets_changed {
                acc.nets_changed = true;
            }
            Ok(UndoOp::SetText {
                item,
                field,
                text: old_text,
            })
        }
        Command::Batch(cmds) => apply_batch(doc, cmds, acc),
    }
}

fn apply_batch(
    doc: &mut Doc,
    cmds: Vec<Command>,
    acc: &mut ChangeAcc,
) -> Result<UndoOp, CommandError> {
    let parts_len = doc.parts.slots.len();
    let wires_len = doc.wires.slots.len();
    let labels_len = doc.labels.slots.len();
    let mut undo_ops = Vec::with_capacity(cmds.len());
    for cmd in cmds {
        match apply_inner(doc, cmd, acc) {
            Ok(op) => undo_ops.push(op),
            Err(err) => {
                let mut scratch = ChangeAcc::default();
                for op in undo_ops.into_iter().rev() {
                    undo::apply_undo_op(doc, op, &mut scratch);
                }
                // Rolled-back allocations were never issued by a committed command.
                // Preserve all previously issued IDs while restoring exact pre-batch state.
                doc.parts.slots.truncate(parts_len);
                doc.wires.slots.truncate(wires_len);
                doc.labels.slots.truncate(labels_len);
                return Err(err);
            }
        }
    }
    Ok(UndoOp::Batch(undo_ops))
}

fn ensure_exists(doc: &Doc, item: ItemId) -> Result<(), CommandError> {
    let exists = match item {
        ItemId::Part(id) => doc.parts.get(id.0).is_some(),
        ItemId::Wire(id) => doc.wires.get(id.0).is_some(),
        ItemId::Label(id) => doc.labels.get(id.0).is_some(),
    };
    if exists {
        Ok(())
    } else {
        Err(CommandError::MissingItem(item))
    }
}

fn check_unique(items: &[ItemId]) -> Result<(), CommandError> {
    if items.len() <= 1 {
        return Ok(());
    }
    if items.len() <= 16 {
        for i in 0..items.len() {
            for j in (i + 1)..items.len() {
                if items[i] == items[j] {
                    return Err(CommandError::DuplicateItem(items[i]));
                }
            }
        }
        return Ok(());
    }
    let mut keys: SmallVec<[u64; 128]> = items
        .iter()
        .map(|&id| {
            let (tag, raw) = match id {
                ItemId::Part(p) => (0u64, p.0),
                ItemId::Wire(w) => (1u64, w.0),
                ItemId::Label(l) => (2u64, l.0),
            };
            (tag << 32) | u64::from(raw)
        })
        .collect();
    keys.sort_unstable();
    for pair in keys.windows(2) {
        if pair[0] == pair[1] {
            let tag = (pair[0] >> 32) as u8;
            let raw = pair[0] as u32;
            let dup = match tag {
                0 => ItemId::Part(PartId(raw)),
                1 => ItemId::Wire(WireId(raw)),
                _ => ItemId::Label(LabelId(raw)),
            };
            return Err(CommandError::DuplicateItem(dup));
        }
    }
    Ok(())
}

fn validate_move(doc: &Doc, items: &[ItemId], delta: Pt) -> Result<(), CommandError> {
    for &item in items {
        match item {
            ItemId::Part(id) => {
                let stored = doc.parts.get(id.0).ok_or(CommandError::MissingItem(item))?;
                let new_at = stored.part.at.checked_add(delta)?;
                for &pin in &stored.pins {
                    pin.checked_add(delta)?;
                }
                Doc::part_bounds_at(new_at, stored.part.orient, stored.local_bounds)?;
            }
            ItemId::Wire(id) => {
                let wire = doc.wires.get(id.0).ok_or(CommandError::MissingItem(item))?;
                wire.a.checked_add(delta)?;
                wire.b.checked_add(delta)?;
            }
            ItemId::Label(id) => {
                let label = doc
                    .labels
                    .get(id.0)
                    .ok_or(CommandError::MissingItem(item))?;
                label.at.checked_add(delta)?;
            }
        }
    }
    check_unique(items)
}

fn validate_reorient(
    doc: &Doc,
    items: &[ItemId],
    pivot: Pt,
    by: Orient,
) -> Result<(), CommandError> {
    for &item in items {
        match item {
            ItemId::Part(id) => {
                let stored = doc.parts.get(id.0).ok_or(CommandError::MissingItem(item))?;
                let new_at = by.checked_apply_around(stored.part.at, pivot)?;
                let new_orient = stored.part.orient.then(by);
                for &pin in &stored.pins {
                    by.checked_apply_around(pin, pivot)?;
                }
                Doc::part_bounds_at(new_at, new_orient, stored.local_bounds)?;
            }
            ItemId::Wire(id) => {
                let wire = doc.wires.get(id.0).ok_or(CommandError::MissingItem(item))?;
                by.checked_apply_around(wire.a, pivot)?;
                by.checked_apply_around(wire.b, pivot)?;
            }
            ItemId::Label(id) => {
                let label = doc
                    .labels
                    .get(id.0)
                    .ok_or(CommandError::MissingItem(item))?;
                by.checked_apply_around(label.at, pivot)?;
            }
        }
    }
    check_unique(items)
}

pub(crate) fn apply_move_raw(doc: &mut Doc, items: &[ItemId], delta: Pt, acc: &mut ChangeAcc) {
    if delta == Pt::ZERO {
        return;
    }

    // Pass 1: detach old positions from spatial index, connectivity, and point incidence.
    for &item in items {
        match item {
            ItemId::Part(id) => {
                let old_bounds = doc
                    .index
                    .remove(item)
                    .expect("validated part must exist in index");
                acc.record_dirty(old_bounds);
                doc.connectivity.remove_part(id);
                let pins = &doc.parts.get(id.0).expect("validated part exists").pins;
                if !pins.is_empty() {
                    acc.nets_changed = true;
                }
                for (pin, &at) in pins.iter().enumerate() {
                    doc.points.remove(at, End::Pin { part: id, pin });
                }
            }
            ItemId::Wire(id) => {
                let old_bounds = doc
                    .index
                    .remove(item)
                    .expect("validated wire must exist in index");
                acc.record_dirty(old_bounds);
                doc.connectivity.remove_wire(id);
                let wire = *doc.wires.get(id.0).expect("validated wire exists");
                doc.points.remove(wire.a, End::Wire(id));
                doc.points.remove(wire.b, End::Wire(id));
                acc.nets_changed = true;
            }
            ItemId::Label(id) => {
                let old_bounds = doc
                    .index
                    .remove(item)
                    .expect("validated label must exist in index");
                acc.record_dirty(old_bounds);
                doc.connectivity.remove_label(id);
                acc.nets_changed = true;
            }
        }
    }

    // Pass 2: mutate coordinates in-place and re-register in spatial index, connectivity, points.
    for &item in items {
        match item {
            ItemId::Part(id) => {
                let stored = doc.parts.get_mut(id.0).expect("validated part exists");
                stored.part.at = stored.part.at + delta;
                for pin in &mut stored.pins {
                    *pin = *pin + delta;
                }
                let area = Doc::part_envelope_raw(
                    stored.part.at,
                    stored.part.orient,
                    stored.local_bounds,
                    &stored.pins,
                )
                .expect("validated moved part bounds");
                doc.index.insert(item, area).unwrap();
                doc.connectivity.add_part(id, &stored.pins);
                for (pin, &at) in stored.pins.iter().enumerate() {
                    doc.points.insert(at, End::Pin { part: id, pin });
                }
                acc.record_dirty(area);
            }
            ItemId::Wire(id) => {
                let wire = doc.wires.get_mut(id.0).expect("validated wire exists");
                wire.a = wire.a + delta;
                wire.b = wire.b + delta;
                let moved = *wire;
                let area = Rect::from_points(moved.a, moved.b).unwrap();
                doc.index.insert(item, area).unwrap();
                doc.connectivity.add_wire(id, moved);
                doc.points.insert(moved.a, End::Wire(id));
                doc.points.insert(moved.b, End::Wire(id));
                acc.record_dirty(area);
            }
            ItemId::Label(id) => {
                let label = doc.labels.get_mut(id.0).expect("validated label exists");
                label.at = label.at + delta;
                let area = Rect {
                    min: label.at,
                    max: label.at,
                };
                doc.index.insert(item, area).unwrap();
                doc.connectivity.add_label(id, label);
                acc.record_dirty(area);
            }
        }
    }
}

pub(crate) fn apply_reorient_raw(
    doc: &mut Doc,
    items: &[ItemId],
    pivot: Pt,
    by: Orient,
    acc: &mut ChangeAcc,
) {
    if by == Orient::IDENTITY {
        return;
    }

    // Pass 1: detach old state.
    for &item in items {
        match item {
            ItemId::Part(id) => {
                let old_bounds = doc
                    .index
                    .remove(item)
                    .expect("validated part must exist in index");
                acc.record_dirty(old_bounds);
                doc.connectivity.remove_part(id);
                let pins = &doc.parts.get(id.0).expect("validated part exists").pins;
                if !pins.is_empty() {
                    acc.nets_changed = true;
                }
                for (pin, &at) in pins.iter().enumerate() {
                    doc.points.remove(at, End::Pin { part: id, pin });
                }
            }
            ItemId::Wire(id) => {
                let old_bounds = doc
                    .index
                    .remove(item)
                    .expect("validated wire must exist in index");
                acc.record_dirty(old_bounds);
                doc.connectivity.remove_wire(id);
                let wire = *doc.wires.get(id.0).expect("validated wire exists");
                doc.points.remove(wire.a, End::Wire(id));
                doc.points.remove(wire.b, End::Wire(id));
                acc.nets_changed = true;
            }
            ItemId::Label(id) => {
                let old_bounds = doc
                    .index
                    .remove(item)
                    .expect("validated label must exist in index");
                acc.record_dirty(old_bounds);
                doc.connectivity.remove_label(id);
                acc.nets_changed = true;
            }
        }
    }

    // Pass 2: reorient in-place and re-register.
    for &item in items {
        match item {
            ItemId::Part(id) => {
                let stored = doc.parts.get_mut(id.0).expect("validated part exists");
                stored.part.at = by
                    .checked_apply_around(stored.part.at, pivot)
                    .expect("validated part anchor");
                stored.part.orient = stored.part.orient.then(by);
                for pin in &mut stored.pins {
                    *pin = by.checked_apply_around(*pin, pivot).expect("validated pin");
                }
                let area = Doc::part_envelope_raw(
                    stored.part.at,
                    stored.part.orient,
                    stored.local_bounds,
                    &stored.pins,
                )
                .expect("validated reoriented part bounds");
                doc.index.insert(item, area).unwrap();
                doc.connectivity.add_part(id, &stored.pins);
                for (pin, &at) in stored.pins.iter().enumerate() {
                    doc.points.insert(at, End::Pin { part: id, pin });
                }
                acc.record_dirty(area);
            }
            ItemId::Wire(id) => {
                let wire = doc.wires.get_mut(id.0).expect("validated wire exists");
                wire.a = by
                    .checked_apply_around(wire.a, pivot)
                    .expect("validated wire.a");
                wire.b = by
                    .checked_apply_around(wire.b, pivot)
                    .expect("validated wire.b");
                let moved = *wire;
                let area = Rect::from_points(moved.a, moved.b).unwrap();
                doc.index.insert(item, area).unwrap();
                doc.connectivity.add_wire(id, moved);
                doc.points.insert(moved.a, End::Wire(id));
                doc.points.insert(moved.b, End::Wire(id));
                acc.record_dirty(area);
            }
            ItemId::Label(id) => {
                let label = doc.labels.get_mut(id.0).expect("validated label exists");
                label.at = by
                    .checked_apply_around(label.at, pivot)
                    .expect("validated label.at");
                let area = Rect {
                    min: label.at,
                    max: label.at,
                };
                doc.index.insert(item, area).unwrap();
                doc.connectivity.add_label(id, label);
                acc.record_dirty(area);
            }
        }
    }
}

fn apply_wire_plan(doc: &mut Doc, plan: crate::route::AddWiresPlan, acc: &mut ChangeAcc) -> UndoOp {
    if plan.remove.is_empty() && plan.insert.is_empty() {
        return UndoOp::Batch(Vec::new());
    }
    let kept: rustc_hash::FxHashSet<WireId> =
        plan.insert.iter().filter_map(|(keep, _)| *keep).collect();
    let mut removed: Vec<(WireId, Wire)> = Vec::with_capacity(plan.remove.len());
    for (id, _) in &plan.remove {
        let (wire, bounds) = doc.remove_wire_raw(*id).expect("planned wire must exist");
        acc.record_dirty(bounds);
        if !kept.contains(id) {
            acc.record_removed(ItemId::Wire(*id));
        }
        removed.push((*id, wire));
    }
    let mut added: Vec<WireId> = Vec::with_capacity(plan.insert.len());
    for (keep, wire) in plan.insert {
        let (id, bounds) = match keep {
            Some(id) => {
                let bounds = doc
                    .restore_wire_raw(id, wire)
                    .expect("planned restore must target reserved empty slot");
                (id, bounds)
            }
            None => doc
                .insert_wire_raw(wire)
                .expect("pre-validated wire insert must succeed"),
        };
        acc.record_dirty(bounds);
        if keep.is_none() {
            acc.record_added(ItemId::Wire(id));
        }
        acc.nets_changed = true;
        added.push(id);
    }
    UndoOp::ReplaceWires { removed, added }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Connectivity, GRID_NM, SHEET_LIMIT_NM, SymbolId};
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
        Rect::from_points(Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }).unwrap()
    }

    fn full_sheet() -> Rect {
        rect(
            -SHEET_LIMIT_NM,
            -SHEET_LIMIT_NM,
            SHEET_LIMIT_NM,
            SHEET_LIMIT_NM,
        )
    }

    fn make_part(at: Pt, orient: Orient, designator: &str, value: &str) -> Part {
        Part {
            symbol: SymbolId(0),
            at,
            orient,
            designator: designator.into(),
            value: value.into(),
        }
    }

    fn assert_doc_consistent(doc: &Doc) {
        let oracle = Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &w)| (id, w)),
            doc.labels(),
            doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
        );
        crate::connectivity::tests::assert_partitions(&doc.connectivity, &oracle);

        let sheet = full_sheet();
        let mut found = Vec::new();
        doc.query(sheet, &mut found);
        let mut expected: Vec<ItemId> = doc
            .parts()
            .filter_map(|(id, _)| {
                let mut b = doc.part_body(id).unwrap();
                for &p in doc.pins(id).unwrap() {
                    b = b.union(Rect { min: p, max: p });
                }
                b.intersects(sheet).then_some(ItemId::Part(id))
            })
            .chain(doc.wires().filter_map(|(id, w)| {
                Rect::from_points(w.a, w.b)
                    .unwrap()
                    .intersects(sheet)
                    .then_some(ItemId::Wire(id))
            }))
            .chain(
                doc.labels()
                    .filter_map(|(id, l)| sheet.contains(l.at).then_some(ItemId::Label(id))),
            )
            .collect();
        found.sort_by_key(|&id| crate::hit::item_key(id));
        expected.sort_by_key(|&id| crate::hit::item_key(id));
        assert_eq!(found, expected);

        let mut counts = HashMap::<Pt, usize>::new();
        for (_, w) in doc.wires() {
            *counts.entry(w.a).or_default() += 1;
            *counts.entry(w.b).or_default() += 1;
        }
        for (id, _) in doc.parts() {
            for &p in doc.pins(id).unwrap() {
                *counts.entry(p).or_default() += 1;
            }
        }
        let mut junctions = Vec::new();
        doc.junctions(sheet, &mut junctions);
        let mut expected_junctions: Vec<Pt> = counts
            .into_iter()
            .filter_map(|(p, n)| (n >= 3).then_some(p))
            .collect();
        junctions.sort_by_key(|p| (p.x, p.y));
        expected_junctions.sort_by_key(|p| (p.x, p.y));
        assert_eq!(junctions, expected_junctions);

        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        let loaded = Doc::load(bytes.as_slice()).unwrap();
        assert_eq!(&loaded, doc);
        assert_eq!(loaded.revision(), doc.revision());
    }

    #[test]
    fn no_op_commands_preserve_history_revision_and_redo() {
        let label = Label {
            at: Pt { x: 100, y: 0 },
            text: "GND".into(),
        };
        let mut doc = Doc::new();
        doc.apply(Command::AddLabel(label.clone())).unwrap();
        doc.apply(Command::AddLabel(Label {
            at: Pt::ZERO,
            text: "VCC".into(),
        }))
        .unwrap();
        doc.undo().unwrap(); // Keep a live label and an available redo operation.
        let before = doc.save_to_vec_for_test();
        let revision = doc.revision();
        let item = ItemId::Label(crate::LabelId(0));
        for cmd in [
            Command::Batch(vec![]),
            Command::AddWires(vec![]),
            Command::Delete(vec![]),
            Command::Move {
                items: vec![],
                delta: Pt { x: 1, y: 0 },
            },
            Command::Reorient {
                items: vec![],
                pivot: Pt::ZERO,
                by: Orient::ROT_90,
            },
            Command::Move {
                items: vec![item],
                delta: Pt::ZERO,
            },
            Command::Reorient {
                items: vec![item],
                pivot: Pt::ZERO,
                by: Orient::IDENTITY,
            },
            Command::SetText {
                item,
                field: TextField::Label,
                text: label.text,
            },
            Command::Batch(vec![Command::Batch(vec![]), Command::Delete(vec![])]),
        ] {
            let change = doc.apply(cmd).unwrap();
            assert_eq!(change.revision, revision);
            assert_eq!(change.dirty, None);
            assert!(change.added.is_empty() && change.removed.is_empty());
            assert!(!change.nets_changed);
            assert_eq!(doc.save_to_vec_for_test(), before);
            assert_doc_consistent(&doc);
        }
        assert!(doc.redo().is_some(), "no-op must not consume redo history");
        assert_eq!(doc.labels().count(), 2);
        doc.undo().unwrap();
        doc.undo().unwrap();
        assert!(doc.undo().is_none(), "no-op must not add undo entries");
        // A semantically inert command still validates its target and field.
        assert!(matches!(
            doc.apply(Command::Move {
                items: vec![item],
                delta: Pt::ZERO
            }),
            Err(CommandError::MissingItem(_))
        ));
    }

    #[test]
    fn move_100_items_success_and_failure_rollback() {
        let mut doc = Doc::new();
        let local_bounds = rect(-10, -20, 10, 20);
        let local_pins = vec![Pt { x: -10, y: 0 }, Pt { x: 10, y: 0 }];
        let mut items = Vec::with_capacity(100);
        for i in 0..40 {
            let at = Pt { x: i * 100, y: 0 };
            let ch = doc
                .apply(Command::PlacePart {
                    part: make_part(at, Orient::IDENTITY, "R", "1k"),
                    local_pins: local_pins.clone(),
                    local_bounds,
                })
                .unwrap();
            items.extend(ch.added);
        }
        let ch_wires = doc
            .apply(Command::AddWires(
                (0..40)
                    .map(|i| Wire {
                        a: Pt { x: i * 100, y: 50 },
                        b: Pt {
                            x: i * 100 + 20,
                            y: 50,
                        },
                    })
                    .collect(),
            ))
            .unwrap();
        items.extend(ch_wires.added);
        for i in 0..20 {
            let ch = doc
                .apply(Command::AddLabel(Label {
                    at: Pt { x: i * 100, y: 100 },
                    text: "NET".into(),
                }))
                .unwrap();
            items.extend(ch.added);
        }
        assert_eq!(items.len(), 100);

        // Undo one step then redo to verify non-empty redo stack survives invalid commands.
        doc.undo().unwrap();
        let before_invalid = doc.save_to_vec_for_test();
        let rev_before = doc.revision();

        // Failure case 1: 100th item is out-of-bounds after delta.
        let mut oob_items = items.clone();
        oob_items.pop(); // 99 valid items (LabelId(19) was undone so replace with LabelId(18), etc.)
        let last_label = doc
            .apply(Command::AddLabel(Label {
                at: Pt {
                    x: SHEET_LIMIT_NM - 5,
                    y: 0,
                },
                text: "EDGE".into(),
            }))
            .unwrap()
            .added[0];
        let mut items_with_edge = items[..99].to_vec();
        items_with_edge.push(last_label);
        assert_eq!(items_with_edge.len(), 100);
        // Undo once so redo stack has something, then re-establish state with edge label.
        doc.apply(Command::AddLabel(Label {
            at: Pt { x: 0, y: 200 },
            text: "R".into(),
        }))
        .unwrap();
        doc.undo().unwrap();
        let snap_bytes = doc.save_to_vec_for_test();
        let rev_snap = doc.revision();

        let err = doc.apply(Command::Move {
            items: items_with_edge.clone(),
            delta: Pt { x: 10, y: 0 },
        });
        assert_eq!(err, Err(CommandError::OutOfBounds));
        assert_eq!(doc.revision(), rev_snap);
        assert_eq!(doc.save_to_vec_for_test(), snap_bytes);
        assert_doc_consistent(&doc);

        // Failure case 2: duplicate ID at the end of a 100-item move.
        let mut dup_items = items[..99].to_vec();
        dup_items.push(items[0]);
        assert_eq!(
            doc.apply(Command::Move {
                items: dup_items,
                delta: Pt { x: 10, y: 0 },
            }),
            Err(CommandError::DuplicateItem(items[0]))
        );
        assert_eq!(doc.revision(), rev_snap);
        assert_eq!(doc.save_to_vec_for_test(), snap_bytes);

        // Failure case 3: missing ID at the end of a 100-item move.
        let mut missing_items = items[..99].to_vec();
        let missing = ItemId::Part(PartId(999));
        missing_items.push(missing);
        assert_eq!(
            doc.apply(Command::Move {
                items: missing_items,
                delta: Pt { x: 10, y: 0 },
            }),
            Err(CommandError::MissingItem(missing))
        );
        assert_eq!(doc.revision(), rev_snap);
        assert_eq!(doc.save_to_vec_for_test(), snap_bytes);

        // Redo stack was NOT cleared by the failed commands.
        assert!(doc.redo().is_some());
        doc.undo().unwrap();
        let _ = (before_invalid, rev_before);

        // Valid 100-item move + undo + redo.
        let pre_move_rev = doc.revision();
        let ch = doc
            .apply(Command::Move {
                items: items_with_edge.clone(),
                delta: Pt { x: -50, y: 25 },
            })
            .unwrap();
        assert_eq!(ch.revision, pre_move_rev + 1);
        assert!(ch.nets_changed);
        // Dirty rect covers old & new extents of all moved items including edge label at SHEET_LIMIT_NM - 5.
        assert!(ch.dirty.unwrap().contains(Pt {
            x: SHEET_LIMIT_NM - 5,
            y: 0
        }));
        assert!(ch.dirty.unwrap().contains(Pt {
            x: SHEET_LIMIT_NM - 55,
            y: 25
        }));
        assert!(ch.dirty.unwrap().contains(Pt { x: -10, y: -20 }));
        assert_doc_consistent(&doc);

        let moved_snap = doc.save_to_vec_for_test();
        let undo_ch = doc.undo().unwrap();
        assert_eq!(undo_ch.dirty, ch.dirty);
        assert_eq!(doc.revision(), pre_move_rev + 2);
        assert_doc_consistent(&doc);
        let loaded_snap = Doc::load(snap_bytes.as_slice()).unwrap();
        assert!(doc.content_eq(&loaded_snap));

        let redo_ch = doc.redo().unwrap();
        assert_eq!(redo_ch.dirty, ch.dirty);
        assert!(doc.content_eq(&Doc::load(moved_snap.as_slice()).unwrap()));
        assert_doc_consistent(&doc);
    }

    #[test]
    fn delete_restore_reorient_pivots_set_text_and_batch_rollback() {
        let mut doc = Doc::new();
        let bounds = rect(-20, -10, 20, 10);
        let pins = vec![Pt { x: -20, y: 0 }, Pt { x: 20, y: 0 }];
        let p0 = match doc
            .apply(Command::PlacePart {
                part: make_part(Pt { x: 100, y: 0 }, Orient::IDENTITY, "R1", "10k"),
                local_pins: pins.clone(),
                local_bounds: bounds,
            })
            .unwrap()
            .added[0]
        {
            ItemId::Part(id) => id,
            _ => unreachable!(),
        };
        let w0 = match doc
            .apply(Command::AddWires(vec![Wire {
                a: Pt { x: 120, y: 0 },
                b: Pt { x: 200, y: 0 },
            }]))
            .unwrap()
            .added[0]
        {
            ItemId::Wire(id) => id,
            _ => unreachable!(),
        };
        let l0 = match doc
            .apply(Command::AddLabel(Label {
                at: Pt { x: 200, y: 0 },
                text: "SIG".into(),
            }))
            .unwrap()
            .added[0]
        {
            ItemId::Label(id) => id,
            _ => unreachable!(),
        };
        let l1 = match doc
            .apply(Command::AddLabel(Label {
                at: Pt { x: 500, y: 0 },
                text: "OTHER".into(),
            }))
            .unwrap()
            .added[0]
        {
            ItemId::Label(id) => id,
            _ => unreachable!(),
        };
        assert_ne!(
            doc.net_at(Pt { x: 200, y: 0 }),
            doc.net_at(Pt { x: 500, y: 0 })
        );

        // SetText joins nets when label becomes "SIG", and separates on undo.
        let c = doc
            .apply(Command::SetText {
                item: ItemId::Label(l1),
                field: TextField::Label,
                text: "SIG".into(),
            })
            .unwrap();
        assert!(c.nets_changed);
        assert_eq!(c.dirty, Some(rect(500, 0, 500, 0)));
        assert_eq!(
            doc.net_at(Pt { x: 200, y: 0 }),
            doc.net_at(Pt { x: 500, y: 0 })
        );
        assert_doc_consistent(&doc);

        let c_part = doc
            .apply(Command::SetText {
                item: ItemId::Part(p0),
                field: TextField::Value,
                text: "22k".into(),
            })
            .unwrap();
        assert!(!c_part.nets_changed);
        assert_eq!(doc.part(p0).unwrap().value.as_ref(), "22k");

        // Invalid SetText field errors cleanly.
        assert_eq!(
            doc.apply(Command::SetText {
                item: ItemId::Part(p0),
                field: TextField::Label,
                text: "bad".into(),
            }),
            Err(CommandError::InvalidTextField {
                item: ItemId::Part(p0),
                field: TextField::Label
            })
        );
        assert_eq!(
            doc.apply(Command::SetText {
                item: ItemId::Wire(w0),
                field: TextField::Designator,
                text: "bad".into(),
            }),
            Err(CommandError::InvalidTextField {
                item: ItemId::Wire(w0),
                field: TextField::Designator
            })
        );
        assert_eq!(
            doc.apply(Command::SetText {
                item: ItemId::Label(l0),
                field: TextField::Designator,
                text: "bad".into(),
            }),
            Err(CommandError::InvalidTextField {
                item: ItemId::Label(l0),
                field: TextField::Designator
            })
        );

        // Reorient group around an off-origin pivot through all 8 Orient variants.
        let pivot = Pt { x: 150, y: 50 };
        let group = vec![ItemId::Part(p0), ItemId::Wire(w0), ItemId::Label(l0)];
        for &by in &Orient::ALL {
            let pre_pins = doc.pins(p0).unwrap().to_vec();
            let pre_wire = *doc.wire(w0).unwrap();
            let pre_label = doc.label(l0).unwrap().at;
            let pre_bounds = rect(80, -10, 200, 10);
            let before_orient = doc.save_to_vec_for_test();
            let ch = doc
                .apply(Command::Reorient {
                    items: group.clone(),
                    pivot,
                    by,
                })
                .unwrap();
            if by == Orient::IDENTITY {
                assert_eq!(ch.dirty, None);
                assert_eq!(doc.save_to_vec_for_test(), before_orient);
                assert_doc_consistent(&doc);
                continue; // Identity adds no undo step.
            }
            let expected_pins: Vec<Pt> = pre_pins
                .iter()
                .map(|&p| by.checked_apply_around(p, pivot).unwrap())
                .collect();
            assert_eq!(doc.pins(p0).unwrap(), expected_pins.as_slice());
            let new_wire = *doc.wire(w0).unwrap();
            assert_eq!(
                new_wire.a,
                by.checked_apply_around(pre_wire.a, pivot).unwrap()
            );
            assert_eq!(
                new_wire.b,
                by.checked_apply_around(pre_wire.b, pivot).unwrap()
            );
            assert_eq!(
                doc.label(l0).unwrap().at,
                by.checked_apply_around(pre_label, pivot).unwrap()
            );
            assert!(ch.dirty.unwrap().contains_rect(pre_bounds));
            assert!(ch.dirty.unwrap().contains(new_wire.a));
            assert!(ch.dirty.unwrap().contains(new_wire.b));
            assert_doc_consistent(&doc);

            doc.undo().unwrap();
            assert_eq!(doc.pins(p0).unwrap(), pre_pins.as_slice());
            assert_eq!(*doc.wire(w0).unwrap(), pre_wire);
            assert_eq!(doc.label(l0).unwrap().at, pre_label);
            assert_doc_consistent(&doc);

            doc.redo().unwrap();
            assert_doc_consistent(&doc);
            doc.undo().unwrap();
        }

        // Delete parts, wires, labels; undo restores exact IDs; new edit clears redo.
        let del_ch = doc.apply(Command::Delete(group.clone())).unwrap();
        assert_eq!(del_ch.removed, group);
        assert!(del_ch.added.is_empty());
        assert!(doc.part(p0).is_none());
        assert!(doc.wire(w0).is_none());
        assert!(doc.label(l0).is_none());
        assert_doc_consistent(&doc);

        let undo_del = doc.undo().unwrap();
        assert_eq!(undo_del.added, group);
        assert!(doc.part(p0).is_some());
        assert!(doc.wire(w0).is_some());
        assert!(doc.label(l0).is_some());
        assert_doc_consistent(&doc);

        // New edit clears redo stack.
        doc.undo().unwrap(); // undoes SetText(p0) -> redo has SetText(p0)
        assert!(doc.redo().is_some());
        doc.undo().unwrap();
        doc.apply(Command::AddLabel(Label {
            at: Pt { x: -10, y: -10 },
            text: "CLR".into(),
        }))
        .unwrap();
        assert!(doc.redo().is_none());

        // Batch failure rollback (nested Batch with PlacePart + AddWires + AddLabel + Move + Invalid).
        doc.undo().unwrap(); // put something on redo? no, this adds to redo!
        let before_batch_bytes = doc.save_to_vec_for_test();
        let before_batch_rev = doc.revision();
        let bad_batch = Command::Batch(vec![
            Command::PlacePart {
                part: make_part(Pt { x: -500, y: -500 }, Orient::IDENTITY, "C1", "100n"),
                local_pins: vec![Pt::ZERO],
                local_bounds: rect(-5, -5, 5, 5),
            },
            Command::AddWires(vec![Wire {
                a: Pt::ZERO,
                b: Pt { x: 10, y: 0 },
            }]),
            Command::Batch(vec![
                Command::AddLabel(Label {
                    at: Pt { x: 10, y: 0 },
                    text: "TEMP".into(),
                }),
                Command::Move {
                    items: vec![ItemId::Part(p0)],
                    delta: Pt { x: 50, y: 50 },
                },
                Command::AddWires(vec![Wire {
                    a: Pt::ZERO,
                    b: Pt { x: 10, y: 10 },
                }]), // diagonal -> invalid!
            ]),
        ]);
        assert_eq!(doc.apply(bad_batch), Err(CommandError::InvalidWire));
        assert_eq!(doc.revision(), before_batch_rev);
        assert_eq!(doc.save_to_vec_for_test(), before_batch_bytes);
        assert_doc_consistent(&doc);
        // Redo stack survived the failed batch.
        assert!(doc.redo().is_some());
    }

    #[test]
    fn dirty_rect_includes_old_and_new_bounds_on_partial_change() {
        let mut doc = Doc::new();
        let bounds = rect(-100, -50, 100, 50);
        let p0 = doc
            .apply(Command::PlacePart {
                part: make_part(Pt::ZERO, Orient::IDENTITY, "U1", "OPA"),
                local_pins: vec![Pt { x: -120, y: 0 }, Pt { x: 120, y: 0 }],
                local_bounds: bounds,
            })
            .unwrap();
        assert_eq!(p0.dirty, Some(rect(-120, -50, 120, 50)));
        let mv = doc
            .apply(Command::Move {
                items: p0.added.clone(),
                delta: Pt { x: GRID_NM, y: 0 },
            })
            .unwrap();
        assert_eq!(mv.dirty, Some(rect(-120, -50, GRID_NM + 120, 50)));
    }

    #[test]
    fn add_wires_validates_all_before_inserting_any() {
        let mut doc = Doc::new();
        let err = doc.apply(Command::AddWires(vec![
            Wire {
                a: Pt::ZERO,
                b: Pt { x: 10, y: 0 },
            },
            Wire {
                a: Pt { x: 10, y: 0 },
                b: Pt { x: 10, y: 10 },
            },
            Wire {
                a: Pt { x: 10, y: 10 },
                b: Pt { x: 10, y: 10 }, // zero-length
            },
        ]));
        assert_eq!(err, Err(CommandError::InvalidWire));
        assert_eq!(doc.revision(), 0);
        assert_eq!(doc.wires().count(), 0);
    }

    #[test]
    fn fresh_ids_after_undo_and_edge_straddling_part_reorient() {
        let mut doc = Doc::new();
        let bounds = rect(-100, -50, 100, 50);
        let p0 = doc
            .apply(Command::PlacePart {
                part: make_part(
                    Pt {
                        x: SHEET_LIMIT_NM - 10,
                        y: 0,
                    },
                    Orient::IDENTITY,
                    "U1",
                    "OP",
                ),
                local_pins: vec![Pt { x: -20, y: 0 }, Pt::ZERO],
                local_bounds: bounds,
            })
            .unwrap()
            .added[0];
        assert_eq!(p0, ItemId::Part(PartId(0)));
        assert_eq!(
            doc.part_body(PartId(0)).unwrap(),
            rect(SHEET_LIMIT_NM - 110, -50, SHEET_LIMIT_NM, 50)
        );
        doc.apply(Command::Reorient {
            items: vec![p0],
            pivot: Pt {
                x: SHEET_LIMIT_NM - 10,
                y: 0,
            },
            by: Orient::ROT_90,
        })
        .unwrap();
        assert_eq!(
            doc.part_body(PartId(0)).unwrap(),
            rect(SHEET_LIMIT_NM - 60, -100, SHEET_LIMIT_NM, 100)
        );
        assert_doc_consistent(&doc);
        doc.undo().unwrap();
        doc.undo().unwrap(); // undoes PlacePart(PartId(0))

        // New PlacePart after undo allocates fresh ID PartId(1), not reusing PartId(0), and clears redo.
        let p1 = doc
            .apply(Command::PlacePart {
                part: make_part(Pt::ZERO, Orient::IDENTITY, "U2", "OP"),
                local_pins: vec![Pt::ZERO],
                local_bounds: bounds,
            })
            .unwrap()
            .added[0];
        assert_eq!(p1, ItemId::Part(PartId(1)));
        assert!(doc.redo().is_none());
        assert_doc_consistent(&doc);
    }

    impl Doc {
        fn save_to_vec_for_test(&self) -> Vec<u8> {
            let mut bytes = Vec::new();
            self.save(&mut bytes).unwrap();
            bytes
        }
    }

    #[derive(Debug, Clone)]
    enum GenStep {
        PlacePart {
            x: i32,
            y: i32,
            orient: u8,
            invalid_oob: bool,
        },
        AddWires {
            x: i32,
            y: i32,
            horiz: bool,
            bad_diag: bool,
        },
        AddLabel {
            x: i32,
            y: i32,
            net: u8,
            invalid_oob: bool,
        },
        Move {
            pick: usize,
            dx: i32,
            dy: i32,
            dup: bool,
            missing: bool,
        },
        Reorient {
            pick: usize,
            px: i32,
            py: i32,
            orient: u8,
            dup: bool,
        },
        Delete {
            pick: usize,
            count: usize,
            dup: bool,
            missing: bool,
        },
        SetText {
            pick: usize,
            field: u8,
            net: u8,
            missing: bool,
        },
        BatchMoveAndAdd {
            pick: usize,
            dx: i32,
            fail_second: bool,
        },
        Undo,
        Redo,
    }

    fn gen_step() -> impl Strategy<Value = GenStep> {
        prop_oneof![
            3 => (-10i32..=10, -10i32..=10, 0u8..8, prop::bool::weighted(0.15)).prop_map(
                |(x, y, orient, invalid_oob)| GenStep::PlacePart { x, y, orient, invalid_oob }
            ),
            4 => (-10i32..=10, -10i32..=10, any::<bool>(), prop::bool::weighted(0.15)).prop_map(
                |(x, y, horiz, bad_diag)| GenStep::AddWires { x, y, horiz, bad_diag }
            ),
            3 => (-10i32..=10, -10i32..=10, 0u8..4, prop::bool::weighted(0.1)).prop_map(
                |(x, y, net, invalid_oob)| GenStep::AddLabel { x, y, net, invalid_oob }
            ),
            4 => (0usize..32, -5i32..=5, -5i32..=5, prop::bool::weighted(0.1), prop::bool::weighted(0.1)).prop_map(
                |(pick, dx, dy, dup, missing)| GenStep::Move { pick, dx, dy, dup, missing }
            ),
            3 => (0usize..32, -4i32..=4, -4i32..=4, 0u8..8, prop::bool::weighted(0.1)).prop_map(
                |(pick, px, py, orient, dup)| GenStep::Reorient { pick, px, py, orient, dup }
            ),
            3 => (0usize..32, 1usize..4, prop::bool::weighted(0.1), prop::bool::weighted(0.1)).prop_map(
                |(pick, count, dup, missing)| GenStep::Delete { pick, count, dup, missing }
            ),
            3 => (0usize..32, 0u8..3, 0u8..4, prop::bool::weighted(0.1)).prop_map(
                |(pick, field, net, missing)| GenStep::SetText { pick, field, net, missing }
            ),
            2 => (0usize..32, -4i32..=4, prop::bool::weighted(0.35)).prop_map(
                |(pick, dx, fail_second)| GenStep::BatchMoveAndAdd { pick, dx, fail_second }
            ),
            3 => Just(GenStep::Undo),
            2 => Just(GenStep::Redo),
        ]
    }

    fn all_live_ids(doc: &Doc) -> Vec<ItemId> {
        doc.parts()
            .map(|(id, _)| ItemId::Part(id))
            .chain(doc.wires().map(|(id, _)| ItemId::Wire(id)))
            .chain(doc.labels().map(|(id, _)| ItemId::Label(id)))
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn inv4_and_inv10_over_random_legal_and_invalid_sequences(
            steps in prop::collection::vec(gen_step(), 1..80),
        ) {
            let mut doc = Doc::new();
            let local_bounds = rect(-2, -1, 2, 1);
            let local_pins = vec![Pt::ZERO, Pt { x: 1, y: 0 }];
            let names = ["GND", "VCC", "SIG", "CLK"];

            for step in steps {
                match step {
                    GenStep::Undo => {
                        let pre_rev = doc.revision();
                        let snap_before = doc.save_to_vec_for_test();
                        if doc.undo().is_some() {
                            prop_assert_eq!(doc.revision(), pre_rev + 1);
                            assert_doc_consistent(&doc);
                            let snap_after_undo = doc.save_to_vec_for_test();
                            // INV4: undo then redo recovers document equal to pre-undo (IDs included).
                            prop_assert!(doc.redo().is_some());
                            prop_assert_eq!(doc.revision(), pre_rev + 2);
                            let before_doc = Doc::load(snap_before.as_slice()).unwrap();
                            prop_assert!(doc.content_eq(&before_doc));
                            assert_doc_consistent(&doc);
                            // Undo back so history state stays at the undone point.
                            prop_assert!(doc.undo().is_some());
                            prop_assert!(doc.content_eq(&Doc::load(snap_after_undo.as_slice()).unwrap()));
                        } else {
                            prop_assert_eq!(doc.revision(), pre_rev);
                        }
                    }
                    GenStep::Redo => {
                        let pre_rev = doc.revision();
                        if doc.redo().is_some() {
                            prop_assert_eq!(doc.revision(), pre_rev + 1);
                            assert_doc_consistent(&doc);
                        } else {
                            prop_assert_eq!(doc.revision(), pre_rev);
                        }
                    }
                    other => {
                        let live = all_live_ids(&doc);
                        let cmd = match other {
                            GenStep::PlacePart { x, y, orient, invalid_oob } => Command::PlacePart {
                                part: make_part(
                                    Pt {
                                        x: if invalid_oob { SHEET_LIMIT_NM + 1 } else { x },
                                        y,
                                    },
                                    Orient::ALL[orient as usize],
                                    "U1",
                                    "1k",
                                ),
                                local_pins: local_pins.clone(),
                                local_bounds,
                            },
                            GenStep::AddWires { x, y, horiz, bad_diag } => {
                                let a = Pt { x, y };
                                let b = if bad_diag {
                                    Pt { x: x + 1, y: y + 1 }
                                } else if horiz {
                                    Pt { x: x + 1, y }
                                } else {
                                    Pt { x, y: y + 1 }
                                };
                                Command::AddWires(vec![Wire { a, b }])
                            }
                            GenStep::AddLabel { x, y, net, invalid_oob } => Command::AddLabel(Label {
                                at: Pt {
                                    x: if invalid_oob { SHEET_LIMIT_NM + 1 } else { x },
                                    y,
                                },
                                text: names[net as usize].into(),
                            }),
                            GenStep::Move { pick, dx, dy, dup, missing } => {
                                let mut items = if live.is_empty() {
                                    Vec::new()
                                } else {
                                    vec![live[pick % live.len()]]
                                };
                                if dup && !items.is_empty() {
                                    items.push(items[0]);
                                }
                                if missing {
                                    items.push(ItemId::Part(PartId(99_999)));
                                }
                                Command::Move {
                                    items,
                                    delta: Pt { x: dx, y: dy },
                                }
                            }
                            GenStep::Reorient { pick, px, py, orient, dup } => {
                                let mut items = if live.is_empty() {
                                    Vec::new()
                                } else {
                                    vec![live[pick % live.len()]]
                                };
                                if dup && !items.is_empty() {
                                    items.push(items[0]);
                                }
                                Command::Reorient {
                                    items,
                                    pivot: Pt { x: px, y: py },
                                    by: Orient::ALL[orient as usize],
                                }
                            }
                            GenStep::Delete { pick, count, dup, missing } => {
                                let mut items = Vec::new();
                                for i in 0..count {
                                    if !live.is_empty() {
                                        let id = live[(pick + i) % live.len()];
                                        if !items.contains(&id) {
                                            items.push(id);
                                        }
                                    }
                                }
                                if dup && !items.is_empty() {
                                    items.push(items[0]);
                                }
                                if missing {
                                    items.push(ItemId::Label(LabelId(99_999)));
                                }
                                Command::Delete(items)
                            }
                            GenStep::SetText { pick, field, net, missing } => {
                                let item = if missing || live.is_empty() {
                                    ItemId::Wire(WireId(99_999))
                                } else {
                                    live[pick % live.len()]
                                };
                                let field = match field {
                                    0 => TextField::Designator,
                                    1 => TextField::Value,
                                    _ => TextField::Label,
                                };
                                Command::SetText {
                                    item,
                                    field,
                                    text: names[net as usize].into(),
                                }
                            }
                            GenStep::BatchMoveAndAdd { pick, dx, fail_second } => {
                                let mut cmds = vec![Command::PlacePart {
                                    part: make_part(Pt::ZERO, Orient::IDENTITY, "B", "10"),
                                    local_pins: local_pins.clone(),
                                    local_bounds,
                                }];
                                if !live.is_empty() {
                                    cmds.push(Command::Move {
                                        items: vec![live[pick % live.len()]],
                                        delta: Pt { x: dx, y: 0 },
                                    });
                                }
                                if fail_second {
                                    cmds.push(Command::AddWires(vec![Wire {
                                        a: Pt::ZERO,
                                        b: Pt::ZERO,
                                    }]));
                                }
                                Command::Batch(cmds)
                            }
                            GenStep::Undo | GenStep::Redo => unreachable!(),
                        };

                        let before_bytes = doc.save_to_vec_for_test();
                        let before_doc = Doc::load(before_bytes.as_slice()).unwrap();
                        let pre_rev = doc.revision();
                        match doc.apply(cmd) {
                            Ok(change) if change.dirty.is_none() => {
                                prop_assert_eq!(change.revision, pre_rev);
                                prop_assert_eq!(doc.revision(), pre_rev);
                                prop_assert_eq!(doc.save_to_vec_for_test(), before_bytes);
                                prop_assert_eq!(&doc, &before_doc);
                                prop_assert!(change.added.is_empty() && change.removed.is_empty());
                                prop_assert!(!change.nets_changed);
                                assert_doc_consistent(&doc);
                            }
                            Ok(change) => {
                                prop_assert_eq!(change.revision, pre_rev + 1);
                                prop_assert_eq!(doc.revision(), pre_rev + 1);
                                assert_doc_consistent(&doc);
                                let after_doc_bytes = doc.save_to_vec_for_test();
                                let after_doc = Doc::load(after_doc_bytes.as_slice()).unwrap();

                                // INV4: apply(c) then undo() gives a document equal to original, IDs included.
                                let undo_change = doc.undo();
                                prop_assert!(undo_change.is_some());
                                prop_assert_eq!(doc.revision(), pre_rev + 2);
                                prop_assert!(doc.content_eq(&before_doc));
                                assert_doc_consistent(&doc);

                                // INV4: undo() then redo() gives a document equal to after apply(c), IDs included.
                                let redo_change = doc.redo();
                                prop_assert!(redo_change.is_some());
                                prop_assert_eq!(doc.revision(), pre_rev + 3);
                                prop_assert!(doc.content_eq(&after_doc));
                                assert_doc_consistent(&doc);
                            }
                            Err(_) => {
                                prop_assert_eq!(doc.revision(), pre_rev);
                                prop_assert_eq!(doc.save_to_vec_for_test(), before_bytes);
                                prop_assert_eq!(&doc, &before_doc);
                                assert_doc_consistent(&doc);
                            }
                        }
                    }
                }
            }
        }
    }
}
