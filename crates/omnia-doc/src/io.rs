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
    pub fn save(&self, mut w: impl Write) -> io::Result<()> {
        let text = ron::ser::to_string_pretty(&self.sheet(), ron::ser::PrettyConfig::default())
            .map_err(invalid)?;
        w.write_all(text.as_bytes())
    }

    /// Loads a self-contained test sheet, validating geometry and rebuilding incidence.
    /// Symbol IDs retain their meaning in the library used to generate the sheet.
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

// Equality covers authoritative state, including holes, pin geometry and revision.
// Incidence is derived and its hash-map traversal order is deliberately excluded.
impl PartialEq for Doc {
    fn eq(&self, other: &Self) -> bool {
        self.sheet() == other.sheet()
    }
}
impl Eq for Doc {}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(doc, loaded);
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
