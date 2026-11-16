//! Deterministic test sheets. SplitMix64 is specified here so seeds survive dependency updates.

use omnia_doc::{Doc, Label, Orient, PIN_LATTICE_NM, Part, Pt, Wire};
use omnia_symbols::SymbolLibrary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StressSheet {
    Parts100,
    Parts1k,
    Parts10k,
    DenseWiring,
    TextHeavy,
}

impl StressSheet {
    pub const ALL: [Self; 5] = [
        Self::Parts100,
        Self::Parts1k,
        Self::Parts10k,
        Self::DenseWiring,
        Self::TextHeavy,
    ];

    /// Number of parts and axis-aligned wire segments in this workload.
    pub const fn counts(self) -> (usize, usize) {
        match self {
            Self::Parts100 => (100, 200),
            Self::Parts1k | Self::TextHeavy => (1_000, 2_000),
            Self::Parts10k => (10_000, 20_000),
            Self::DenseWiring => (1_000, 12_000),
        }
    }
}

struct SplitMix64(u64);
impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
}

/// Generates a bounded, lattice-aligned sheet using every built-in symbol and orientation.
/// Each part's first pin is shared by its outgoing wires, providing real junction work.
/// `TextHeavy` has exactly 12,000 designator/value characters (six per field).
pub fn generate(sheet: StressSheet, seed: u64) -> Doc {
    let symbols = SymbolLibrary::builtin();
    let defs: Vec<_> = symbols.iter().collect();
    let (count, wire_count) = sheet.counts();
    let columns = if count == 10_000 {
        100
    } else if count == 100 {
        10
    } else {
        32
    };
    let rows = count.div_ceil(columns);
    let pitch = 12 * PIN_LATTICE_NM;
    let mut rng = SplitMix64(seed);
    let mut parts = Vec::with_capacity(count);
    let mut wires = Vec::with_capacity(wire_count);
    for i in 0..count {
        let def = defs[(rng.next() % defs.len() as u64) as usize];
        let orient = Orient::ALL[(rng.next() % 8) as usize];
        let at = Pt {
            x: (i % columns) as i32 * pitch
                - (columns as i32 - 1) * pitch / 2 / PIN_LATTICE_NM * PIN_LATTICE_NM,
            y: (i / columns) as i32 * pitch
                - (rows as i32 - 1) * pitch / 2 / PIN_LATTICE_NM * PIN_LATTICE_NM,
        };
        let pin = at + orient.apply(def.pins[0].at);
        for j in 0..wire_count / count {
            let distance = PIN_LATTICE_NM;
            let delta = match j % 4 {
                0 => Pt { x: distance, y: 0 },
                1 => Pt { x: 0, y: distance },
                2 => Pt { x: -distance, y: 0 },
                _ => Pt { x: 0, y: -distance },
            };
            // Dense rays are consecutive unit segments, so no endpoint can lie
            // on another segment's interior (INV2), even where rays cross.
            let a = pin
                + Pt {
                    x: delta.x * (j / 4) as i32,
                    y: delta.y * (j / 4) as i32,
                };
            wires.push(Wire { a, b: a + delta });
        }
        let (designator, value) = if sheet == StressSheet::TextHeavy {
            (format!("R{i:05}"), format!("{:06}", rng.next() % 1_000_000))
        } else {
            (format!("U{}", i + 1), format!("{}k", 1 + rng.next() % 999))
        };
        parts.push((
            Part {
                symbol: def.id,
                at,
                orient,
                designator: designator.into(),
                value: value.into(),
            },
            def.pins.iter().map(|p| p.at).collect(),
            def.bounds,
        ));
    }
    Doc::from_items(parts, wires, std::iter::empty::<Label>())
        .expect("generator geometry fits K1 and INV1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{PartId, Rect, SHEET_LIMIT_NM, WireId};
    use std::{collections::HashMap, time::Instant};

    #[test]
    fn deterministic_counts_round_trip_and_invariants() {
        let symbols = SymbolLibrary::builtin();
        for sheet in StressSheet::ALL {
            for seed in [0, 42, u64::MAX] {
                let doc = generate(sheet, seed);
                assert_eq!(doc, generate(sheet, seed));
                assert_ne!(doc, generate(sheet, seed.wrapping_add(1)));
                let mut bytes = Vec::new();
                doc.save(&mut bytes).unwrap();
                assert_eq!(doc, Doc::load(bytes.as_slice()).unwrap());
                let (parts, wires) = sheet.counts();
                assert!(doc.part(PartId(parts as u32)).is_none());
                assert!(doc.wire(WireId(wires as u32)).is_none());
                let mut incidence = HashMap::<Pt, usize>::new();
                let mut characters = 0;
                for i in 0..parts {
                    let p = doc.part(PartId(i as u32)).unwrap();
                    characters += p.designator.len() + p.value.len();
                    for pin in symbols.get(p.symbol).pins {
                        let at = p.at + p.orient.apply(pin.at);
                        assert!(at.is_in_bounds());
                        assert_eq!((at.x % PIN_LATTICE_NM, at.y % PIN_LATTICE_NM), (0, 0));
                        *incidence.entry(at).or_default() += 1;
                    }
                }
                if sheet == StressSheet::TextHeavy {
                    assert_eq!(characters, 12_000);
                }
                for i in 0..wires {
                    let w = doc.wire(WireId(i as u32)).unwrap();
                    assert_ne!(w.a, w.b);
                    assert_eq!(w.a.manhattan_distance(w.b), i64::from(PIN_LATTICE_NM));
                    assert!(w.a.x == w.b.x || w.a.y == w.b.y);
                    for at in [w.a, w.b] {
                        assert!(at.is_in_bounds());
                        assert_eq!((at.x % PIN_LATTICE_NM, at.y % PIN_LATTICE_NM), (0, 0));
                        *incidence.entry(at).or_default() += 1;
                    }
                }
                let mut expected: Vec<_> = incidence
                    .into_iter()
                    .filter_map(|(p, n)| (n >= 3).then_some(p))
                    .collect();
                let mut dots = Vec::new();
                doc.junctions(
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
                    .unwrap(),
                    &mut dots,
                );
                expected.sort_by_key(|p| (p.x, p.y));
                dots.sort_by_key(|p| (p.x, p.y));
                assert_eq!(dots, expected);
            }
        }
    }

    /// Explicit measurement only: timing is recorded with hardware, never a noisy CI gate.
    #[test]
    #[ignore]
    fn measure_10k() {
        for seed in 0..10 {
            let start = Instant::now();
            let doc = generate(StressSheet::Parts10k, seed);
            let elapsed = start.elapsed();
            std::hint::black_box(doc);
            println!("10k seed={seed}: {:.3} ms", elapsed.as_secs_f64() * 1000.0);
        }
    }
}
