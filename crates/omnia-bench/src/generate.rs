//! Deterministic test sheets. SplitMix64 is specified here so seeds survive dependency updates.

use omnia_doc::{Doc, Label, Orient, PIN_LATTICE_NM, Part, Pt, SymbolId, Wire};
use omnia_symbols::SymbolLibrary;

/// Fixed list of the six original built-in symbols in their original `SymbolId` order (`PD11`).
///
/// Existing stress sheets back certified reference budgets and must remain item-for-item
/// identical when new symbols are added to `omnia-symbols`; a workload that exercises new
/// symbols uses a new [`StressSheet`] variant.
const STRESS_SYMBOLS: [SymbolId; 6] = [
    SymbolId(0),
    SymbolId(1),
    SymbolId(2),
    SymbolId(3),
    SymbolId(4),
    SymbolId(5),
];

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

/// Generates a bounded, lattice-aligned sheet using the fixed [`STRESS_SYMBOLS`] list and all
/// eight orientations.
/// Each part's first pin is shared by its outgoing wires, providing real junction work.
/// `TextHeavy` has exactly 12,000 designator/value characters (six per field).
pub fn generate(sheet: StressSheet, seed: u64) -> Doc {
    let symbols = SymbolLibrary::builtin();
    let defs: Vec<_> = STRESS_SYMBOLS.iter().map(|&id| symbols.get(id)).collect();
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

    /// 64-bit FNV-1a over a generated sheet's items, independent of file format (`PD11`, `P1.4`):
    /// every part's id, symbol id, position, orientation, stored world pin coordinates, local
    /// bounds, designator and value; every wire; every label.
    fn fingerprint(doc: &Doc) -> u64 {
        struct Fnv1a(u64);
        impl Fnv1a {
            const fn new() -> Self {
                Self(0xcbf2_9ce4_8422_2325)
            }
            fn bytes(&mut self, bytes: &[u8]) {
                for &b in bytes {
                    self.0 ^= u64::from(b);
                    self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
                }
            }
            fn u8(&mut self, v: u8) {
                self.bytes(&[v]);
            }
            fn u16(&mut self, v: u16) {
                self.bytes(&v.to_le_bytes());
            }
            fn u32(&mut self, v: u32) {
                self.bytes(&v.to_le_bytes());
            }
            fn i32(&mut self, v: i32) {
                self.bytes(&v.to_le_bytes());
            }
            fn usize(&mut self, v: usize) {
                self.bytes(&(v as u64).to_le_bytes());
            }
            fn pt(&mut self, p: Pt) {
                self.i32(p.x);
                self.i32(p.y);
            }
            fn str(&mut self, s: &str) {
                self.usize(s.len());
                self.bytes(s.as_bytes());
            }
        }

        let mut h = Fnv1a::new();
        h.u8(b'P');
        let parts: Vec<_> = doc.parts().collect();
        h.usize(parts.len());
        for (id, part) in parts {
            h.u32(id.0);
            h.u16(part.symbol.raw());
            h.pt(part.at);
            h.u8(part.orient.to_raw());
            let pins = doc.pins(id).expect("live part has stored pins");
            h.usize(pins.len());
            for &pin in pins {
                h.pt(pin);
            }
            let def_id = doc.part_def_id(id).expect("live part has definition id");
            let bounds = doc
                .part_def(def_id)
                .expect("live part definition exists")
                .local_bounds;
            h.pt(bounds.min);
            h.pt(bounds.max);
            h.str(&part.designator);
            h.str(&part.value);
        }

        h.u8(b'W');
        let wires: Vec<_> = doc.wires().collect();
        h.usize(wires.len());
        for (id, wire) in wires {
            h.u32(id.0);
            h.pt(wire.a);
            h.pt(wire.b);
        }

        h.u8(b'L');
        let labels: Vec<_> = doc.labels().collect();
        h.usize(labels.len());
        for (id, label) in labels {
            h.u32(id.0);
            h.pt(label.at);
            h.str(&label.text);
        }

        h.0
    }

    #[test]
    fn stress_sheet_fingerprints_stay_fixed_across_symbol_additions() {
        // Expected 64-bit FNV-1a fingerprints computed on `main` before adding new symbols to
        // `crates/omnia-symbols/pins.toml` (PD11, P1.4).
        const EXPECTED: [(StressSheet, u64, u64); 10] = [
            (StressSheet::Parts100, 42, 0x30a1_33dc_d049_3e6c),
            (StressSheet::Parts100, 0, 0xd5fe_6d0d_75ee_ae8f),
            (StressSheet::Parts1k, 42, 0xf5c4_aef3_7200_22c0),
            (StressSheet::Parts1k, 0, 0x2d65_1bd0_9c9e_1f7e),
            (StressSheet::Parts10k, 42, 0x8e2c_84ae_42a7_d2b6),
            (StressSheet::Parts10k, 0, 0xe083_fb50_1369_ea9f),
            (StressSheet::DenseWiring, 42, 0x103a_ab79_5b4e_ba2b),
            (StressSheet::DenseWiring, 0, 0x9498_8e48_0e27_6044),
            (StressSheet::TextHeavy, 42, 0x8438_4af0_5621_420e),
            (StressSheet::TextHeavy, 0, 0x1675_8688_e412_6abe),
        ];

        for sheet in StressSheet::ALL {
            assert!(
                EXPECTED
                    .iter()
                    .any(|&(s, seed, _)| s == sheet && seed == crate::scene::STRESS_SEED),
                "missing baseline fingerprint for {sheet:?} at STRESS_SEED"
            );
        }
        for (sheet, seed, expected) in EXPECTED {
            let actual = fingerprint(&generate(sheet, seed));
            assert_eq!(
                actual, expected,
                "stress sheet {sheet:?} (seed {seed}) changed: got {actual:#018x}, expected {expected:#018x}"
            );
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
