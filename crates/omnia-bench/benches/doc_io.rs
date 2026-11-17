//! U0.5 / US4: Criterion benchmarks and per-call observations for `Doc::save` into `Vec<u8>` and
//! `Doc::load` from bytes on `1k` and `10k` sheets (ui-plan.md UB10, US4; canvas-plan.md D8).
use criterion::{Criterion, criterion_group, criterion_main};
use omnia_bench::generate::{StressSheet, generate};
use omnia_doc::Doc;
use std::{
    fs::File,
    hint::black_box,
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

const SEED: u64 = 42;
const OBSERVATIONS: usize = 100;

#[derive(Clone, Copy)]
enum Op {
    Save,
    Load,
}

struct Case {
    name: &'static str,
    sheet_label: &'static str,
    sheet: StressSheet,
    op: Op,
}

const CASES: [Case; 4] = [
    Case {
        name: "save_1k",
        sheet_label: "1k",
        sheet: StressSheet::Parts1k,
        op: Op::Save,
    },
    Case {
        name: "load_1k",
        sheet_label: "1k",
        sheet: StressSheet::Parts1k,
        op: Op::Load,
    },
    Case {
        name: "save_10k",
        sheet_label: "10k",
        sheet: StressSheet::Parts10k,
        op: Op::Save,
    },
    Case {
        name: "load_10k",
        sheet_label: "10k",
        sheet: StressSheet::Parts10k,
        op: Op::Load,
    },
];

fn nearest_rank(sorted: &[u64], pct: usize) -> u64 {
    let idx = (sorted.len() * pct).div_ceil(100).saturating_sub(1);
    sorted[idx]
}

fn write_csv(path: &str, bytes_len: usize, samples_ns: &[u64]) {
    let file = File::create(path).unwrap_or_else(|e| panic!("failed to create CSV {path}: {e}"));
    let mut w = BufWriter::new(file);
    writeln!(w, "sample,ns,bytes").expect("write CSV header");
    for (idx, &ns) in samples_ns.iter().enumerate() {
        writeln!(w, "{idx},{ns},{bytes_len}").expect("write CSV row");
    }
    w.flush().expect("flush CSV");
}

fn run_case(c: &mut Criterion, case: &Case, csv_path: Option<&str>) {
    let doc = generate(case.sheet, SEED);
    let mut bytes = Vec::new();
    doc.save(&mut bytes).expect("save generated sheet");
    let loaded = Doc::load(bytes.as_slice()).expect("load saved sheet");
    assert_eq!(loaded, doc, "round-trip mismatch for {}", case.sheet_label);
    drop(loaded);

    let (parts, wires) = case.sheet.counts();
    let bytes_len = bytes.len();
    println!(
        "size {}: sheet={} parts={} wires={} bytes={}",
        case.name, case.sheet_label, parts, wires, bytes_len
    );

    let obs_count = if std::env::args().any(|a| a == "--bench") {
        OBSERVATIONS
    } else {
        1
    };
    let mut samples_ns = Vec::with_capacity(obs_count);
    for _ in 0..obs_count {
        let elapsed = match case.op {
            Op::Save => {
                let mut buf = Vec::new();
                let start = Instant::now();
                doc.save(black_box(&mut buf)).expect("Doc::save");
                let elapsed = start.elapsed();
                assert_eq!(buf.len(), bytes_len);
                black_box(&buf);
                drop(buf);
                elapsed
            }
            Op::Load => {
                let start = Instant::now();
                let doc_out = Doc::load(black_box(bytes.as_slice())).expect("Doc::load");
                let elapsed = start.elapsed();
                black_box(&doc_out);
                drop(doc_out);
                elapsed
            }
        };
        samples_ns.push(u64::try_from(elapsed.as_nanos()).expect("duration fits u64 ns"));
    }

    if let Some(path) = csv_path {
        write_csv(path, bytes_len, &samples_ns);
    }

    let sum_ns: u128 = samples_ns.iter().map(|&ns| u128::from(ns)).sum();
    let mut sorted = samples_ns;
    sorted.sort_unstable();
    let p50_ms = nearest_rank(&sorted, 50) as f64 / 1e6;
    let p99_ms = nearest_rank(&sorted, 99) as f64 / 1e6;
    let max_ms = *sorted.last().unwrap() as f64 / 1e6;
    let mean_ms = (sum_ns as f64 / sorted.len() as f64) / 1e6;
    println!(
        "observations {}: n={} bytes={} p50_ms={:.6} p99_ms={:.6} max_ms={:.6} mean_ms={:.6}",
        case.name,
        sorted.len(),
        bytes_len,
        p50_ms,
        p99_ms,
        max_ms,
        mean_ms
    );

    match case.op {
        Op::Save => {
            c.bench_function(case.name, |b| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let mut buf = Vec::new();
                        let start = Instant::now();
                        doc.save(black_box(&mut buf)).expect("Doc::save");
                        total += start.elapsed();
                        black_box(&buf);
                        drop(buf);
                    }
                    total
                });
            });
        }
        Op::Load => {
            c.bench_function(case.name, |b| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        let start = Instant::now();
                        let doc_out = Doc::load(black_box(bytes.as_slice())).expect("Doc::load");
                        total += start.elapsed();
                        black_box(&doc_out);
                        drop(doc_out);
                    }
                    total
                });
            });
        }
    }
}

fn bench(c: &mut Criterion) {
    let selected = std::env::var("OMNIA_DOC_IO_CASE").ok();
    let csv_path = std::env::var("OMNIA_DOC_IO_CSV").ok();
    if csv_path.is_some() && selected.is_none() {
        panic!("OMNIA_DOC_IO_CSV requires OMNIA_DOC_IO_CASE to select one case");
    }

    match selected.as_deref() {
        Some(name) => {
            let case = CASES
                .iter()
                .find(|case| case.name == name)
                .unwrap_or_else(|| panic!("unknown OMNIA_DOC_IO_CASE: {name}"));
            run_case(c, case, csv_path.as_deref());
        }
        None => {
            for case in &CASES {
                run_case(c, case, None);
            }
        }
    }
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
