//! PS1 verification: every deck expectation, the hostile set, and the index prototype's
//! determinism and `PK3` refusals.

use std::io::{Cursor, Write};
use std::path::PathBuf;

use omnia_p03_spike::index::{self, IndexError};
use omnia_p03_spike::spice::{Deck, DefKind, Mode, Vfs};
use omnia_p03_spike::{decks, pack};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn ps1_twenty_decks_extract_as_expected() {
    let report = decks::run(&root());
    assert_eq!(report.decks, 20, "PS1 asks for 20 decks");
    let failures = report.failures();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn ps1_raw_deck_with_include_is_refused_in_embedded_mode() {
    let err = decks::load_embedded(&root(), "17-shunt-ref-include.lib").unwrap_err();
    assert_eq!(err.code(), "include_in_embedded");
}

#[test]
fn pd3_crlf_and_bom_are_normalised_and_nothing_else() {
    let report = decks::run(&root());
    let e = report
        .entries
        .iter()
        .find(|e| e.name == "PNP_GENERIC")
        .expect("entry");
    assert!(e.text.starts_with(".MODEL PNP_GENERIC PNP(IS=650.6E-18"));
    assert!(!e.text.contains('\r'));
    let tabs = report
        .entries
        .iter()
        .find(|e| e.name == "LED_RED")
        .expect("entry");
    assert!(
        tabs.text.contains(".model\tLED_RED\tD\tIS = 93.2e-21"),
        "tabs and spacing kept"
    );
    let shadow = report
        .entries
        .iter()
        .find(|e| e.name == "OPTO_GENERIC")
        .expect("entry");
    assert!(
        shadow
            .text
            .contains("; local model that shadows the top-level one"),
        "inline comments kept"
    );
}

#[test]
fn extraction_is_independent_of_vfs_insertion_order() {
    let dir = root().join("decks");
    let a = Vfs::from_dir(&dir).unwrap();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.reverse();
    let mut b = Vfs::new();
    for p in files.iter().filter(|p| p.is_file()) {
        b.insert(
            &p.file_name().unwrap().to_string_lossy(),
            std::fs::read(p).unwrap(),
        );
    }
    for sub in ["inc/ref-common.inc", "inc/corners.lib"] {
        b.insert(sub, a.get(sub).unwrap().to_vec());
    }
    for (file, entry) in [
        ("17-shunt-ref-include.lib", "SHUNT_REF_2V5"),
        ("19-opto-nested-shadow.lib", "OPTO_GENERIC"),
    ] {
        let x = Deck::load(&a, file, Mode::Archive)
            .unwrap()
            .extract(entry, DefKind::Subckt)
            .unwrap();
        let y = Deck::load(&b, file, Mode::Archive)
            .unwrap()
            .extract(entry, DefKind::Subckt)
            .unwrap();
        assert_eq!(x.text, y.text);
    }
}

#[test]
fn pk2_ids() {
    for ok in ["resistor", "a", "opamp-generic", "x.y_z-1", &"a".repeat(64)] {
        assert!(index::valid_id(ok), "{ok}");
    }
    for bad in ["", "Resistor", "-a", ".a", "a b", "a/b", &"a".repeat(65)] {
        assert!(!index::valid_id(bad), "{bad}");
    }
}

#[test]
fn pb3_index_reads_500_parts_sorted_and_same_for_zip_and_folder() {
    let generated = pack::pack(500);
    let work = std::env::temp_dir().join(format!("p03-spike-test-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let olib = work.join("pack.olib");
    pack::write_olib(&generated, &olib).unwrap();
    pack::write_dir(&generated, &work.join("dir")).unwrap();
    let z = index::index_olib_file(&olib).unwrap();
    let d = index::index_dir(&work.join("dir")).unwrap();
    assert_eq!(z.entries.len(), 500);
    assert_eq!(z.entries, d.entries);
    assert!(
        z.entries
            .windows(2)
            .all(|w| w[0].qualified_id < w[1].qualified_id)
    );
    // Byte-identical archive from the same inputs.
    let again = work.join("again.olib");
    pack::write_olib(&pack::pack(500), &again).unwrap();
    assert_eq!(
        std::fs::read(&olib).unwrap(),
        std::fs::read(&again).unwrap()
    );
    std::fs::remove_dir_all(&work).unwrap();
}

fn zip_with(entries: &[(&str, Vec<u8>, zip::CompressionMethod)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, m) in entries {
        let o = zip::write::SimpleFileOptions::default().compression_method(*m);
        w.start_file(*name, o).unwrap();
        w.write_all(bytes).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const MANIFEST: &[u8] = b"schema = 1\nid = \"p\"\nname = \"p\"\nversion = \"1\"\n";

#[test]
fn pk3_and_pd2_refusals_before_decompression() {
    use zip::CompressionMethod::{Deflated, Stored};
    let bomb = zip_with(&[
        ("manifest.toml", MANIFEST.to_vec(), Stored),
        ("models/z.lib", vec![b'0'; 1_500_000], Deflated),
    ]);
    assert!(matches!(
        index::index_olib(Cursor::new(bomb)),
        Err(IndexError::Ratio(_))
    ));
    let escape = zip_with(&[
        ("manifest.toml", MANIFEST.to_vec(), Stored),
        ("../escape.lib", b"x".to_vec(), Stored),
    ]);
    assert!(matches!(
        index::index_olib(Cursor::new(escape)),
        Err(IndexError::UnsafePath(_))
    ));
    let svg = zip_with(&[
        ("manifest.toml", MANIFEST.to_vec(), Stored),
        ("symbol.svg", b"<svg/>".to_vec(), Stored),
    ]);
    assert!(matches!(
        index::index_olib(Cursor::new(svg)),
        Err(IndexError::CustomSymbolUnsupported(_))
    ));
    let big = zip_with(&[
        ("manifest.toml", MANIFEST.to_vec(), Stored),
        ("models/big.lib", vec![b'*'; 2 * 1024 * 1024 + 1], Stored),
    ]);
    assert!(matches!(
        index::index_olib(Cursor::new(big)),
        Err(IndexError::EntryTooLarge(..))
    ));
    let many: Vec<(String, Vec<u8>)> = (0..1025)
        .map(|i| (format!("m/{i}.lib"), Vec::new()))
        .collect();
    let many_refs: Vec<(&str, Vec<u8>, zip::CompressionMethod)> = many
        .iter()
        .map(|(n, b)| (n.as_str(), b.clone(), Stored))
        .collect();
    assert!(matches!(
        index::index_olib(Cursor::new(zip_with(&many_refs))),
        Err(IndexError::TooManyEntries(1025))
    ));
}

fn extract_one(text: &str, entry: &str, kind: DefKind) -> omnia_p03_spike::spice::Extraction {
    let mut v = Vfs::new();
    v.insert("t.lib", text.as_bytes().to_vec());
    Deck::load(&v, "t.lib", Mode::Archive)
        .unwrap()
        .extract(entry, kind)
        .unwrap()
}

fn block_names(x: &omnia_p03_spike::spice::Extraction) -> Vec<String> {
    x.blocks
        .iter()
        .map(|b| format!("{}:{}", b.kind.as_str(), b.name))
        .collect()
}

/// Review of PR #133: an element named like the top-level model it uses keeps the model.
#[test]
fn same_name_element_and_model_keeps_dependency() {
    let x = extract_one(
        ".model DSAME D(IS=1e-14)\n.subckt CLAMP a k\nDSAME a k DSAME\n.ends CLAMP\n",
        "CLAMP",
        DefKind::Subckt,
    );
    assert_eq!(block_names(&x), ["model:DSAME", "subckt:CLAMP"]);
}

/// Review of PR #133: an `X` instance named like the top-level subcircuit it calls.
#[test]
fn same_name_instance_and_subckt_keeps_dependency() {
    let x = extract_one(
        ".subckt XAMP i o\nR1 i o 1k\n.ends XAMP\n.subckt TOP i o\nXAMP i o XAMP\n.ends TOP\n",
        "TOP",
        DefKind::Subckt,
    );
    assert_eq!(block_names(&x), ["subckt:XAMP", "subckt:TOP"]);
}

/// Review of PR #133: a `.model` using a top-level `.param` of the same name keeps it.
#[test]
fn same_name_model_and_param_keeps_dependency() {
    let x = extract_one(
        ".param VT=0.7\n.model VT NMOS(LEVEL=1 VTO={VT})\n",
        "VT",
        DefKind::Model,
    );
    assert_eq!(block_names(&x), ["param:VT", "model:VT"]);
}

/// Review of PR #133: a `.param` whose right-hand side uses a same-named outer value, and a
/// `.param` line that defines one name from another on the same line.
#[test]
fn param_right_hand_sides_are_references() {
    let x = extract_one(
        ".param BASE=2\n.param GAIN={BASE*2}\n.model QG NPN(BF={GAIN})\n",
        "QG",
        DefKind::Model,
    );
    assert_eq!(block_names(&x), ["param:BASE", "param:GAIN", "model:QG"]);
}

/// Review of PR #133: an unsafe include in an unselected `.lib` section is still refused.
#[test]
fn pk10_unsafe_include_in_unselected_section_is_refused() {
    let mut v = Vfs::new();
    v.insert("top.lib", b".lib \"c.lib\" TT\n".to_vec());
    v.insert(
        "c.lib",
        b".lib TT\n.model D1 D\n.endl TT\n.lib FF\n.include \"../x.lib\"\n.endl FF\n".to_vec(),
    );
    let err = Deck::load(&v, "top.lib", Mode::Archive).unwrap_err();
    assert_eq!(err.code(), "include_unsafe_path");
}

/// Re-check of PR #133: a comparison inside a `.param` expression is not an assignment, so the
/// compared name stays a dependency and no bogus parameter is defined.
#[test]
fn param_comparisons_are_references_not_assignments() {
    for op in [">=", "<=", "==", "!="] {
        for gain in [
            format!("{{ LIMIT{op}1 }}"),
            format!("{{ LIMIT {op} 1 }}"),
            format!("LIMIT{op}1"),
        ] {
            let text =
                format!(".param LIMIT=2\n.param GAIN = {gain}\n.model QG NPN(BF={{GAIN}})\n");
            let x = extract_one(&text, "QG", DefKind::Model);
            assert_eq!(
                block_names(&x),
                ["param:LIMIT", "param:GAIN", "model:QG"],
                "{text}"
            );
            let mut v = Vfs::new();
            v.insert("t.lib", text.as_bytes().to_vec());
            let deck = Deck::load(&v, "t.lib", Mode::Archive).unwrap();
            assert_eq!(deck.top_level(DefKind::Param), ["LIMIT", "GAIN"], "{text}");
        }
    }
}

/// Header `PARAMS:` defaults with comparisons define only the assigned names.
#[test]
fn subckt_header_params_ignore_comparisons() {
    let x = extract_one(
        ".param LIMIT=2\n.subckt S a b PARAMS: G={LIMIT>=1} H = { LIMIT != 3 }\nR1 a b {G+H}\n.ends S\n",
        "S",
        DefKind::Subckt,
    );
    assert_eq!(
        x.ports.as_deref(),
        Some(&["a".to_owned(), "b".to_owned()][..])
    );
    assert_eq!(block_names(&x), ["param:LIMIT", "subckt:S"]);
}
