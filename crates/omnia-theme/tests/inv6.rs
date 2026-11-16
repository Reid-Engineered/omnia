//! INV6: `omnia-canvas` contains no colour literal; colours come from `omnia-theme`.
//!
//! Every file under `crates/omnia-canvas/src` is scanned as raw text, comments included. It
//! fails on a hex colour (`#` then 3, 4, 6 or 8 hex digits, as a whole word) and on a call to
//! `rgb`, `rgba` or `hsla` (the name as a whole word, including after `::`, then `(`).

use std::{fs, path::Path};

const CALLS: [&str; 3] = ["rgb", "rgba", "hsla"];

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Every colour literal in `text`, as `(line, excerpt)`.
fn colour_literals(text: &str) -> Vec<(usize, String)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let line_of = |i: usize| text[..i].bytes().filter(|&b| b == b'\n').count() + 1;
    for i in 0..bytes.len() {
        let after_word = i > 0 && is_word(bytes[i - 1]);
        if bytes[i] == b'#' && !after_word {
            let digits = bytes[i + 1..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            let end = i + 1 + digits;
            let whole = end == bytes.len() || !is_word(bytes[end]);
            if whole && matches!(digits, 3 | 4 | 6 | 8) {
                found.push((line_of(i), text[i..end].to_owned()));
            }
        }
        if !after_word {
            for call in CALLS {
                let Some(rest) = bytes[i..].strip_prefix(call.as_bytes()) else {
                    continue;
                };
                if rest.first().is_some_and(|&b| is_word(b)) {
                    continue;
                }
                if rest.trim_ascii_start().starts_with(b"(") {
                    found.push((line_of(i), format!("{call}(")));
                }
            }
        }
    }
    found
}

fn scan(dir: &Path, hits: &mut Vec<String>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            scan(&path, hits);
            continue;
        }
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let text = String::from_utf8_lossy(&bytes);
        for (line, literal) in colour_literals(&text) {
            hits.push(format!("{}:{line}: {literal}", path.display()));
        }
    }
}

#[test]
fn inv6_omnia_canvas_has_no_colour_literal() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../omnia-canvas/src");
    assert!(src.is_dir(), "{} is missing", src.display());
    let mut hits = Vec::new();
    scan(&src, &mut hits);
    assert!(
        hits.is_empty(),
        "INV6: colour literals in omnia-canvas; take colours from omnia-theme instead:\n{}",
        hits.join("\n")
    );
}

#[test]
fn detector_finds_planted_literals() {
    let planted = [
        "let c = \"#2F6FEB\";",
        "let c = \"#fff\";",
        "let c = \"#00000022\";",
        "// grid is #1d1d",
        "let c = rgb(0x2f6feb);",
        "let c = gpui::rgba(0x2f6febff);",
        "let c = hsla (0.5, 0.5, 0.5, 1.0);",
    ];
    for line in planted {
        assert_eq!(colour_literals(line).len(), 1, "missed: {line}");
    }
}

#[test]
fn detector_ignores_lookalikes() {
    let clean = [
        "#[derive(Clone)]",
        "#![allow(dead_code)]",
        "let s = r#\"text\"#;",
        "let n = #12345;",
        "let x = #abcdeg;",
        "fn to_rgb() {}",
        "let v = rgba_from(theme);",
        "let rgb = theme.stage_ink;",
        "use omnia_theme::Rgba;",
        "Rgba::new(r, g, b, a)",
    ];
    for line in clean {
        assert!(colour_literals(line).is_empty(), "false positive: {line}");
    }
}

#[test]
fn detector_handles_non_ascii_text() {
    assert!(colour_literals("// café — ünïcode").is_empty());
    assert_eq!(colour_literals("// é then rgb(1)").len(), 1);
}
