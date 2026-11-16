//! Document storage. Mutation stays inside the crate for the T5.1 command layer.

#[path = "io.rs"]
mod io;

use std::{fmt, sync::Arc};

use crate::{
    Orient, Pt, Rect,
    arena::Arena,
    points::{End, Points},
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
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OutOfBounds => "item or pin lies outside K1 sheet limits",
            Self::InvalidWire => "wire must be axis-aligned with distinct ends",
        })
    }
}

impl std::error::Error for StoreError {}

#[derive(Debug)]
struct StoredPart {
    part: Part,
    pins: Vec<Pt>,
}

#[derive(Debug, Default)]
pub struct Doc {
    parts: Arena<StoredPart>,
    wires: Arena<Wire>,
    labels: Arena<Label>,
    points: Points,
    revision: u64,
}

impl Doc {
    /// Creates an empty store. Symbol-library integration belongs to T2.3;
    /// internal part insertion currently receives the symbol's local pin positions.
    pub fn new() -> Self {
        Self::default()
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

    pub(crate) fn insert_part(
        &mut self,
        part: Part,
        local_pins: &[Pt],
    ) -> Result<PartId, StoreError> {
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
        let id = PartId(self.parts.insert(StoredPart { part, pins }));
        for (pin, &at) in self.parts.get(id.0).unwrap().pins.iter().enumerate() {
            self.points.insert(at, End::Pin { part: id, pin });
        }
        self.bump_revision();
        Ok(id)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "crate-private editing entry point for T5.1 commands"
        )
    )]
    pub(crate) fn remove_part(&mut self, id: PartId) -> Option<Part> {
        let stored = self.parts.remove(id.0)?;
        for (pin, at) in stored.pins.into_iter().enumerate() {
            self.points.remove(at, End::Pin { part: id, pin });
        }
        self.bump_revision();
        Some(stored.part)
    }

    pub(crate) fn insert_wire(&mut self, wire: Wire) -> Result<WireId, StoreError> {
        check_point(wire.a)?;
        check_point(wire.b)?;
        if wire.a == wire.b || (wire.a.x != wire.b.x && wire.a.y != wire.b.y) {
            return Err(StoreError::InvalidWire);
        }
        let id = WireId(self.wires.insert(wire));
        self.points.insert(wire.a, End::Wire(id));
        self.points.insert(wire.b, End::Wire(id));
        self.bump_revision();
        Ok(id)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "crate-private editing entry point for T5.1 commands"
        )
    )]
    pub(crate) fn remove_wire(&mut self, id: WireId) -> Option<Wire> {
        let wire = self.wires.remove(id.0)?;
        self.points.remove(wire.a, End::Wire(id));
        self.points.remove(wire.b, End::Wire(id));
        self.bump_revision();
        Some(wire)
    }

    pub(crate) fn insert_label(&mut self, label: Label) -> Result<LabelId, StoreError> {
        check_point(label.at)?;
        let id = LabelId(self.labels.insert(label));
        self.bump_revision();
        Ok(id)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "crate-private editing entry point for T5.1 commands"
        )
    )]
    pub(crate) fn remove_label(&mut self, id: LabelId) -> Option<Label> {
        let label = self.labels.remove(id.0)?;
        self.bump_revision();
        Some(label)
    }

    fn bump_revision(&mut self) {
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
                &[Pt { x: 1, y: 0 }]
            ),
            Err(StoreError::OutOfBounds)
        );
        assert_eq!(doc.remove_wire(WireId(0)), None);
        assert_eq!(doc.remove_part(PartId(0)), None);
        assert_eq!(doc.remove_label(LabelId(0)), None);
        assert_eq!(doc.revision(), 0);
        let p = doc
            .insert_part(part(Pt::ZERO, Orient::IDENTITY), &[])
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
            .insert_part(part(Pt::ZERO, Orient::IDENTITY), &[Pt::ZERO])
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
                            let id = doc.insert_part(part(at, o), &pins).unwrap();
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
