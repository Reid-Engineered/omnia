use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::{GRID_NM, Pt, Rect};
use std::hint::black_box;
fn bench(c: &mut Criterion) {
    let doc = generate(StressSheet::Parts10k, 42);
    let mut out = Vec::with_capacity(30000);
    for (name, area) in [
        (
            "doc_query_4k_real_10k",
            Rect::from_points(
                Pt {
                    x: -96 * GRID_NM,
                    y: -54 * GRID_NM,
                },
                Pt {
                    x: 96 * GRID_NM,
                    y: 54 * GRID_NM,
                },
            )
            .unwrap(),
        ),
        (
            "doc_query_whole_real_10k",
            Rect::from_points(
                Pt {
                    x: -1000000000,
                    y: -1000000000,
                },
                Pt {
                    x: 1000000000,
                    y: 1000000000,
                },
            )
            .unwrap(),
        ),
    ] {
        c.bench_function(name, |b| {
            b.iter(|| doc.query(black_box(area), black_box(&mut out)))
        });
    }
    c.bench_function("generate_real_10k", |b| {
        b.iter(|| black_box(generate(StressSheet::Parts10k, 42)))
    });
}
criterion_group! {name=benches;config=Criterion::default().sample_size(20).warm_up_time(std::time::Duration::from_secs(1)).measurement_time(std::time::Duration::from_secs(2));targets=bench}
criterion_main!(benches);
