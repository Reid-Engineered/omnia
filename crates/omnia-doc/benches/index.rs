//! T2.2: time to query a 4K viewport on a 10,000-part sheet.
//!
//! The 10,000-part stress sheet belongs to T2.4 (`omnia-bench`'s generator), which does not
//! exist yet, so this bench builds an equivalent synthetic sheet: exactly 10,000 parts and
//! 20,000 wires, the counts plan section 2 names for `10k`. Parts sit on a 100 x 100 grid,
//! 7 grid steps apart (1.78 m square, inside K1), sized like the K7 symbols (4 grid steps for
//! most, 9 for op-amps, 2.4 for grounds). Each part has one horizontal wire to its right-hand
//! neighbour and one vertical wire to the part below; 2% of the vertical wires are long buses
//! running 20 to 60 rows. Wire bounds include half the K5 stroke. Run with
//! `cargo bench -p omnia-doc --bench index`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use omnia_doc::{GRID_NM, ItemId, PartId, Pt, Rect, SHEET_LIMIT_NM, SpatialIndex, WireId};

const PARTS_PER_SIDE: i32 = 100;
const PITCH: i32 = 7 * GRID_NM;
const HALF_STROKE: i32 = 114_300; // K5 wire stroke 228,600 nm / 2

/// splitmix64: a fixed-seed generator, so every run measures the same sheet.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
    let clamp = |v: i32| v.clamp(-SHEET_LIMIT_NM, SHEET_LIMIT_NM);
    Rect::from_points(
        Pt {
            x: clamp(x0),
            y: clamp(y0),
        },
        Pt {
            x: clamp(x1),
            y: clamp(y1),
        },
    )
    .unwrap()
}

fn wire(a: Pt, b: Pt) -> Rect {
    rect(
        a.x.min(b.x) - HALF_STROKE,
        a.y.min(b.y) - HALF_STROKE,
        a.x.max(b.x) + HALF_STROKE,
        a.y.max(b.y) + HALF_STROKE,
    )
}

fn sheet_10k() -> SpatialIndex {
    let mut rng = Rng(0x0A11_CE5E_ED00_2002);
    let mut index = SpatialIndex::new();
    let origin = -(PARTS_PER_SIDE - 1) * PITCH / 2;
    let (mut parts, mut wires) = (0u32, 0u32);
    for row in 0..PARTS_PER_SIDE {
        for col in 0..PARTS_PER_SIDE {
            let at = Pt {
                x: origin + col * PITCH,
                y: origin + row * PITCH,
            };
            // Half the placed size in nm: K7 sizes 4, 9 and 2.4 grid steps.
            let half = match rng.below(20) {
                0 | 1 => 9 * GRID_NM / 2,
                2 => 12 * GRID_NM / 10,
                _ => 2 * GRID_NM,
            };
            index
                .insert(
                    ItemId::Part(PartId(parts)),
                    rect(at.x - half, at.y - half, at.x + half, at.y + half),
                )
                .unwrap();
            parts += 1;

            let right = Pt {
                x: at.x + 2 * GRID_NM,
                y: at.y,
            };
            let next_left = Pt {
                x: at.x + PITCH - 2 * GRID_NM,
                y: at.y,
            };
            index
                .insert(ItemId::Wire(WireId(wires)), wire(right, next_left))
                .unwrap();
            wires += 1;

            let rows = if rng.below(50) == 0 {
                20 + rng.below(41) as i32
            } else {
                1
            };
            let bottom = Pt {
                x: at.x,
                y: at.y + 2 * GRID_NM,
            };
            let below = Pt {
                x: at.x,
                y: at.y + rows * PITCH - 2 * GRID_NM,
            };
            index
                .insert(ItemId::Wire(WireId(wires)), wire(bottom, below))
                .unwrap();
            wires += 1;
        }
    }
    assert_eq!((parts, wires), (10_000, 20_000));
    index
}

/// A viewport of `w` x `h` logical px at `zoom` (K3: 20 px per grid step at 100%), centred on
/// `center`.
fn viewport(center: Pt, w: i64, h: i64, zoom: f64) -> Rect {
    let nm_per_px = f64::from(GRID_NM) / (20.0 * zoom);
    let hw = (w as f64 * nm_per_px / 2.0).round() as i32;
    let hh = (h as f64 * nm_per_px / 2.0).round() as i32;
    rect(center.x - hw, center.y - hh, center.x + hw, center.y + hh)
}

fn bench(c: &mut Criterion) {
    let index = sheet_10k();
    let mut out = Vec::with_capacity(index.len());
    let cases = [
        (
            "4k_viewport_100pct_centre",
            viewport(Pt::ZERO, 3840, 2160, 1.0),
        ),
        (
            "4k_viewport_100pct_corner",
            viewport(
                Pt {
                    x: 700_000_000,
                    y: 700_000_000,
                },
                3840,
                2160,
                1.0,
            ),
        ),
        (
            "4k_viewport_fit_sheet",
            viewport(Pt::ZERO, 3840, 2160, 0.15),
        ),
    ];
    for (name, area) in cases {
        index.query(area, &mut out);
        eprintln!("{name}: area {area:?}, {} items", out.len());
        c.bench_function(name, |b| {
            b.iter(|| index.query(black_box(area), black_box(&mut out)))
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
