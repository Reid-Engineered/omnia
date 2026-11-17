//! Command-boundary timing cases for T2.8/T5.1 (gate-blocker audit section 3.4).
//!
//! This runner extends the T2.8 CPU runner (`docs/measurements/T2.8-reference/runner`)
//! without changing its Connectivity-only cases: the four original cases below are
//! copied unchanged so old and new numbers stay comparable. The new `doc_*` cases
//! time the public command boundary, `Doc::apply`, on the same workloads.
//!
//! Frozen case list (also mirrored in `run.sh`; do not edit after seeing results):
//!
//! Connectivity-only, unchanged: `real_10k_leaf`, `label_5k_gnd_vcc_bridge`,
//! `chain_20k_middle`, `index_10k`.
//! Public `Doc::apply`: `doc_10k_wire_add`, `doc_10k_wire_delete`,
//! `doc_10k_move_100`, `doc_10k_move_100_connected`, `doc_chain_20k_add`, `doc_chain_20k_delete`,
//! `doc_label_bridge_add`, `doc_label_bridge_delete`.
//!
//! Timed span: exactly one `Doc::apply` call per observation. Inside the span are
//! command validation (K1 bounds, INV1, existence/uniqueness), arena mutation,
//! `SpatialIndex` maintenance, point-incidence maintenance, incremental
//! `Connectivity` maintenance (including split/bridge floods), `Change`/dirty-rect
//! computation, inverse construction and undo-history push with revision bump.
//! Outside the span are workload generation (`generate`/`Doc::from_items`), all
//! `Command`/`Vec` construction and cloning, the `undo()` that restores the
//! starting workload, effectiveness assertions, and CSV output (samples are
//! buffered and printed only after the loop). Only public APIs are used; the
//! crate-private `Doc::insert_wire`/`remove_wire` helpers are never called.
//!
//! Each case invocation records 1,000 observations of one operation. CSV formats:
//! pair cases `sample,add_ns,remove_ns`, query `sample,query_ns,count`, and
//! command cases `sample,apply_ns`.

#[path = "../../../../crates/omnia-bench/src/generate.rs"]
mod generate;
use generate::{StressSheet, generate};
use omnia_doc::{
    Command, Connectivity, Doc, GRID_NM, ItemId, Label, LabelId, PartId, Pt, Rect, Wire, WireId,
};
use std::{hint::black_box, time::Instant};

const N: usize = 1000;

fn pair(conn: &mut Connectivity, id: WireId, wire: Wire) {
    let mut samples = Vec::with_capacity(N);
    for _ in 0..N {
        let start = Instant::now();
        conn.add_wire(black_box(id), black_box(wire));
        let add = start.elapsed().as_nanos();
        let start = Instant::now();
        black_box(conn.remove_wire(black_box(id)));
        samples.push((add, start.elapsed().as_nanos()));
    }
    // Output happens after all observations; no timed file or terminal IO.
    println!("sample,add_ns,remove_ns");
    for (i, (add, remove)) in samples.into_iter().enumerate() {
        println!("{i},{add},{remove}");
    }
}

/// Times one pre-built command per observation and restores with untimed undo.
fn apply_case(doc: &mut Doc, reference: &Doc, mut build: impl FnMut() -> Command) {
    let mut samples = Vec::with_capacity(N);
    for _ in 0..N {
        let cmd = build();
        let start = Instant::now();
        let result = doc.apply(black_box(cmd));
        let elapsed = start.elapsed().as_nanos();
        let change = black_box(result).expect("frozen case command must validate");
        assert!(
            change.nets_changed,
            "frozen case command must change connectivity"
        );
        samples.push(elapsed);
        black_box(doc.undo()).expect("undo must restore the starting workload");
    }
    assert!(
        doc.content_eq(reference),
        "INV4: exact live content and IDs restored"
    );
    println!("sample,apply_ns");
    for (i, ns) in samples.into_iter().enumerate() {
        println!("{i},{ns}");
    }
}

fn chain_doc(with_middle: bool) -> Doc {
    let wires: Vec<Wire> = (0..20_000i32)
        .filter(|&i| with_middle || i != 10_000)
        .map(|i| Wire {
            a: Pt { x: i, y: 0 },
            b: Pt { x: i + 1, y: 0 },
        })
        .collect();
    let doc = Doc::from_items([], wires, []).expect("chain geometry fits K1 and INV1");
    assert_eq!(
        doc.wires().count(),
        if with_middle { 20_000 } else { 19_999 }
    );
    doc
}

fn bridge_doc() -> Doc {
    let wires: Vec<Wire> = (0..5_000i32)
        .map(|i| Wire {
            a: Pt { x: i * 10, y: 0 },
            b: Pt {
                x: i * 10 + 1,
                y: 0,
            },
        })
        .collect();
    let labels: Vec<Label> = (0..5_000i32)
        .map(|i| Label {
            at: Pt { x: i * 10, y: 0 },
            text: if i % 2 == 0 { "GND" } else { "VCC" }.into(),
        })
        .collect();
    let doc = Doc::from_items([], wires, labels).expect("bridge geometry fits K1 and INV1");
    assert_eq!(doc.wires().count(), 5_000);
    assert_eq!(doc.labels().count(), 5_000);
    doc
}

fn main() {
    assert_eq!(StressSheet::ALL.len(), 5);
    match std::env::args().nth(1).as_deref().expect("case required") {
        "real_10k_leaf" => {
            let doc = generate(StressSheet::Parts10k, 42);
            let mut conn = Connectivity::rebuild_with_parts(
                doc.wires().map(|(id, &w)| (id, w)),
                doc.labels(),
                doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
            );
            let a = doc.wires().next().unwrap().1.a;
            pair(
                &mut conn,
                WireId(20000),
                Wire {
                    a,
                    b: Pt { x: a.x + 1, y: a.y },
                },
            );
        }
        "label_5k_gnd_vcc_bridge" => {
            let mut conn = Connectivity::new();
            for i in 0..5000 {
                let x = i * 10;
                conn.add_wire(
                    WireId(i as u32),
                    Wire {
                        a: Pt { x, y: 0 },
                        b: Pt { x: x + 1, y: 0 },
                    },
                );
                conn.add_label(
                    LabelId(i as u32),
                    &Label {
                        at: Pt { x, y: 0 },
                        text: if i % 2 == 0 { "GND" } else { "VCC" }.into(),
                    },
                );
            }
            pair(
                &mut conn,
                WireId(5000),
                Wire {
                    a: Pt { x: 0, y: 0 },
                    b: Pt { x: 10, y: 0 },
                },
            );
        }
        "chain_20k_middle" => {
            let mut conn = Connectivity::new();
            for i in 0..20000 {
                conn.add_wire(
                    WireId(i as u32),
                    Wire {
                        a: Pt { x: i, y: 0 },
                        b: Pt { x: i + 1, y: 0 },
                    },
                );
            }
            let id = WireId(10000);
            let wire = conn.remove_wire(id).unwrap();
            pair(&mut conn, id, wire);
        }
        "index_10k" => {
            let doc = generate(StressSheet::Parts10k, 42);
            assert_eq!(doc.parts().count(), 10000);
            assert_eq!(doc.wires().count(), 20000);
            let area = Rect::from_points(
                Pt {
                    x: -96 * GRID_NM,
                    y: -54 * GRID_NM,
                },
                Pt {
                    x: 96 * GRID_NM,
                    y: 54 * GRID_NM,
                },
            )
            .unwrap();
            let mut out = Vec::with_capacity(30000);
            doc.query(area, &mut out); // warm and verify a real query
            let count = out.len();
            assert!(count > 0);
            let mut samples = Vec::with_capacity(N);
            for _ in 0..N {
                let start = Instant::now();
                doc.query(black_box(area), black_box(&mut out));
                samples.push(start.elapsed().as_nanos());
            }
            println!("sample,query_ns,count");
            for (i, ns) in samples.into_iter().enumerate() {
                println!("{i},{ns},{count}");
            }
        }
        // INV2: extend wire 0 at its far end, without overlapping its interior.
        "doc_10k_wire_add" => {
            let reference = generate(StressSheet::Parts10k, 42);
            let mut doc = generate(StressSheet::Parts10k, 42);
            assert_eq!(doc.parts().count(), 10000);
            assert_eq!(doc.wires().count(), 20000);
            let a = doc.wire(WireId(0)).unwrap().b;
            let wire = Wire {
                a,
                b: Pt { x: a.x + 1, y: a.y },
            };
            apply_case(&mut doc, &reference, move || Command::AddWires(vec![wire]));
            assert_eq!(doc.wires().count(), 20000);
        }
        // One effective wire delete on the actual 10k workload: `WireId(0)` is a
        // leaf from part 0's first pin, matching the leaf class above. Undo
        // restores the same ID outside the span.
        "doc_10k_wire_delete" => {
            let reference = generate(StressSheet::Parts10k, 42);
            let mut doc = generate(StressSheet::Parts10k, 42);
            assert_eq!(doc.parts().count(), 10000);
            assert_eq!(doc.wires().count(), 20000);
            assert!(doc.wire(WireId(0)).is_some());
            apply_case(&mut doc, &reference, || {
                Command::Delete(vec![ItemId::Wire(WireId(0))])
            });
            assert!(doc.wire(WireId(0)).is_some());
            assert_eq!(doc.wires().count(), 20000);
        }
        // T5.1 done-when case: one `Move` of 100 items (first 100 parts, whose
        // pins, bodies, index and connectivity all move) by one grid step.
        "doc_10k_move_100" => {
            let reference = generate(StressSheet::Parts10k, 42);
            let mut doc = generate(StressSheet::Parts10k, 42);
            assert_eq!(doc.parts().count(), 10000);
            assert_eq!(doc.wires().count(), 20000);
            let items: Vec<ItemId> = (0..100).map(|i| ItemId::Part(PartId(i))).collect();
            let first_at = doc.part(PartId(0)).unwrap().at;
            let delta = Pt {
                x: GRID_NM,
                y: GRID_NM,
            };
            apply_case(&mut doc, &reference, || Command::Move {
                items: items.clone(),
                delta,
            });
            assert_eq!(doc.part(PartId(0)).unwrap().at, first_at);
        }
        // Pin-count worst case on the actual 10k sheet: max-pin parts first,
        // ties by lowest ID. Every selected part leaves connected wire ends.
        "doc_10k_move_100_connected" => {
            let reference = generate(StressSheet::Parts10k, 42);
            let mut doc = generate(StressSheet::Parts10k, 42);
            let mut ranked: Vec<_> = doc
                .parts()
                .map(|(id, _)| (std::cmp::Reverse(doc.pins(id).unwrap().len()), id.0))
                .collect();
            ranked.sort_unstable();
            let items: Vec<_> = ranked
                .into_iter()
                .take(100)
                .map(|(_, id)| {
                    let id = PartId(id);
                    assert!(
                        doc.pins(id)
                            .unwrap()
                            .iter()
                            .any(|pin| { doc.wires().any(|(_, w)| w.a == *pin || w.b == *pin) })
                    );
                    ItemId::Part(id)
                })
                .collect();
            assert_eq!(items.len(), 100);
            apply_case(&mut doc, &reference, || Command::Move {
                items: items.clone(),
                delta: Pt {
                    x: GRID_NM,
                    y: GRID_NM,
                },
            });
        }
        // Worst case at command level: adding the missing middle segment joins
        // the two 10k halves of the 20k chain.
        "doc_chain_20k_add" => {
            let reference = chain_doc(false);
            let mut doc = chain_doc(false);
            let middle = Wire {
                a: Pt { x: 10000, y: 0 },
                b: Pt { x: 10001, y: 0 },
            };
            apply_case(&mut doc, &reference, move || {
                Command::AddWires(vec![middle])
            });
            assert_eq!(doc.wires().count(), 19999);
        }
        // Worst case at command level: deleting the middle segment splits the
        // 20k chain (the T2.8 split case).
        "doc_chain_20k_delete" => {
            let reference = chain_doc(true);
            let mut doc = chain_doc(true);
            apply_case(&mut doc, &reference, || {
                Command::Delete(vec![ItemId::Wire(WireId(10000))])
            });
            assert!(doc.wire(WireId(10000)).is_some());
            assert_eq!(doc.wires().count(), 20000);
        }
        // Worst case at command level: adding the bridge shorts the GND/VCC
        // logical nets across 5,000 one-wire components.
        "doc_label_bridge_add" => {
            let reference = bridge_doc();
            let mut doc = bridge_doc();
            let bridge = Wire {
                a: Pt { x: 1, y: 0 },
                b: Pt { x: 10, y: 0 },
            };
            apply_case(&mut doc, &reference, move || {
                Command::AddWires(vec![bridge])
            });
            assert_eq!(doc.wires().count(), 5000);
        }
        // Worst case at command level: deleting the bridge forces the logical
        // re-flood. Setup adds the bridge outside any timed span; its issued ID
        // is read back from the public `Change`.
        "doc_label_bridge_delete" => {
            let mut reference = bridge_doc();
            let mut doc = bridge_doc();
            let bridge = Wire {
                a: Pt { x: 1, y: 0 },
                b: Pt { x: 10, y: 0 },
            };
            reference.apply(Command::AddWires(vec![bridge])).unwrap();
            let setup = doc
                .apply(Command::AddWires(vec![bridge]))
                .expect("bridge setup must validate");
            assert_eq!(setup.added.len(), 1);
            let ItemId::Wire(bridge_id) = setup.added[0] else {
                panic!("bridge setup must add exactly one wire");
            };
            apply_case(&mut doc, &reference, move || {
                Command::Delete(vec![ItemId::Wire(bridge_id)])
            });
            assert!(doc.wire(bridge_id).is_some());
            assert_eq!(doc.wires().count(), 5001);
        }
        _ => panic!("unknown case"),
    }
}
