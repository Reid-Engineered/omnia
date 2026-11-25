//! Runs the PS1 extractor over the 20 decks and the hostile set.
//!
//! Usage: `extract [<P0.3 dir> [<output dir>]]` (defaults: the parent of this crate, and a
//! fresh `results/runs/extract-<unix seconds>/`). Writes `<output dir>/extract.json` and one
//! `<output dir>/extracted/<deck>--<entry>.lib` per extraction, and refuses an output directory
//! that already holds a run.
//! Exits 1 if any expectation in `decks/expect.toml` fails.

use std::fmt::Write as _;
use std::path::PathBuf;

use omnia_p03_spike::decks;

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn list(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|s| esc(s)).collect::<Vec<_>>().join(", ")
    )
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."));
    let report = decks::run(&root);
    // Never write over the committed campaign in `results/` (review of PR #133): the output
    // goes to the directory given, or to a fresh `results/runs/extract-<unix seconds>/`.
    let out_dir = std::env::args()
        .nth(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_secs();
            root.join("results")
                .join("runs")
                .join(format!("extract-{secs}"))
        });
    assert!(
        !out_dir.join("extract.json").exists(),
        "{} already holds a run; choose a fresh output directory",
        out_dir.display()
    );
    let ex_dir = out_dir.join("extracted");
    std::fs::create_dir_all(&ex_dir).expect("results dir");

    let mut j = String::from("{\n  \"spike\": \"PS1\",\n  \"task\": \"P0.3\",\n");
    let _ = writeln!(j, "  \"decks\": {},", report.decks);
    let _ = writeln!(j, "  \"extractions\": {},", report.entries.len());
    let failures = report.failures();
    let _ = writeln!(j, "  \"failures\": {},", list(&failures));
    j.push_str("  \"deck_sha256\": {\n");
    let n = report.deck_sha256.len();
    for (i, (k, v)) in report.deck_sha256.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {}: {}{}",
            esc(k),
            esc(v),
            if i + 1 < n { "," } else { "" }
        );
    }
    j.push_str("  },\n  \"other_directives\": {\n");
    let n = report.other_directives.len();
    for (i, (k, v)) in report.other_directives.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {}: {}{}",
            esc(k),
            list(v),
            if i + 1 < n { "," } else { "" }
        );
    }
    j.push_str("  },\n  \"entries\": [\n");
    let n = report.entries.len();
    for (i, e) in report.entries.iter().enumerate() {
        let ports = e
            .ports
            .as_ref()
            .map(|p| list(p))
            .unwrap_or_else(|| "null".into());
        let _ = writeln!(
            j,
            "    {{\"deck\": {}, \"entry\": {}, \"kind\": {}, \"ports\": {ports}, \"port_count\": {}, \"blocks\": {}, \"lines\": {}, \"text_sha256\": {}, \"cold_micros\": {:.1}, \"pass\": {}, \"failures\": {}}}{}",
            esc(&e.deck),
            esc(&e.name),
            esc(&e.kind),
            e.ports
                .as_ref()
                .map(|p| p.len().to_string())
                .unwrap_or_else(|| "null".into()),
            list(&e.blocks),
            e.lines,
            esc(&e.text_sha256),
            e.micros,
            e.failures.is_empty(),
            list(&e.failures),
            if i + 1 < n { "," } else { "" }
        );
        let stem = e.deck.trim_end_matches(".lib");
        std::fs::write(ex_dir.join(format!("{stem}--{}.lib", e.name)), &e.text)
            .expect("write extraction");
    }
    j.push_str("  ],\n  \"hostile\": [\n");
    let n = report.hostile.len();
    for (i, h) in report.hostile.iter().enumerate() {
        let _ = writeln!(
            j,
            "    {{\"file\": {}, \"expected\": {}, \"got\": {}, \"pass\": {}, \"detail\": {}}}{}",
            esc(&h.file),
            esc(&h.expected),
            esc(&h.got),
            h.got == h.expected,
            esc(&h.detail),
            if i + 1 < n { "," } else { "" }
        );
    }
    let embedded = decks::load_embedded(&root, "17-shunt-ref-include.lib");
    let _ = writeln!(
        j,
        "  ],\n  \"raw_include_deck_in_embedded_mode\": {}\n}}",
        esc(&match embedded {
            Ok(()) => "accepted".to_owned(),
            Err(e) => e.code().to_owned(),
        })
    );
    std::fs::write(out_dir.join("extract.json"), j).expect("write extract.json");

    let passed = report
        .entries
        .iter()
        .filter(|e| e.failures.is_empty())
        .count();
    let hostile_ok = report
        .hostile
        .iter()
        .filter(|h| h.got == h.expected)
        .count();
    println!(
        "decks: {}  extractions: {passed}/{} pass  hostile: {hostile_ok}/{} refused as expected",
        report.decks,
        report.entries.len(),
        report.hostile.len()
    );
    for f in &failures {
        println!("FAIL {f}");
    }
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
