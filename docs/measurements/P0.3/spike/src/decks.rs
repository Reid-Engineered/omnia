//! Runs the extractor over `decks/expect.toml` and checks every expectation.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::spice::{Deck, DefKind, Mode, SpiceError, Vfs, screen_embedded};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    pub deck: Vec<DeckExpect>,
    pub hostile: Vec<HostileExpect>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckExpect {
    pub file: String,
    pub constructs: Vec<String>,
    pub entry: Vec<EntryExpect>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryExpect {
    pub name: String,
    pub kind: String,
    pub ports: Option<Vec<String>>,
    pub blocks: Vec<String>,
    pub lines: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostileExpect {
    pub file: String,
    pub code: String,
    pub entry: Option<String>,
}

#[derive(Debug)]
pub struct EntryResult {
    pub deck: String,
    pub name: String,
    pub kind: String,
    pub ports: Option<Vec<String>>,
    pub blocks: Vec<String>,
    pub lines: usize,
    pub text: String,
    pub text_sha256: String,
    /// Load + flatten + parse + extract, microseconds (one cold call, diagnostic only).
    pub micros: f64,
    pub failures: Vec<String>,
}

#[derive(Debug)]
pub struct HostileResult {
    pub file: String,
    pub expected: String,
    pub got: String,
    pub detail: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub entries: Vec<EntryResult>,
    pub hostile: Vec<HostileResult>,
    pub deck_sha256: BTreeMap<String, String>,
    pub other_directives: BTreeMap<String, Vec<String>>,
    pub decks: usize,
}

impl Report {
    pub fn failures(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.entries {
            for f in &e.failures {
                out.push(format!("{} {}: {f}", e.deck, e.name));
            }
        }
        for h in &self.hostile {
            if h.got != h.expected {
                out.push(format!(
                    "{}: expected {}, got {} ({})",
                    h.file, h.expected, h.got, h.detail
                ));
            }
        }
        out
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn kind(s: &str) -> DefKind {
    match s {
        "model" => DefKind::Model,
        "subckt" => DefKind::Subckt,
        other => panic!("unknown entry kind {other}"),
    }
}

/// Runs every deck and hostile fixture. `root` is `docs/measurements/P0.3`.
pub fn run(root: &Path) -> Report {
    let decks_dir = root.join("decks");
    let expect: Expect = toml::from_str(
        &std::fs::read_to_string(decks_dir.join("expect.toml")).expect("expect.toml"),
    )
    .expect("parse");
    let vfs = Vfs::from_dir(&decks_dir).expect("decks");
    let mut report = Report {
        decks: expect.deck.len(),
        ..Report::default()
    };
    for d in &expect.deck {
        let bytes = vfs.get(&d.file).expect("deck file");
        report
            .deck_sha256
            .insert(d.file.clone(), hex(&Sha256::digest(bytes)));
        for e in &d.entry {
            let k = kind(&e.kind);
            let t0 = Instant::now();
            let res = Deck::load(&vfs, &d.file, Mode::Archive).and_then(|deck| {
                let x = deck.extract(&e.name, k)?;
                Ok((deck, x))
            });
            let micros = t0.elapsed().as_secs_f64() * 1e6;
            let mut r = EntryResult {
                deck: d.file.clone(),
                name: e.name.clone(),
                kind: e.kind.clone(),
                ports: None,
                blocks: Vec::new(),
                lines: 0,
                text: String::new(),
                text_sha256: String::new(),
                micros,
                failures: Vec::new(),
            };
            match res {
                Err(err) => r.failures.push(format!("refused: {err}")),
                Ok((deck, x)) => {
                    let dirs: Vec<String> = deck.other_directives.iter().cloned().collect();
                    if !dirs.is_empty() {
                        report.other_directives.insert(d.file.clone(), dirs);
                    }
                    r.ports = x.ports.clone();
                    r.blocks = x
                        .blocks
                        .iter()
                        .map(|b| format!("{}:{}", b.kind.as_str(), b.name))
                        .collect();
                    r.lines = x.text.lines().count();
                    r.text_sha256 = hex(&Sha256::digest(x.text.as_bytes()));
                    if r.blocks != e.blocks {
                        r.failures
                            .push(format!("blocks {:?} != expected {:?}", r.blocks, e.blocks));
                    }
                    if r.lines != e.lines {
                        r.failures
                            .push(format!("lines {} != expected {}", r.lines, e.lines));
                    }
                    if r.ports != e.ports {
                        r.failures
                            .push(format!("ports {:?} != expected {:?}", r.ports, e.ports));
                    }
                    check_invariants(&x.text, &e.name, k, &mut r.failures);
                    r.text = x.text;
                }
            }
            report.entries.push(r);
        }
    }
    let hostile_vfs = Vfs::from_dir(&root.join("hostile")).expect("hostile");
    for h in &expect.hostile {
        let res =
            Deck::load(&hostile_vfs, &h.file, Mode::Archive).and_then(|deck| match &h.entry {
                Some(e) => deck.extract(e, DefKind::Subckt).map(|_| ()),
                None => Ok(()),
            });
        let (got, detail) = match res {
            Ok(()) => ("accepted".to_owned(), String::new()),
            Err(e) => (e.code().to_owned(), e.to_string()),
        };
        report.hostile.push(HostileResult {
            file: h.file.clone(),
            expected: h.code.clone(),
            got,
            detail,
        });
    }
    report
}

/// Checks that hold for every extraction:
/// - no CR and no byte-order mark survive (`PD3` normalisation);
/// - the text passes the embedded `PK10` screen (it is already flattened);
/// - re-extracting the entry from the extracted text alone, in embedded mode, gives the same
///   text byte for byte (the block is self-contained and extraction is a fixpoint).
fn check_invariants(text: &str, entry: &str, k: DefKind, failures: &mut Vec<String>) {
    if text.contains('\r') || text.contains('\u{feff}') {
        failures.push("CR or byte-order mark in output".into());
    }
    if let Err(e) = screen_embedded(text) {
        failures.push(format!("embedded screen refused output: {e}"));
    }
    let mut v = Vfs::new();
    v.insert("embedded.lib", text.as_bytes().to_vec());
    match Deck::load(&v, "embedded.lib", Mode::Embedded).and_then(|d| d.extract(entry, k)) {
        Ok(again) if again.text == text => {}
        Ok(_) => failures.push("re-extraction changed the text".into()),
        Err(e) => failures.push(format!("re-extraction refused: {e}")),
    }
}

/// Loads a deck in embedded mode (used to show that raw library decks with includes are refused).
pub fn load_embedded(root: &Path, file: &str) -> Result<(), SpiceError> {
    let vfs = Vfs::from_dir(&root.join("decks")).expect("decks");
    let bytes = vfs.get(file).expect("file").to_vec();
    let mut v = Vfs::new();
    v.insert(file, bytes);
    Deck::load(&v, file, Mode::Embedded).map(|_| ())
}
