//! CPU-only observations of the workloads in omnia-bench/benches/{connectivity,index}.rs.
#[path = "../../../../crates/omnia-bench/src/generate.rs"]
mod generate;
use generate::{StressSheet, generate};
use omnia_doc::{Connectivity, GRID_NM, Label, LabelId, Pt, Rect, Wire, WireId};
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
        _ => panic!("unknown case"),
    }
}
