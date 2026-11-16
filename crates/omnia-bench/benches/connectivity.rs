use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::{Connectivity, Label, LabelId, Pt, Wire, WireId};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
fn pair(c: &mut Criterion, name: &str, conn: &mut Connectivity, id: WireId, w: Wire) {
    // Individual observations expose scheduler tails and >1 ms misses hidden by means.
    let mut adds = Vec::with_capacity(1000);
    let mut removes = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let now = Instant::now();
        conn.add_wire(black_box(id), black_box(w));
        adds.push(now.elapsed());
        let now = Instant::now();
        black_box(conn.remove_wire(black_box(id)));
        removes.push(now.elapsed());
    }
    for (operation, mut samples) in [("add", adds), ("remove", removes)] {
        samples.sort();
        println!(
            "observations {name}_{operation}: n=1000 p99_ms={:.6} max_ms={:.6} over_1ms={}",
            samples[989].as_secs_f64() * 1000.0,
            samples[999].as_secs_f64() * 1000.0,
            samples
                .iter()
                .filter(|&&t| t >= Duration::from_millis(1))
                .count()
        );
    }
    c.bench_function(&format!("{name}_add"), |b| {
        b.iter_custom(|n| {
            let mut t = Duration::ZERO;
            for _ in 0..n {
                let now = Instant::now();
                conn.add_wire(black_box(id), black_box(w));
                t += now.elapsed();
                conn.remove_wire(id);
            }
            t
        })
    });
    c.bench_function(&format!("{name}_remove"), |b| {
        b.iter_custom(|n| {
            let mut t = Duration::ZERO;
            for _ in 0..n {
                conn.add_wire(id, w);
                let now = Instant::now();
                black_box(conn.remove_wire(black_box(id)));
                t += now.elapsed();
            }
            t
        })
    });
}
fn bench(c: &mut Criterion) {
    let doc = generate(StressSheet::Parts10k, 42);
    let mut conn = Connectivity::rebuild_with_parts(
        doc.wires().map(|(id, &w)| (id, w)),
        doc.labels(),
        doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
    );
    let a = doc.wires().next().unwrap().1.a;
    pair(
        c,
        "real_10k_leaf",
        &mut conn,
        WireId(20000),
        Wire {
            a,
            b: Pt { x: a.x + 1, y: a.y },
        },
    );
    c.bench_function("rebuild_real_10k", |b| {
        b.iter(|| {
            black_box(Connectivity::rebuild_with_parts(
                doc.wires().map(|(id, &w)| (id, w)),
                doc.labels(),
                doc.parts().map(|(id, _)| (id, doc.pins(id).unwrap())),
            ))
        })
    });
    // 5,000 separate geometric components, alternating GND/VCC; the bridge shorts both texts.
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
        c,
        "label_5k_gnd_vcc_bridge",
        &mut conn,
        WireId(5000),
        Wire {
            a: Pt { x: 0, y: 0 },
            b: Pt { x: 10, y: 0 },
        },
    );
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
    // Remove the middle segment, leaving two 10k chains; every measured removal re-floods 20k wires.
    let id = WireId(10000);
    let w = conn.remove_wire(id).unwrap();
    pair(c, "chain_20k_middle", &mut conn, id, w);
}
criterion_group! {name=benches;config=Criterion::default().sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));targets=bench}
criterion_main!(benches);
