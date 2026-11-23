//! WB3: 100 parts with follow enabled on the seeded 10k stress sheet.
//!
//! With `--bench`, the bench first times `OBSERVATIONS` individual applies (each on a
//! freshly loaded sheet, load and undo outside the timer) and prints their nearest-rank
//! p50/p99. When `OMNIA_WB3_CSV` names a file, those per-apply samples are written there
//! for the section 2.1 campaign in `docs/measurements/W2.2/run.sh`.
use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::{Command, Doc, GRID_NM, ItemId, PartId, Pt, prefs::FollowPolicy};
use std::{
    fs::File,
    hint::black_box,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

const OBSERVATIONS: usize = 100;

fn nearest_rank(sorted: &[u64], pct: usize) -> u64 {
    let idx = (sorted.len() * pct).div_ceil(100).saturating_sub(1);
    sorted[idx]
}

fn write_csv(path: &str, samples_ns: &[u64]) {
    let file = File::create(path).unwrap_or_else(|e| panic!("failed to create CSV {path}: {e}"));
    let mut w = BufWriter::new(file);
    writeln!(w, "sample,ns").expect("write CSV header");
    for (idx, &ns) in samples_ns.iter().enumerate() {
        writeln!(w, "{idx},{ns}").expect("write CSV row");
    }
    w.flush().expect("flush CSV");
}

/// One timed apply on a freshly loaded sheet; load, command construction and undo
/// are outside the timed interval.
fn timed_apply(baseline: &[u8], items: &[ItemId], delta: Pt, follow: FollowPolicy) -> Duration {
    // INV4/D2 reserve ids on undo: reload so slot counts and history stay fixed.
    let mut doc = Doc::load(baseline).unwrap();
    let command = Command::Move {
        items: items.to_vec(),
        delta,
        follow,
    };
    let start = Instant::now();
    black_box(
        doc.apply(black_box(command))
            .expect("WB3 move must succeed"),
    );
    let elapsed = start.elapsed();
    black_box(doc.undo().unwrap());
    elapsed
}

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

    let obs_count = if std::env::args().any(|a| a == "--bench") {
        OBSERVATIONS
    } else {
        1
    };
    let samples_ns: Vec<u64> = (0..obs_count)
        .map(|_| {
            let elapsed = timed_apply(&baseline, &items, delta, follow);
            u64::try_from(elapsed.as_nanos()).expect("duration fits u64 ns")
        })
        .collect();
    if let Ok(path) = std::env::var("OMNIA_WB3_CSV") {
        write_csv(&path, &samples_ns);
    }
    let mut sorted = samples_ns;
    sorted.sort_unstable();
    println!(
        "observations follow_move_100_parts_10k_apply: n={} p50_ms={:.6} p99_ms={:.6} max_ms={:.6}",
        sorted.len(),
        nearest_rank(&sorted, 50) as f64 / 1e6,
        nearest_rank(&sorted, 99) as f64 / 1e6,
        *sorted.last().unwrap() as f64 / 1e6,
    );

    c.bench_function("follow_move_100_parts_10k_apply", |b| {
        b.iter_custom(|n| {
            (0..n)
                .map(|_| timed_apply(&baseline, &items, delta, follow))
                .sum()
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
