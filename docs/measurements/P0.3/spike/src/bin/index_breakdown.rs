//! Diagnostic only (not part of the fixed PB3 schedule): where a 500-part index spends time.
//!
//! Usage: `index_breakdown --work <dir> --out <file.json>`; expects `index_bench` to have
//! written `<work>/spike-pack.olib` and `<work>/spike-pack/`. Phases, each 5 reps x 50 iters:
//! - `zip-open`: `ZipArchive::new` over the in-memory archive (central directory only);
//! - `zip-inflate-parts`: open plus inflate every `parts/**.toml` entry, no parsing;
//! - `toml-parse`: parse 500 pre-read `part.toml` strings (serde + `PK1`/`PK2`/`PK7` checks);
//! - `folder-read`: walk `parts/` and read every file, no parsing;
//! - `folder-open-only`: open and close every part file without reading it.

use std::fmt::Write as _;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use omnia_p03_spike::index;

type Phase<'a> = (&'static str, Box<dyn Fn() + 'a>);

fn part_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("parts")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).expect("read_dir") {
            let e = e.expect("entry");
            if e.file_type().expect("type").is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

fn pct(sorted: &[u64], p: f64) -> u64 {
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

fn main() {
    let v: Vec<String> = std::env::args().collect();
    let arg = |k: &str| {
        v.iter()
            .position(|a| a == k)
            .map(|i| PathBuf::from(&v[i + 1]))
            .expect(k)
    };
    let work = arg("--work");
    let out = arg("--out");
    let bytes = std::fs::read(work.join("spike-pack.olib")).expect("olib");
    let dir = work.join("spike-pack");
    let files = part_files(&dir);
    let texts: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(&dir)
                .expect("rel")
                .to_string_lossy()
                .replace('\\', "/");
            (rel, std::fs::read_to_string(p).expect("read"))
        })
        .collect();

    let phases: Vec<Phase> = vec![
        (
            "zip-open",
            Box::new(|| {
                std::hint::black_box(zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("zip"));
            }),
        ),
        (
            "zip-inflate-parts",
            Box::new(|| {
                let mut za = zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("zip");
                let mut buf = Vec::new();
                for i in 0..za.len() {
                    let mut f = za.by_index(i).expect("entry");
                    if f.name().expect("name").starts_with("parts/") {
                        buf.clear();
                        f.read_to_end(&mut buf).expect("inflate");
                    }
                }
                std::hint::black_box(&buf);
            }),
        ),
        (
            "toml-parse",
            Box::new(|| {
                for (rel, t) in &texts {
                    std::hint::black_box(index::parse_part("spike-pack", rel, t).expect("parse"));
                }
            }),
        ),
        (
            "folder-read",
            Box::new(|| {
                for p in part_files(&dir) {
                    std::hint::black_box(std::fs::read(p).expect("read"));
                }
            }),
        ),
        (
            "folder-open-only",
            Box::new(|| {
                for p in &files {
                    std::hint::black_box(std::fs::File::open(p).expect("open"));
                }
            }),
        ),
    ];

    let mut json = String::from(
        "{\n  \"diagnostic\": true,\n  \"reps\": 5,\n  \"iters\": 50,\n  \"phases\": [\n",
    );
    let n = phases.len();
    for (k, (name, f)) in phases.iter().enumerate() {
        for _ in 0..5 {
            f();
        }
        let mut s = Vec::new();
        for _ in 0..5 {
            for _ in 0..50 {
                let t = Instant::now();
                f();
                s.push(t.elapsed().as_nanos() as u64);
            }
        }
        s.sort_unstable();
        let (p50, p99) = (pct(&s, 50.0) as f64 / 1e6, pct(&s, 99.0) as f64 / 1e6);
        println!("{name:<18} p50 {p50:>8.3} ms  p99 {p99:>8.3} ms");
        let _ = writeln!(
            json,
            "    {{\"phase\": \"{name}\", \"p50_ms\": {p50:.4}, \"p99_ms\": {p99:.4}, \"samples\": {}}}{}",
            s.len(),
            if k + 1 < n { "," } else { "" }
        );
    }
    json.push_str("  ]\n}\n");
    std::fs::write(out, json).expect("write");
}
