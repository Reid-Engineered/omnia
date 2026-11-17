//! W1.2: `Doc::runs_at` on the stress sheets, called at each part's first pin (where the
//! generators attach their wires).
use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::{Doc, PartId, Pt};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn first_pins(doc: &Doc) -> Vec<Pt> {
    doc.parts()
        .filter_map(|(id, _): (PartId, _)| doc.pins(id).and_then(|p| p.first().copied()))
        .collect()
}

fn bench(c: &mut Criterion) {
    for (name, sheet) in [
        ("runs_at_10k", StressSheet::Parts10k),
        ("runs_at_dense", StressSheet::DenseWiring),
    ] {
        let doc = generate(sheet, 42);
        let points = first_pins(&doc);
        let mut out = Vec::with_capacity(16);
        // One observation per point exposes the tail that a mean hides.
        let mut samples: Vec<Duration> = points
            .iter()
            .map(|&p| {
                let now = Instant::now();
                doc.runs_at(black_box(p), &mut out);
                let t = now.elapsed();
                black_box(&out);
                t
            })
            .collect();
        let runs: usize = points
            .iter()
            .map(|&p| {
                doc.runs_at(p, &mut out);
                out.len()
            })
            .sum();
        samples.sort();
        let n = samples.len();
        println!(
            "observations {name}: points={n} runs={runs} p50_us={:.3} p99_us={:.3} max_us={:.3}",
            samples[n / 2].as_secs_f64() * 1e6,
            samples[n * 99 / 100].as_secs_f64() * 1e6,
            samples[n - 1].as_secs_f64() * 1e6,
        );
        let mut i = 0;
        c.bench_function(name, |b| {
            b.iter(|| {
                i = (i + 1) % points.len();
                doc.runs_at(black_box(points[i]), &mut out);
                black_box(&out);
            })
        });
    }
}

criterion_group! {name=benches;config=Criterion::default().sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));targets=bench}
criterion_main!(benches);
