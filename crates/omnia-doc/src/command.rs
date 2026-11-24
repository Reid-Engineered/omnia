//! Document commands, transactional validation, and change reporting (D2, INV1, INV4).

use std::{fmt, sync::Arc};

use smallvec::SmallVec;

use crate::{
    Doc, FollowError, FollowPlan, GeomError, Isometry, ItemId, Label, LabelId, Orient, Part,
    PartId, Pt, Rect, StoreError, Wire, WireId,
    points::End,
    prefs::FollowPolicy,
    route::Blocker,
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
        follow: FollowPolicy,
    },
    Reorient {
        items: Vec<ItemId>,
        pivot: Pt,
        by: Orient,
        follow: FollowPolicy,
    },
    /// WD7: drag one wire segment perpendicular to its axis by `offset` nm.
    DragSegment {
        wire: WireId,
        offset: i32,
    },
    /// WD10: optionally remove runs from selected parts' pins to their far anchors.
    Delete {
        items: Vec<ItemId>,
        with_wires: bool,
    },
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
    InvalidTextField {
        item: ItemId,
        field: TextField,
    },
    Segment(Blocker),
    Follow(FollowError),
    /// WD3/WINV1: a selected wire, after `Move` or `Reorient`, would hold a
    /// foreign pin or end in its interior or share a stretch with a foreign
    /// wire. Applies under every follow policy; checked after `plan_follow`,
    /// so a `Follow` refusal takes precedence.
    Capture {
        wire: WireId,
        blocker: Blocker,
    },
    /// WK1/WINV3: `Move` or `Reorient` would take a wire end off the pin
    /// lattice, or land an off-lattice pin or label on a wire body.
    OffLattice(Pt),
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
            Self::Segment(blocker) => write!(f, "segment drag blocked by {blocker:?}"),
            Self::Follow(error) => error.fmt(f),
            Self::Capture { wire, blocker } => {
                write!(f, "moved wire {wire:?} would capture {blocker:?}")
            }
            Self::OffLattice(at) => {
                write!(f, "move would put a wire point off the lattice at {at:?}")
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
        Command::Move {
            items,
            delta,
            follow,
        } => {
            validate_move(doc, &items, delta)?;
            apply_isometry(doc, items, Isometry::Translate(delta), follow, acc)
        }
        Command::Reorient {
            items,
            pivot,
            by,
            follow,
        } => {
            validate_reorient(doc, &items, pivot, by)?;
            apply_isometry(doc, items, Isometry::Reorient { pivot, by }, follow, acc)
        }
        Command::DragSegment { wire, offset } => {
            ensure_exists(doc, ItemId::Wire(wire))?;
            let Some(plan) = crate::route::plan_segment_drag_inner(doc, wire, offset)
                .map_err(CommandError::Segment)?
            else {
                return Ok(UndoOp::Batch(Vec::new()));
            };
            Ok(apply_wire_plan(doc, plan, acc))
        }
        Command::Delete {
            mut items,
            with_wires,
        } => {
            for &item in &items {
                ensure_exists(doc, item)?;
            }
            check_unique(&items)?;

            // WD10: discover all runs against the original sheet, before removing any
            // anchors. A shared run (or explicitly selected wire) is removed only once.
            if with_wires {
                let mut attached = Vec::new();
                let mut runs = Vec::new();
                for &item in &items {
                    if let ItemId::Part(id) = item {
                        for &pin in doc.pins(id).expect("validated part exists") {
                            doc.runs_at(pin, &mut runs);
                            attached.extend(runs.iter().flat_map(|run| run.wires.iter().copied()));
                        }
                    }
                }
                attached.sort_unstable_by_key(|id| id.0);
                attached.dedup();
                for id in attached {
                    let item = ItemId::Wire(id);
                    if !items.contains(&item) {
                        items.push(item);
                    }
                }
            }

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

/// W2.2: plan against the untouched sheet, then commit geometry and wires as one
/// history entry. Redo replays the stored geometry; it never plans again (INV4).
fn apply_isometry(
    doc: &mut Doc,
    items: Vec<ItemId>,
    transform: Isometry,
    policy: FollowPolicy,
    acc: &mut ChangeAcc,
) -> Result<UndoOp, CommandError> {
    if items.is_empty()
        || transform == Isometry::Translate(Pt::ZERO)
        || matches!(
            transform,
            Isometry::Reorient {
                by: Orient::IDENTITY,
                ..
            }
        )
    {
        return Ok(UndoOp::Batch(Vec::new()));
    }
    let mut plan = FollowPlan::default();
    crate::plan_follow(doc, &items, transform, policy, &mut plan).map_err(CommandError::Follow)?;
    validate_moved_wires(doc, &items, transform, &plan)?;
    validate_lattice(doc, &items, transform, &plan)?;
    // Every fallible operation precedes the first edit (WINV4).
    for &wire in &plan.add {
        Doc::validate_wire(wire)?;
    }
    let mut vacated = Vec::new();
    let remove = plan
        .remove
        .iter()
        .map(|&id| {
            let wire = *doc.wire(id).expect("planned wire exists");
            vacated.extend([wire.a, wire.b]);
            (id, wire)
        })
        .collect();
    let mut inverses = vec![apply_wire_plan(
        doc,
        crate::route::AddWiresPlan {
            remove,
            insert: Vec::new(),
        },
        acc,
    )];
    // Every point a moved item leaves may now hold only two collinear wires.
    // Normal form wins over leaving detached wires untouched (WINV3 over WINV2,
    // owner decision 2026-11-17): those wires merge after the move.
    for &item in &items {
        match item {
            ItemId::Wire(id) => {
                let wire = doc
                    .wire(id)
                    .expect("selected wires are outside affected runs");
                vacated.extend([wire.a, wire.b]);
            }
            ItemId::Part(id) => vacated.extend_from_slice(doc.pins(id).expect("validated part")),
            ItemId::Label(id) => vacated.push(doc.label(id).expect("validated label").at),
        }
    }
    for &(id, _) in &plan.labels {
        vacated.push(doc.label(id).expect("planned label exists").at);
    }
    let movement = match transform {
        Isometry::Translate(delta) => {
            apply_move_raw(doc, &items, delta, acc);
            UndoOp::Move {
                items: items.clone(),
                delta: -delta,
            }
        }
        Isometry::Reorient { pivot, by } => {
            apply_reorient_raw(doc, &items, pivot, by, acc);
            UndoOp::Reorient {
                items: items.clone(),
                pivot,
                by: by.inverse(),
            }
        }
    };
    inverses.push(movement);
    let mut landed = Vec::new();
    for (id, at) in plan.labels {
        // WD9: a label riding the run is a labelled point wherever it lands.
        landed.push(at);
        inverses.push(undo::apply_undo_op(doc, UndoOp::SetLabelAt { id, at }, acc));
    }

    // TC-move-14: including existing segments in AddWires' local input splits
    // them at landed pins/ends while retaining their first piece's identity.
    let mut additions = plan.add;
    let mut has_selected_wire = false;
    for &item in &items {
        match item {
            ItemId::Part(id) => landed.extend_from_slice(doc.pins(id).unwrap()),
            ItemId::Wire(id) => {
                has_selected_wire = true;
                let wire = *doc.wire(id).unwrap();
                landed.extend([wire.a, wire.b]);
                additions.push(wire);
            }
            // WD9: a moved label splits the wire body it lands on.
            ItemId::Label(id) => landed.push(doc.label(id).unwrap().at),
        }
    }
    landed.sort_unstable_by_key(|p| (p.x, p.y));
    landed.dedup();
    let mut touched = Vec::new();
    for at in landed {
        doc.index
            .visit_wires_unstamped(Rect { min: at, max: at }, |id, _| {
                touched.push(id);
            });
    }
    touched.sort_unstable_by_key(|id| id.0);
    touched.dedup();
    let normal = if !has_selected_wire && touched.is_empty() && doc.labels().next().is_none() {
        crate::route::plan_follow_add_wires(doc, &additions)
    } else {
        additions.extend(touched.into_iter().map(|id| *doc.wire(id).unwrap()));
        crate::route::plan_add_wires(doc, &additions)
    };
    inverses.push(apply_wire_plan(doc, normal, acc));
    let merges = crate::route::plan_delete_merges(doc, &mut vacated);
    inverses.push(apply_wire_plan(doc, merges, acc));
    // Undo batches run backwards: undo normalisation, labels and the selected
    // geometry before restoring the old affected runs under their exact ids.
    Ok(UndoOp::Batch(inverses))
}

// WD3/WINV1: a foreign pin/end cannot be captured by a selected wire's
// interior. Its own ends may land on foreign bodies (TC-move-14). Selected
// geometry shares the same isometry, so its existing contacts are preserved.
fn validate_moved_wires(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    plan: &FollowPlan,
) -> Result<(), CommandError> {
    let mut wires: Vec<_> = items
        .iter()
        .filter_map(|item| match *item {
            ItemId::Wire(id) => {
                let w = *doc.wire(id).unwrap();
                Some((
                    id,
                    Wire {
                        a: t.apply(w.a).unwrap(),
                        b: t.apply(w.b).unwrap(),
                    },
                ))
            }
            _ => None,
        })
        .collect();
    wires.sort_unstable_by_key(|(id, w)| {
        (
            w.a.x.min(w.b.x),
            w.a.y.min(w.b.y),
            w.a.x.max(w.b.x),
            w.a.y.max(w.b.y),
            id.0,
        )
    });
    if wires.is_empty() {
        return Ok(());
    }
    // Sorted lookups keep large selections linear-logarithmic (no per-item scans).
    let item_key = |item: &ItemId| match *item {
        ItemId::Part(id) => (0u8, id.0),
        ItemId::Wire(id) => (1, id.0),
        ItemId::Label(id) => (2, id.0),
    };
    let mut selected: Vec<_> = items.iter().map(item_key).collect();
    selected.sort_unstable();
    let mut removed: Vec<_> = plan.remove.iter().map(|id| id.0).collect();
    removed.sort_unstable();
    // Replacement ends, sorted by (x, y) for vertical and (y, x) for horizontal
    // range queries along a moved wire.
    let mut ends_xy: Vec<_> = plan.add.iter().flat_map(|w| [w.a, w.b]).collect();
    ends_xy.sort_unstable_by_key(|p| (p.x, p.y));
    ends_xy.dedup();
    let mut ends_yx = ends_xy.clone();
    ends_yx.sort_unstable_by_key(|p| (p.y, p.x));
    let mut scratch = Vec::new();
    let mut best = None;
    for (order, &(wire_id, wire)) in wires.iter().enumerate() {
        let bounds = Rect::from_points(wire.a, wire.b).unwrap();
        // Rank from the canonical low end so stored a/b order never decides.
        let low = bounds.min;
        let interior = |p| p != wire.a && p != wire.b && bounds.contains(p);
        doc.query(bounds, &mut scratch);
        let mut consider = |rule, p, kind, id, pin, blocker| {
            let key = (
                rule,
                order,
                low.manhattan_distance(p),
                p.x,
                p.y,
                kind,
                id,
                pin,
            );
            if best.as_ref().is_none_or(|&(old, _)| key < old) {
                best = Some((
                    key,
                    CommandError::Capture {
                        wire: wire_id,
                        blocker,
                    },
                ));
            }
        };
        // A replaced run's fixed far end still exists in the final geometry.
        // The router permits its own endpoint on a moved wire body; that does
        // not authorize the selected wire to capture it (WINV1).
        let along = if wire.a.y == wire.b.y {
            let lo = ends_yx.partition_point(|p| (p.y, p.x) < (bounds.min.y, bounds.min.x));
            let hi = ends_yx.partition_point(|p| (p.y, p.x) <= (bounds.max.y, bounds.max.x));
            &ends_yx[lo..hi]
        } else {
            let lo = ends_xy.partition_point(|p| (p.x, p.y) < (bounds.min.x, bounds.min.y));
            let hi = ends_xy.partition_point(|p| (p.x, p.y) <= (bounds.max.x, bounds.max.y));
            &ends_xy[lo..hi]
        };
        for &p in along {
            if interior(p) {
                let source = plan
                    .remove
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let old = doc.wire(id).unwrap();
                        old.a == p || old.b == p
                    })
                    .min_by_key(|id| id.0)
                    .or_else(|| plan.remove.iter().copied().min_by_key(|id| id.0))
                    .expect("replacement wires have an original run");
                consider(0, p, 1, source.0, 0, Blocker::WireEnd(source));
            }
        }
        for &item in &scratch {
            if selected.binary_search(&item_key(&item)).is_ok() {
                continue;
            }
            match item {
                ItemId::Part(part) => {
                    for (pin, &p) in doc.pins(part).unwrap().iter().enumerate() {
                        if interior(p) {
                            consider(0, p, 0, part.0, pin, Blocker::Pin { part, pin });
                        }
                    }
                }
                ItemId::Wire(id) if removed.binary_search(&id.0).is_err() => {
                    let other = *doc.wire(id).unwrap();
                    for p in [other.a, other.b] {
                        if interior(p) {
                            consider(0, p, 1, id.0, 0, Blocker::WireEnd(id));
                        }
                    }
                    let other_bounds = Rect::from_points(other.a, other.b).unwrap();
                    if (wire.a.y == wire.b.y
                        && other.a.y == other.b.y
                        && wire.a.y == other.a.y
                        && bounds.min.x.max(other_bounds.min.x)
                            < bounds.max.x.min(other_bounds.max.x))
                        || (wire.a.x == wire.b.x
                            && other.a.x == other.b.x
                            && wire.a.x == other.a.x
                            && bounds.min.y.max(other_bounds.min.y)
                                < bounds.max.y.min(other_bounds.max.y))
                    {
                        let first = Pt {
                            x: low.x.clamp(other_bounds.min.x, other_bounds.max.x),
                            y: low.y.clamp(other_bounds.min.y, other_bounds.max.y),
                        };
                        consider(1, first, 1, id.0, 0, Blocker::Overlap(id));
                    }
                }
                _ => {}
            }
        }
    }
    if let Some((_, error)) = best {
        Err(error)
    } else {
        Ok(())
    }
}

fn on_lattice(p: Pt) -> bool {
    p.x % crate::PIN_LATTICE_NM == 0 && p.y % crate::PIN_LATTICE_NM == 0
}

// WINV3/WK1: a move never takes a wire end off the lattice, and never creates a
// split off the lattice: no off-lattice pin, label or end lands on a wire body
// (TC-move-14/WD9), and no moved or routed wire covers a stationary off-lattice
// label. Ends already off the lattice (legacy fixtures) are not judged on their
// own. The lowest violating point is reported, independent of item and end order.
fn validate_lattice(
    doc: &Doc,
    items: &[ItemId],
    t: Isometry,
    plan: &FollowPlan,
) -> Result<(), CommandError> {
    let mut worst: Option<Pt> = None;
    let mut report = |p: Pt| {
        if worst.is_none_or(|w| (p.x, p.y) < (w.x, w.y)) {
            worst = Some(p);
        }
    };
    let has_labels = doc.labels().next().is_some();
    let mut landed = Vec::new();
    let mut legacy_ends = Vec::new();
    let mut moved_wires = if has_labels {
        plan.add.clone()
    } else {
        Vec::new()
    };
    let mut selected_wires = Vec::new();
    let mut moved_labels: Vec<u32> = if has_labels {
        plan.labels.iter().map(|&(id, _)| id.0).collect()
    } else {
        Vec::new()
    };
    for &item in items {
        match item {
            ItemId::Wire(id) => {
                selected_wires.push(id.0);
                let w = *doc.wire(id).unwrap();
                let moved = Wire {
                    a: t.apply(w.a)?,
                    b: t.apply(w.b)?,
                };
                for (old, at) in [(w.a, moved.a), (w.b, moved.b)] {
                    if on_lattice(old) && !on_lattice(at) {
                        report(at);
                    } else if !on_lattice(at) {
                        // A legacy off-lattice end may still split legacy wires,
                        // as AddWires does, but never a lattice wire.
                        legacy_ends.push(at);
                    }
                }
                // Only a lattice wire can be split off the lattice by a label.
                if has_labels && on_lattice(moved.a) && on_lattice(moved.b) {
                    moved_wires.push(moved);
                }
            }
            ItemId::Part(id) => {
                for &p in doc.pins(id).unwrap() {
                    let at = t.apply(p)?;
                    if !on_lattice(at) {
                        landed.push(at);
                    }
                }
            }
            ItemId::Label(id) => {
                if has_labels {
                    moved_labels.push(id.0);
                }
                let at = t.apply(doc.label(id).unwrap().at)?;
                if !on_lattice(at) {
                    landed.push(at);
                }
            }
        }
    }
    for &(_, at) in &plan.labels {
        if !on_lattice(at) {
            landed.push(at);
        }
    }
    let interior = |w: Wire, p: Pt| {
        p != w.a && p != w.b && Rect::from_points(w.a, w.b).is_ok_and(|r| r.contains(p))
    };
    let mut query = Vec::new();
    if !landed.is_empty() || !legacy_ends.is_empty() {
        landed.sort_unstable_by_key(|p| (p.x, p.y));
        landed.dedup();
        let mut removed: Vec<_> = plan.remove.iter().map(|id| id.0).collect();
        removed.sort_unstable();
        selected_wires.sort_unstable();
        let tagged = landed
            .into_iter()
            .map(|p| (p, false))
            .chain(legacy_ends.into_iter().map(|p| (p, true)));
        for (at, legacy) in tagged {
            let judged = |w: Wire| !legacy || (on_lattice(w.a) && on_lattice(w.b));
            let mut hit = plan.add.iter().any(|&w| judged(w) && interior(w, at));
            doc.query(Rect { min: at, max: at }, &mut query);
            hit |= query.iter().any(|item| match *item {
                ItemId::Wire(id) => {
                    let w = *doc.wire(id).unwrap();
                    removed.binary_search(&id.0).is_err()
                        && selected_wires.binary_search(&id.0).is_err()
                        && judged(w)
                        && interior(w, at)
                }
                _ => false,
            });
            if hit {
                report(at);
            }
        }
    }
    if has_labels {
        moved_labels.sort_unstable();
        for wire in moved_wires {
            let Ok(bounds) = Rect::from_points(wire.a, wire.b) else {
                continue;
            };
            doc.query(bounds, &mut query);
            for item in &query {
                if let ItemId::Label(id) = *item
                    && moved_labels.binary_search(&id.0).is_err()
                {
                    let at = doc.label(id).unwrap().at;
                    if !on_lattice(at) && interior(wire, at) {
                        report(at);
                    }
                }
            }
        }
    }
    match worst {
        Some(at) => Err(CommandError::OffLattice(at)),
        None => Ok(()),
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

    // W3.5 fixtures use the pin lattice so the normal-form oracle checks WINV3 too.
    fn del_point(x: i32, y: i32) -> Pt {
        Pt {
            x: x * 1_270_000,
            y: y * 1_270_000,
        }
    }

    fn del_part(doc: &mut Doc, at: Pt) -> ItemId {
        let change = doc
            .apply(Command::PlacePart {
                part: make_part(at, Orient::IDENTITY, "P", ""),
                local_pins: vec![Pt::ZERO],
                local_bounds: rect(0, 0, 0, 0),
            })
            .unwrap();
        change.added[0]
    }

    fn del_wire(a: Pt, b: Pt) -> Wire {
        Wire { a, b }
    }

    fn del_snapshot(doc: &Doc) -> Doc {
        Doc::load(doc.save_to_vec_for_test().as_slice()).unwrap()
    }

    fn follow_policy(on: bool) -> FollowPolicy {
        FollowPolicy::new(on, 10, false).unwrap()
    }

    fn pin_partition(doc: &Doc) -> Vec<bool> {
        let pins: Vec<_> = doc
            .parts()
            .flat_map(|(id, _)| doc.pins(id).unwrap())
            .collect();
        pins.iter()
            .flat_map(|&&a| {
                pins.iter()
                    .map(move |&&b| doc.net_at(a).is_some() && doc.net_at(a) == doc.net_at(b))
            })
            .collect()
    }

    fn follow_roundtrip(doc: &mut Doc, command: Command) -> Result<Change, CommandError> {
        let before = del_snapshot(doc);
        let bytes = doc.save_to_vec_for_test();
        let result = doc.apply(command);
        match &result {
            Err(_) => {
                assert_eq!(doc.save_to_vec_for_test(), bytes); // WINV4 includes revision/reservations.
            }
            Ok(change) if change.dirty.is_some() => {
                assert_eq!(doc.revision(), before.revision() + 1);
                assert_doc_consistent(doc); // INV10 independent rebuild, point map and index.
                let after = del_snapshot(doc);
                doc.undo().unwrap();
                assert!(doc.content_eq(&before)); // INV4: every live id and stored geometry.
                assert_doc_consistent(doc);
                doc.redo().unwrap();
                assert!(doc.content_eq(&after));
            }
            Ok(_) => assert_eq!(doc.save_to_vec_for_test(), bytes),
        }
        assert_doc_consistent(doc);
        result
    }

    #[test]
    fn tc_move_14_landed_pin_and_wire_end_split_body_inv4_inv10() {
        for wire_end in [false, true] {
            let mut doc = Doc::new();
            let selected = if wire_end {
                doc.apply(Command::AddWires(vec![del_wire(
                    del_point(-2, 0),
                    del_point(0, 0),
                )]))
                .unwrap()
                .added[0]
            } else {
                del_part(&mut doc, del_point(0, 0))
            };
            let target = doc
                .apply(Command::AddWires(vec![del_wire(
                    del_point(2, -2),
                    del_point(2, 4),
                )]))
                .unwrap()
                .added[0];
            follow_roundtrip(
                &mut doc,
                Command::Move {
                    items: vec![selected],
                    delta: del_point(2, 0),
                    follow: follow_policy(true),
                },
            )
            .unwrap();
            doc.assert_normal_form();
            assert_eq!(doc.net_at(del_point(2, 0)), doc.net_at(del_point(2, -2)));
            if let ItemId::Wire(id) = target {
                assert!(doc.wire(id).is_some()); // D2 first piece retains the existing id.
            }
        }
    }

    #[test]
    fn w22_selected_wire_interior_contacts_refused_crossings_allowed() {
        // The capture rule is geometric and holds under follow and detach alike.
        for rotate in [false, true] {
            // 0 foreign pin, 1 foreign end, 2 foreign ends plus shared stretch
            // (the end rule ranks first), 3 shared stretch only, 4 crossing.
            for (kind, on) in (0..5).flat_map(|k| [(k, false), (k, true)]) {
                let mut doc = Doc::new();
                // Both reach (-2, 2)-(2, 2). The quarter turn's original touches
                // x = 2, so the overlap-only case uses a half turn to stay apart.
                let half = rotate && kind == 3;
                let original = if rotate && !half {
                    del_wire(del_point(2, 2), del_point(2, -2))
                } else {
                    del_wire(del_point(-2, -2), del_point(2, -2))
                };
                let selected = doc.apply(Command::AddWires(vec![original])).unwrap().added[0];
                let ItemId::Wire(wire_id) = selected else {
                    unreachable!()
                };
                let foreign = match kind {
                    0 => {
                        del_part(&mut doc, del_point(0, 2));
                        None
                    }
                    1 => Some(del_wire(del_point(0, 2), del_point(0, 3))),
                    2 => Some(del_wire(del_point(-1, 2), del_point(1, 2))),
                    3 => Some(del_wire(del_point(-3, 2), del_point(3, 2))),
                    _ => Some(del_wire(del_point(0, 1), del_point(0, 3))),
                };
                if let Some(wire) = foreign {
                    doc.apply(Command::AddWires(vec![wire])).unwrap();
                }
                let command = if rotate {
                    Command::Reorient {
                        items: vec![selected],
                        pivot: Pt::ZERO,
                        by: if half {
                            Orient::ROT_180
                        } else {
                            Orient::ROT_90
                        },
                        follow: follow_policy(on),
                    }
                } else {
                    Command::Move {
                        items: vec![selected],
                        delta: del_point(0, 4),
                        follow: follow_policy(on),
                    }
                };
                let result = follow_roundtrip(&mut doc, command);
                let blocker = match result {
                    Err(CommandError::Capture { wire, blocker }) => {
                        assert_eq!(wire, wire_id);
                        Some(blocker)
                    }
                    Err(other) => panic!("kind {kind} follow {on}: {other:?}"),
                    Ok(_) => None,
                };
                match kind {
                    0 => assert!(matches!(blocker, Some(Blocker::Pin { .. }))),
                    1 | 2 => assert!(matches!(blocker, Some(Blocker::WireEnd(_)))),
                    3 => assert!(matches!(blocker, Some(Blocker::Overlap(_)))),
                    _ => {
                        assert_eq!(blocker, None);
                        doc.assert_normal_form();
                        assert_eq!(doc.wires().count(), 2);
                    }
                }
            }
        }
    }

    #[test]
    fn w22_detach_merges_collinear_wires_at_vacated_pin_winv3() {
        // Owner decision 2026-11-17: normal form wins over WINV2 at points the
        // moved part leaves. Follow re-routes both runs instead.
        for on in [false, true] {
            let mut doc = Doc::new();
            let part = del_part(&mut doc, del_point(0, 0));
            let added = doc
                .apply(Command::AddWires(vec![
                    del_wire(del_point(-2, 0), del_point(0, 0)),
                    del_wire(del_point(0, 0), del_point(2, 0)),
                ]))
                .unwrap()
                .added;
            assert_eq!(added.len(), 2);
            follow_roundtrip(
                &mut doc,
                Command::Move {
                    items: vec![part],
                    delta: del_point(0, 2),
                    follow: follow_policy(on),
                },
            )
            .unwrap();
            doc.assert_normal_form();
            if !on {
                let wires: Vec<_> = doc.wires().map(|(_, &w)| w).collect();
                assert_eq!(wires.len(), 1);
                assert_eq!(
                    Rect::from_points(wires[0].a, wires[0].b).unwrap(),
                    Rect::from_points(del_point(-2, 0), del_point(2, 0)).unwrap()
                );
            }
        }
    }

    #[test]
    fn w22_moved_label_landing_on_wire_body_splits_it_wd9() {
        for on in [false, true] {
            let mut doc = Doc::new();
            doc.apply(Command::AddWires(vec![del_wire(
                del_point(0, 0),
                del_point(4, 0),
            )]))
            .unwrap();
            let label = doc
                .apply(Command::AddLabel(Label {
                    at: del_point(2, 3),
                    text: "N".into(),
                }))
                .unwrap()
                .added[0];
            follow_roundtrip(
                &mut doc,
                Command::Move {
                    items: vec![label],
                    delta: del_point(0, -3),
                    follow: follow_policy(on),
                },
            )
            .unwrap();
            doc.assert_normal_form();
            assert_eq!(doc.wires().count(), 2, "WD9: the labelled point splits");
            let (wire, _) = doc.wires().next().unwrap();
            assert_eq!(doc.net_at(del_point(2, 0)), doc.net_of(wire));
        }
    }

    #[test]
    fn w22_off_lattice_moves_refused_wk1_winv3() {
        let mut doc = Doc::new();
        let wire = doc
            .apply(Command::AddWires(vec![del_wire(
                del_point(0, 0),
                del_point(4, 0),
            )]))
            .unwrap()
            .added[0];
        let part = del_part(&mut doc, del_point(2, 3));
        let nudge = Pt { x: 1, y: 0 };
        for on in [false, true] {
            // An isolated wire may not leave the lattice.
            assert_eq!(
                follow_roundtrip(
                    &mut doc,
                    Command::Move {
                        items: vec![wire],
                        delta: nudge,
                        follow: follow_policy(on),
                    },
                ),
                Err(CommandError::OffLattice(Pt { x: 1, y: 0 }))
            );
            // An off-lattice pin may not land on a wire body (it would split there).
            let landing = Pt {
                x: del_point(2, 0).x + 1,
                y: 0,
            };
            assert_eq!(
                follow_roundtrip(
                    &mut doc,
                    Command::Move {
                        items: vec![part],
                        delta: Pt {
                            x: 1,
                            y: del_point(0, -3).y,
                        },
                        follow: follow_policy(on),
                    },
                ),
                Err(CommandError::OffLattice(landing))
            );
        }
        // An unconnected part may sit off the lattice where it touches nothing.
        follow_roundtrip(
            &mut doc,
            Command::Move {
                items: vec![part],
                delta: nudge,
                follow: follow_policy(true),
            },
        )
        .unwrap();
        doc.assert_normal_form();

        // The reported point is the lowest one, whatever the stored end order.
        for reversed in [false, true] {
            let mut doc = Doc::new();
            let (a, b) = (del_point(0, 0), del_point(4, 0));
            let wire = doc
                .apply(Command::AddWires(vec![if reversed {
                    del_wire(b, a)
                } else {
                    del_wire(a, b)
                }]))
                .unwrap()
                .added[0];
            assert_eq!(
                follow_roundtrip(
                    &mut doc,
                    Command::Move {
                        items: vec![wire],
                        delta: nudge,
                        follow: follow_policy(false),
                    },
                ),
                Err(CommandError::OffLattice(nudge))
            );
        }

        // A legacy off-lattice end may not split a lattice wire.
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![del_wire(
            del_point(0, 0),
            del_point(4, 0),
        )]))
        .unwrap();
        let legacy = doc
            .apply(Command::AddWires(vec![del_wire(
                Pt {
                    x: 1,
                    y: del_point(0, 2).y,
                },
                Pt {
                    x: 1,
                    y: del_point(0, 3).y,
                },
            )]))
            .unwrap()
            .added[0];
        assert_eq!(
            follow_roundtrip(
                &mut doc,
                Command::Move {
                    items: vec![legacy],
                    delta: del_point(0, -2),
                    follow: follow_policy(false),
                },
            ),
            Err(CommandError::OffLattice(Pt { x: 1, y: 0 }))
        );

        // A lattice wire may not come to cover a stationary off-lattice label:
        // normalisation would split it there (WD9).
        let mut doc = Doc::new();
        let wire = doc
            .apply(Command::AddWires(vec![del_wire(
                del_point(0, 0),
                del_point(4, 0),
            )]))
            .unwrap()
            .added[0];
        let label_at = Pt {
            x: del_point(2, 0).x + 1,
            y: del_point(0, 3).y,
        };
        doc.apply(Command::AddLabel(Label {
            at: label_at,
            text: "N".into(),
        }))
        .unwrap();
        assert_eq!(
            follow_roundtrip(
                &mut doc,
                Command::Move {
                    items: vec![wire],
                    delta: del_point(0, 3),
                    follow: follow_policy(false),
                },
            ),
            Err(CommandError::OffLattice(label_at))
        );
    }

    #[test]
    fn w22_capture_blocker_ignores_stored_end_order() {
        // Determinism: the reported blocker depends on geometry, not on a/b order.
        let errors: Vec<_> = [false, true]
            .into_iter()
            .map(|reversed| {
                let mut doc = Doc::new();
                del_part(&mut doc, del_point(1, 5));
                del_part(&mut doc, del_point(3, 5));
                let (a, b) = (del_point(0, 0), del_point(4, 0));
                let wire = doc
                    .apply(Command::AddWires(vec![if reversed {
                        del_wire(b, a)
                    } else {
                        del_wire(a, b)
                    }]))
                    .unwrap()
                    .added[0];
                follow_roundtrip(
                    &mut doc,
                    Command::Move {
                        items: vec![wire],
                        delta: del_point(0, 5),
                        follow: follow_policy(false),
                    },
                )
                .unwrap_err()
            })
            .collect();
        assert_eq!(errors[0], errors[1]);
        assert!(matches!(
            errors[0],
            CommandError::Capture {
                blocker: Blocker::Pin {
                    part: PartId(0),
                    pin: 0
                },
                ..
            }
        ));
    }

    #[test]
    fn w22_selected_wire_merge_and_mixed_rigid_label_restore_exact_ids() {
        let mut doc = Doc::new();
        // Insert the fixed wire first: merging consumes the selected wire's id.
        doc.apply(Command::AddWires(vec![del_wire(
            del_point(0, 0),
            del_point(2, 0),
        )]))
        .unwrap();
        let selected = doc
            .apply(Command::AddWires(vec![del_wire(
                del_point(-2, -2),
                del_point(0, -2),
            )]))
            .unwrap()
            .added[0];
        follow_roundtrip(
            &mut doc,
            Command::Move {
                items: vec![selected],
                delta: del_point(0, 2),
                follow: follow_policy(true),
            },
        )
        .unwrap();
        let ItemId::Wire(id) = selected else {
            unreachable!()
        };
        assert!(doc.wire(id).is_none());
        assert_eq!(doc.wires().count(), 1);
        doc.assert_normal_form();

        let mut doc = Doc::new();
        let a = del_part(&mut doc, del_point(0, 0));
        let b = del_part(&mut doc, del_point(4, 0));
        let wire = doc
            .apply(Command::AddWires(vec![del_wire(
                del_point(0, 0),
                del_point(4, 0),
            )]))
            .unwrap()
            .added[0];
        let label = doc
            .apply(Command::AddLabel(Label {
                at: del_point(0, 0),
                text: "selected".into(),
            }))
            .unwrap()
            .added[0];
        follow_roundtrip(
            &mut doc,
            Command::Reorient {
                items: vec![label, b, wire, a],
                pivot: Pt::ZERO,
                by: Orient::ROT_90,
                follow: follow_policy(true),
            },
        )
        .unwrap();
        assert_eq!(doc.labels().next().unwrap().1.at, Pt::ZERO);
        doc.assert_normal_form();
    }

    #[test]
    fn w22_selected_wire_cannot_capture_replaced_runs_fixed_far_end() {
        let mut doc = Doc::new();
        let selected = doc
            .apply(Command::AddWires(vec![del_wire(
                del_point(0, 0),
                del_point(4, 0),
            )]))
            .unwrap()
            .added[0];
        let part = del_part(&mut doc, del_point(2, 6));
        doc.apply(Command::AddWires(vec![del_wire(
            del_point(2, 2),
            del_point(2, 6),
        )]))
        .unwrap();
        let error = follow_roundtrip(
            &mut doc,
            Command::Move {
                items: vec![selected, part],
                delta: del_point(0, 2),
                follow: follow_policy(true),
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            CommandError::Capture {
                blocker: Blocker::WireEnd(_),
                ..
            }
        ));
    }

    #[test]
    fn tc_move_13_follow_refusal_preserves_redo_and_batch_allocations() {
        let mut doc = Doc::new();
        let moved = del_part(&mut doc, del_point(0, 0));
        del_part(&mut doc, del_point(-6, 0));
        doc.apply(Command::AddWires(vec![del_wire(
            del_point(0, 0),
            del_point(-6, 0),
        )]))
        .unwrap();
        // No valid first leg from the final point (WD4).
        for at in [
            del_point(2, 0),
            del_point(0, 0),
            del_point(1, 1),
            del_point(1, -1),
        ] {
            del_part(&mut doc, at);
        }
        doc.apply(Command::AddLabel(Label {
            at: del_point(20, 20),
            text: "redo".into(),
        }))
        .unwrap();
        doc.undo().unwrap();
        let command = Command::Move {
            items: vec![moved],
            delta: del_point(1, 0),
            follow: follow_policy(true),
        };
        assert!(matches!(
            follow_roundtrip(&mut doc, command.clone()),
            Err(CommandError::Follow(_))
        ));
        let batch = Command::Batch(vec![
            Command::AddWires(vec![del_wire(del_point(20, 0), del_point(21, 0))]),
            command,
        ]);
        assert!(matches!(
            follow_roundtrip(&mut doc, batch),
            Err(CommandError::Follow(_))
        ));
        doc.redo().unwrap();
        assert_eq!(doc.labels().next().unwrap().1.text.as_ref(), "redo");
    }

    #[test]
    fn tc_move_06_07_rigid_labels_once_and_16_direct_pin_contact() {
        for invert in [false, true] {
            let mut doc = Doc::new();
            let a = del_part(&mut doc, del_point(0, 0));
            let b = del_part(&mut doc, del_point(4, 0));
            doc.apply(Command::AddWires(vec![
                del_wire(del_point(0, 0), del_point(0, 2)),
                del_wire(del_point(0, 2), del_point(4, 2)),
                del_wire(del_point(4, 2), del_point(4, 0)),
            ]))
            .unwrap();
            doc.apply(Command::AddLabel(Label {
                at: del_point(0, 2),
                text: "rigid".into(),
            }))
            .unwrap();
            follow_roundtrip(
                &mut doc,
                Command::Move {
                    items: vec![b, a],
                    delta: del_point(1, 0),
                    follow: FollowPolicy::new(true, 10, invert).unwrap(),
                },
            )
            .unwrap();
            assert_eq!(doc.labels().next().unwrap().1.at, del_point(1, 2));
            assert_eq!(doc.wires().count(), 3);
            doc.assert_normal_form();
        }
        let mut doc = Doc::new();
        let a = del_part(&mut doc, Pt::ZERO);
        del_part(&mut doc, Pt::ZERO);
        follow_roundtrip(
            &mut doc,
            Command::Move {
                items: vec![a],
                delta: del_point(1, 0),
                follow: follow_policy(true),
            },
        )
        .unwrap();
        assert_eq!(doc.wires().count(), 0);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn w22_winv1_winv2_winv4_inv10_random_sheets_moves_and_reorientations(
            counts in prop::collection::vec(1usize..6, 1..6),
            pick in any::<usize>(), dx in -2i32..3, dy in -2i32..3,
            orientation in 0usize..8, rotate in any::<bool>(),
            obstacles in prop::collection::vec((0u8..4, -14i32..5, -3i32..22), 0..8),
            second in any::<Option<usize>>(),
        ) {
            let mut base = Doc::new();
            let mut movable = Vec::new();
            for (i, &count) in counts.iter().enumerate() {
                let x = i as i32 * 40;
                let change = base.apply(Command::PlacePart {
                    part: make_part(del_point(x, 0), Orient::IDENTITY, "P", ""),
                    local_pins: (0..count).map(|j| del_point(0, j as i32 * 4)).collect(),
                    local_bounds: rect(0, 0, 0, 0),
                }).unwrap();
                movable.push(change.added[0]);
                for j in 0..count {
                    let y = j as i32 * 4;
                    del_part(&mut base, del_point(x - 12, y));
                    base.apply(Command::AddWires(vec![
                        del_wire(del_point(x, y), del_point(x - 6, y)),
                        del_wire(del_point(x - 6, y), del_point(x - 6, y + 2)),
                        del_wire(del_point(x - 6, y + 2), del_point(x - 12, y + 2)),
                        del_wire(del_point(x - 12, y + 2), del_point(x - 12, y)),
                    ])).unwrap();
                }
            }
            let index = pick % movable.len();
            // Foreign pins and short wires around the selected part, so moved pins
            // land on, route around and get blocked by unrelated geometry.
            let ox = index as i32 * 40;
            let mut hits = Vec::new();
            for &(kind, x, y) in &obstacles {
                let at = del_point(ox + x, y);
                // PlacePart does not split wires: keep the base sheet in normal
                // form by placing foreign pins only on empty points.
                base.query(Rect { min: at, max: at }, &mut hits);
                if kind == 0 && !hits.is_empty() {
                    continue;
                }
                // Kind 3: a foreign pin directly on one of the selected part's pins (WQ6).
                let at = if kind == 3 { del_point(ox, y.rem_euclid(counts[index] as i32) * 4) } else { at };
                let _ = match kind {
                    0 | 3 => base.apply(Command::PlacePart {
                        part: make_part(at, Orient::IDENTITY, "Q", ""),
                        local_pins: vec![Pt::ZERO],
                        local_bounds: rect(0, 0, 0, 0),
                    }),
                    1 => base.apply(Command::AddWires(vec![del_wire(at, del_point(ox + x + 1, y))])),
                    _ => base.apply(Command::AddWires(vec![del_wire(at, del_point(ox + x, y + 1))])),
                };
            }
            // Selections of one or two parts; both keep their relative geometry.
            let mut selection = vec![movable[index]];
            if let Some(other) = second.map(|s| s % movable.len()).filter(|&s| s != index) {
                selection.push(movable[other]);
            }
            // A selected pin on an unselected pin connects directly; WQ6 lets that
            // contact separate. Which side keeps a wire at such a junction is open
            // (logged for the owner, 2026-11-17), so under follow only pairs that
            // include such a left-behind foreign pin are exempt.
            let all_pins: Vec<_> = base.parts().flat_map(|(id, _)| base.pins(id).unwrap().iter().map(move |&p| (ItemId::Part(id), p))).collect();
            let left_behind: Vec<bool> = all_pins.iter()
                .map(|&(a, p)| !selection.contains(&a) && all_pins.iter().any(|&(b, q)| selection.contains(&b) && q == p))
                .collect();
            base.assert_normal_form();
            let before = pin_partition(&base);
            for follow in [false, true] {
                let mut doc = del_snapshot(&base);
                let command = if rotate {
                    Command::Reorient { items: selection.clone(), pivot: del_point(index as i32 * 40, 0), by: Orient::ALL[orientation], follow: follow_policy(follow) }
                } else {
                    Command::Move { items: selection.clone(), delta: del_point(dx, dy), follow: follow_policy(follow) }
                };
                let transform = if rotate {
                    Isometry::Reorient { pivot: del_point(index as i32 * 40, 0), by: Orient::ALL[orientation] }
                } else { Isometry::Translate(del_point(dx, dy)) };
                let mut preview = FollowPlan::default();
                let expected = crate::plan_follow(&base, &selection, transform, follow_policy(follow), &mut preview)
                    .ok().map(|()| expected_follow_partition(&base, &selection, transform, follow));
                let result = follow_roundtrip(&mut doc, command);
                // An identity transform is an inert command (T5.1) even where the
                // planner, asked to re-route in place, would refuse; logged for W2.1.
                let identity = transform == Isometry::Translate(Pt::ZERO)
                    || matches!(transform, Isometry::Reorient { by: Orient::IDENTITY, .. });
                if identity {
                    prop_assert!(result.is_ok() && doc.content_eq(&base));
                    continue;
                }
                // Only parts move here, so the command refuses exactly when planning does.
                prop_assert_eq!(result.is_ok(), expected.is_some(), "{:?}", result);
                if result.is_ok() {
                    let after = pin_partition(&doc);
                    let n = all_pins.len();
                    let exempt = |k: usize| follow && (left_behind[k / n] || left_behind[k % n]);
                    let expected = expected.as_ref().unwrap();
                    for k in 0..after.len() {
                        if !exempt(k) {
                            prop_assert_eq!(after[k], expected[k], "pin pair {}", k);
                        } else {
                            // A left-behind pin may lose connections, never gain one.
                            prop_assert!(!after[k] || expected[k], "pin pair {} gained a connection", k);
                        }
                    }
                    if follow {
                        for (k, &connected) in before.iter().enumerate() {
                            if connected && !exempt(k) {
                                prop_assert!(after[k], "WINV1: pin pair {} lost its connection", k);
                            }
                        }
                        doc.assert_normal_form();
                    } else {
                        // WINV3 holds in detach mode too (owner decision 2026-11-17).
                        doc.assert_normal_form();
                        let parts: Vec<_> = selection.iter().map(|&item| match item {
                            ItemId::Part(part) => part,
                            _ => unreachable!(),
                        }).collect();
                        let old_pins: Vec<Pt> = parts.iter().flat_map(|&p| base.pins(p).unwrap().iter().copied()).collect();
                        let new_pins: Vec<Pt> = parts.iter().flat_map(|&p| doc.pins(p).unwrap().iter().copied()).collect();
                        for (id, &wire) in base.wires() {
                            let landed = new_pins.iter().any(|&at| contains(wire, at));
                            let vacated = old_pins.contains(&wire.a) || old_pins.contains(&wire.b);
                            if !landed && !vacated {
                                // WINV2 outside TC-move-14 splits and vacated-point merges.
                                prop_assert_eq!(doc.wire(id), Some(&wire));
                            }
                        }
                        // The only moving pins belong to the selected boundary parts.
                        for (id, _) in base.parts() {
                            if !selection.contains(&ItemId::Part(id)) {
                                prop_assert_eq!(doc.pins(id), base.pins(id));
                            }
                        }
                    }
                }
            }
        }
    }

    fn contains(wire: Wire, at: Pt) -> bool {
        Rect::from_points(wire.a, wire.b).unwrap().contains(at)
    }

    // WINV1/WINV2: original nets plus the explicitly permitted WD3 landing contacts.
    // Which wires are replaced comes from the planner's `remove` list: the oracle
    // checks how the command applies a plan, not which runs the planner chose.
    fn expected_follow_partition(
        base: &Doc,
        selected: &[ItemId],
        t: Isometry,
        follow: bool,
    ) -> Vec<bool> {
        let pins: Vec<_> = base
            .parts()
            .flat_map(|(id, _)| {
                base.pins(id)
                    .unwrap()
                    .iter()
                    .map(move |&p| (ItemId::Part(id), p))
            })
            .collect();
        let mut plan = FollowPlan::default();
        crate::plan_follow(base, selected, t, follow_policy(follow), &mut plan).unwrap();
        let n = pins.len();
        let mut expected = pin_partition(base);
        if !follow {
            for i in 0..n {
                for j in 0..n {
                    if i != j && (selected.contains(&pins[i].0) || selected.contains(&pins[j].0)) {
                        expected[i * n + j] = false;
                    }
                }
            }
        }
        for (i, &(part, old)) in pins.iter().enumerate() {
            if !selected.contains(&part) {
                continue;
            }
            let at = t.apply(old).unwrap();
            for (j, &(other, p)) in pins.iter().enumerate() {
                let p = if selected.contains(&other) {
                    t.apply(p).unwrap()
                } else {
                    p
                };
                let contact = at == p
                    || base.wires().any(|(id, &w)| {
                        !plan.remove.contains(&id)
                            && contains(w, at)
                            && base.net_of(id) == base.net_at(pins[j].1)
                            && (follow || !selected.contains(&other))
                    });
                if contact {
                    expected[i * n + j] = true;
                    expected[j * n + i] = true;
                }
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    expected[i * n + j] |= expected[i * n + k] && expected[k * n + j];
                }
            }
        }
        expected
    }

    fn assert_delete_roundtrip(doc: &mut Doc, items: Vec<ItemId>, with_wires: bool) {
        let before = del_snapshot(doc);
        let change = doc.apply(Command::Delete { items, with_wires }).unwrap();
        doc.assert_normal_form();
        assert_doc_consistent(doc); // INV10: independent connectivity rebuild.
        let after = del_snapshot(doc);
        if change.dirty.is_some() {
            doc.undo().unwrap();
            assert!(doc.content_eq(&before)); // INV4 includes exact live ids.
            doc.assert_normal_form();
            assert_doc_consistent(doc);
            doc.redo().unwrap();
            assert!(doc.content_eq(&after));
            doc.assert_normal_form();
            assert_doc_consistent(doc);
        }
    }

    #[test]
    fn tc_del_01_02_runs_to_far_pin_flag_on_and_off_inv4_inv10() {
        for with_wires in [false, true] {
            let mut doc = Doc::new();
            let part = del_part(&mut doc, del_point(0, 0));
            del_part(&mut doc, del_point(2, 2));
            doc.apply(Command::AddWires(vec![
                del_wire(del_point(0, 0), del_point(2, 0)),
                del_wire(del_point(2, 0), del_point(2, 2)),
                del_wire(del_point(2, 2), del_point(4, 2)),
            ]))
            .unwrap();
            let wires: Vec<_> = doc.wires().map(|(id, w)| (id, *w)).collect();
            assert_delete_roundtrip(&mut doc, vec![part], with_wires);
            assert_eq!(doc.wires().count(), if with_wires { 1 } else { 3 });
            for (id, wire) in wires {
                let beyond_pin = wire.a == del_point(2, 2);
                assert_eq!(doc.wire(id).is_some(), !with_wires || beyond_pin);
            }
        }
    }

    #[test]
    fn tc_del_03_junction_other_wires_merge_locally_inv4_inv10() {
        let mut doc = Doc::new();
        let part = del_part(&mut doc, del_point(0, 0));
        doc.apply(Command::AddWires(vec![
            del_wire(del_point(0, 0), del_point(2, 0)),
            del_wire(del_point(2, 0), del_point(2, 2)),
            del_wire(del_point(0, 2), del_point(2, 2)),
            del_wire(del_point(2, 2), del_point(4, 2)),
        ]))
        .unwrap();
        assert_delete_roundtrip(&mut doc, vec![part], true);
        assert_eq!(doc.wires().count(), 1);
        let (_, wire) = doc.wires().next().unwrap();
        assert_eq!(*wire, del_wire(del_point(0, 2), del_point(4, 2)));
    }

    #[test]
    fn wd10_shared_run_explicit_selection_and_refusal_are_atomic() {
        let mut doc = Doc::new();
        let a = del_part(&mut doc, del_point(0, 0));
        let b = del_part(&mut doc, del_point(2, 0));
        doc.apply(Command::AddWires(vec![del_wire(
            del_point(0, 0),
            del_point(2, 0),
        )]))
        .unwrap();
        let wire = ItemId::Wire(doc.wires().next().unwrap().0);
        let before = doc.save_to_vec_for_test();
        for items in [vec![a, a], vec![a, ItemId::Wire(WireId(999))]] {
            assert!(
                doc.apply(Command::Delete {
                    items,
                    with_wires: true
                })
                .is_err()
            );
            assert_eq!(doc.save_to_vec_for_test(), before);
        }
        assert_delete_roundtrip(&mut doc, vec![b, wire, a], true);
        assert_eq!(doc.parts().count(), 0);
        assert_eq!(doc.wires().count(), 0);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        #[test]
        fn wd10_random_sheets_selections_inv4_inv10_and_order(
            shapes in prop::collection::vec((any::<bool>(), any::<bool>(), any::<bool>()), 1..12),
            mask in prop::collection::vec(any::<bool>(), 0..80),
        ) {
            let mut base = Doc::new();
            for (i, &(junction, far_pin, labelled)) in shapes.iter().enumerate() {
                let x = i as i32 * 6;
                del_part(&mut base, del_point(x, 0));
                if far_pin { del_part(&mut base, del_point(x + 4, 2)); }
                if labelled {
                    base.apply(Command::AddLabel(Label { at: del_point(x + 2, 0), text: "N".into() })).unwrap();
                }
                let mut wires = vec![
                    del_wire(del_point(x, 0), del_point(x + 2, 0)),
                    del_wire(del_point(x + 2, 0), del_point(x + 2, 2)),
                    del_wire(del_point(x + 2, 2), del_point(x + 4, 2)),
                ];
                if junction { wires.push(del_wire(del_point(x, 2), del_point(x + 2, 2))); }
                base.apply(Command::AddWires(wires)).unwrap();
            }
            let live: Vec<_> = base.parts().map(|(id, _)| ItemId::Part(id))
                .chain(base.wires().map(|(id, _)| ItemId::Wire(id))).collect();
            let items: Vec<_> = live.into_iter().enumerate()
                .filter_map(|(i, item)| mask.get(i).copied().unwrap_or(false).then_some(item)).collect();
            for with_wires in [false, true] {
                let mut doc = del_snapshot(&base);
                let mut reversed = del_snapshot(&base);
                assert_delete_roundtrip(&mut doc, items.clone(), with_wires);
                assert_delete_roundtrip(&mut reversed, items.iter().copied().rev().collect(), with_wires);
                prop_assert!(doc.content_eq(&reversed));
            }
        }
    }

    #[test]
    fn no_op_commands_preserve_history_revision_and_redo() {
        let label = Label {
            at: Pt { x: 100, y: 0 },
            text: "GND".into(),
        };
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![Wire {
            a: Pt::ZERO,
            b: Pt { x: 100, y: 0 },
        }]))
        .unwrap();
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
            Command::Delete {
                items: vec![],
                with_wires: false,
            },
            Command::Move {
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                items: vec![],
                delta: Pt { x: 1, y: 0 },
            },
            Command::Reorient {
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                items: vec![],
                pivot: Pt::ZERO,
                by: Orient::ROT_90,
            },
            Command::Move {
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                items: vec![item],
                delta: Pt::ZERO,
            },
            Command::Reorient {
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                items: vec![item],
                pivot: Pt::ZERO,
                by: Orient::IDENTITY,
            },
            Command::DragSegment {
                wire: WireId(0),
                offset: 0,
            },
            Command::SetText {
                item,
                field: TextField::Label,
                text: label.text,
            },
            Command::Batch(vec![
                Command::Batch(vec![]),
                Command::Delete {
                    items: vec![],
                    with_wires: false,
                },
            ]),
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
        doc.undo().unwrap();
        assert!(doc.undo().is_none(), "no-op must not add undo entries");
        // A semantically inert command still validates its target and field.
        assert!(matches!(
            doc.apply(Command::Move {
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                items: vec![item],
                delta: Pt::ZERO
            }),
            Err(CommandError::MissingItem(_))
        ));
        assert_eq!(
            doc.apply(Command::DragSegment {
                wire: WireId(0),
                offset: 0,
            }),
            Err(CommandError::MissingItem(ItemId::Wire(WireId(0))))
        );
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
            follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                    follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
        let del_ch = doc
            .apply(Command::Delete {
                items: group.clone(),
                with_wires: false,
            })
            .unwrap();
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
                    follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
            follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                                Command::Move { follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                                Command::Reorient { follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
                                Command::Delete { items, with_wires: false }
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
                                    cmds.push(Command::Move { follow: crate::prefs::FollowPolicy::new(false, 10, false).unwrap(),
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
