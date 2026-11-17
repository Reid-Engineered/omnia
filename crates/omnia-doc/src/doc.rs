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

    /// Replaces `out` with the labels whose point is exactly `at`, as [`ItemId::Label`], in
    /// ascending id order. The point map holds only wire ends and pins, so the lookup goes
    /// through the spatial index, which every label insert, remove and move keeps current.
    /// Allocates nothing when `out` has room for every item whose bounds meet `at`.
    pub fn labels_at(&self, at: Pt, out: &mut Vec<ItemId>) {
        self.index.query(Rect { min: at, max: at }, out);
        out.retain(|&item| match item {
            ItemId::Label(id) => self.labels.get(id.0).is_some_and(|label| label.at == at),
            _ => false,
        });
        out.sort_unstable_by_key(|&item| match item {
            ItemId::Label(id) => id.0,
            _ => unreachable!(),
        });
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
    }
}
