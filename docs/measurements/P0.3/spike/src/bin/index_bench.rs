//! PS1 / `PB3` timing: index a generated 500-part `.olib` and the same pack as a folder.
//!
//! Usage:
//!   index_bench --work <dir> --out <results prefix> [--parts 500] [--reps 10] [--iters 200] [--warmup 20]
//!
//! Protocol (fixed before the first timed run, canvas plan section 2.1 shape): `reps`
//! repetitions; in each, the case order rotates by one; each case runs `warmup` untimed
//! iterations and then `iters` timed ones. Each repetition gives a p99 per case; the summary
//! reports the median of the run p99s and their range. Every timed sample is kept in the CSV.
//! The OS file cache is warm after generation (no cold-cache step exists on this platform).

use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;

use omnia_p03_spike::index::{self, Index};
use omnia_p03_spike::pack;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
enum Case {
    OlibFile,
    OlibMemory,
    Folder,
}

impl Case {
    const ALL: [Case; 3] = [Case::OlibFile, Case::OlibMemory, Case::Folder];

    fn name(self) -> &'static str {
        match self {
            Case::OlibFile => "olib-file",
            Case::OlibMemory => "olib-memory",
            Case::Folder => "folder",
        }
    }
}

struct Args {
    work: PathBuf,
    out: PathBuf,
    parts: usize,
    reps: usize,
    iters: usize,
    warmup: usize,
}

fn args() -> Args {
    let mut a = Args {
        work: PathBuf::from("target/p03-work"),
        out: PathBuf::from("results/index"),
        parts: 500,
        reps: 10,
        iters: 200,
        warmup: 20,
    };
    let v: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < v.len() {
        let val = v.get(i + 1).cloned().unwrap_or_default();
        match v[i].as_str() {
            "--work" => a.work = PathBuf::from(val),
            "--out" => a.out = PathBuf::from(val),
            "--parts" => a.parts = val.parse().expect("--parts"),
            "--reps" => a.reps = val.parse().expect("--reps"),
            "--iters" => a.iters = val.parse().expect("--iters"),
            "--warmup" => a.warmup = val.parse().expect("--warmup"),
            other => panic!("unknown argument {other}"),
        }
        i += 2;
    }
    a
}

fn run_case(case: Case, olib: &Path, bytes: &[u8], dir: &Path) -> Index {
    match case {
        Case::OlibFile => index::index_olib_file(olib),
        Case::OlibMemory => index::index_olib(Cursor::new(bytes)),
        Case::Folder => index::index_dir(dir),
    }
    .expect("index")
}

/// Nearest-rank percentile of sorted samples.
fn pct(sorted: &[u64], p: f64) -> u64 {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

fn ms(ns: u64) -> f64 {
    ns as f64 / 1e6
}

fn median(v: &mut [u64]) -> u64 {
    v.sort_unstable();
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2
    }
}

fn main() {
    let a = args();
    std::fs::create_dir_all(&a.work).expect("work dir");
    let olib = a.work.join("spike-pack.olib");
    let dir = a.work.join("spike-pack");
    let generated = pack::pack(a.parts);
    pack::write_olib(&generated, &olib).expect("write olib");
    pack::write_dir(&generated, &dir).expect("write dir");
    let bytes = std::fs::read(&olib).expect("read olib");
    let sha = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let uncompressed: usize = generated.files.iter().map(|f| f.1.len()).sum();
    let part_files = generated
        .files
        .iter()
        .filter(|f| f.0.starts_with("parts/"))
        .count();
    let part_bytes: usize = generated
        .files
        .iter()
        .filter(|f| f.0.starts_with("parts/"))
        .map(|f| f.1.len())
        .sum();

    // First calls (file cache warm from generation, process caches cold), then a correctness check.
    let mut first_call = Vec::new();
    let mut reference: Option<Index> = None;
    for c in Case::ALL {
        let t = Instant::now();
        let ix = run_case(c, &olib, &bytes, &dir);
        first_call.push((c.name(), t.elapsed().as_nanos() as u64));
        assert_eq!(
            ix.entries.len(),
            a.parts,
            "{} indexed {} parts",
            c.name(),
            ix.entries.len()
        );
        if let Some(r) = &reference {
            assert_eq!(r.entries, ix.entries, "{} differs from olib-file", c.name());
        } else {
            reference = Some(ix);
        }
    }

    let mut csv = String::from("rep,case,iter,ns\n");
    let mut order_log = String::new();
    // run p99s per case, all samples per case
    let mut p99s: Vec<Vec<u64>> = vec![Vec::new(); Case::ALL.len()];
    let mut all: Vec<Vec<u64>> = vec![Vec::new(); Case::ALL.len()];
    let mut per_run = String::new();
    for rep in 0..a.reps {
        let order: Vec<usize> = (0..Case::ALL.len())
            .map(|k| (k + rep) % Case::ALL.len())
            .collect();
        let _ = writeln!(
            order_log,
            "rep {rep}: {}",
            order
                .iter()
                .map(|&k| Case::ALL[k].name())
                .collect::<Vec<_>>()
                .join(" ")
        );
        for &k in &order {
            let c = Case::ALL[k];
            for _ in 0..a.warmup {
                std::hint::black_box(run_case(c, &olib, &bytes, &dir));
            }
            let mut s = Vec::with_capacity(a.iters);
            for it in 0..a.iters {
                let t = Instant::now();
                let ix = run_case(c, &olib, &bytes, &dir);
                let ns = t.elapsed().as_nanos() as u64;
                std::hint::black_box(ix);
                s.push(ns);
                let _ = writeln!(csv, "{rep},{},{it},{ns}", c.name());
            }
            all[k].extend_from_slice(&s);
            s.sort_unstable();
            let (p50, p99, max) = (pct(&s, 50.0), pct(&s, 99.0), *s.last().expect("samples"));
            p99s[k].push(p99);
            let _ = writeln!(
                per_run,
                "      {{\"rep\": {rep}, \"case\": \"{}\", \"p50_ms\": {:.4}, \"p99_ms\": {:.4}, \"max_ms\": {:.4}}},",
                c.name(),
                ms(p50),
                ms(p99),
                ms(max)
            );
        }
    }
    let per_run = per_run.trim_end().trim_end_matches(',').to_owned();

    let mut summary = String::new();
    let mut table = String::new();
    for (k, c) in Case::ALL.iter().enumerate() {
        let mut runs = p99s[k].clone();
        let med = median(&mut runs);
        let (lo, hi) = (runs[0], *runs.last().expect("runs"));
        let mut s = all[k].clone();
        s.sort_unstable();
        let _ = writeln!(
            summary,
            "    {{\"case\": \"{}\", \"median_run_p99_ms\": {:.4}, \"run_p99_min_ms\": {:.4}, \"run_p99_max_ms\": {:.4}, \"all_p50_ms\": {:.4}, \"all_p99_ms\": {:.4}, \"all_max_ms\": {:.4}, \"samples\": {}}}{}",
            c.name(),
            ms(med),
            ms(lo),
            ms(hi),
            ms(pct(&s, 50.0)),
            ms(pct(&s, 99.0)),
            ms(*s.last().expect("samples")),
            s.len(),
            if k + 1 < Case::ALL.len() { "," } else { "" }
        );
        let _ = writeln!(
            table,
            "{:<12} median run p99 {:>8.3} ms  range {:.3}-{:.3} ms  p50 {:.3} ms",
            c.name(),
            ms(med),
            ms(lo),
            ms(hi),
            ms(pct(&s, 50.0))
        );
    }
    let first = first_call
        .iter()
        .map(|(n, ns)| format!("\"{n}\": {:.4}", ms(*ns)))
        .collect::<Vec<_>>()
        .join(", ");
    let json = format!(
        "{{\n  \"spike\": \"PS1\",\n  \"budget\": \"PB3\",\n  \"parts\": {},\n  \"reps\": {},\n  \"iters\": {},\n  \"warmup\": {},\n  \
         \"olib_bytes\": {},\n  \"olib_sha256\": \"{sha}\",\n  \"entries\": {},\n  \"uncompressed_bytes\": {uncompressed},\n  \
         \"part_files\": {part_files},\n  \"part_bytes\": {part_bytes},\n  \"first_call_ms\": {{{first}}},\n  \"cases\": [\n{summary}  ],\n  \"runs\": [\n{per_run}\n  ]\n}}\n",
        a.parts,
        a.reps,
        a.iters,
        a.warmup,
        bytes.len(),
        generated.files.len(),
    );
    if let Some(p) = a.out.parent() {
        std::fs::create_dir_all(p).expect("out dir");
    }
    let stem = a.out.to_string_lossy().into_owned();
    std::fs::write(format!("{stem}.json"), json).expect("json");
    std::fs::write(format!("{stem}.csv"), csv).expect("csv");
    std::fs::write(format!("{stem}-order.txt"), order_log).expect("order");
    println!(
        "{} parts, {} entries, olib {} bytes (sha256 {sha})\nfirst calls: {first}\n{table}",
        a.parts,
        generated.files.len(),
        bytes.len()
    );
}
