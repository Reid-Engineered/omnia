//! C1.2 invariant tests: CINV1 (dependency boundaries and read-only access) and CINV10
//! (disallowed hash-map iteration and insertion-order determinism).

use std::{path::Path, process::Command as ProcessCommand, sync::Arc};

use omnia_circuit::{Circuit, CircuitKey, EvidenceTier, NetKind};
use omnia_doc::{Command, Doc, ItemId, Label, LabelId, Orient, PartId, Pt, Wire, WireId};
use omnia_symbols::SymbolLibrary;

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

/// CINV10: `crates/omnia-circuit/clippy.toml` exists and disallows iterating `HashMap` / `HashSet`
/// (which also covers `FxHashMap` / `FxHashSet`).
#[test]
fn cinv10_clippy_toml_disallows_hashmap_iteration() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("clippy.toml");
    let content = std::fs::read_to_string(&path).expect("clippy.toml must exist in omnia-circuit");
    for required in [
        "disallowed-methods",
        "disallowed-types",
        "std::collections::HashMap::iter",
        "std::collections::HashMap::keys",
        "std::collections::HashMap::values",
        "std::collections::HashMap::drain",
        "std::collections::hash_map::IntoIter",
    ] {
        assert!(
            content.contains(required),
            "CINV10: clippy.toml missing required disallowed method `{required}`"
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct NormalizedNet {
    key_tag: String,
    display_name: String,
    candidates: Vec<String>,
    kind: NetKind,
    evidence: EvidenceTier,
    is_ground_ref: bool,
    is_hub: bool,
    /// Member pins represented as `(designator, pin_index)` so they compare across id remappings.
    pins: Vec<(String, u32)>,
    /// Member labels represented as `(text, x, y)`.
    labels: Vec<(String, i32, i32)>,
}

fn normalized_nets(circuit: &Circuit, doc: &Doc) -> Vec<NormalizedNet> {
    circuit
        .net_rows()
        .map(|row| {
            let key_tag = match circuit.net_key(row) {
                CircuitKey::Label(text) => format!("label:{text}"),
                CircuitKey::Pin { part, pin } => {
                    let part_row = circuit.part_row(*part).unwrap();
                    format!("pin:{}.{pin}", circuit.part_designator(part_row))
                }
                CircuitKey::Wire(wire) => {
                    let w = doc.wire(*wire).unwrap();
                    let a = (w.a.x, w.a.y);
                    let b = (w.b.x, w.b.y);
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    format!("wire:{},{}-{},{}", lo.0, lo.1, hi.0, hi.1)
                }
            };
            let pins = circuit
                .net_pins(row)
                .iter()
                .map(|&(part, pin_idx)| {
                    let pr = circuit.part_row(part).unwrap();
                    (circuit.part_designator(pr).to_owned(), pin_idx)
                })
                .collect();
            let labels = circuit
                .net_labels(row)
                .iter()
                .map(|&lid| {
                    let l = doc.label(lid).unwrap();
                    (l.text.to_string(), l.at.x, l.at.y)
                })
                .collect();
            NormalizedNet {
                key_tag,
                display_name: match circuit.net_key(row) {
                    CircuitKey::Wire(_) => "<wire-name>".to_owned(),
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

/// CINV10 / early CINV3 check: rebuilding the same sheet with parts, wires and labels inserted in
/// reversed order produces identical net order, anchors, display names, candidates, kinds, flags
/// and member order after id remapping.
#[test]
fn cinv10_insertion_order_independence() {
    let symbols = SymbolLibrary::builtin();

    let build_sheet = |reverse: bool| -> (Doc, Circuit) {
        let mut doc = Doc::new();
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
                Pt { x: 6 * STEP, y: 0 },
                Orient::IDENTITY,
                "R2",
                "4k7",
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
            // R1.2 to R2.1
            Wire {
                a: Pt { x: 2 * STEP, y: 0 },
                b: Pt { x: 4 * STEP, y: 0 },
            },
            // R2.2 to R10.1
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

        let part_iter: Vec<_> = if reverse {
            part_specs.into_iter().rev().collect()
        } else {
            part_specs.into_iter().collect()
        };
        for (sym_name, at, orient, des, val) in part_iter {
            let sym = symbols.by_name(sym_name).unwrap();
            let change = doc
                .apply(symbols.place_command(sym, at, orient, Arc::from(des), Arc::from(val)))
                .unwrap();
            assert!(matches!(change.added[..], [ItemId::Part(PartId(_))]));
        }

        let wire_iter: Vec<_> = if reverse {
            wire_specs
                .into_iter()
                .rev()
                .map(|w| Wire { a: w.b, b: w.a })
                .collect()
        } else {
            wire_specs.into_iter().collect()
        };
        for w in wire_iter {
            let change = doc.apply(Command::AddWires(vec![w])).unwrap();
            assert!(matches!(change.added[..], [ItemId::Wire(WireId(_))]));
        }

        let label_iter: Vec<_> = if reverse {
            label_specs.into_iter().rev().collect()
        } else {
            label_specs.into_iter().collect()
        };
        for (at, text) in label_iter {
            let change = doc
                .apply(Command::AddLabel(Label {
                    at,
                    text: Arc::from(text),
                }))
                .unwrap();
            assert!(matches!(change.added[..], [ItemId::Label(LabelId(_))]));
        }

        let circuit = Circuit::build(&doc, &symbols);
        (doc, circuit)
    };

    let (doc_fwd, circuit_fwd) = build_sheet(false);
    let (doc_rev, circuit_rev) = build_sheet(true);

    assert_eq!(
        normalized_nets(&circuit_fwd, &doc_fwd),
        normalized_nets(&circuit_rev, &doc_rev)
    );
}
