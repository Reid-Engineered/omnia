//! Reversible document operations and undo/redo history (D2, INV4).

use std::sync::Arc;

use crate::{
    Change, Doc, ItemId, Label, LabelId, Orient, PartId, Pt, Wire, WireId,
    command::{self, ChangeAcc, TextField},
    doc::StoredPart,
};

#[derive(Debug, Clone)]
pub(crate) enum UndoOp {
    RemovePart(PartId),
    RestorePart {
        id: PartId,
        stored: StoredPart,
    },
    RemoveWires(Vec<WireId>),
    RestoreWires(Vec<(WireId, Wire)>),
    /// One inverse for an `AddWires` split/merge or local Delete merge (T6.1, W1.4): take out `added`,
    /// restore `removed` under their old ids.
    ReplaceWires {
        removed: Vec<(WireId, Wire)>,
        added: Vec<WireId>,
    },
    RemoveLabel(LabelId),
    SetLabelAt {
        id: LabelId,
        at: Pt,
    },
    RestoreLabel {
        id: LabelId,
        label: Label,
    },
    Move {
        items: Vec<ItemId>,
        delta: Pt,
    },
    Reorient {
        items: Vec<ItemId>,
        pivot: Pt,
        by: Orient,
    },
    DeleteRestore {
        parts: Vec<(PartId, StoredPart)>,
        wires: Vec<(WireId, Wire)>,
        labels: Vec<(LabelId, Label)>,
    },
    DeleteRemove(Vec<ItemId>),
    SetText {
        item: ItemId,
        field: TextField,
        text: Arc<str>,
    },
    Batch(Vec<UndoOp>),
}

#[derive(Debug, Default)]
pub(crate) struct History {
    undo: Vec<UndoOp>,
    redo: Vec<UndoOp>,
}

impl History {
    pub(crate) fn push_undo(&mut self, op: UndoOp) {
        self.undo.push(op);
        self.redo.clear();
    }

    pub(crate) fn pop_undo(&mut self) -> Option<UndoOp> {
        self.undo.pop()
    }

    pub(crate) fn push_redo(&mut self, op: UndoOp) {
        self.redo.push(op);
    }

    pub(crate) fn pop_redo(&mut self) -> Option<UndoOp> {
        self.redo.pop()
    }

    pub(crate) fn push_undo_from_redo(&mut self, op: UndoOp) {
        self.undo.push(op);
    }
}

pub(crate) fn undo(doc: &mut Doc) -> Option<Change> {
    let op = doc.history.pop_undo()?;
    let mut acc = ChangeAcc::default();
    let inverse = apply_undo_op(doc, op, &mut acc);
    doc.history.push_redo(inverse);
    doc.bump_revision();
    Some(acc.into_change(doc.revision()))
}

pub(crate) fn redo(doc: &mut Doc) -> Option<Change> {
    let op = doc.history.pop_redo()?;
    let mut acc = ChangeAcc::default();
    let inverse = apply_undo_op(doc, op, &mut acc);
    doc.history.push_undo_from_redo(inverse);
    doc.bump_revision();
    Some(acc.into_change(doc.revision()))
}

pub(crate) fn apply_undo_op(doc: &mut Doc, op: UndoOp, acc: &mut ChangeAcc) -> UndoOp {
    match op {
        UndoOp::RemovePart(id) => {
            let (stored, bounds) = doc
                .remove_stored_part_raw(id)
                .expect("undo part must exist");
            acc.record_dirty(bounds);
            acc.record_removed(ItemId::Part(id));
            if !stored.pins.is_empty() {
                acc.nets_changed = true;
            }
            UndoOp::RestorePart { id, stored }
        }
        UndoOp::RestorePart { id, stored } => {
            let has_pins = !stored.pins.is_empty();
            let bounds = doc
                .restore_stored_part_raw(id, stored)
                .expect("undo/redo restore part must target reserved empty slot");
            acc.record_dirty(bounds);
            acc.record_added(ItemId::Part(id));
            if has_pins {
                acc.nets_changed = true;
            }
            UndoOp::RemovePart(id)
        }
        UndoOp::RemoveWires(ids) => {
            let mut restored = Vec::with_capacity(ids.len());
            for id in ids {
                let (wire, bounds) = doc.remove_wire_raw(id).expect("undo wire must exist");
                acc.record_dirty(bounds);
                acc.record_removed(ItemId::Wire(id));
                acc.nets_changed = true;
                restored.push((id, wire));
            }
            UndoOp::RestoreWires(restored)
        }
        UndoOp::RestoreWires(items) => {
            let mut ids = Vec::with_capacity(items.len());
            for (id, wire) in items {
                let bounds = doc
                    .restore_wire_raw(id, wire)
                    .expect("undo/redo restore wire must target reserved empty slot");
                acc.record_dirty(bounds);
                acc.record_added(ItemId::Wire(id));
                acc.nets_changed = true;
                ids.push(id);
            }
            UndoOp::RemoveWires(ids)
        }
        UndoOp::ReplaceWires { removed, added } => {
            let mut present = Vec::with_capacity(added.len());
            for id in added {
                let (wire, bounds) = doc
                    .remove_wire_raw(id)
                    .expect("undo replace wire must exist");
                acc.record_dirty(bounds);
                acc.record_removed(ItemId::Wire(id));
                acc.nets_changed = true;
                present.push((id, wire));
            }
            let mut restored = Vec::with_capacity(removed.len());
            for (id, wire) in removed {
                let bounds = doc
                    .restore_wire_raw(id, wire)
                    .expect("undo replace restore must target reserved empty slot");
                acc.record_dirty(bounds);
                acc.record_added(ItemId::Wire(id));
                acc.nets_changed = true;
                restored.push(id);
            }
            UndoOp::ReplaceWires {
                removed: present,
                added: restored,
            }
        }
        UndoOp::SetLabelAt { id, at } => {
            let (mut label, old_bounds) = doc.remove_label_raw(id).expect("planned label exists");
            let old = label.at;
            label.at = at;
            let bounds = doc
                .restore_label_raw(id, label)
                .expect("validated label point");
            acc.record_dirty(old_bounds);
            acc.record_dirty(bounds);
            acc.nets_changed = true;
            UndoOp::SetLabelAt { id, at: old }
        }
        UndoOp::RemoveLabel(id) => {
            let (label, bounds) = doc.remove_label_raw(id).expect("undo label must exist");
            acc.record_dirty(bounds);
            acc.record_removed(ItemId::Label(id));
            acc.nets_changed = true;
            UndoOp::RestoreLabel { id, label }
        }
        UndoOp::RestoreLabel { id, label } => {
            let bounds = doc
                .restore_label_raw(id, label)
                .expect("undo/redo restore label must target reserved empty slot");
            acc.record_dirty(bounds);
            acc.record_added(ItemId::Label(id));
            acc.nets_changed = true;
            UndoOp::RemoveLabel(id)
        }
        UndoOp::Move { items, delta } => {
            command::apply_move_raw(doc, &items, delta, acc);
            let inv_delta = if items.is_empty() { Pt::ZERO } else { -delta };
            UndoOp::Move {
                items,
                delta: inv_delta,
            }
        }
        UndoOp::Reorient { items, pivot, by } => {
            command::apply_reorient_raw(doc, &items, pivot, by, acc);
            UndoOp::Reorient {
                items,
                pivot,
                by: by.inverse(),
            }
        }
        UndoOp::DeleteRestore {
            parts,
            wires,
            labels,
        } => {
            let mut ids = Vec::with_capacity(parts.len() + wires.len() + labels.len());
            for (id, stored) in parts {
                let has_pins = !stored.pins.is_empty();
                let bounds = doc
                    .restore_stored_part_raw(id, stored)
                    .expect("undo delete part must target reserved empty slot");
                acc.record_dirty(bounds);
                let item = ItemId::Part(id);
                acc.record_added(item);
                if has_pins {
                    acc.nets_changed = true;
                }
                ids.push(item);
            }
            for (id, wire) in wires {
                let bounds = doc
                    .restore_wire_raw(id, wire)
                    .expect("undo delete wire must target reserved empty slot");
                acc.record_dirty(bounds);
                let item = ItemId::Wire(id);
                acc.record_added(item);
                acc.nets_changed = true;
                ids.push(item);
            }
            for (id, label) in labels {
                let bounds = doc
                    .restore_label_raw(id, label)
                    .expect("undo delete label must target reserved empty slot");
                acc.record_dirty(bounds);
                let item = ItemId::Label(id);
                acc.record_added(item);
                acc.nets_changed = true;
                ids.push(item);
            }
            UndoOp::DeleteRemove(ids)
        }
        UndoOp::DeleteRemove(ids) => {
            let mut parts = Vec::new();
            let mut wires = Vec::new();
            let mut labels = Vec::new();
            for item in ids {
                match item {
                    ItemId::Part(id) => {
                        let (stored, bounds) = doc
                            .remove_stored_part_raw(id)
                            .expect("redo part must exist");
                        acc.record_dirty(bounds);
                        acc.record_removed(item);
                        if !stored.pins.is_empty() {
                            acc.nets_changed = true;
                        }
                        parts.push((id, stored));
                    }
                    ItemId::Wire(id) => {
                        let (wire, bounds) = doc.remove_wire_raw(id).expect("redo wire must exist");
                        acc.record_dirty(bounds);
                        acc.record_removed(item);
                        acc.nets_changed = true;
                        wires.push((id, wire));
                    }
                    ItemId::Label(id) => {
                        let (label, bounds) =
                            doc.remove_label_raw(id).expect("redo label must exist");
                        acc.record_dirty(bounds);
                        acc.record_removed(item);
                        acc.nets_changed = true;
                        labels.push((id, label));
                    }
                }
            }
            UndoOp::DeleteRestore {
                parts,
                wires,
                labels,
            }
        }
        UndoOp::SetText { item, field, text } => {
            let (old_text, bounds, nets_changed) = doc
                .set_text_raw(item, field, text)
                .expect("undo/redo set_text target must remain valid");
            acc.record_dirty(bounds);
            if nets_changed {
                acc.nets_changed = true;
            }
            UndoOp::SetText {
                item,
                field,
                text: old_text,
            }
        }
        UndoOp::Batch(ops) => {
            let mut next_ops = Vec::with_capacity(ops.len());
            for sub in ops.into_iter().rev() {
                next_ops.push(apply_undo_op(doc, sub, acc));
            }
            UndoOp::Batch(next_ops)
        }
    }
}
