//! T6.1 / D8 diagnostic only: run with --release --ignored --nocapture.
//! No threshold here certifies a budget on a development machine.
use omnia_doc::{Command, Doc, GRID_NM, Pt, Wire};
use std::time::Instant;

fn wire(x: i32, y: i32, dx: i32) -> Wire {
    Wire {
        a: Pt { x, y },
        b: Pt { x: x + dx, y },
    }
}

#[test]
#[ignore = "explicit non-reference timing diagnostic"]
fn t61_add_wires_locality_timing() {
    let cases = [
        ("isolated_2k", 2_000),
        ("isolated_10k", 10_000),
        ("isolated_20k", 20_000),
        ("bridge_5k", 5_000),
    ];
    // Fixed rotating order, ten repetitions, twenty samples each. Construction
    // and undo are outside the timed scope. Undo keeps every sample equivalent.
    println!("T61,run,case,p99_ns,min_ns,max_ns");
    for run in 0..10 {
        for offset in 0..cases.len() {
            let (name, count) = cases[(run + offset) % cases.len()];
            let mut wires: Vec<_> = (0..count)
                .map(|i| {
                    wire(
                        -400_000_000 + (i % 100) * 2 * GRID_NM,
                        -600_000_000 + (i / 100) * 2 * GRID_NM,
                        GRID_NM,
                    )
                })
                .collect();
            let added = if name.starts_with("bridge") {
                wires.push(wire(-900_000_000 - GRID_NM, -900_000_000, GRID_NM));
                wires.push(wire(-900_000_000 + GRID_NM, -900_000_000, GRID_NM));
                wire(-900_000_000, -900_000_000, GRID_NM)
            } else {
                wire(-900_000_000 - GRID_NM, -900_000_000, GRID_NM)
            };
            let mut doc = Doc::from_items([], wires, []).unwrap();
            let mut samples = Vec::with_capacity(20);
            for _ in 0..20 {
                let command = Command::AddWires(vec![added]);
                let start = Instant::now();
                let change = doc.apply(command).unwrap();
                samples.push(start.elapsed().as_nanos());
                assert!(change.dirty.is_some());
                doc.undo().unwrap();
            }
            samples.sort_unstable();
            println!(
                "T61,{run},{name},{},{},{}",
                samples[19], samples[0], samples[19]
            );
        }
    }
}
