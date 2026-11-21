//! A1.1: `Doc::fork` time and size on the `10k` stress sheet (mcp-plan.md AD3; mcp-roadmap.md
//! A1.1 done-when). No budget gates `fork` alone; AB2 (A1.10) gates preview with fork included.
//!
//! Size is the heap a live fork holds: the counting allocator's live bytes with the fork alive,
//! minus the live bytes before it was taken, together with the allocation count. Timed
//! observations exclude dropping the fork. `OMNIA_FORK_CSV=<path>` also writes the individual observations (A1.1 campaign script).
use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::CountingAlloc;
use omnia_bench::alloc::{count_allocs, heap_bytes};
use omnia_bench::generate::{StressSheet, generate};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

const SEED: u64 = 42;
const OBSERVATIONS: usize = 100;

fn nearest_rank(sorted: &[u64], pct: usize) -> u64 {
    let idx = (sorted.len() * pct).div_ceil(100).saturating_sub(1);
    sorted[idx]
}

fn bench(c: &mut Criterion) {
    let doc = generate(StressSheet::Parts10k, SEED);
    let (parts, wires) = StressSheet::Parts10k.counts();

    // Size: heap held by one live fork.
    let (live_before, _) = heap_bytes();
    let (fork, allocs) = count_allocs(|| doc.fork());
    let (live_after, _) = heap_bytes();
    assert_eq!(fork, doc, "fork must equal the original (A1.1)");
    let fork_bytes = live_after.saturating_sub(live_before);
    drop(fork);
    println!(
        "size fork_10k: sheet=10k parts={parts} wires={wires} revision={} fork_heap_bytes={fork_bytes} fork_allocs={allocs}",
        doc.revision()
    );

    let obs_count = if std::env::args().any(|a| a == "--bench") {
        OBSERVATIONS
    } else {
        1
    };
    let mut samples_ns = Vec::with_capacity(obs_count);
    for _ in 0..obs_count {
        let start = Instant::now();
        let fork = black_box(&doc).fork();
        let elapsed = start.elapsed();
        black_box(&fork);
        drop(fork);
        samples_ns.push(u64::try_from(elapsed.as_nanos()).expect("duration fits u64 ns"));
    }
    if let Ok(path) = std::env::var("OMNIA_FORK_CSV") {
        let mut csv = String::from("sample,ns,fork_heap_bytes,fork_allocs\n");
        for (idx, ns) in samples_ns.iter().enumerate() {
            csv.push_str(&format!("{idx},{ns},{fork_bytes},{allocs}\n"));
        }
        std::fs::write(&path, csv).unwrap_or_else(|e| panic!("failed to write CSV {path}: {e}"));
    }
    let sum_ns: u128 = samples_ns.iter().map(|&ns| u128::from(ns)).sum();
    samples_ns.sort_unstable();
    println!(
        "observations fork_10k: n={} p50_ms={:.6} p99_ms={:.6} max_ms={:.6} mean_ms={:.6}",
        samples_ns.len(),
        nearest_rank(&samples_ns, 50) as f64 / 1e6,
        nearest_rank(&samples_ns, 99) as f64 / 1e6,
        *samples_ns.last().unwrap() as f64 / 1e6,
        (sum_ns as f64 / samples_ns.len() as f64) / 1e6,
    );

    // Dropping the fork is outside the timed region, as in a preview that keeps it.
    c.bench_function("fork_10k", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                let fork = black_box(&doc).fork();
                total += start.elapsed();
                black_box(&fork);
                drop(fork);
            }
            total
        });
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(4));
    targets = bench
}
criterion_main!(benches);
