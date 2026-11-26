//! Versioned RON test sheets. Derived state is rebuilt through validated inserts.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use super::*;

type Coords = (i32, i32);

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sheet {
    version: u32,
    revision: u64,
    parts: Vec<Option<PartRecord>>,
    wires: Vec<Option<(Coords, Coords)>>,
    labels: Vec<Option<(Coords, String)>>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartRecord {
    symbol: u16,
    at: Coords,
    orient: u8,
    designator: String,
    value: String,
    local_pins: Vec<Coords>,
    local_bounds: (Coords, Coords),
}

fn coords(p: Pt) -> Coords {
    (p.x, p.y)
}
fn point(p: Coords) -> Pt {
    Pt { x: p.0, y: p.1 }
}
fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

impl Doc {
    /// Builds a sheet atomically through the same validation as command insertion.
    /// Pins and bounds are symbol-local; callers obtain them from their symbol library.
    pub fn from_items(
        parts: impl IntoIterator<Item = (Part, Vec<Pt>, Rect)>,
        wires: impl IntoIterator<Item = Wire>,
        labels: impl IntoIterator<Item = Label>,
    ) -> Result<Self, StoreError> {
        let mut doc = Self::new();
        for (part, pins, bounds) in parts {
            doc.insert_part(part, &pins, bounds)?;
        }
        for wire in wires {
            doc.insert_wire(wire)?;
        }
        for label in labels {
            doc.insert_label(label)?;
        }
        Ok(doc)
    }

    fn sheet(&self) -> Sheet {
        Sheet {
            version: 2,
            revision: self.revision,
            parts: self
                .parts
                .slots
                .iter()
                .map(|slot| {
                    slot.as_ref().map(|s| PartRecord {
                        symbol: s.part.symbol.raw(),
                        local_bounds: (coords(s.local_bounds.min), coords(s.local_bounds.max)),
                        at: coords(s.part.at),
                        orient: s.part.orient.to_raw(),
                        designator: s.part.designator.to_string(),
                        value: s.part.value.to_string(),
                        local_pins: s
                            .pins
                            .iter()
                            .map(|&p| coords(s.part.orient.inverse().apply(p - s.part.at)))
                            .collect(),
                    })
                })
                .collect(),
            wires: self
                .wires
                .slots
                .iter()
                .map(|slot| slot.map(|w| (coords(w.a), coords(w.b))))
                .collect(),
            labels: self
                .labels
                .slots
                .iter()
                .map(|slot| slot.as_ref().map(|l| (coords(l.at), l.text.to_string())))
                .collect(),
        }
    }

    /// Writes version 2 RON, including reserved IDs and revision, in stable slot order.
    /// Local bounds and pin geometry are embedded to keep this crate independent of `omnia-symbols`.
    ///
    /// Version 2 holds no definitions or parameters, so until the version 3 writer lands
    /// (`P2.3`) a document whose parts use a definition other than an unmodelled one, or carry
    /// parameter overrides, is refused with `InvalidData` and nothing is written (`PD8`, no
    /// lossy save). A document of unmodelled parts saves exactly as before.
    pub fn save(&self, mut w: impl Write) -> io::Result<()> {
        if !self.is_v2_representable() {
            return Err(invalid(
                "document holds part definitions or parameters that this build cannot save",
            ));
        }
        let text = ron::ser::to_string_pretty(&self.sheet(), ron::ser::PrettyConfig::default())
            .map_err(invalid)?;
        w.write_all(text.as_bytes())
    }

    /// Whether every live part has no overrides and uses an unmodelled definition (`PD8`).
    fn is_v2_representable(&self) -> bool {
        self.parts.slots.iter().flatten().all(|stored| {
            stored.params.is_empty()
                && self
                    .part_defs
                    .get(stored.def)
                    .is_some_and(|def| def.is_unmodelled())
        })
    }

    /// Loads a self-contained test sheet, validating geometry and rebuilding incidence.
    /// Symbol IDs retain their meaning in the library used to generate the sheet.
    ///
    /// Strict v2 migration (`PD8`, `XD1`): each part is inserted through the same path as an
    /// unmodelled placement, so each unique `(symbol, local_pins, local_bounds)` becomes one
    /// unmodelled definition, in order of first appearance by part slot, and every part has
    /// empty `params`.
    pub fn load(r: impl Read) -> io::Result<Self> {
        let sheet: Sheet = ron::de::from_reader(r).map_err(invalid)?;
        if sheet.version != 2 {
            return Err(invalid("unsupported test-sheet version"));
        }
        for len in [sheet.parts.len(), sheet.wires.len(), sheet.labels.len()] {
            if len > u32::MAX as usize {
                return Err(invalid("arena id space exhausted"));
            }
        }
        let mut doc = Self::new();
        for slot in sheet.parts {
            if let Some(p) = slot {
                let orient =
                    Orient::from_raw(p.orient).ok_or_else(|| invalid("invalid orientation"))?;
                doc.insert_part(
                    Part {
                        symbol: SymbolId(p.symbol),
                        at: point(p.at),
                        orient,
                        designator: p.designator.into(),
                        value: p.value.into(),
                    },
                    &p.local_pins.into_iter().map(point).collect::<Vec<_>>(),
                    Rect::new(point(p.local_bounds.0), point(p.local_bounds.1)).map_err(invalid)?,
                )
                .map_err(invalid)?;
            } else {
                doc.parts.slots.push(None);
            }
        }
        for slot in sheet.wires {
            if let Some((a, b)) = slot {
                doc.insert_wire(Wire {
                    a: point(a),
                    b: point(b),
                })
                .map_err(invalid)?;
            } else {
                doc.wires.slots.push(None);
            }
        }
        for slot in sheet.labels {
            if let Some((at, text)) = slot {
                doc.insert_label(Label {
                    at: point(at),
                    text: text.into(),
                })
                .map_err(invalid)?;
            } else {
                doc.labels.slots.push(None);
            }
        }
        doc.revision = sheet.revision;
        Ok(doc)
    }
}

fn trim_trailing_none<T>(slots: &[Option<T>]) -> &[Option<T>] {
    let end = slots
        .iter()
        .rposition(Option::is_some)
        .map_or(0, |last| last + 1);
    &slots[..end]
}

impl Doc {
    /// Compares live content and exact live IDs, including interior holes and pin/art geometry.
    ///
    /// Unlike `PartialEq`, this ignores revision and trailing reserved slots. Undo restores
    /// this content while advancing revision and retaining issued IDs for redo (INV4/D2/D8).
    /// Equal content does not imply equal serialization or equal future ID allocation.
    ///
    /// Live equality (`PD8`, `PINV7`): each live part's `def` id and `params` are compared with
    /// its slot, and so is the record that id resolves to. The arena length is not compared.
    pub fn content_eq(&self, other: &Self) -> bool {
        trim_trailing_none(&self.parts.slots) == trim_trailing_none(&other.parts.slots)
            && trim_trailing_none(&self.wires.slots) == trim_trailing_none(&other.wires.slots)
            && trim_trailing_none(&self.labels.slots) == trim_trailing_none(&other.labels.slots)
            && self
                .parts
                .slots
                .iter()
                .flatten()
                .all(|stored| self.part_defs.get(stored.def) == other.part_defs.get(stored.def))
    }
}

// Persistent-state equality retains the pre-command contract: revision and all reserved
// slots matter. History and derived caches are not serialized and are not compared.
// PD8: every slot's `def` and `params`, the definition arena length and the records of
// referenced definitions are persistent too; retained records are history state and are
// neither saved nor compared.
impl PartialEq for Doc {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.parts.slots == other.parts.slots
            && self.wires.slots == other.wires.slots
            && self.labels.slots == other.labels.slots
            && self.part_defs.referenced_eq(&other.part_defs)
    }
}
impl Eq for Doc {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_equality_does_not_hide_revision_or_reserved_ids() {
        let fresh = Doc::new();
        let mut edited = Doc::new();
        let label = Label {
            at: Pt::ZERO,
            text: "GND".into(),
        };
        edited
            .apply(crate::Command::AddLabel(label.clone()))
            .unwrap();
        edited.undo().unwrap();
        assert!(edited.content_eq(&fresh));
        assert_ne!(edited, fresh);
        let mut saved = Vec::new();
        edited.save(&mut saved).unwrap();
        let mut loaded = Doc::load(saved.as_slice()).unwrap();
        assert_eq!(edited, loaded);
        let mut same_revision = Doc::new();
        same_revision.revision = edited.revision;
        assert_ne!(
            edited, same_revision,
            "reserved slots remain part of equality"
        );
        let mut different_revision = Doc::load(saved.as_slice()).unwrap();
        different_revision.revision += 1;
        assert_ne!(
            edited, different_revision,
            "revision remains part of equality"
        );
        let mut fresh = fresh;
        let fresh_id = fresh
            .apply(crate::Command::AddLabel(label.clone()))
            .unwrap()
            .added;
        let edited_id = loaded.apply(crate::Command::AddLabel(label)).unwrap().added;
        assert_eq!(fresh_id, vec![ItemId::Label(LabelId(0))]);
        assert_eq!(edited_id, vec![ItemId::Label(LabelId(1))]);
        assert!(
            !fresh.content_eq(&loaded),
            "live IDs remain part of content comparison"
        );
    }

    #[test]
    fn round_trip_preserves_holes_revision_pins_and_future_ids() {
        let mut doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(4),
                    at: Pt::ZERO,
                    orient: Orient::ROT_90,
                    designator: "R\"1\nΩ".into(),
                    value: "".into(),
                },
                vec![Pt::ZERO],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )],
            [
                Wire {
                    a: Pt::ZERO,
                    b: Pt { x: 1, y: 0 },
                },
                Wire {
                    a: Pt::ZERO,
                    b: Pt { x: 0, y: 1 },
                },
            ],
            [Label {
                at: Pt::ZERO,
                text: "net".into(),
            }],
        )
        .unwrap();
        let removed = doc
            .insert_part(
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "gone".into(),
                    value: "".into(),
                },
                &[],
                Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
            )
            .unwrap();
        doc.remove_part(removed);
        doc.remove_label(LabelId(0));
        let wire = doc
            .insert_wire(Wire {
                a: Pt::ZERO,
                b: Pt { x: -1, y: 0 },
            })
            .unwrap();
        doc.remove_wire(wire);
        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        let mut loaded = Doc::load(bytes.as_slice()).unwrap();
        // Version 2 carries no definition table, so a trailing retained definition from the
        // removed part cannot survive save/load until RON v3 lands in P2.3 (`PINV4`); live
        // content, reserved item slots, revision and v2 bytes all match.
        assert!(doc.content_eq(&loaded));
        assert_eq!(doc.revision(), loaded.revision());
        let mut again = Vec::new();
        loaded.save(&mut again).unwrap();
        assert_eq!(again, bytes);
        let area = Rect::from_points(Pt { x: -10, y: -10 }, Pt { x: 10, y: 10 }).unwrap();
        let mut dots = Vec::new();
        loaded.junctions(area, &mut dots);
        assert_eq!(dots, [Pt::ZERO]);
        assert_eq!(
            loaded
                .insert_wire(Wire {
                    a: Pt::ZERO,
                    b: Pt { x: 2, y: 0 }
                })
                .unwrap(),
            WireId(3)
        );
        assert_eq!(
            loaded
                .insert_label(Label {
                    at: Pt::ZERO,
                    text: "new".into()
                })
                .unwrap(),
            LabelId(1)
        );
        assert_eq!(
            loaded
                .insert_part(
                    Part {
                        symbol: SymbolId(0),
                        at: Pt::ZERO,
                        orient: Orient::IDENTITY,
                        designator: "new".into(),
                        value: "".into()
                    },
                    &[],
                    Rect::new(Pt::ZERO, Pt::ZERO).unwrap()
                )
                .unwrap(),
            PartId(2)
        );
    }

    #[test]
    fn rejects_corrupt_geometry_and_versions() {
        for text in [
            "garbage",
            "(version:3,revision:0,parts:[],wires:[],labels:[])",
            "(version:2,revision:0,parts:[],wires:[Some(((0,0),(1,1)))],labels:[])",
            "(version:2,revision:0,parts:[],wires:[Some(((0,0),(0,0)))],labels:[])",
            "(version:2,revision:0,parts:[],wires:[],labels:[Some(((1000000001,0),\"bad\"))])",
        ] {
            assert_eq!(
                Doc::load(text.as_bytes()).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[test]
    fn bulk_constructor_and_load_reject_invalid_parts() {
        let p = Part {
            symbol: SymbolId(0),
            at: Pt::ZERO,
            orient: Orient::IDENTITY,
            designator: "R1".into(),
            value: "1k".into(),
        };
        assert_eq!(
            Doc::from_items(
                [],
                [Wire {
                    a: Pt::ZERO,
                    b: Pt::ZERO
                }],
                []
            )
            .unwrap_err(),
            StoreError::InvalidWire
        );
        assert_eq!(
            Doc::from_items(
                [(
                    p.clone(),
                    vec![Pt {
                        x: crate::SHEET_LIMIT_NM + 1,
                        y: 0
                    }],
                    Rect::new(Pt::ZERO, Pt::ZERO).unwrap()
                )],
                [],
                []
            )
            .unwrap_err(),
            StoreError::OutOfBounds
        );
        let doc = Doc::from_items(
            [(p, vec![Pt::ZERO], Rect::new(Pt::ZERO, Pt::ZERO).unwrap())],
            [],
            [],
        )
        .unwrap();
        for (orient, at, pin) in [
            (8, (0, 0), (0, 0)),
            (0, (crate::SHEET_LIMIT_NM, 0), (1, 0)),
            (0, (0, 0), (i32::MIN, 0)),
        ] {
            let mut sheet = doc.sheet();
            let part = sheet.parts[0].as_mut().unwrap();
            part.orient = orient;
            part.at = at;
            part.local_pins = vec![pin];
            let text = ron::ser::to_string(&sheet).unwrap();
            assert_eq!(
                Doc::load(text.as_bytes()).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    /// `PTC-v2-compat` (`PD8`, `P2.1`): unmodelled documents still save as version 2, byte for
    /// byte. The committed goldens were written by the v2 writer. The writer's line ending is
    /// the platform's and a checkout may rewrite the goldens' line endings, so both sides are
    /// compared with CRLF folded to LF; everything else must match exactly.
    #[test]
    fn p2_1_v2_save_load_is_byte_compatible_for_unmodelled_documents() {
        for golden in [
            include_str!("../../../tests/golden/inverting-amp.ron"),
            include_str!("../../../tests/golden/wien-oscillator.ron"),
        ] {
            let golden = golden.replace("\r\n", "\n");
            let doc = Doc::load(golden.as_bytes()).unwrap();
            let mut saved = Vec::new();
            doc.save(&mut saved).unwrap();
            let text = String::from_utf8(saved.clone()).unwrap();
            assert_eq!(text.replace("\r\n", "\n"), golden);
            let mut again = Vec::new();
            Doc::load(saved.as_slice())
                .unwrap()
                .save(&mut again)
                .unwrap();
            assert_eq!(again, saved);
        }
    }

    /// `PD8` (`P2.1`): strict v2 migration interns one unmodelled definition per unique
    /// `(symbol, local_pins, local_bounds)` tuple in part-slot order (skipping holes), with
    /// empty `params` on every loaded part.
    #[test]
    fn p2_1_v2_load_interns_unmodelled_defs_in_part_slot_order() {
        let bounds_a = ((-10, -5), (10, 5));
        let bounds_b = ((-4, -4), (4, 4));
        let part = |symbol: u16, x: i32, pins: Vec<Coords>, bounds: (Coords, Coords)| PartRecord {
            symbol,
            at: (x, 0),
            orient: 0,
            designator: format!("U{x}"),
            value: "1k".into(),
            local_pins: pins,
            local_bounds: bounds,
        };
        let sheet = Sheet {
            version: 2,
            revision: 7,
            parts: vec![
                Some(part(0, 0, vec![(-10, 0), (10, 0)], bounds_a)),
                None,
                Some(part(1, 20, vec![(0, 0)], bounds_b)),
                Some(part(0, 40, vec![(-10, 0), (10, 0)], bounds_a)),
                Some(part(0, 60, vec![(10, 0), (-10, 0)], bounds_a)),
            ],
            wires: vec![],
            labels: vec![],
        };
        let text = ron::ser::to_string(&sheet).unwrap();
        let doc = Doc::load(text.as_bytes()).unwrap();

        assert_eq!(doc.revision(), 7);
        assert_eq!(doc.part_def_id(PartId(0)), Some(PartDefId(0)));
        assert_eq!(doc.part_def_id(PartId(1)), None);
        assert_eq!(doc.part_def_id(PartId(2)), Some(PartDefId(1)));
        assert_eq!(doc.part_def_id(PartId(3)), Some(PartDefId(0)));
        assert_eq!(doc.part_def_id(PartId(4)), Some(PartDefId(2)));

        let defs: Vec<_> = doc.part_defs().collect();
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].0, PartDefId(0));
        assert_eq!(&*defs[0].1.source_id, "legacy:0");
        assert_eq!(defs[1].0, PartDefId(1));
        assert_eq!(&*defs[1].1.source_id, "legacy:1");
        assert_eq!(defs[2].0, PartDefId(2));
        assert_eq!(&*defs[2].1.source_id, "legacy:0");
        assert_ne!(defs[0].1.local_pins, defs[2].1.local_pins);

        for (pid, p) in doc.parts() {
            assert_eq!(doc.part_params(pid), Some(&crate::Params::new()));
            let def = doc.part_def(doc.part_def_id(pid).unwrap()).unwrap();
            assert!(def.is_unmodelled());
            assert_eq!(crate::part_def::validate(def), Ok(()));
            assert_eq!(
                crate::part_def::validate_instance(
                    def,
                    p.symbol,
                    &def.local_pins,
                    def.local_bounds,
                    &p.value,
                    doc.part_params(pid).unwrap(),
                ),
                Ok(())
            );
        }
    }

    #[test]
    fn empty_round_trip_and_writer_failure() {
        let doc = Doc::new();
        let mut bytes = Vec::new();
        doc.save(&mut bytes).unwrap();
        assert_eq!(Doc::load(bytes.as_slice()).unwrap(), doc);
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            doc.save(Broken).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}
