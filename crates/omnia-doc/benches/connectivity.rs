//! T4.1: incremental wire add and wire remove latency on a 10,000-part / 20,000-wire sheet.
//!
//! T2.4 owns the stress-sheet generator (`crates/omnia-bench/src/generate.rs`), which has not
//! landed yet, so this bench builds an equivalent seeded synthetic sheet: exactly 10,000 parts
//! and 20,000 wires (the counts plan section 2 names for `10k`), plus net labels across power
//! and signal nets. Once T2.4 lands, switch this bench to use its `10k` generator.
//!
//! Run with `cargo bench -p omnia-doc --bench connectivity`.

use std::{
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{Criterion, criterion_group, criterion_main};
use omnia_doc::{Connectivity, GRID_NM, Label, LabelId, Pt, Wire, WireId};

const PARTS_PER_SIDE: i32 = 100;
const PITCH: i32 = 7 * GRID_NM;

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

struct SyntheticSheet {
    conn: Connectivity,
    wires: Vec<(WireId, Wire)>,
    labels: Vec<(LabelId, Label)>,
    next_wire_id: u32,
}

/// Builds a seeded 10,000-part / 20,000-wire schematic with chained signal nets, 2% long
/// vertical buses, and labeled power/clock nets (`GND`, `VCC`, `CLK`).
fn sheet_10k() -> SyntheticSheet {
    let mut rng = Rng(0x0A11_CE5E_ED00_4100);
    let mut wires = Vec::with_capacity(20_000);
    let mut labels = Vec::new();
    let origin = -(PARTS_PER_SIDE - 1) * PITCH / 2;
    let mut wire_id = 0u32;
    let mut label_id = 0u32;

    let gnd: Arc<str> = Arc::from("GND");
    let vcc: Arc<str> = Arc::from("VCC");
    let clk: Arc<str> = Arc::from("CLK");

    for row in 0..PARTS_PER_SIDE {
        for col in 0..PARTS_PER_SIDE {
            let at = Pt {
                x: origin + col * PITCH,
                y: origin + row * PITCH,
            };

            // Horizontal wire chaining groups of ~8 parts together into multi-wire components.
            let h_start = Pt {
                x: at.x + 2 * GRID_NM,
                y: at.y,
            };
            let h_end = if col % 8 == 7 {
                Pt {
                    x: at.x + 4 * GRID_NM,
                    y: at.y,
                }
            } else {
                Pt {
                    x: at.x + PITCH + 2 * GRID_NM,
                    y: at.y,
                }
            };
            wires.push((
                WireId(wire_id),
                Wire {
                    a: h_start,
                    b: h_end,
                },
            ));
            wire_id += 1;

            // Vertical wire: either a short stub or a 2% long bus joining rows.
            let span = if rng.below(50) == 0 {
                20 + rng.below(41) as i32
            } else {
                1
            };
            let v_start = Pt {
                x: at.x,
                y: at.y + 2 * GRID_NM,
            };
            let v_end = Pt {
                x: at.x,
                y: at.y + span * PITCH - 2 * GRID_NM,
            };
            wires.push((
                WireId(wire_id),
                Wire {
                    a: v_start,
                    b: v_end,
                },
            ));
            wire_id += 1;

            // Attach power/clock labels on ~10% of parts.
            match rng.below(30) {
                0 => {
                    labels.push((
                        LabelId(label_id),
                        Label {
                            at: h_start,
                            text: Arc::clone(&gnd),
                        },
                    ));
                    label_id += 1;
                }
                1 => {
                    labels.push((
                        LabelId(label_id),
                        Label {
                            at: h_start,
                            text: Arc::clone(&vcc),
                        },
                    ));
                    label_id += 1;
                }
                2 => {
                    labels.push((
                        LabelId(label_id),
                        Label {
                            at: v_start,
                            text: Arc::clone(&clk),
                        },
                    ));
                    label_id += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!(wires.len(), 20_000);

    let conn = Connectivity::rebuild(wires.iter().copied(), labels.iter().map(|(id, l)| (*id, l)));
    SyntheticSheet {
        conn,
        wires,
        labels,
        next_wire_id: wire_id,
    }
}

fn bench_add_only(c: &mut Criterion, name: &str, conn: &mut Connectivity, id: WireId, wire: Wire) {
    c.bench_function(name, |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let t0 = Instant::now();
                conn.add_wire(black_box(id), black_box(wire));
                total += t0.elapsed();
                conn.remove_wire(id);
            }
            total
        });
    });
}

fn bench_remove_only(
    c: &mut Criterion,
    name: &str,
    conn: &mut Connectivity,
    id: WireId,
    wire: Wire,
) {
    c.bench_function(name, |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                conn.add_wire(id, wire);
                let t0 = Instant::now();
                black_box(conn.remove_wire(black_box(id)));
                total += t0.elapsed();
            }
            total
        });
    });
}

fn bench(c: &mut Criterion) {
    let mut sheet = sheet_10k();
    let origin = -(PARTS_PER_SIDE - 1) * PITCH / 2;
    let probe_id = WireId(sheet.next_wire_id);
    sheet.next_wire_id += 1;

    // 1. Leaf wire attached to an existing component.
    let leaf_wire = Wire {
        a: Pt {
            x: origin + 2 * GRID_NM,
            y: origin,
        },
        b: Pt {
            x: origin + 2 * GRID_NM,
            y: origin - GRID_NM,
        },
    };
    bench_add_only(c, "wire_add_leaf_10k", &mut sheet.conn, probe_id, leaf_wire);
    bench_remove_only(
        c,
        "wire_remove_leaf_10k",
        &mut sheet.conn,
        probe_id,
        leaf_wire,
    );

    // 2. Bridge wire joining two ~8-wire components (union on add, BFS re-flood on remove).
    let bridge_wire = Wire {
        a: Pt {
            x: origin + 2 * GRID_NM,
            y: origin,
        },
        b: Pt {
            x: origin + 2 * GRID_NM,
            y: origin + PITCH,
        },
    };
    bench_add_only(
        c,
        "wire_add_bridge_10k",
        &mut sheet.conn,
        probe_id,
        bridge_wire,
    );
    bench_remove_only(
        c,
        "wire_remove_split_10k",
        &mut sheet.conn,
        probe_id,
        bridge_wire,
    );
    c.bench_function("wire_add_and_remove_pair_10k", |b| {
        b.iter(|| {
            sheet
                .conn
                .add_wire(black_box(probe_id), black_box(bridge_wire));
            black_box(sheet.conn.remove_wire(black_box(probe_id)));
        });
    });

    // 3. Stress case: bridge and split two 100-wire chains (200-wire component re-flood).
    let chain_a_base = sheet.next_wire_id;
    let chain_b_base = sheet.next_wire_id + 100;
    let y_a = origin - 10 * PITCH;
    let y_b = origin - 11 * PITCH;
    for i in 0..100 {
        let offset = i as u32;
        sheet.conn.add_wire(
            WireId(chain_a_base + offset),
            Wire {
                a: Pt {
                    x: origin + i * GRID_NM,
                    y: y_a,
                },
                b: Pt {
                    x: origin + (i + 1) * GRID_NM,
                    y: y_a,
                },
            },
        );
        sheet.conn.add_wire(
            WireId(chain_b_base + offset),
            Wire {
                a: Pt {
                    x: origin + i * GRID_NM,
                    y: y_b,
                },
                b: Pt {
                    x: origin + (i + 1) * GRID_NM,
                    y: y_b,
                },
            },
        );
    }
    let big_bridge_id = WireId(chain_b_base + 100);
    let big_bridge = Wire {
        a: Pt {
            x: origin + 50 * GRID_NM,
            y: y_a,
        },
        b: Pt {
            x: origin + 50 * GRID_NM,
            y: y_b,
        },
    };
    bench_add_only(
        c,
        "wire_add_200wire_bridge_10k",
        &mut sheet.conn,
        big_bridge_id,
        big_bridge,
    );
    bench_remove_only(
        c,
        "wire_remove_200wire_split_10k",
        &mut sheet.conn,
        big_bridge_id,
        big_bridge,
    );

    // 4. From-scratch rebuild on the 10k sheet (20,000 wires + labels).
    c.bench_function("rebuild_10k", |b| {
        b.iter(|| {
            black_box(Connectivity::rebuild(
                black_box(sheet.wires.iter().copied()),
                black_box(sheet.labels.iter().map(|(id, l)| (*id, l))),
            ))
        });
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
