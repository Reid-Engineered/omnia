//! Reversible document operations and undo/redo history (D2, INV4).

use std::sync::Arc;

use crate::{
    Change, Doc, ItemId, Label, LabelId, Orient, ParamValue, Params, PartDefId, PartId, Pt, Wire,
    WireId,
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
    SetParam {
        part: PartId,
        name: Arc<str>,
        value: Option<ParamValue>,
        part_value: Arc<str>,
    },
    UpdatePartDef {
        from: PartDefId,
        to: PartDefId,
        parts: Vec<(PartId, Arc<str>, Params)>,
    },
    Batch(Vec<UndoOp>),
}

/// One history step: the operation that moves between the `before` and
/// `after` states, whose cursors name those states (UD6, U2.1).
#[derive(Debug)]
pub(crate) struct HistoryEntry {
    pub(crate) op: UndoOp,
    pub(crate) before: u64,
    pub(crate) after: u64,
}

/// Undo/redo stacks plus the history cursor (UD6, U2.1). The cursor names the
/// current state: an effective `apply` issues a fresh value from `next_cursor`
/// (never reissued, so a state on a discarded branch can never be named
/// again), while `undo` and `redo` move back to the value stored with the
/// entry. Validated no-ops never reach `push_undo`, so they change nothing.
#[derive(Debug)]
pub(crate) struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    cursor: u64,
    next_cursor: u64,
}

impl Default for History {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            cursor: 0,
            next_cursor: 1,
        }
    }
}

impl History {
    pub(crate) fn cursor(&self) -> u64 {
        self.cursor
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(crate) fn push_undo(&mut self, op: UndoOp) {
        let before = self.cursor;
        let after = self.next_cursor;
        self.next_cursor = after.checked_add(1).expect("history cursor exhausted");
        self.undo.push(HistoryEntry { op, before, after });
        self.redo.clear();
        self.cursor = after;
    }

    pub(crate) fn pop_undo(&mut self) -> Option<HistoryEntry> {
        self.undo.pop()
    }

    pub(crate) fn push_redo(&mut self, before: u64, after: u64, inverse: UndoOp) {
        self.cursor = before;
        self.redo.push(HistoryEntry {
            op: inverse,
            before,
            after,
        });
    }

    pub(crate) fn pop_redo(&mut self) -> Option<HistoryEntry> {
        self.redo.pop()
    }

    pub(crate) fn push_undo_from_redo(&mut self, before: u64, after: u64, inverse: UndoOp) {
        self.cursor = after;
        self.undo.push(HistoryEntry {
            op: inverse,
            before,
            after,
        });
    }
}

pub(crate) fn undo(doc: &mut Doc) -> Option<Change> {
    let entry = doc.history.pop_undo()?;
    let (op, before, after) = (entry.op, entry.before, entry.after);
    let mut acc = ChangeAcc::default();
    let inverse = apply_undo_op(doc, op, &mut acc);
    doc.history.push_redo(before, after, inverse);
    doc.bump_revision();
    Some(acc.into_change(doc.revision()))
}

pub(crate) fn redo(doc: &mut Doc) -> Option<Change> {
    let entry = doc.history.pop_redo()?;
    let (op, before, after) = (entry.op, entry.before, entry.after);
    let mut acc = ChangeAcc::default();
    let inverse = apply_undo_op(doc, op, &mut acc);
    doc.history.push_undo_from_redo(before, after, inverse);
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
        UndoOp::SetParam {
            part,
            name,
            value,
            part_value,
        } => {
            let stored = doc
                .parts
                .get_mut(part.0)
                .expect("undo/redo set_param part must exist");
            let prev_value = match value {
                Some(v) => stored.params.insert(Arc::clone(&name), v),
                None => stored.params.remove(&name),
            };
            let prev_part_value = std::mem::replace(&mut stored.part.value, part_value);
            let bounds = doc
                .index
                .bounds(ItemId::Part(part))
                .expect("live part must exist in index");
            acc.record_dirty(bounds);
            acc.record_modified(part);
            UndoOp::SetParam {
                part,
                name,
                value: prev_value,
                part_value: prev_part_value,
            }
        }
        UndoOp::UpdatePartDef { from, to, parts } => {
            let mut inverse_parts = Vec::with_capacity(parts.len());
            for (part, target_value, target_params) in parts {
                doc.part_defs.release(from);
                doc.part_defs.acquire(to);
                let stored = doc
                    .parts
                    .get_mut(part.0)
                    .expect("undo/redo update_part_def part must exist");
                debug_assert_eq!(stored.def, from);
                stored.def = to;
                let prev_value = std::mem::replace(&mut stored.part.value, target_value);
                let prev_params = std::mem::replace(&mut stored.params, target_params);
                let bounds = doc
                    .index
                    .bounds(ItemId::Part(part))
                    .expect("live part must exist in index");
                acc.record_dirty(bounds);
                acc.record_modified(part);
                inverse_parts.push((part, prev_value, prev_params));
            }
            UndoOp::UpdatePartDef {
                from: to,
                to: from,
                parts: inverse_parts,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, PlaceDef, Rect, SymbolId, TextField};
    use proptest::prelude::*;
    use std::collections::HashMap;

    /// In-memory copy without history (`P2.1`, `PD8`): a v2 save/load round trip cannot
    /// carry definition ids, which `content_eq` compares.
    fn snapshot(doc: &Doc) -> Doc {
        doc.fork()
    }

    fn place(x: i32, y: i32) -> Command {
        Command::PlacePart {
            part: crate::Part {
                symbol: SymbolId(0),
                at: Pt { x, y },
                orient: Orient::IDENTITY,
                designator: "R1".into(),
                value: "1k".into(),
            },
            local_pins: vec![Pt::ZERO],
            local_bounds: Rect::from_points(Pt { x: -2, y: -1 }, Pt { x: 2, y: 1 }).unwrap(),
            def: PlaceDef::Unmodelled,
            params: Params::new(),
        }
    }

    fn wire_cmd(x: i32, y: i32) -> Command {
        Command::AddWires(vec![Wire {
            a: Pt { x, y },
            b: Pt { x: x + 10, y },
        }])
    }

    #[test]
    fn u2_1_cursor_moves_with_history_and_counts() {
        let mut doc = Doc::new();
        let c0 = doc.history_cursor();
        assert!(!doc.can_undo());
        assert!(!doc.can_redo());
        assert_eq!(doc.part_count(), 0);
        assert_eq!(doc.net_count(), 0);

        doc.apply(place(0, 0)).unwrap();
        let c1 = doc.history_cursor();
        assert_ne!(c1, c0);
        assert!(doc.can_undo());
        assert!(!doc.can_redo());
        assert_eq!(doc.part_count(), 1);
        assert_eq!(doc.part_count(), doc.parts().count());

        doc.apply(wire_cmd(100, 100)).unwrap();
        doc.apply(Command::AddWires(vec![Wire {
            a: Pt { x: 200, y: 200 },
            b: Pt { x: 210, y: 200 },
        }]))
        .unwrap();
        // Two disjoint wires plus the placed part's lone pin: three nets.
        assert_eq!(doc.net_count(), 3);
        let mut wires_only = Doc::new();
        wires_only.apply(wire_cmd(100, 100)).unwrap();
        wires_only
            .apply(Command::AddWires(vec![Wire {
                a: Pt { x: 200, y: 200 },
                b: Pt { x: 210, y: 200 },
            }]))
            .unwrap();
        assert_eq!(wires_only.net_count(), 2);
        let c3 = doc.history_cursor();
        assert_ne!(c3, c1);

        doc.undo().unwrap();
        doc.undo().unwrap();
        assert_eq!(doc.history_cursor(), c1);
        assert!(doc.can_redo());
        doc.redo().unwrap();
        doc.redo().unwrap();
        assert_eq!(doc.history_cursor(), c3);
        assert!(!doc.can_redo());
    }

    #[test]
    fn u2_1_noops_and_failures_change_nothing() {
        let mut doc = Doc::new();
        doc.apply(place(0, 0)).unwrap();
        doc.undo().unwrap();
        let cursor = doc.history_cursor();
        assert!(doc.can_redo());

        for cmd in [
            Command::AddWires(vec![]),
            Command::Batch(vec![]),
            Command::Move {
                items: vec![],
                delta: Pt { x: 1, y: 0 },
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
            },
        ] {
            let change = doc.apply(cmd).unwrap();
            assert_eq!(change.dirty, None);
            assert_eq!(doc.history_cursor(), cursor);
            assert!(!doc.can_undo());
            assert!(doc.can_redo());
        }
        // A rejected command changes nothing either (D2).
        assert!(
            doc.apply(Command::AddWires(vec![Wire {
                a: Pt::ZERO,
                b: Pt { x: 1, y: 1 },
            }]))
            .is_err()
        );
        assert_eq!(doc.history_cursor(), cursor);
        assert!(doc.redo().is_some(), "redo must survive no-ops");
    }

    /// UTC-saved-point at the doc level (UD6): modified is `cursor != saved_cursor`.
    #[test]
    fn utc_saved_point_at_doc_level() {
        let mut doc = Doc::new();
        doc.apply(place(0, 0)).unwrap(); // A
        let saved = doc.history_cursor();
        doc.apply(place(50, 0)).unwrap(); // B
        assert_ne!(doc.history_cursor(), saved);
        doc.undo().unwrap();
        assert_eq!(doc.history_cursor(), saved, "not modified at save point");
        doc.undo().unwrap();
        assert_ne!(doc.history_cursor(), saved, "modified past save point");
        doc.redo().unwrap();
        assert_eq!(doc.history_cursor(), saved, "not modified again");
    }

    /// UTC-branch at the doc level (UD6): an edit after undoing past the save
    /// point discards its branch; the saved cursor stays unreachable.
    #[test]
    fn utc_branch_at_doc_level() {
        let mut doc = Doc::new();
        doc.apply(place(0, 0)).unwrap(); // A
        let saved = doc.history_cursor();
        doc.undo().unwrap();
        doc.apply(place(50, 0)).unwrap(); // B, on a new branch
        assert_ne!(doc.history_cursor(), saved);
        assert!(!doc.can_redo(), "branching must discard redo");
        doc.undo().unwrap();
        assert_ne!(doc.history_cursor(), saved, "still modified");
        // No undo/redo walk from here ever reaches the saved cursor again.
        let mut reached = vec![doc.history_cursor()];
        while doc.undo().is_some() {
            reached.push(doc.history_cursor());
        }
        while doc.redo().is_some() {
            reached.push(doc.history_cursor());
        }
        assert!(!reached.contains(&saved));
    }

    #[derive(Debug, Clone)]
    enum Step {
        Place { x: i32, y: i32 },
        Wire { x: i32, y: i32, bad: bool },
        Label { x: i32, y: i32 },
        Move { pick: usize, dx: i32 },
        Delete { pick: usize },
        SetText { pick: usize },
        Noop,
        Undo,
        Redo,
    }

    fn gen_step() -> impl Strategy<Value = Step> {
        prop_oneof![
            3 => (-20i32..=20, -20i32..=20).prop_map(|(x, y)| Step::Place { x, y }),
            3 => (-20i32..=20, -20i32..=20, any::<bool>())
                .prop_map(|(x, y, bad)| Step::Wire { x, y, bad }),
            2 => (-20i32..=20, -20i32..=20).prop_map(|(x, y)| Step::Label { x, y }),
            2 => (0usize..16, -3i32..=3).prop_map(|(pick, dx)| Step::Move { pick, dx }),
            2 => (0usize..16).prop_map(|pick| Step::Delete { pick }),
            1 => (0usize..16).prop_map(|pick| Step::SetText { pick }),
            1 => Just(Step::Noop),
            3 => Just(Step::Undo),
            2 => Just(Step::Redo),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        /// U2.1 done-when: equal cursors exactly when one state is reached from
        /// the other by undo and redo alone. Only undo/redo may produce a
        /// previously issued cursor, they return to the exact earlier cursor,
        /// and an effective apply always issues a fresh one (even on a new
        /// branch, so a discarded save point can never be reissued).
        #[test]
        fn u2_1_cursor_identity_over_random_command_undo_redo(
            steps in prop::collection::vec(gen_step(), 1..60),
        ) {
            let mut doc = Doc::new();
            let mut seen: HashMap<u64, Doc> = HashMap::new();
            seen.insert(doc.history_cursor(), snapshot(&doc));
            let mut undo_model: Vec<u64> = Vec::new();
            let mut redo_model: Vec<u64> = Vec::new();

            for step in steps {
                let before = doc.history_cursor();
                match step {
                    Step::Undo => match doc.undo() {
                        Some(_) => {
                            let expected = undo_model.pop().expect("model undo stack");
                            prop_assert_eq!(doc.history_cursor(), expected);
                            redo_model.push(before);
                        }
                        None => {
                            prop_assert!(undo_model.is_empty());
                            prop_assert_eq!(doc.history_cursor(), before);
                        }
                    },
                    Step::Redo => match doc.redo() {
                        Some(_) => {
                            let expected = redo_model.pop().expect("model redo stack");
                            prop_assert_eq!(doc.history_cursor(), expected);
                            undo_model.push(before);
                        }
                        None => {
                            prop_assert!(redo_model.is_empty());
                            prop_assert_eq!(doc.history_cursor(), before);
                        }
                    },
                    step => {
                        let live: Vec<ItemId> = doc
                            .parts()
                            .map(|(id, _)| ItemId::Part(id))
                            .chain(doc.wires().map(|(id, _)| ItemId::Wire(id)))
                            .chain(doc.labels().map(|(id, _)| ItemId::Label(id)))
                            .collect();
                        let cmd = match step {
                            Step::Place { x, y } => place(x, y),
                            Step::Wire { x, y, bad } => {
                                if bad {
                                    Command::AddWires(vec![Wire {
                                        a: Pt { x, y },
                                        b: Pt { x: x + 1, y: y + 1 },
                                    }])
                                } else {
                                    wire_cmd(x, y)
                                }
                            }
                            Step::Label { x, y } => Command::AddLabel(Label {
                                at: Pt { x, y },
                                text: "SIG".into(),
                            }),
                            Step::Move { pick, dx } => Command::Move {
                                items: if live.is_empty() {
                                    vec![]
                                } else {
                                    vec![live[pick % live.len()]]
                                },
                                delta: Pt { x: dx, y: 0 },
                                follow: crate::prefs::FollowPolicy::new(false, 10, false)
                                    .unwrap(),
                            },
                            Step::Delete { pick } => Command::Delete {
                                items: if live.is_empty() {
                                    vec![]
                                } else {
                                    vec![live[pick % live.len()]]
                                },
                                with_wires: false,
                            },
                            Step::SetText { pick } => Command::SetText {
                                item: if live.is_empty() {
                                    ItemId::Wire(WireId(99_999))
                                } else {
                                    live[pick % live.len()]
                                },
                                field: TextField::Label,
                                text: "CLK".into(),
                            },
                            Step::Noop => Command::AddWires(vec![]),
                            Step::Undo | Step::Redo => unreachable!(),
                        };
                        match doc.apply(cmd) {
                            Ok(change) if change.dirty.is_some() => {
                                let cursor = doc.history_cursor();
                                prop_assert!(
                                    !seen.contains_key(&cursor),
                                    "effective apply must issue a fresh cursor"
                                );
                                undo_model.push(before);
                                redo_model.clear();
                                seen.insert(cursor, snapshot(&doc));
                            }
                            Ok(_) | Err(_) => {
                                prop_assert_eq!(doc.history_cursor(), before);
                            }
                        }
                    }
                }
                prop_assert_eq!(doc.can_undo(), !undo_model.is_empty());
                prop_assert_eq!(doc.can_redo(), !redo_model.is_empty());
                prop_assert_eq!(doc.part_count(), doc.parts().count());
                if let Some(snap) = seen.get(&doc.history_cursor()) {
                    prop_assert!(
                        doc.content_eq(snap),
                        "equal cursor must mean the same state (INV4 content)"
                    );
                }
            }
        }
    }
}
