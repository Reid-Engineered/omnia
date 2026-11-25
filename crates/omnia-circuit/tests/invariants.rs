//! C1.2 invariant tests: CINV1 (dependency boundaries and read-only access) and CINV10
//! (disallowed hash-map iteration and insertion-order determinism).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::Arc,
};

use omnia_circuit::{Circuit, CircuitKey, EvidenceTier, NetKind};
use omnia_doc::{Command, Doc, ItemId, Label, Orient, Pt, Wire};
use omnia_symbols::SymbolLibrary;
use proptest::prelude::*;

const STEP: i32 = 2_540_000;

fn cargo_tree(package: &str) -> String {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("workspace root");
    let output = ProcessCommand::new(env!("CARGO"))
        .current_dir(workspace)
        .args([
            "tree",
            "--locked",
            "--offline",
            "--all-features",
            "--target",
            "all",
            "--edges",
            "normal,build,dev",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "-p",
            package,
        ])
        .output()
        .expect("run cargo tree for CINV1");
    assert!(
        output.status.success(),
        "cargo tree failed for {package}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8 cargo tree output")
}

/// CINV1: `omnia-circuit` does not depend on GPUI, an async runtime, `omnia-agent`, `omnia-parts`,
/// `omnia-sim`, `omnia-canvas` or `omnia-app`; and `omnia-doc` does not depend on `omnia-circuit`.
#[test]
fn cinv1_dependency_boundaries() {
    let circuit_tree = cargo_tree("omnia-circuit");
    let forbidden_prefixes = ["gpui_", "tokio", "async-std", "smol", "futures"];
    let forbidden_exact = [
        "gpui",
        "omnia-agent",
        "omnia-parts",
        "omnia-sim",
        "omnia-canvas",
        "omnia-app",
    ];
    for line in circuit_tree.lines() {
        let name = line.split_whitespace().next().unwrap_or_default();
        assert!(
            !forbidden_exact.contains(&name)
                && !forbidden_prefixes.iter().any(|p| name.starts_with(p)),
            "CINV1 violation: omnia-circuit depends on {line}"
        );
    }

    let doc_tree = cargo_tree("omnia-doc");
    for line in doc_tree.lines() {
        let name = line.split_whitespace().next().unwrap_or_default();
        assert_ne!(
            name, "omnia-circuit",
            "CINV1 violation: omnia-doc depends on {line}"
        );
    }
}

fn clippy_driver_path() -> PathBuf {
    let out = ProcessCommand::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .expect("query rustc sysroot for clippy-driver");
    assert!(out.status.success(), "rustc --print sysroot failed");
    let sysroot = String::from_utf8(out.stdout)
        .expect("UTF-8 sysroot")
        .trim()
        .to_owned();
    let exe = if cfg!(windows) {
        "clippy-driver.exe"
    } else {
        "clippy-driver"
    };
    let path = PathBuf::from(sysroot).join("bin").join(exe);
    assert!(
        path.is_file(),
        "clippy-driver not found at {}",
        path.display()
    );
    path
}

fn cargo_clippy_deny_flags(manifest_dir: &Path) -> Vec<String> {
    let cargo_toml = std::fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .expect("Cargo.toml must exist in omnia-circuit");
    let mut in_clippy_lints = false;
    let mut flags = Vec::new();
    for raw_line in cargo_toml.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_clippy_lints = line == "[lints.clippy]";
            continue;
        }
        if in_clippy_lints && let Some((name, val)) = line.split_once('=') {
            let name = name.trim();
            let val = val.trim().trim_matches('"');
            if val == "deny" {
                flags.push("-D".to_owned());
                flags.push(format!("clippy::{name}"));
            }
        }
    }
    assert!(
        !flags.is_empty(),
        "CINV10: [lints.clippy] in Cargo.toml must declare deny lints"
    );
    flags
}

/// CINV10: `crates/omnia-circuit/clippy.toml` and `[lints.clippy]` in `Cargo.toml` reject every
/// `HashMap`/`HashSet` traversal path (including `iter()`, `for`, `.into_iter()`, `std::iter::zip`,
/// and generic `IntoIterator` helpers) while allowing ordered `Vec`/`BTreeMap`/`BTreeSet`
/// `.into_iter()` and `for` loops, verified with positive and negative controls under Cargo's
/// lint configuration.
#[test]
fn cinv10_clippy_rejects_all_hash_iteration_paths() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let clippy_toml = std::fs::read_to_string(manifest_dir.join("clippy.toml"))
        .expect("clippy.toml must exist in omnia-circuit");
    for required in [
        "disallowed-methods",
        "disallowed-types",
        "std::collections::HashMap\"",
        "std::collections::HashSet\"",
        "std::collections::HashMap::iter",
        "std::collections::HashMap::keys",
        "std::collections::HashMap::values",
        "std::collections::HashMap::drain",
        "std::collections::hash_map::IntoIter",
    ] {
        assert!(
            clippy_toml.contains(required),
            "CINV10: clippy.toml missing required entry `{required}`"
        );
    }

    let deny_flags = cargo_clippy_deny_flags(manifest_dir);
    for expected in [
        "clippy::iter_over_hash_type",
        "clippy::disallowed_methods",
        "clippy::disallowed_types",
    ] {
        assert!(
            deny_flags.iter().any(|f| f == expected),
            "CINV10: Cargo.toml [lints.clippy] missing `{expected}`"
        );
    }

    let driver = clippy_driver_path();
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir().join(format!("omnia-circuit-cinv10-{pid}"));
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir for CINV10 probe");

    let run_probe = |name: &str, body: &str| {
        let src = tmp_dir.join(format!("{name}.rs"));
        let rlib = tmp_dir.join(format!("{name}.rlib"));
        std::fs::write(&src, body).expect("write CINV10 probe source");
        let output = ProcessCommand::new(&driver)
            .current_dir(manifest_dir)
            .args(["--crate-type", "lib", "--edition", "2024", "-D", "warnings"])
            .args(&deny_flags)
            .arg("-o")
            .arg(&rlib)
            .arg(&src)
            .output()
            .expect("invoke clippy-driver for CINV10 probe");
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        (output.status.success(), stderr)
    };

    // Positive control: ordered collections (`Vec`, `BTreeMap`, `BTreeSet`) and `.into_iter()`
    // pass cleanly under the exact same Cargo lint configuration and `clippy.toml`.
    let (ok, ok_stderr) = run_probe(
        "ordered_positive_control",
        "use std::collections::{BTreeMap, BTreeSet};\n\
         pub fn ordered_ok(v: Vec<u32>, m: BTreeMap<u32, u32>, s: BTreeSet<u32>) -> usize {\n\
             let a: Vec<u32> = v.into_iter().collect();\n\
             let b: Vec<(u32, u32)> = m.into_iter().collect();\n\
             let c: Vec<u32> = s.into_iter().collect();\n\
             let mut n = a.len() + b.len() + c.len();\n\
             for x in &a { n += *x as usize; }\n\
             n\n\
         }\n",
    );
    assert!(
        ok,
        "CINV10 positive control failed under Cargo lints + clippy.toml:\n{ok_stderr}"
    );

    // Negative controls: every unordered traversal path must be rejected specifically by Clippy's
    // CINV10 lints (never by an unrelated rustc error).
    let cases: &[(&str, &str, &str)] = &[
        (
            "map_iter",
            "use std::collections::HashMap; pub fn f(m: &HashMap<u32, u32>) -> usize { m.iter().count() }\n",
            "disallowed_methods",
        ),
        (
            "map_for_ref",
            "use std::collections::HashMap; pub fn f(m: &HashMap<u32, u32>) -> usize { let mut n = 0; for _ in m { n += 1; } n }\n",
            "iter_over_hash_type",
        ),
        (
            "map_for_val",
            "use std::collections::HashMap; pub fn f(m: HashMap<u32, u32>) -> usize { let mut n = 0; for _ in m { n += 1; } n }\n",
            "iter_over_hash_type",
        ),
        (
            "map_into_iter",
            "use std::collections::HashMap; pub fn f(m: HashMap<u32, u32>) -> Vec<(u32, u32)> { m.into_iter().collect() }\n",
            "disallowed_types",
        ),
        (
            "map_zip",
            "use std::collections::HashMap; pub fn f(m: HashMap<u32, u32>) -> Vec<((u32, u32), usize)> { std::iter::zip(m, 0usize..).collect() }\n",
            "disallowed_types",
        ),
        (
            "map_generic_into_iter",
            "use std::collections::HashMap;\n\
             fn consume<I: IntoIterator>(it: I) -> usize { let mut n = 0; for _ in it { n += 1; } n }\n\
             pub fn f(m: HashMap<u32, u32>) -> usize { consume(m) }\n",
            "disallowed_types",
        ),
        (
            "set_for_ref",
            "use std::collections::HashSet; pub fn f(s: &HashSet<u32>) -> usize { let mut n = 0; for _ in s { n += 1; } n }\n",
            "iter_over_hash_type",
        ),
        (
            "set_into_iter",
            "use std::collections::HashSet; pub fn f(s: HashSet<u32>) -> Vec<u32> { s.into_iter().collect() }\n",
            "disallowed_types",
        ),
        (
            "set_zip",
            "use std::collections::HashSet; pub fn f(s: HashSet<u32>) -> Vec<(u32, usize)> { std::iter::zip(s, 0usize..).collect() }\n",
            "disallowed_types",
        ),
    ];

    for (name, body, expected_lint) in cases {
        let (succeeded, stderr) = run_probe(name, body);
        assert!(
            !succeeded,
            "CINV10 negative control `{name}` was unexpectedly accepted by Clippy"
        );
        assert!(
            !stderr.contains("error[E"),
            "CINV10 negative control `{name}` failed with a rustc compile error instead of Clippy:\n{stderr}"
        );
        assert!(
            stderr.contains(expected_lint),
            "CINV10 negative control `{name}` did not emit expected lint `{expected_lint}`:\n{stderr}"
        );
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[derive(Debug, PartialEq, Eq)]
struct NormalizedNet {
    /// Key represented via original spec indices (`part_spec`, `wire_spec`) so id remapping is
    /// verified explicitly even when parts share a designator (CINV3, CINV10).
    key_tag: String,
    display_name: String,
    candidates: Vec<String>,
    kind: NetKind,
    evidence: EvidenceTier,
    is_ground_ref: bool,
    is_hub: bool,
    /// Member pins represented as `(part_spec_index, pin_index)`.
    pins: Vec<(usize, u32)>,
    /// Member labels represented as `(label_spec_index, text, x, y)`.
    labels: Vec<(usize, String, i32, i32)>,
}

struct IdRemap {
    part_to_spec: BTreeMap<u32, usize>,
    wire_to_spec: BTreeMap<u32, usize>,
    label_to_spec: BTreeMap<u32, usize>,
}

fn normalized_nets(circuit: &Circuit, doc: &Doc, remap: &IdRemap) -> Vec<NormalizedNet> {
    circuit
        .net_rows()
        .map(|row| {
            let key_tag = match circuit.net_key(row) {
                CircuitKey::Label(text) => format!("label:{text}"),
                CircuitKey::Pin { part, pin } => {
                    let spec_idx = remap.part_to_spec[&part.0];
                    format!("pin:spec{spec_idx}.{pin}")
                }
                CircuitKey::Wire(wire) => {
                    let spec_idx = remap.wire_to_spec[&wire.0];
                    format!("wire:spec{spec_idx}")
                }
            };
            let pins = circuit
                .net_pins(row)
                .iter()
                .map(|&(part, pin_idx)| (remap.part_to_spec[&part.0], pin_idx))
                .collect();
            let labels = circuit
                .net_labels(row)
                .iter()
                .map(|&lid| {
                    let l = doc.label(lid).unwrap();
                    (
                        remap.label_to_spec[&lid.0],
                        l.text.to_string(),
                        l.at.x,
                        l.at.y,
                    )
                })
                .collect();
            NormalizedNet {
                key_tag,
                display_name: match circuit.net_key(row) {
                    CircuitKey::Wire(_) => {
                        let raw_name = circuit.net_display_name(row);
                        let id_str = raw_name
                            .strip_prefix("N(wire:")
                            .and_then(|s| s.strip_suffix(')'))
                            .unwrap_or_else(|| {
                                panic!("wire net display name `{raw_name}` must match N(wire:<id>)")
                            });
                        let raw_id: u32 = id_str.parse().expect("numeric wire id in N(wire:<id>)");
                        format!("N(wire:spec{})", remap.wire_to_spec[&raw_id])
                    }
                    _ => circuit.net_display_name(row).to_owned(),
                },
                candidates: circuit
                    .net_candidate_names(row)
                    .map(str::to_owned)
                    .collect(),
                kind: circuit.net_kind(row),
                evidence: circuit.net_kind_evidence(row),
                is_ground_ref: circuit.net_is_ground_reference(row),
                is_hub: circuit.net_is_hub(row),
                pins,
                labels,
            }
        })
        .collect()
}

fn build_permuted_sheet(
    symbols: &SymbolLibrary,
    part_order: &[usize],
    wire_order: &[usize],
    wire_flips: &[bool],
    label_order: &[usize],
) -> (Doc, Circuit, IdRemap) {
    let mut doc = Doc::new();
    // Includes duplicate-designator parts ("R1" at (0, 0) and (0, 4*STEP)) joined on their equal
    // pin number "1" so geometry decides after designator and pin number tie, plus natural-order
    // designators ("R2", "R10") and a ground symbol.
    let part_specs = [
        (
            "Resistor-IEEE-Standard",
            Pt { x: 0, y: 0 },
            Orient::IDENTITY,
            "R1",
            "10k",
        ),
        (
            "Resistor-IEEE-Standard",
            Pt { x: 0, y: 4 * STEP },
            Orient::IDENTITY,
            "R1",
            "4k7",
        ),
        (
            "Resistor-IEEE-Standard",
            Pt { x: 6 * STEP, y: 0 },
            Orient::IDENTITY,
            "R2",
            "2k2",
        ),
        (
            "Resistor-IEEE-Standard",
            Pt { x: 12 * STEP, y: 0 },
            Orient::IDENTITY,
            "R10",
            "1k",
        ),
        (
            "Ground-COM-General",
            Pt { x: 14 * STEP, y: 0 },
            Orient::IDENTITY,
            "",
            "",
        ),
    ];
    let wire_specs = [
        // Duplicate-designator R1[0].1 (-2*STEP, 0) to R1[1].1 (-2*STEP, 4*STEP): equal
        // designator AND equal pin number "1", so geometry (y=0 < y=4*STEP) decides the anchor.
        Wire {
            a: Pt { x: -2 * STEP, y: 0 },
            b: Pt {
                x: -2 * STEP,
                y: 4 * STEP,
            },
        },
        // R1[0].2 (2*STEP, 0) to R2.1 (4*STEP, 0)
        Wire {
            a: Pt { x: 2 * STEP, y: 0 },
            b: Pt { x: 4 * STEP, y: 0 },
        },
        // R2.2 (8*STEP, 0) to R10.1 (10*STEP, 0)
        Wire {
            a: Pt { x: 8 * STEP, y: 0 },
            b: Pt { x: 10 * STEP, y: 0 },
        },
        // Bare wire island (two perpendicular wires)
        Wire {
            a: Pt { x: 0, y: 10 * STEP },
            b: Pt {
                x: STEP,
                y: 10 * STEP,
            },
        },
        Wire {
            a: Pt { x: 0, y: 10 * STEP },
            b: Pt { x: 0, y: 11 * STEP },
        },
    ];
    let label_specs = [
        (Pt { x: 8 * STEP, y: 0 }, "VCC"),
        (Pt { x: 10 * STEP, y: 0 }, "V3V3"),
        (Pt { x: 0, y: 20 * STEP }, "SOLO_B"),
        (
            Pt {
                x: 2 * STEP,
                y: 20 * STEP,
            },
            "SOLO_A",
        ),
    ];

    let mut part_to_spec = BTreeMap::new();
    for &idx in part_order {
        let (sym_name, at, orient, des, val) = part_specs[idx];
        let sym = symbols.by_name(sym_name).unwrap();
        let change = doc
            .apply(symbols.place_command(sym, at, orient, Arc::from(des), Arc::from(val)))
            .unwrap();
        let [ItemId::Part(pid)] = change.added[..] else {
            panic!("expected one placed part");
        };
        part_to_spec.insert(pid.0, idx);
    }

    let mut wire_to_spec = BTreeMap::new();
    for &idx in wire_order {
        let mut w = wire_specs[idx];
        if wire_flips[idx] {
            w = Wire { a: w.b, b: w.a };
        }
        let change = doc.apply(Command::AddWires(vec![w])).unwrap();
        let [ItemId::Wire(wid)] = change.added[..] else {
            panic!("expected one added wire");
        };
        wire_to_spec.insert(wid.0, idx);
    }

    let mut label_to_spec = BTreeMap::new();
    for &idx in label_order {
        let (at, text) = label_specs[idx];
        let change = doc
            .apply(Command::AddLabel(Label {
                at,
                text: Arc::from(text),
            }))
            .unwrap();
        let [ItemId::Label(lid)] = change.added[..] else {
            panic!("expected one added label");
        };
        label_to_spec.insert(lid.0, idx);
    }

    let circuit = Circuit::build(&doc, symbols);
    (
        doc,
        circuit,
        IdRemap {
            part_to_spec,
            wire_to_spec,
            label_to_spec,
        },
    )
}

/// CINV3 / CINV10: rebuilding the same sheet (including duplicate designators and wire endpoint
/// flips) under arbitrary insertion permutations produces identical net order, remapped anchors,
/// display names, candidates, kinds, flags, and member order.
#[test]
fn cinv10_insertion_order_independence_proptest() {
    let symbols = SymbolLibrary::builtin();
    let (base_doc, base_circuit, base_remap) = build_permuted_sheet(
        &symbols,
        &[0, 1, 2, 3, 4],
        &[0, 1, 2, 3, 4],
        &[false; 5],
        &[0, 1, 2, 3],
    );
    let expected = normalized_nets(&base_circuit, &base_doc, &base_remap);

    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig::with_cases(64));
    let strat = (
        Just(vec![0usize, 1, 2, 3, 4]).prop_shuffle(),
        Just(vec![0usize, 1, 2, 3, 4]).prop_shuffle(),
        prop::collection::vec(any::<bool>(), 5),
        Just(vec![0usize, 1, 2, 3]).prop_shuffle(),
    );
    runner
        .run(
            &strat,
            |(part_order, wire_order, wire_flips, label_order)| {
                let (doc, circuit, remap) = build_permuted_sheet(
                    &symbols,
                    &part_order,
                    &wire_order,
                    &wire_flips,
                    &label_order,
                );
                prop_assert_eq!(normalized_nets(&circuit, &doc, &remap), expected.as_slice());
                Ok(())
            },
        )
        .expect("CINV10 insertion-order permutation proptest failed");
}
