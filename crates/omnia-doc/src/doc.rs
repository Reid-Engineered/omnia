//! Document storage. Mutation stays inside the crate for the T5.1 command layer.

#[path = "io.rs"]
mod io;

use std::{fmt, sync::Arc};

use crate::{
    Change, Command, CommandError, Connectivity, GeomError, NetId, Orient, Pt, Rect, SpatialIndex,
    TextField,
    arena::Arena,
    points::{End, Points},
    undo::History,
};

macro_rules! id {
    ($name:ident, $repr:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub $repr);
    };
}

id!(PartId, u32);
id!(WireId, u32);
id!(LabelId, u32);
/// Key of a symbol definition. The definitions live in `omnia-symbols`, which re-exports this
/// type; it is defined here because `omnia-symbols` depends on `omnia-doc` for geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolId(pub u16);

impl SymbolId {
    /// Returns the zero-based index of this symbol in the library.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns the raw numeric identifier.
    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemId {
    Part(PartId),
    Wire(WireId),
    Label(LabelId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub symbol: SymbolId,
    pub at: Pt,
    pub orient: Orient,
    pub designator: Arc<str>,
    pub value: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wire {
    pub a: Pt,
    pub b: Pt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub at: Pt,
    pub text: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    OutOfBounds,
    InvalidWire,
    InvalidId,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OutOfBounds => "item or pin lies outside K1 sheet limits",
            Self::InvalidId => "restore requires an allocated empty slot",
            Self::InvalidWire => "wire must be axis-aligned with distinct ends",
        })
    }
}

impl std::error::Error for StoreError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredPart {
    pub(crate) part: Part,
    pub(crate) pins: Vec<Pt>,
    pub(crate) local_bounds: Rect,
}

#[derive(Debug, Default)]
pub struct Doc {
    pub(crate) parts: Arena<StoredPart>,
    pub(crate) wires: Arena<Wire>,
    pub(crate) labels: Arena<Label>,
    pub(crate) points: Points,
    pub(crate) index: SpatialIndex,
    pub(crate) connectivity: Connectivity,
    pub(crate) history: History,
    revision: u64,
}

impl Doc {
    /// Creates an empty store. Callers supply symbol-local pin positions and art bounds.
    pub fn new() -> Self {
        Self::default()
    }

    /// Full copy for previews and jobs (A1.1, AD3): every arena slot, reserved ones
    /// included, the point map, the spatial index, the connectivity and the revision. The
    /// history is empty, so the fork cannot undo past its own commands. `fork() == self`
    /// under `PartialEq`, and the same command applied to both gives the same `Change`.
    pub fn fork(&self) -> Doc {
        Doc {
            parts: self.parts.clone(),
            wires: self.wires.clone(),
            labels: self.labels.clone(),
            points: self.points.clone(),
            index: self.index.clone(),
            connectivity: self.connectivity.clone(),
            history: History::default(),
            revision: self.revision,
        }
    }

    /// Validates and executes a command transactionally, recording its inverse for undo.
    pub fn apply(&mut self, cmd: Command) -> Result<Change, CommandError> {
        crate::command::apply_command(self, cmd)
    }

    /// Reverts the most recent command, restoring exact IDs and derived state.
    pub fn undo(&mut self) -> Option<Change> {
        crate::undo::undo(self)
    }

    /// Reapplies the most recently undone command, restoring exact IDs and derived state.
    pub fn redo(&mut self) -> Option<Change> {
        crate::undo::redo(self)
    }

    /// Replaces out; allocates nothing when its capacity holds the result.
    pub fn query(&self, area: Rect, out: &mut Vec<ItemId>) {
        self.index.query(area, out);
    }

    /// Replaces `out` with every live item wholly inside `area` (WD8 Enclosed).
    ///
    /// `area` is normalised before testing, so a box drawn in any direction produces the same
    /// result. Edge handling is inclusive on all four sides (`Rect::contains_rect`):
    /// - **Part**: its clamped world symbol bounds ([`Doc::part_body`], pins excluded) lie
    ///   wholly inside `area`.
    /// - **Wire**: both endpoints (and thus its entire axis-aligned segment) lie inside `area`.
    /// - **Label**: its anchor point lies inside `area` (text extents are excluded, WD8).
    ///
    /// Allocates nothing when `out` already has capacity for the result.
    pub fn query_within(&self, area: Rect, out: &mut Vec<ItemId>) {
        let area = crate::index::normalize_rect(area);
        self.index
            .query_filtered(area, out, |item, bounds| match item {
                ItemId::Part(id) => self
                    .part_body(id)
                    .is_some_and(|body| area.contains_rect(body)),
                ItemId::Wire(_) | ItemId::Label(_) => area.contains_rect(bounds),
            });
    }

    /// Replaces `out` with every live item touched by `area` (WD8 Intersecting).
    ///
    /// `area` is normalised before testing, so a box drawn in any direction produces the same
    /// result. Edge handling is inclusive on all four sides (`Rect::intersects`):
    /// - **Part**: its clamped world symbol bounds ([`Doc::part_body`], pins excluded)
    ///   intersect `area` — a box touching only a pin stub outside the body does not match.
    /// - **Wire**: its axis-aligned segment intersects `area`.
    /// - **Label**: its anchor point lies inside `area` (text extents are excluded, WD8).
    ///
    /// Unlike [`Doc::query`] (which uses the body+pins envelope for picking and snapping), this
    /// normalises inverted boxes and counts parts by symbol bounds only. Allocates nothing when
    /// `out` already has capacity for the result.
    pub fn query_touching(&self, area: Rect, out: &mut Vec<ItemId>) {
        let area = crate::index::normalize_rect(area);
        self.index
            .query_filtered(area, out, |item, bounds| match item {
                ItemId::Part(id) => self.part_body(id).is_some_and(|body| body.intersects(area)),
                ItemId::Wire(_) | ItemId::Label(_) => bounds.intersects(area),
            });
    }
    pub fn part_body(&self, id: PartId) -> Option<Rect> {
        let stored = self.parts.get(id.0)?;
        crate::hit::part_bounds(&stored.part, stored.local_bounds).ok()
    }
    /// Net handles remain valid only until the next edit.
    pub fn net_of(&self, id: WireId) -> Option<NetId> {
        self.connectivity.net_of(id)
    }
    pub fn net_at(&self, at: Pt) -> Option<NetId> {
        self.connectivity.net_at(at)
    }
    pub fn net_wires(&self, net: NetId, out: &mut Vec<WireId>) {
        self.connectivity.net_wires(net, out);
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Opaque id of the current state in the undo history (UD6, U2.1). An
    /// effective `apply` moves to a fresh value; `undo` and `redo` move back
    /// to the stored value, so equality means the same history state. A
    /// document is modified exactly when this differs from the cursor
    /// captured at its last save.
    pub fn history_cursor(&self) -> u64 {
        self.history.cursor()
    }

    /// Whether `undo` would change the document.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether `redo` would change the document.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Number of live parts.
    pub fn part_count(&self) -> usize {
        self.parts().count()
    }

    /// Number of live logical nets.
    pub fn net_count(&self) -> usize {
        self.connectivity.net_count()
    }

    pub fn part(&self, id: PartId) -> Option<&Part> {
        self.parts.get(id.0).map(|stored| &stored.part)
    }

    pub fn wire(&self, id: WireId) -> Option<&Wire> {
        self.wires.get(id.0)
    }

    pub fn label(&self, id: LabelId) -> Option<&Label> {
        self.labels.get(id.0)
    }

    /// World-space pin positions; missing or removed IDs return None.
    pub fn pins(&self, id: PartId) -> Option<&[Pt]> {
        self.parts.get(id.0).map(|stored| stored.pins.as_slice())
    }

    /// Live parts in stable ID order, skipping removed slots.
    pub fn parts(&self) -> impl Iterator<Item = (PartId, &Part)> {
        self.parts
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|s| (PartId(i as u32), &s.part)))
    }

    /// Live wires in stable ID order, skipping removed slots.
    pub fn wires(&self) -> impl Iterator<Item = (WireId, &Wire)> {
        self.wires
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|w| (WireId(i as u32), w)))
    }

    /// Live labels in stable ID order, skipping removed slots.
    pub fn labels(&self) -> impl Iterator<Item = (LabelId, &Label)> {
        self.labels
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|l| (LabelId(i as u32), l)))
    }

    /// Replaces `out` with points inside `area` having at least three ends/pins.
    /// Ordering is unspecified. Dots are always derived from the incidence map.
    pub fn junctions(&self, area: Rect, out: &mut Vec<Pt>) {
        self.points.junctions(area, out);
    }

    /// Replaces `out` with the labels whose point is exactly `at`, in ascending id order. Read
    /// from the connectivity point store, which tracks the labels at each point (D2). Allocates
    /// nothing when `out` has room for the result.
    pub fn labels_at(&self, at: Pt, out: &mut Vec<LabelId>) {
        out.clear();
        out.extend_from_slice(self.connectivity.labels_at(at));
        out.sort_unstable_by_key(|l| l.0);
    }

    pub(crate) fn validate_wire(wire: Wire) -> Result<(), CommandError> {
        check_point(wire.a)?;
        check_point(wire.b)?;
        if wire.a == wire.b || (wire.a.x != wire.b.x && wire.a.y != wire.b.y) {
            return Err(CommandError::InvalidWire);
        }
        Ok(())
    }

    pub(crate) fn part_bounds_at(
        at: Pt,
        orient: Orient,
        local_bounds: Rect,
    ) -> Result<Rect, GeomError> {
        crate::hit::part_bounds_at(at, orient, local_bounds)
    }

    pub(crate) fn part_envelope_raw(
        at: Pt,
        orient: Orient,
        local_bounds: Rect,
        world_pins: &[Pt],
    ) -> Result<Rect, GeomError> {
        let body = Self::part_bounds_at(at, orient, local_bounds)?;
        Ok(world_pins
            .iter()
            .fold(body, |area, &pin| area.union(Rect { min: pin, max: pin })))
    }

    pub(crate) fn insert_part_raw(
        &mut self,
        part: Part,
        local_pins: &[Pt],
        local_bounds: Rect,
    ) -> Result<(PartId, Rect), StoreError> {
        Rect::new(local_bounds.min, local_bounds.max).map_err(|_| StoreError::OutOfBounds)?;
        check_point(part.at)?;
        let pins = local_pins
            .iter()
            .map(|&pin| {
                check_point(pin)?;
                part.at
                    .checked_add(part.orient.apply(pin))
                    .map_err(|_| StoreError::OutOfBounds)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let area = Self::part_envelope_raw(part.at, part.orient, local_bounds, &pins)
            .map_err(|_| StoreError::OutOfBounds)?;
        let id = PartId(self.parts.insert(StoredPart {
            part,
            pins,
            local_bounds,
        }));
        self.index.insert(ItemId::Part(id), area).unwrap();
        let pins_slice = &self.parts.get(id.0).unwrap().pins;
        self.connectivity.add_part(id, pins_slice);
        for (pin, &at) in pins_slice.iter().enumerate() {
            self.points.insert(at, End::Pin { part: id, pin });
        }
        Ok((id, area))
    }

    pub(crate) fn restore_stored_part_raw(
        &mut self,
        id: PartId,
        stored: StoredPart,
    ) -> Result<Rect, StoreError> {
        Rect::new(stored.local_bounds.min, stored.local_bounds.max)
            .map_err(|_| StoreError::OutOfBounds)?;
        check_point(stored.part.at)?;
        for &pin in &stored.pins {
            check_point(pin)?;
        }
        let area = Self::part_envelope_raw(
            stored.part.at,
            stored.part.orient,
            stored.local_bounds,
            &stored.pins,
        )
        .map_err(|_| StoreError::OutOfBounds)?;
        self.parts
            .restore(id.0, stored)
            .map_err(|_| StoreError::InvalidId)?;
        self.index.insert(ItemId::Part(id), area).unwrap();
        let pins_slice = &self.parts.get(id.0).unwrap().pins;
        self.connectivity.add_part(id, pins_slice);
        for (pin, &at) in pins_slice.iter().enumerate() {
            self.points.insert(at, End::Pin { part: id, pin });
        }
        Ok(area)
    }

    pub(crate) fn remove_stored_part_raw(&mut self, id: PartId) -> Option<(StoredPart, Rect)> {
        let stored = self.parts.remove(id.0)?;
        let bounds = self
            .index
            .remove(ItemId::Part(id))
            .expect("live part must exist in index");
        self.connectivity.remove_part(id);
        for (pin, &at) in stored.pins.iter().enumerate() {
            self.points.remove(at, End::Pin { part: id, pin });
        }
        Some((stored, bounds))
    }

    pub(crate) fn insert_wire_raw(&mut self, wire: Wire) -> Result<(WireId, Rect), StoreError> {
        Self::validate_wire(wire).map_err(|err| match err {
            CommandError::OutOfBounds => StoreError::OutOfBounds,
            CommandError::InvalidWire => StoreError::InvalidWire,
            _ => unreachable!(),
        })?;
        let area = Rect::from_points(wire.a, wire.b).unwrap();
        let id = WireId(self.wires.insert(wire));
        self.index.insert(ItemId::Wire(id), area).unwrap();
        self.connectivity.add_wire(id, wire);
        self.points.insert(wire.a, End::Wire(id));
        self.points.insert(wire.b, End::Wire(id));
        Ok((id, area))
    }

    pub(crate) fn restore_wire_raw(&mut self, id: WireId, wire: Wire) -> Result<Rect, StoreError> {
        Self::validate_wire(wire).map_err(|err| match err {
            CommandError::OutOfBounds => StoreError::OutOfBounds,
            CommandError::InvalidWire => StoreError::InvalidWire,
            _ => unreachable!(),
        })?;
        self.wires
            .restore(id.0, wire)
            .map_err(|_| StoreError::InvalidId)?;
        let area = Rect::from_points(wire.a, wire.b).unwrap();
        self.index.insert(ItemId::Wire(id), area).unwrap();
        self.connectivity.add_wire(id, wire);
        self.points.insert(wire.a, End::Wire(id));
        self.points.insert(wire.b, End::Wire(id));
        Ok(area)
    }

    pub(crate) fn remove_wire_raw(&mut self, id: WireId) -> Option<(Wire, Rect)> {
        let wire = self.wires.remove(id.0)?;
        let bounds = self
            .index
            .remove(ItemId::Wire(id))
            .expect("live wire must exist in index");
        self.connectivity.remove_wire(id);
        self.points.remove(wire.a, End::Wire(id));
        self.points.remove(wire.b, End::Wire(id));
        Some((wire, bounds))
    }

    pub(crate) fn insert_label_raw(&mut self, label: Label) -> Result<(LabelId, Rect), StoreError> {
        check_point(label.at)?;
        let area = Rect {
            min: label.at,
            max: label.at,
        };
        let id = LabelId(self.labels.insert(label));
        let label = self.labels.get(id.0).unwrap();
        self.index.insert(ItemId::Label(id), area).unwrap();
        self.connectivity.add_label(id, label);
        Ok((id, area))
    }

    pub(crate) fn restore_label_raw(
        &mut self,
        id: LabelId,
        label: Label,
    ) -> Result<Rect, StoreError> {
        check_point(label.at)?;
        self.labels
            .restore(id.0, label)
            .map_err(|_| StoreError::InvalidId)?;
        let label = self.labels.get(id.0).unwrap();
        let area = Rect {
            min: label.at,
            max: label.at,
        };
        self.index.insert(ItemId::Label(id), area).unwrap();
        self.connectivity.add_label(id, label);
        Ok(area)
    }

    pub(crate) fn remove_label_raw(&mut self, id: LabelId) -> Option<(Label, Rect)> {
        let label = self.labels.remove(id.0)?;
        let bounds = self
            .index
            .remove(ItemId::Label(id))
            .expect("live label must exist in index");
        self.connectivity.remove_label(id);
        Some((label, bounds))
    }

    pub(crate) fn set_text_raw(
        &mut self,
        item: ItemId,
        field: TextField,
        text: Arc<str>,
    ) -> Result<(Arc<str>, Rect, bool), CommandError> {
        match item {
            ItemId::Part(id) => {
                let stored = self
                    .parts
                    .get_mut(id.0)
                    .ok_or(CommandError::MissingItem(item))?;
                let slot = match field {
                    TextField::Designator => &mut stored.part.designator,
                    TextField::Value => &mut stored.part.value,
                    TextField::Label => {
                        return Err(CommandError::InvalidTextField { item, field });
                    }
                };
                let old = std::mem::replace(slot, text);
                let bounds = self
                    .index
                    .bounds(item)
                    .expect("live part must exist in index");
                Ok((old, bounds, false))
            }
            ItemId::Wire(id) => {
                if self.wires.get(id.0).is_none() {
                    return Err(CommandError::MissingItem(item));
                }
                Err(CommandError::InvalidTextField { item, field })
            }
            ItemId::Label(id) => {
                let Some(existing) = self.labels.get(id.0) else {
                    return Err(CommandError::MissingItem(item));
                };
                if field != TextField::Label {
                    return Err(CommandError::InvalidTextField { item, field });
                }
                let changed = existing.text != text;
                if changed {
                    self.connectivity.remove_label(id);
                }
                let label = self.labels.get_mut(id.0).unwrap();
                let old = std::mem::replace(&mut label.text, text);
                if changed {
                    self.connectivity.add_label(id, label);
                }
                let bounds = Rect {
                    min: label.at,
                    max: label.at,
                };
                Ok((old, bounds, changed))
            }
        }
    }

    pub(crate) fn insert_part(
        &mut self,
        part: Part,
        local_pins: &[Pt],
        local_bounds: Rect,
    ) -> Result<PartId, StoreError> {
        let (id, _) = self.insert_part_raw(part, local_pins, local_bounds)?;
        self.bump_revision();
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn restore_part(
        &mut self,
        id: PartId,
        part: Part,
        local_pins: &[Pt],
        local_bounds: Rect,
    ) -> Result<PartId, StoreError> {
        Rect::new(local_bounds.min, local_bounds.max).map_err(|_| StoreError::OutOfBounds)?;
        check_point(part.at)?;
        let pins = local_pins
            .iter()
            .map(|&pin| {
                check_point(pin)?;
                part.at
                    .checked_add(part.orient.apply(pin))
                    .map_err(|_| StoreError::OutOfBounds)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.restore_stored_part_raw(
            id,
            StoredPart {
                part,
                pins,
                local_bounds,
            },
        )?;
        self.bump_revision();
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn remove_part(&mut self, id: PartId) -> Option<Part> {
        let (stored, _) = self.remove_stored_part_raw(id)?;
        self.bump_revision();
        Some(stored.part)
    }

    pub(crate) fn insert_wire(&mut self, wire: Wire) -> Result<WireId, StoreError> {
        let (id, _) = self.insert_wire_raw(wire)?;
        self.bump_revision();
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn restore_wire(&mut self, id: WireId, wire: Wire) -> Result<WireId, StoreError> {
        self.restore_wire_raw(id, wire)?;
        self.bump_revision();
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn remove_wire(&mut self, id: WireId) -> Option<Wire> {
        let (wire, _) = self.remove_wire_raw(id)?;
        self.bump_revision();
        Some(wire)
    }

    pub(crate) fn insert_label(&mut self, label: Label) -> Result<LabelId, StoreError> {
        let (id, _) = self.insert_label_raw(label)?;
        self.bump_revision();
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn restore_label(
        &mut self,
        id: LabelId,
        label: Label,
    ) -> Result<LabelId, StoreError> {
        self.restore_label_raw(id, label)?;
        self.bump_revision();
        Ok(id)
    }

    #[cfg(test)]
    pub(crate) fn remove_label(&mut self, id: LabelId) -> Option<Label> {
        let (label, _) = self.remove_label_raw(id)?;
        self.bump_revision();
        Some(label)
    }

    pub(crate) fn bump_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("document revision exhausted");
    }
}

fn check_point(at: Pt) -> Result<(), StoreError> {
    if at.is_in_bounds() {
        Ok(())
    } else {
        Err(StoreError::OutOfBounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SHEET_LIMIT_NM;
    use proptest::prelude::*;
    use std::collections::HashMap;

    fn part(at: Pt, orient: Orient) -> Part {
        Part {
            symbol: SymbolId(0),
            at,
            orient,
            designator: "R1".into(),
            value: "1k".into(),
        }
    }

    fn sheet() -> Rect {
        Rect::from_points(
            Pt {
                x: -SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
            Pt {
                x: SHEET_LIMIT_NM,
                y: SHEET_LIMIT_NM,
            },
        )
        .unwrap()
    }

    #[test]
    fn revisions_and_rejected_mutations() {
        let mut doc = Doc::new();
        let bad = Pt {
            x: SHEET_LIMIT_NM + 1,
            y: 0,
        };
        assert_eq!(
            doc.insert_wire(Wire {
                a: Pt::ZERO,
                b: bad
            }),
            Err(StoreError::OutOfBounds)
        );
        assert_eq!(
            doc.insert_label(Label {
                at: bad,
                text: "bad".into()
            }),
            Err(StoreError::OutOfBounds)
        );
        assert_eq!(
            doc.insert_part(
                part(
                    Pt {
                        x: SHEET_LIMIT_NM,
                        y: 0
                    },
                    Orient::IDENTITY
                ),
                &[Pt { x: 1, y: 0 }],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap()
            ),
            Err(StoreError::OutOfBounds)
        );
        assert_eq!(doc.remove_wire(WireId(0)), None);
        assert_eq!(doc.remove_part(PartId(0)), None);
        assert_eq!(doc.remove_label(LabelId(0)), None);
        assert_eq!(doc.revision(), 0);
        let p = doc
            .insert_part(
                part(Pt::ZERO, Orient::IDENTITY),
                &[],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )
            .unwrap();
        let l = doc
            .insert_label(Label {
                at: Pt::ZERO,
                text: "net".into(),
            })
            .unwrap();
        assert!(doc.part(p).is_some());
        assert!(doc.label(l).is_some());
        assert_eq!(doc.revision(), 2);
        doc.remove_part(p).unwrap();
        doc.remove_label(l).unwrap();
        assert_eq!(doc.revision(), 4);
        assert!(doc.part(p).is_none());
        assert!(doc.label(l).is_none());
    }

    #[test]
    fn bodies_do_not_count_and_removed_ends_leave_no_dot() {
        let mut doc = Doc::new();
        let horizontal = Wire {
            a: Pt { x: -10, y: 0 },
            b: Pt { x: 10, y: 0 },
        };
        doc.insert_wire(horizontal).unwrap();
        doc.insert_wire(Wire {
            a: Pt { x: 0, y: -10 },
            b: Pt { x: 0, y: 10 },
        })
        .unwrap();
        let mut out = vec![Pt::ZERO];
        doc.junctions(sheet(), &mut out);
        assert!(out.is_empty());
        let a = doc
            .insert_wire(Wire {
                a: Pt::ZERO,
                b: Pt { x: 1, y: 0 },
            })
            .unwrap();
        let b = doc
            .insert_wire(Wire {
                a: Pt::ZERO,
                b: Pt { x: 0, y: 1 },
            })
            .unwrap();
        doc.insert_label(Label {
            at: Pt::ZERO,
            text: "not an end".into(),
        })
        .unwrap();
        doc.junctions(sheet(), &mut out);
        assert!(out.is_empty());
        let p = doc
            .insert_part(
                part(Pt::ZERO, Orient::IDENTITY),
                &[Pt::ZERO],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )
            .unwrap();
        doc.junctions(sheet(), &mut out);
        assert_eq!(out, vec![Pt::ZERO]);
        doc.remove_part(p).unwrap();
        doc.junctions(sheet(), &mut out);
        assert!(out.is_empty());
        doc.remove_wire(a).unwrap();
        doc.remove_wire(b).unwrap();
        doc.junctions(sheet(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn invalid_local_bounds_rejected_without_mutation() {
        let p = part(Pt::ZERO, Orient::IDENTITY);
        let bounds = Rect::new(Pt::ZERO, Pt { x: 10, y: 10 }).unwrap();
        let mut doc = Doc::new();
        let id = doc.insert_part(p.clone(), &[Pt::ZERO], bounds).unwrap();
        doc.remove_part(id).unwrap();
        let mut before = Vec::new();
        doc.save(&mut before).unwrap();
        for bad in [
            Rect {
                min: Pt { x: 10, y: 0 },
                max: Pt::ZERO,
            },
            Rect {
                min: Pt { x: 0, y: 10 },
                max: Pt::ZERO,
            },
            Rect {
                min: Pt::ZERO,
                max: Pt {
                    x: SHEET_LIMIT_NM + 1,
                    y: 0,
                },
            },
        ] {
            assert_eq!(
                doc.insert_part(p.clone(), &[Pt::ZERO], bad),
                Err(StoreError::OutOfBounds)
            );
            assert_eq!(
                doc.restore_part(id, p.clone(), &[Pt::ZERO], bad),
                Err(StoreError::OutOfBounds)
            );
            assert_eq!(
                Doc::from_items([(p.clone(), vec![Pt::ZERO], bad)], [], []).unwrap_err(),
                StoreError::OutOfBounds
            );
            let mut after = Vec::new();
            doc.save(&mut after).unwrap();
            assert_eq!(before, after);
            let mut hits = Vec::new();
            doc.query(sheet(), &mut hits);
            assert!(hits.is_empty());
            crate::connectivity::tests::assert_invariants(&doc.connectivity);
        }
        doc.restore_part(id, p, &[Pt::ZERO], bounds).unwrap();
        let mut saved = Vec::new();
        doc.save(&mut saved).unwrap();
        assert_eq!(doc, Doc::load(saved.as_slice()).unwrap());
    }

    #[test]
    fn rejected_restores_preserve_all_state() {
        let mut doc = Doc::new();
        let bounds = Rect::new(Pt::ZERO, Pt::ZERO).unwrap();
        let p = part(Pt::ZERO, Orient::IDENTITY);
        let w = Wire {
            a: Pt::ZERO,
            b: Pt { x: 1, y: 0 },
        };
        let l = Label {
            at: Pt::ZERO,
            text: "GND".into(),
        };
        let pid = doc.insert_part(p.clone(), &[Pt::ZERO], bounds).unwrap();
        let wid = doc.insert_wire(w).unwrap();
        let lid = doc.insert_label(l.clone()).unwrap();
        let mut before = Vec::new();
        doc.save(&mut before).unwrap();
        for raw in [0, 100] {
            assert_eq!(
                doc.restore_part(PartId(raw), p.clone(), &[Pt::ZERO], bounds),
                Err(StoreError::InvalidId)
            );
            assert_eq!(doc.restore_wire(WireId(raw), w), Err(StoreError::InvalidId));
            assert_eq!(
                doc.restore_label(LabelId(raw), l.clone()),
                Err(StoreError::InvalidId)
            );
        }
        assert_eq!(
            doc.insert_wire(Wire {
                a: Pt::ZERO,
                b: Pt::ZERO
            }),
            Err(StoreError::InvalidWire)
        );
        let mut after = Vec::new();
        doc.save(&mut after).unwrap();
        assert_eq!(before, after);
        doc.remove_part(pid);
        doc.restore_part(pid, p, &[Pt::ZERO], bounds).unwrap();
        doc.remove_wire(wid);
        doc.restore_wire(wid, w).unwrap();
        doc.remove_label(lid);
        doc.restore_label(lid, l).unwrap();
        crate::connectivity::tests::assert_invariants(&doc.connectivity);
    }

    #[test]
    fn abutting_pins_and_label_only_components() {
        let b = Rect::new(Pt::ZERO, Pt::ZERO).unwrap();
        let mut doc = Doc::new();
        let a = doc
            .insert_part(part(Pt::ZERO, Orient::IDENTITY), &[Pt::ZERO], b)
            .unwrap();
        let c = doc
            .insert_part(part(Pt::ZERO, Orient::IDENTITY), &[Pt::ZERO], b)
            .unwrap();
        assert_eq!(doc.connectivity.net_count(), 1);
        doc.remove_part(a);
        assert!(doc.net_at(Pt::ZERO).is_some());
        doc.remove_part(c);
        assert!(doc.net_at(Pt::ZERO).is_none());
        for x in [10, 20] {
            doc.insert_label(Label {
                at: Pt { x, y: 0 },
                text: "GND".into(),
            })
            .unwrap();
        }
        assert_eq!(
            doc.net_at(Pt { x: 10, y: 0 }),
            doc.net_at(Pt { x: 20, y: 0 })
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]
        #[test]
        fn integrated_edits_restore_and_rebuild(edits in prop::collection::vec((0u8..9,0usize..32,-4i32..5,-4i32..5,any::<bool>()),1..200)) {
            let mut doc=Doc::new();
            let mut removed_parts=Vec::new(); let mut removed_wires=Vec::new(); let mut removed_labels=Vec::new();
            let bounds=Rect::from_points(Pt{x:-2,y:-2},Pt{x:2,y:2}).unwrap();
            for (op,pick,x,y,h) in edits {
                let at=Pt{x,y};
                match op {
                    0 => { doc.insert_part(part(at,Orient::IDENTITY),&[Pt::ZERO,Pt{x:1,y:0}],bounds).unwrap(); }
                    1 => { doc.insert_wire(Wire{a:at,b:if h {Pt{x:x+1,y}} else {Pt{x,y:y+1}}}).unwrap(); }
                    2 => { doc.insert_label(Label{at,text:if h {"GND"} else {"VCC"}.into()}).unwrap(); }
                    3 => { let ids:Vec<_>=doc.parts().map(|(id,_)|id).collect(); if !ids.is_empty() { let id=ids[pick%ids.len()]; let st=doc.parts.get(id.0).unwrap(); let pins=st.pins.iter().map(|&p|st.part.orient.inverse().apply(p-st.part.at)).collect::<Vec<_>>(); let b=st.local_bounds; let p=doc.remove_part(id).unwrap(); removed_parts.push((id,p,pins,b)); } }
                    4 => { let ids:Vec<_>=doc.wires().map(|(id,_)|id).collect(); if !ids.is_empty() {let id=ids[pick%ids.len()]; removed_wires.push((id,doc.remove_wire(id).unwrap()));} }
                    5 => { let ids:Vec<_>=doc.labels().map(|(id,_)|id).collect(); if !ids.is_empty() {let id=ids[pick%ids.len()]; removed_labels.push((id,doc.remove_label(id).unwrap()));} }
                    6 => {if !removed_parts.is_empty() {let (id,p,pins,b)=removed_parts.swap_remove(pick%removed_parts.len()); doc.restore_part(id,p,&pins,b).unwrap();} }
                    7 => {if !removed_wires.is_empty() {let (id,w)=removed_wires.swap_remove(pick%removed_wires.len()); doc.restore_wire(id,w).unwrap();} }
                    _ => {if !removed_labels.is_empty() {let (id,l)=removed_labels.swap_remove(pick%removed_labels.len()); doc.restore_label(id,l).unwrap();} }
                }
                let oracle=Connectivity::rebuild_with_parts(doc.wires().map(|(id,&w)|(id,w)),doc.labels(),doc.parts().map(|(id,_)|(id,doc.pins(id).unwrap())));
                crate::connectivity::tests::assert_partitions(&doc.connectivity,&oracle);
                let area=Rect::from_points(at,Pt{x:x+2,y:y+2}).unwrap(); let mut found=Vec::new(); doc.query(area,&mut found);
                let mut expected:Vec<_>=doc.parts().filter_map(|(id,_)| {let mut b=doc.part_body(id).unwrap();for &p in doc.pins(id).unwrap(){b=b.union(Rect{min:p,max:p});} b.intersects(area).then_some(ItemId::Part(id))}).chain(doc.wires().filter_map(|(id,w)|Rect::from_points(w.a,w.b).unwrap().intersects(area).then_some(ItemId::Wire(id)))).chain(doc.labels().filter_map(|(id,l)|area.contains(l.at).then_some(ItemId::Label(id)))).collect();
                found.sort_by_key(|&id|crate::hit::item_key(id)); expected.sort_by_key(|&id|crate::hit::item_key(id)); prop_assert_eq!(found,expected);
                let mut bytes=Vec::new(); doc.save(&mut bytes).unwrap(); let loaded=Doc::load(bytes.as_slice()).unwrap(); prop_assert_eq!(&loaded,&doc); crate::connectivity::tests::assert_partitions(&loaded.connectivity,&oracle);
            }
        }
    }

    proptest! {
        #[test]
        fn inv1_checked_at_insert(ax in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM, ay in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM, bx in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM, by in -SHEET_LIMIT_NM..=SHEET_LIMIT_NM, mode in 0..4u8) {
            let a = Pt { x: ax, y: ay };
            let b = match mode { 0 => a, 1 => Pt { x: ax, y: by }, 2 => Pt { x: bx, y: ay }, _ => Pt { x: bx, y: by } };
            let mut doc = Doc::new();
            let result = doc.insert_wire(Wire { a, b });
            let valid = a != b && (a.x == b.x || a.y == b.y);
            prop_assert_eq!(result.is_ok(), valid);
            prop_assert_eq!(doc.revision(), u64::from(valid));
            if let Ok(id) = result { prop_assert_eq!(doc.wire(id), Some(&Wire { a, b })); }
        }

        #[test]
        fn inv3_matches_brute_force_after_each_edit(
            edits in prop::collection::vec((any::<bool>(), 0u8..3, -4i32..5, -4i32..5, 0usize..100, 0u8..8), 1..150),
            lo in -5i32..5, hi in -5i32..5,
        ) {
            let mut doc = Doc::new();
            let mut live: Vec<(ItemId, Vec<Pt>)> = Vec::new();
            let mut out = vec![Pt::ZERO];
            let area = Rect::from_points(Pt { x: lo, y: lo }, Pt { x: hi, y: hi }).unwrap();
            for (remove, kind, x, y, pick, orient) in edits {
                if remove && !live.is_empty() {
                    let (id, _) = live.swap_remove(pick % live.len());
                    match id {
                        ItemId::Wire(id) => { doc.remove_wire(id).unwrap(); }
                        ItemId::Part(id) => { doc.remove_part(id).unwrap(); }
                        ItemId::Label(id) => { doc.remove_label(id).unwrap(); }
                    }
                } else {
                    let at = Pt { x, y };
                    match kind {
                        0 => {
                            let b = Pt { x: x + 1, y };
                            let id = doc.insert_wire(Wire { a: at, b }).unwrap();
                            live.push((ItemId::Wire(id), vec![at, b]));
                        }
                        1 => {
                            let o = Orient::ALL[orient as usize];
                            let pins = [Pt::ZERO, Pt { x: 1, y: 0 }, Pt::ZERO];
                            let id = doc.insert_part(part(at, o), &pins, Rect::new(Pt::ZERO, Pt::ZERO).unwrap()).unwrap();
                            // Independent reference transform, including coincident pins.
                            let (dx, dy) = match o.quarter_turns() { 0 => (1, 0), 1 => (0, 1), 2 => (-1, 0), _ => (0, -1) };
                            let sign = if o.is_flipped() { -1 } else { 1 };
                            live.push((ItemId::Part(id), vec![at, Pt { x: x + sign * dx, y: y + sign * dy }, at]));
                        }
                        _ => {
                            let id = doc.insert_label(Label { at, text: "net".into() }).unwrap();
                            live.push((ItemId::Label(id), vec![]));
                        }
                    }
                }
                let mut counts = HashMap::<Pt, usize>::new();
                for (_, ends) in &live { for &at in ends { *counts.entry(at).or_default() += 1; } }
                for bounds in [sheet(), area] {
                    doc.junctions(bounds, &mut out);
                    let mut expected: Vec<_> = counts.iter().filter_map(|(&at, &n)| (n >= 3 && bounds.contains(at)).then_some(at)).collect();
                    out.sort_by_key(|p| (p.x, p.y));
                    expected.sort_by_key(|p| (p.x, p.y));
                    prop_assert_eq!(&out, &expected);
                }
            }
        }

        /// W3.4 (doc half): `Doc::query_within` and `Doc::query_touching` match an independent
        /// brute-force oracle using symbol bounds only (excluding pins) over random documents
        /// with varied `local_bounds`/`local_pins`, clamped near-limit parts, and random
        /// normal, reverse-drawn, and degenerate zero-area boxes.
        #[test]
        fn query_within_and_touching_match_independent_brute_force(
            edits in prop::collection::vec(
                (
                    0u8..8,
                    0usize..32,
                    // Region selector: 0 = central sheet, 1..=4 = near ±SHEET_LIMIT_NM edges/corners
                    0u8..5,
                    -30_000_000i32..=30_000_000,
                    -30_000_000i32..=30_000_000,
                    // Local bounds corners & pin pattern
                    (-8_000_000i32..=0, -8_000_000i32..=0, 0i32..=8_000_000, 0i32..=8_000_000),
                    0u8..6,
                    0u8..8,
                    (1i32..=20_000_000, any::<bool>()),
                ),
                1..80,
            ),
            boxes in prop::collection::vec(
                (
                    0u8..4,
                    0u8..6,
                    -40_000_000i32..=40_000_000,
                    -40_000_000i32..=40_000_000,
                    -40_000_000i32..=40_000_000,
                    -40_000_000i32..=40_000_000,
                    any::<bool>(),
                ),
                1..24,
            ),
        ) {
            let mut doc = Doc::new();
            let mut oracle_parts: std::collections::BTreeMap<u32, (Part, Rect)> =
                std::collections::BTreeMap::new();
            let mut oracle_wires: std::collections::BTreeMap<u32, Wire> =
                std::collections::BTreeMap::new();
            let mut oracle_labels: std::collections::BTreeMap<u32, Label> =
                std::collections::BTreeMap::new();

            let place_pt = |region: u8, dx: i32, dy: i32| -> Pt {
                match region {
                    1 => Pt {
                        x: (SHEET_LIMIT_NM - 4_000_000 + (dx % 4_000_000)).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                        y: dy,
                    },
                    2 => Pt {
                        x: (-SHEET_LIMIT_NM + 4_000_000 + (dx % 4_000_000)).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                        y: dy,
                    },
                    3 => Pt {
                        x: (SHEET_LIMIT_NM - 4_000_000 + (dx % 4_000_000)).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                        y: (SHEET_LIMIT_NM - 4_000_000 + (dy % 4_000_000)).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                    },
                    4 => Pt {
                        x: (-SHEET_LIMIT_NM + 4_000_000 + (dx % 4_000_000)).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                        y: (-SHEET_LIMIT_NM + 4_000_000 + (dy % 4_000_000)).clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM),
                    },
                    _ => Pt { x: dx, y: dy },
                }
            };

            for (op, pick, region, dx, dy, (lx0, ly0, lx1, ly1), pin_kind, orient, (len, horiz)) in edits {
                let at = place_pt(region, dx, dy);
                match op {
                    0..=2 => {
                        let o = Orient::ALL[orient as usize];
                        let local_bounds = Rect::new(Pt { x: lx0, y: ly0 }, Pt { x: lx1, y: ly1 }).unwrap();
                        let mid_x = ((i64::from(lx0) + i64::from(lx1)) / 2) as i32;
                        let mid_y = ((i64::from(ly0) + i64::from(ly1)) / 2) as i32;
                        let local_pins: Vec<Pt> = match pin_kind {
                            // No pins
                            0 => Vec::new(),
                            // Pin strictly inside body (when non-degenerate)
                            1 => vec![Pt { x: mid_x, y: mid_y }],
                            // Pins right on body edges / corners
                            2 => vec![Pt { x: lx0, y: mid_y }, Pt { x: lx1, y: ly1 }],
                            // Pins far outside body
                            3 => vec![
                                Pt { x: lx0 - 3_000_000, y: 0 },
                                Pt { x: lx1 + 3_000_000, y: 0 },
                            ],
                            // Mix of inside, edge, and external stub
                            4 => vec![
                                Pt { x: mid_x, y: mid_y },
                                Pt { x: lx0, y: ly0 },
                                Pt { x: 0, y: ly1 + 3_000_000 },
                            ],
                            // Single external stub
                            _ => vec![Pt { x: lx1 + 2_500_000, y: mid_y }],
                        };
                        let p = part(at, o);
                        if let Ok(id) = doc.insert_part(p.clone(), &local_pins, local_bounds) {
                            oracle_parts.insert(id.0, (p, local_bounds));
                        }
                    }
                    3 | 4 => {
                        let b = if horiz {
                            Pt { x: at.x.saturating_add(len).min(SHEET_LIMIT_NM), y: at.y }
                        } else {
                            Pt { x: at.x, y: at.y.saturating_add(len).min(SHEET_LIMIT_NM) }
                        };
                        let w = Wire { a: at, b };
                        if let Ok(id) = doc.insert_wire(w) {
                            oracle_wires.insert(id.0, w);
                        }
                    }
                    5 => {
                        let l = Label { at, text: "N".into() };
                        if let Ok(id) = doc.insert_label(l.clone()) {
                            oracle_labels.insert(id.0, l);
                        }
                    }
                    6 => {
                        if let Some(&raw) = oracle_parts.keys().nth(pick % oracle_parts.len().max(1)) {
                            oracle_parts.remove(&raw);
                            doc.remove_part(PartId(raw));
                        }
                    }
                    _ => {
                        if pick % 2 == 0 {
                            if let Some(&raw) = oracle_wires.keys().nth(pick % oracle_wires.len().max(1)) {
                                oracle_wires.remove(&raw);
                                doc.remove_wire(WireId(raw));
                            }
                        } else if let Some(&raw) = oracle_labels.keys().nth(pick % oracle_labels.len().max(1)) {
                            oracle_labels.remove(&raw);
                            doc.remove_label(LabelId(raw));
                        }
                    }
                }
            }

            // Independently compute clamped world symbol bounds from (part, local_bounds).
            let independent_part_body = |p: &Part, local: Rect| -> Rect {
                let limit = i64::from(SHEET_LIMIT_NM);
                let corners = [
                    local.min,
                    Pt { x: local.max.x, y: local.min.y },
                    local.max,
                    Pt { x: local.min.x, y: local.max.y },
                ];
                let mut min_x = i32::MAX;
                let mut min_y = i32::MAX;
                let mut max_x = i32::MIN;
                let mut max_y = i32::MIN;
                for c in corners {
                    let r = p.orient.apply(c);
                    let wx = (i64::from(r.x) + i64::from(p.at.x)).clamp(-limit, limit) as i32;
                    let wy = (i64::from(r.y) + i64::from(p.at.y)).clamp(-limit, limit) as i32;
                    min_x = min_x.min(wx);
                    min_y = min_y.min(wy);
                    max_x = max_x.max(wx);
                    max_y = max_y.max(wy);
                }
                Rect {
                    min: Pt { x: min_x, y: min_y },
                    max: Pt { x: max_x, y: max_y },
                }
            };

            let box_pt = |region: u8, dx: i32, dy: i32| -> Pt {
                match region {
                    // Near +X edge (reaches past +SHEET_LIMIT_NM)
                    1 => Pt {
                        x: SHEET_LIMIT_NM.saturating_add(dx / 2),
                        y: dy,
                    },
                    // Near -X edge (reaches past -SHEET_LIMIT_NM)
                    2 => Pt {
                        x: (-SHEET_LIMIT_NM).saturating_add(dx / 2),
                        y: dy,
                    },
                    // Near (+X, +Y) corner
                    3 => Pt {
                        x: SHEET_LIMIT_NM.saturating_add(dx / 2),
                        y: SHEET_LIMIT_NM.saturating_add(dy / 2),
                    },
                    // Near (-X, -Y) corner
                    4 => Pt {
                        x: (-SHEET_LIMIT_NM).saturating_add(dx / 2),
                        y: (-SHEET_LIMIT_NM).saturating_add(dy / 2),
                    },
                    // Strictly outside sheet
                    5 => Pt {
                        x: SHEET_LIMIT_NM + 1 + dx.abs() % 1000,
                        y: dy,
                    },
                    _ => Pt { x: dx, y: dy },
                }
            };

            let mut found = Vec::new();
            let mut env_query = Vec::new();
            for (mode, region, x0, y0, x1, y1, horiz) in boxes {
                let p0 = box_pt(region, x0, y0);
                let p1 = box_pt(region, x1, y1);
                let raw_box = match mode {
                    0 => Rect { min: p0, max: p0 },
                    1 => {
                        if horiz {
                            Rect { min: p0, max: Pt { x: p1.x, y: p0.y } }
                        } else {
                            Rect { min: p0, max: Pt { x: p0.x, y: p1.y } }
                        }
                    }
                    _ => Rect { min: p0, max: p1 },
                };
                let norm = crate::index::normalize_rect(raw_box);
                let mut expected_within: Vec<ItemId> = oracle_parts
                    .iter()
                    .filter_map(|(&id, (p, local))| {
                        norm.contains_rect(independent_part_body(p, *local))
                            .then_some(ItemId::Part(PartId(id)))
                    })
                    .chain(oracle_wires.iter().filter_map(|(&id, w)| {
                        let seg = Rect::from_points(w.a, w.b).unwrap();
                        norm.contains_rect(seg).then_some(ItemId::Wire(WireId(id)))
                    }))
                    .chain(oracle_labels.iter().filter_map(|(&id, l)| {
                        norm.contains(l.at).then_some(ItemId::Label(LabelId(id)))
                    }))
                    .collect();
                expected_within.sort_by_key(|&id| crate::hit::item_key(id));

                let mut expected_touching: Vec<ItemId> = oracle_parts
                    .iter()
                    .filter_map(|(&id, (p, local))| {
                        independent_part_body(p, *local)
                            .intersects(norm)
                            .then_some(ItemId::Part(PartId(id)))
                    })
                    .chain(oracle_wires.iter().filter_map(|(&id, w)| {
                        let seg = Rect::from_points(w.a, w.b).unwrap();
                        seg.intersects(norm).then_some(ItemId::Wire(WireId(id)))
                    }))
                    .chain(oracle_labels.iter().filter_map(|(&id, l)| {
                        norm.contains(l.at).then_some(ItemId::Label(LabelId(id)))
                    }))
                    .collect();
                expected_touching.sort_by_key(|&id| crate::hit::item_key(id));

                doc.query(norm, &mut env_query);
                let env_set: std::collections::HashSet<_> =
                    env_query.iter().map(|&id| crate::hit::item_key(id)).collect();
                let touch_set: std::collections::HashSet<_> =
                    expected_touching.iter().map(|&id| crate::hit::item_key(id)).collect();
                for &id in &expected_within {
                    prop_assert!(touch_set.contains(&crate::hit::item_key(id)));
                }
                for &id in &expected_touching {
                    prop_assert!(env_set.contains(&crate::hit::item_key(id)));
                }

                for variant in [
                    raw_box,
                    Rect { min: raw_box.max, max: raw_box.min },
                    Rect {
                        min: Pt { x: raw_box.min.x, y: raw_box.max.y },
                        max: Pt { x: raw_box.max.x, y: raw_box.min.y },
                    },
                    Rect {
                        min: Pt { x: raw_box.max.x, y: raw_box.min.y },
                        max: Pt { x: raw_box.min.x, y: raw_box.max.y },
                    },
                ] {
                    doc.query_within(variant, &mut found);
                    let unique: std::collections::HashSet<_> =
                        found.iter().map(|&id| crate::hit::item_key(id)).collect();
                    prop_assert_eq!(unique.len(), found.len());
                    found.sort_by_key(|&id| crate::hit::item_key(id));
                    prop_assert_eq!(&found, &expected_within);

                    doc.query_touching(variant, &mut found);
                    let unique: std::collections::HashSet<_> =
                        found.iter().map(|&id| crate::hit::item_key(id)).collect();
                    prop_assert_eq!(unique.len(), found.len());
                    found.sort_by_key(|&id| crate::hit::item_key(id));
                    prop_assert_eq!(&found, &expected_touching);
                }
            }
        }
    }

    #[test]
    fn query_within_and_touching_use_symbol_bounds_only() {
        let mut doc = Doc::new();
        let pid = doc
            .insert_part(
                part(Pt::ZERO, Orient::IDENTITY),
                &[Pt::ZERO, Pt { x: 25, y: 0 }],
                Rect::new(Pt::ZERO, Pt { x: 10, y: 10 }).unwrap(),
            )
            .unwrap();
        let wid = doc
            .insert_wire(Wire {
                a: Pt { x: 0, y: 20 },
                b: Pt { x: 25, y: 20 },
            })
            .unwrap();
        let lid = doc
            .insert_label(Label {
                at: Pt { x: 25, y: 0 },
                text: "PIN2".into(),
            })
            .unwrap();

        let mut out = Vec::new();
        // Enclosed regression: box covers the symbol body (0..=10, 0..=10) but NOT the external
        // pin at (25, 0) — the part IS selected (WD8: part counts by its symbol bounds).
        doc.query_within(Rect::new(Pt::ZERO, Pt { x: 10, y: 10 }).unwrap(), &mut out);
        assert_eq!(out, vec![ItemId::Part(pid)]);
        // Reverse-drawn tight box around the body also selects the part.
        doc.query_within(
            Rect {
                min: Pt { x: 10, y: 10 },
                max: Pt::ZERO,
            },
            &mut out,
        );
        assert_eq!(out, vec![ItemId::Part(pid)]);

        // Mirror Intersecting regression: box touches only the external pin stub at (25, 0),
        // missing the symbol body (0..=10, 0..=10). `Doc::query` (index envelope) finds `pid`,
        // but `Doc::query_touching` does NOT select `pid` (only the label at (25, 0)).
        let pin_only_box = Rect::new(Pt { x: 15, y: -5 }, Pt { x: 30, y: 5 }).unwrap();
        doc.query(pin_only_box, &mut out);
        assert!(out.contains(&ItemId::Part(pid)));
        doc.query_touching(pin_only_box, &mut out);
        assert_eq!(out, vec![ItemId::Label(lid)]);
        // Reverse-drawn pin-only box gives the same result.
        doc.query_touching(
            Rect {
                min: Pt { x: 30, y: 5 },
                max: Pt { x: 15, y: -5 },
            },
            &mut out,
        );
        assert_eq!(out, vec![ItemId::Label(lid)]);

        // Touching the body corner inclusively selects the part in `query_touching`,
        // whereas 1 nm outside does not.
        doc.query_touching(
            Rect {
                min: Pt { x: 20, y: 15 },
                max: Pt { x: 10, y: 10 },
            },
            &mut out,
        );
        assert_eq!(out, vec![ItemId::Part(pid)]);
        doc.query_touching(
            Rect::new(Pt { x: 11, y: 0 }, Pt { x: 20, y: 10 }).unwrap(),
            &mut out,
        );
        assert!(out.is_empty());

        // Degenerate 1D box along y = 20 selects only the collinear wire in both modes.
        let line = Rect {
            min: Pt { x: 25, y: 20 },
            max: Pt { x: 0, y: 20 },
        };
        doc.query_within(line, &mut out);
        assert_eq!(out, vec![ItemId::Wire(wid)]);
        doc.query_touching(line, &mut out);
        assert_eq!(out, vec![ItemId::Wire(wid)]);

        // Degenerate point box at (25, 0) selects only the label anchor there in both modes.
        let pt_box = Rect {
            min: Pt { x: 25, y: 0 },
            max: Pt { x: 25, y: 0 },
        };
        doc.query_within(pt_box, &mut out);
        assert_eq!(out, vec![ItemId::Label(lid)]);
        doc.query_touching(pt_box, &mut out);
        assert_eq!(out, vec![ItemId::Label(lid)]);
    }

    // ---- A1.1: Doc::fork (AD3, INV4, INV10) ----

    /// Derived state the fork must carry over: connectivity partitions (INV10), the index's
    /// answer over the whole sheet, in order, and the junction dots, compared between two
    /// documents.
    fn assert_same_derived(a: &Doc, b: &Doc) {
        crate::connectivity::tests::assert_partitions(&a.connectivity, &b.connectivity);
        let (mut qa, mut qb) = (Vec::new(), Vec::new());
        a.query(sheet(), &mut qa);
        b.query(sheet(), &mut qb);
        assert_eq!(qa, qb, "fork index must answer queries in the same order");
        let (mut ja, mut jb) = (Vec::new(), Vec::new());
        a.junctions(sheet(), &mut ja);
        b.junctions(sheet(), &mut jb);
        ja.sort_by_key(|p| (p.x, p.y));
        jb.sort_by_key(|p| (p.x, p.y));
        assert_eq!(ja, jb);
    }

    /// INV10: incremental connectivity equals an independent rebuild.
    fn assert_inv10(doc: &Doc) {
        let oracle = Connectivity::rebuild_with_parts(
            doc.wires().map(|(id, &w)| (id, w)),
            doc.labels(),
            doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
        );
        crate::connectivity::tests::assert_partitions(&doc.connectivity, &oracle);
    }

    #[test]
    fn a1_1_fork_keeps_reserved_slots_and_revision_with_empty_history() {
        let mut doc = Doc::new();
        let bounds = Rect::from_points(Pt { x: -2, y: -1 }, Pt { x: 2, y: 1 }).unwrap();
        let place = |x| Command::PlacePart {
            part: part(Pt { x, y: 0 }, Orient::IDENTITY),
            local_pins: vec![Pt::ZERO],
            local_bounds: bounds,
        };
        doc.apply(place(0)).unwrap();
        doc.apply(place(10)).unwrap();
        doc.apply(Command::Delete {
            items: vec![ItemId::Part(PartId(1))],
            with_wires: false,
        })
        .unwrap();
        // Part 1 is now a reserved (removed) slot.
        let mut fork = doc.fork();
        assert_eq!(fork, doc);
        assert_eq!(fork.revision(), doc.revision());
        assert!(doc.can_undo());
        assert!(
            !fork.can_undo() && !fork.can_redo(),
            "fork history is empty"
        );

        // AD3: a commit at the same revision allocates the same ids in both.
        let on_fork = fork.apply(place(20)).unwrap();
        let on_doc = doc.apply(place(20)).unwrap();
        assert_eq!(on_fork, on_doc);
        assert_eq!(on_fork.added, vec![ItemId::Part(PartId(2))]);
        assert_eq!(fork, doc);
    }

    /// AD8 determinism through a fork: the hash-map-backed outputs (unsorted junctions from the
    /// point map, exact `NetId`s from the connectivity) come out identical, not merely
    /// equivalent, on a sheet with several junctions and nets.
    #[test]
    fn a1_1_fork_keeps_hash_map_order_and_exact_net_ids() {
        let mut doc = Doc::new();
        let mut wires = Vec::new();
        for i in 0..6 {
            let x = i * 20;
            // A cross at (x, 0) and a tee at (x, 30): two junctions per column.
            wires.push(Wire {
                a: Pt { x, y: -10 },
                b: Pt { x, y: 0 },
            });
            wires.push(Wire {
                a: Pt { x, y: 0 },
                b: Pt { x, y: 10 },
            });
            wires.push(Wire {
                a: Pt { x: x - 5, y: 0 },
                b: Pt { x, y: 0 },
            });
            wires.push(Wire {
                a: Pt { x, y: 0 },
                b: Pt { x: x + 5, y: 0 },
            });
            wires.push(Wire {
                a: Pt { x, y: 20 },
                b: Pt { x, y: 30 },
            });
            wires.push(Wire {
                a: Pt { x: x - 5, y: 30 },
                b: Pt { x, y: 30 },
            });
            wires.push(Wire {
                a: Pt { x, y: 30 },
                b: Pt { x: x + 5, y: 30 },
            });
        }
        doc.apply(Command::AddWires(wires)).unwrap();
        doc.apply(Command::AddLabel(Label {
            at: Pt { x: 0, y: 10 },
            text: "VCC".into(),
        }))
        .unwrap();
        doc.apply(Command::AddLabel(Label {
            at: Pt { x: 100, y: 20 },
            text: "VCC".into(),
        }))
        .unwrap();
        let fork = doc.fork();

        let (mut ja, mut jb) = (Vec::new(), Vec::new());
        doc.junctions(sheet(), &mut ja);
        fork.junctions(sheet(), &mut jb);
        assert!(
            ja.len() >= 12,
            "fixture must have several junctions: {ja:?}"
        );
        assert_eq!(ja, jb, "unsorted junction order must survive the fork");

        let mut nets = Vec::new();
        for (id, wire) in doc.wires() {
            assert_eq!(fork.net_of(id), doc.net_of(id));
            for at in [wire.a, wire.b] {
                assert_eq!(fork.net_at(at), doc.net_at(at));
            }
            nets.extend(doc.net_of(id));
        }
        nets.sort();
        nets.dedup();
        assert!(nets.len() >= 6, "fixture must have several nets: {nets:?}");
    }

    #[test]
    fn a1_1_applying_to_a_fork_leaves_the_original_untouched() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![Wire {
            a: Pt::ZERO,
            b: Pt { x: 10, y: 0 },
        }]))
        .unwrap();
        let before = doc.fork();
        let mut fork = doc.fork();
        fork.apply(Command::AddWires(vec![Wire {
            a: Pt { x: 10, y: 0 },
            b: Pt { x: 10, y: 10 },
        }]))
        .unwrap();
        assert_ne!(fork, doc);
        assert_eq!(doc, before);
        assert_same_derived(&doc, &before);
        assert_inv10(&doc);
        assert_inv10(&fork);
    }

    /// The T5.1 random command generator (`command.rs`, test
    /// `inv4_and_inv10_over_random_legal_and_invalid_sequences`), mirrored here because
    /// `command.rs` belongs to the wiring tasks and A1.1 may not touch it (mcp-roadmap.md,
    /// dependency overview). Same steps, weights and ranges, and the same mapping to legal and
    /// invalid commands.
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

    fn step_command(doc: &Doc, step: GenStep) -> Command {
        let local_bounds = Rect::from_points(Pt { x: -2, y: -1 }, Pt { x: 2, y: 1 }).unwrap();
        let local_pins = vec![Pt::ZERO, Pt { x: 1, y: 0 }];
        let names = ["GND", "VCC", "SIG", "CLK"];
        let follow = || crate::prefs::FollowPolicy::new(false, 10, false).unwrap();
        let named = |at: Pt, orient: Orient, designator: &str, value: &str| Part {
            symbol: SymbolId(0),
            at,
            orient,
            designator: designator.into(),
            value: value.into(),
        };
        let live: Vec<ItemId> = doc
            .parts()
            .map(|(id, _)| ItemId::Part(id))
            .chain(doc.wires().map(|(id, _)| ItemId::Wire(id)))
            .chain(doc.labels().map(|(id, _)| ItemId::Label(id)))
            .collect();
        let pick_one = |pick: usize, dup: bool| {
            let mut items = if live.is_empty() {
                Vec::new()
            } else {
                vec![live[pick % live.len()]]
            };
            if dup && !items.is_empty() {
                items.push(items[0]);
            }
            items
        };
        match step {
            GenStep::PlacePart {
                x,
                y,
                orient,
                invalid_oob,
            } => Command::PlacePart {
                part: named(
                    Pt {
                        x: if invalid_oob { SHEET_LIMIT_NM + 1 } else { x },
                        y,
                    },
                    Orient::ALL[orient as usize],
                    "U1",
                    "1k",
                ),
                local_pins,
                local_bounds,
            },
            GenStep::AddWires {
                x,
                y,
                horiz,
                bad_diag,
            } => {
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
            GenStep::AddLabel {
                x,
                y,
                net,
                invalid_oob,
            } => Command::AddLabel(Label {
                at: Pt {
                    x: if invalid_oob { SHEET_LIMIT_NM + 1 } else { x },
                    y,
                },
                text: names[net as usize].into(),
            }),
            GenStep::Move {
                pick,
                dx,
                dy,
                dup,
                missing,
            } => {
                let mut items = pick_one(pick, dup);
                if missing {
                    items.push(ItemId::Part(PartId(99_999)));
                }
                Command::Move {
                    follow: follow(),
                    items,
                    delta: Pt { x: dx, y: dy },
                }
            }
            GenStep::Reorient {
                pick,
                px,
                py,
                orient,
                dup,
            } => Command::Reorient {
                follow: follow(),
                items: pick_one(pick, dup),
                pivot: Pt { x: px, y: py },
                by: Orient::ALL[orient as usize],
            },
            GenStep::Delete {
                pick,
                count,
                dup,
                missing,
            } => {
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
                Command::Delete {
                    items,
                    with_wires: false,
                }
            }
            GenStep::SetText {
                pick,
                field,
                net,
                missing,
            } => Command::SetText {
                item: if missing || live.is_empty() {
                    ItemId::Wire(WireId(99_999))
                } else {
                    live[pick % live.len()]
                },
                field: match field {
                    0 => TextField::Designator,
                    1 => TextField::Value,
                    _ => TextField::Label,
                },
                text: names[net as usize].into(),
            },
            GenStep::BatchMoveAndAdd {
                pick,
                dx,
                fail_second,
            } => {
                let mut cmds = vec![Command::PlacePart {
                    part: named(Pt::ZERO, Orient::IDENTITY, "B", "10"),
                    local_pins,
                    local_bounds,
                }];
                if !live.is_empty() {
                    cmds.push(Command::Move {
                        follow: follow(),
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
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]
        /// A1.1 done-when (AD3, INV4, INV10): at every state the T5.1 generator reaches,
        /// undone and redone states included, `fork() == original`, the fork carries the same
        /// derived state, and applying the same command to both gives equal results (`Change`
        /// or error), `PartialEq` documents and INV10 on each.
        #[test]
        fn a1_1_fork_equals_original_and_commits_identically(
            steps in prop::collection::vec(gen_step(), 1..80),
        ) {
            let mut doc = Doc::new();
            for step in steps {
                let mut fork = doc.fork();
                prop_assert_eq!(&fork, &doc);
                assert_same_derived(&fork, &doc);
                prop_assert!(!fork.can_undo() && !fork.can_redo());
                match step {
                    GenStep::Undo | GenStep::Redo => {
                        if matches!(step, GenStep::Undo) {
                            doc.undo();
                        } else {
                            doc.redo();
                        }
                        // The state undo or redo reached, checked now so a sequence that
                        // ends here is covered too.
                        let after = doc.fork();
                        prop_assert_eq!(&after, &doc);
                        assert_same_derived(&after, &doc);
                        assert_inv10(&after);
                    }
                    step => {
                        let cmd = step_command(&doc, step);
                        let on_fork = fork.apply(cmd.clone());
                        let on_doc = doc.apply(cmd);
                        prop_assert_eq!(&on_fork, &on_doc);
                        prop_assert_eq!(&fork, &doc);
                        assert_same_derived(&fork, &doc);
                        assert_inv10(&fork);
                        assert_inv10(&doc);
                    }
                }
            }
        }
    }
}
