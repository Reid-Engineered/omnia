//! W1.5: one `route` call on the stress sheets. Each case routes from a part's first pin moved
//! one grid step down (where a follow would start) to the next part's first pin, so every
//! route crosses a neighbour's wiring and often its body. Scratch and output are warm.
use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::route::{Axis, Blocker, RouteScratch, route};
use omnia_doc::{Doc, GRID_NM, PartId, Pt};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

fn cases(doc: &Doc) -> Vec<(Pt, Pt, Axis)> {
    let pins: Vec<Pt> = doc
        .parts()
        .filter_map(|(id, _): (PartId, _)| doc.pins(id).and_then(|p| p.first().copied()))
        .collect();
    pins.windows(2)
        .enumerate()
        .map(|(i, w)| {
            let a = Pt {
                x: w[0].x,
                y: w[0].y + GRID_NM,
            };
            let leg = if i % 2 == 0 {
                Axis::Horizontal
            } else {
                Axis::Vertical
            };
            (a, w[1], leg)
        })
        .collect()
}

fn bench(c: &mut Criterion) {
    for (name, sheet) in [
        ("route_10k", StressSheet::Parts10k),
        ("route_dense", StressSheet::DenseWiring),
    ] {
        let doc = generate(sheet, 42);
        let cases = cases(&doc);
        let mut scratch = RouteScratch::new();
        let mut out = Vec::with_capacity(3);
        // Warm the scratch over every case, then time each call once: the tail a mean hides.
        for &(a, b, leg) in &cases {
            let _ = route(&doc, a, b, leg, &mut scratch, &mut out);
        }
        let mut kinds = [0usize; 6]; // ok, pin, end, body, overlap, sheet
        let mut segments = [0usize; 4];
        let mut samples: Vec<Duration> = cases
            .iter()
            .map(|&(a, b, leg)| {
                let now = Instant::now();
                let r = route(
                    &doc,
                    black_box(a),
                    black_box(b),
                    leg,
                    &mut scratch,
                    &mut out,
                );
                let t = now.elapsed();
                kinds[match r {
                    Ok(()) => 0,
                    Err(Blocker::Pin { .. }) => 1,
                    Err(Blocker::WireEnd(_)) => 2,
                    Err(Blocker::WireBody(_)) => 3,
                    Err(Blocker::Overlap(_)) => 4,
                    Err(Blocker::Sheet) => 5,
                }] += 1;
                segments[out.len()] += 1;
                black_box(&out);
                t
            })
            .collect();
        samples.sort();
        let n = samples.len();
        println!(
            "observations {name}: routes={n} ok={} refused(pin/end/body/overlap/sheet)={:?} \
             ok_segments(1/2/3)={:?} p50_us={:.3} p99_us={:.3} max_us={:.3}",
            kinds[0],
            &kinds[1..],
            &segments[1..],
            samples[n / 2].as_secs_f64() * 1e6,
            samples[n * 99 / 100].as_secs_f64() * 1e6,
            samples[n - 1].as_secs_f64() * 1e6,
        );
        let mut i = 0;
        c.bench_function(name, |bencher| {
            bencher.iter(|| {
                i = (i + 1) % cases.len();
                let (a, b, leg) = cases[i];
                let r = route(
                    &doc,
                    black_box(a),
                    black_box(b),
                    leg,
                    &mut scratch,
                    &mut out,
                );
                let _ = black_box((r, &out));
            })
        });
    }
}

criterion_group! {name=benches;config=Criterion::default().sample_size(20).warm_up_time(Duration::from_secs(1)).measurement_time(Duration::from_secs(2));targets=bench}
criterion_main!(benches);
