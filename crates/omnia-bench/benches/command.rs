use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::{Command, Doc, GRID_NM, ItemId, PartId, Pt, WireId};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn bench_move(c: &mut Criterion, name: &str, doc: &mut Doc, items: &[ItemId], delta: Pt) {
    // Individual observations expose scheduler tails and >=1 ms misses hidden by means.
    let mut applies = Vec::with_capacity(1000);
    let mut undos = Vec::with_capacity(1000);
    for _ in 0..1000 {
        let cmd = Command::Move {
            follow: omnia_doc::prefs::FollowPolicy::new(false, 10, false).unwrap(),
            items: items.to_vec(),
            delta,
        };
        let now = Instant::now();
        black_box(doc.apply(black_box(cmd)).unwrap());
        applies.push(now.elapsed());

        let now = Instant::now();
        black_box(doc.undo().unwrap());
        undos.push(now.elapsed());
    }
    for (op, mut samples) in [("apply", applies), ("undo", undos)] {
        samples.sort();
        println!(
            "observations {name}_{op}: n=1000 p50_ms={:.6} p99_ms={:.6} max_ms={:.6} over_1ms={}",
            samples[499].as_secs_f64() * 1000.0,
            samples[989].as_secs_f64() * 1000.0,
            samples[999].as_secs_f64() * 1000.0,
            samples
                .iter()
                .filter(|&&t| t >= Duration::from_millis(1))
                .count()
        );
    }

    c.bench_function(&format!("{name}_apply"), |b| {
        b.iter_custom(|n| {
            let mut total = Duration::ZERO;
            for _ in 0..n {
                let cmd = Command::Move {
                    follow: omnia_doc::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                    items: items.to_vec(),
                    delta,
                };
                let now = Instant::now();
                black_box(doc.apply(black_box(cmd)).unwrap());
                total += now.elapsed();
                black_box(doc.undo().unwrap());
            }
            total
        })
    });

    c.bench_function(&format!("{name}_undo"), |b| {
        b.iter_custom(|n| {
            let mut total = Duration::ZERO;
            for _ in 0..n {
                let cmd = Command::Move {
                    follow: omnia_doc::prefs::FollowPolicy::new(false, 10, false).unwrap(),
                    items: items.to_vec(),
                    delta,
                };
                black_box(doc.apply(cmd).unwrap());
                let now = Instant::now();
                black_box(doc.undo().unwrap());
                total += now.elapsed();
            }
            total
        })
    });
}

fn bench(c: &mut Criterion) {
    let mut doc = generate(StressSheet::Parts10k, 42);
    let delta = Pt {
        x: GRID_NM,
        y: GRID_NM,
    };

    let parts_100: Vec<ItemId> = (0..100).map(|i| ItemId::Part(PartId(i))).collect();
    let wires_100: Vec<ItemId> = (0..100).map(|i| ItemId::Wire(WireId(i))).collect();
    let mixed_100: Vec<ItemId> = (0..50)
        .map(|i| ItemId::Part(PartId(i)))
        .chain((0..50).map(|i| ItemId::Wire(WireId(i))))
        .collect();

    bench_move(c, "move_100_parts_10k", &mut doc, &parts_100, delta);
    bench_move(c, "move_100_wires_10k", &mut doc, &wires_100, delta);
    bench_move(c, "move_100_mixed_10k", &mut doc, &mixed_100, delta);
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(2));
    targets = bench
}
criterion_main!(benches);
