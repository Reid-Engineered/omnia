//! WB3: 100 parts with follow enabled on the seeded 10k stress sheet.
use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::{Command, GRID_NM, ItemId, PartId, Pt, prefs::FollowPolicy};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn bench(c: &mut Criterion) {
    let doc = generate(StressSheet::Parts10k, 42);
    let mut baseline = Vec::new();
    doc.save(&mut baseline).unwrap();
    let items: Vec<_> = (0..100).map(|i| ItemId::Part(PartId(i))).collect();
    let delta = Pt {
        x: GRID_NM,
        y: GRID_NM,
    };
    let follow = FollowPolicy::new(true, 10, false).unwrap();
    c.bench_function("follow_move_100_parts_10k_apply", |b| {
        b.iter_custom(|n| {
            let mut total = Duration::ZERO;
            for _ in 0..n {
                // INV4/D2 reserve ids on undo: reload outside timing so slot
                // counts and history remain fixed across warmup and samples.
                let mut doc = omnia_doc::Doc::load(baseline.as_slice()).unwrap();
                let command = Command::Move {
                    items: items.clone(),
                    delta,
                    follow,
                };
                // Construction and undo are outside the timed apply interval.
                let start = Instant::now();
                black_box(
                    doc.apply(black_box(command))
                        .expect("WB3 move must succeed"),
                );
                total += start.elapsed();
                black_box(doc.undo().unwrap());
            }
            total
        })
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20)
        .warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));
    targets = bench
}
criterion_main!(benches);
