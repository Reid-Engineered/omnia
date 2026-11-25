//! C1.2 scenario and graph tests: CTC-key-01, CTC-name-01, every CK1 key form, CK2 display
//! names and candidates, CK3 roles/symbol classes, CK4 ground evidence, CK7 hubs, and CD18
//! ordering.

use std::sync::Arc;

use omnia_circuit::{
    Circuit, CircuitKey, EvidenceTier, GroundEvidence, HUB_PIN_THRESHOLD, NetKind, PinRole,
    SymbolClass,
};
use omnia_doc::{
    Command, Doc, ItemId, Label, LabelId, Orient, Part, PartId, Pt, Rect, Wire, WireId,
};
use omnia_symbols::SymbolLibrary;

const STEP: i32 = 2_540_000;

fn place_symbol(
    doc: &mut Doc,
    symbols: &SymbolLibrary,
    name: &str,
    at: Pt,
    orient: Orient,
    designator: &str,
    value: &str,
) -> PartId {
    let sym = symbols.by_name(name).expect("built-in symbol exists");
    let cmd = symbols.place_command(sym, at, orient, Arc::from(designator), Arc::from(value));
    let change = doc.apply(cmd).expect("place part succeeds");
    match change.added[..] {
        [ItemId::Part(id)] => id,
        ref other => panic!("expected one placed part, got {other:?}"),
    }
}

fn add_wire(doc: &mut Doc, a: Pt, b: Pt) -> WireId {
    let change = doc
        .apply(Command::AddWires(vec![Wire { a, b }]))
        .expect("add wire succeeds");
    match change.added[..] {
        [ItemId::Wire(id)] => id,
        ref other => panic!("expected one added wire, got {other:?}"),
    }
}

fn add_label(doc: &mut Doc, at: Pt, text: &str) -> LabelId {
    let change = doc
        .apply(Command::AddLabel(Label {
            at,
            text: Arc::from(text),
        }))
        .expect("add label succeeds");
    match change.added[..] {
        [ItemId::Label(id)] => id,
        ref other => panic!("expected one added label, got {other:?}"),
    }
}

/// CTC-key-01 built from scratch: R1 (`part:0`) and R2 (`part:1`), one wire from R1 pin 2 to R2
/// pin 1, no labels; then delete the wire and rebuild from scratch.
#[test]
fn ctc_key_01_from_scratch_before_and_after_wire_delete() {
    let symbols = SymbolLibrary::builtin();
    let mut doc = Doc::new();

    // Resistor pins are at (-5_080_000, 0) [pin "1", index 0] and (+5_080_000, 0) [pin "2", index 1].
    let r1 = place_symbol(
        &mut doc,
        &symbols,
        "Resistor-IEEE-Standard",
        Pt { x: 0, y: 0 },
        Orient::IDENTITY,
        "R1",
        "10k",
    );
    let r2 = place_symbol(
        &mut doc,
        &symbols,
        "Resistor-IEEE-Standard",
        Pt { x: 6 * STEP, y: 0 },
        Orient::IDENTITY,
        "R2",
        "10k",
    );
    assert_eq!(r1, PartId(0));
    assert_eq!(r2, PartId(1));

    let r1_pin2 = doc.pins(r1).unwrap()[1];
    let r2_pin1 = doc.pins(r2).unwrap()[0];
    let w = add_wire(&mut doc, r1_pin2, r2_pin1);

    // ---- Before delete ----
    let before = Circuit::build(&doc, &symbols);
    assert_eq!(before.revision(), doc.revision());
    assert_eq!(before.part_count(), 2);
    assert_eq!(before.pin_count(), 4);
    assert_eq!(before.net_count(), 3);

    // R1.1 single-pin net
    let n_r1_1 = before.net_of_pin(r1, 0).expect("R1.1 net").row();
    assert_eq!(
        before.net_key(n_r1_1),
        &CircuitKey::Pin { part: r1, pin: 0 }
    );
    assert_eq!(before.net_key(n_r1_1).to_string(), "net:pin:0.0");
    assert_eq!(before.net_display_name(n_r1_1), "N(R1.1)");
    assert_eq!(before.net_pins(n_r1_1), &[(r1, 0)]);

    // Joined net: anchor R1.2, key net:pin:0.1, name N(R1.2)
    let n_joined = before.net_of_pin(r1, 1).expect("R1.2 net").row();
    assert_eq!(before.net_of_pin(r2, 0).expect("R2.1 net").row(), n_joined);
    assert_eq!(
        before.net_key(n_joined),
        &CircuitKey::Pin { part: r1, pin: 1 }
    );
    assert_eq!(before.net_key(n_joined).to_string(), "net:pin:0.1");
    assert_eq!(before.net_display_name(n_joined), "N(R1.2)");
    assert_eq!(before.net_pins(n_joined), &[(r1, 1), (r2, 0)]);
    assert_eq!(before.nets_by_name("N(R1.2)"), &[n_joined]);
    assert_eq!(
        before
            .net_by_key(&CircuitKey::Pin { part: r1, pin: 1 })
            .map(|r| r.row()),
        Some(n_joined)
    );

    // R2.2 single-pin net
    let n_r2_2 = before.net_of_pin(r2, 1).expect("R2.2 net").row();
    assert_eq!(
        before.net_key(n_r2_2),
        &CircuitKey::Pin { part: r2, pin: 1 }
    );
    assert_eq!(before.net_key(n_r2_2).to_string(), "net:pin:1.1");
    assert_eq!(before.net_display_name(n_r2_2), "N(R2.2)");
    assert_eq!(before.net_pins(n_r2_2), &[(r2, 1)]);

    // ---- Delete the wire and rebuild ----
    doc.apply(Command::Delete {
        items: vec![ItemId::Wire(w)],
        with_wires: false,
    })
    .expect("delete wire succeeds");

    let after = Circuit::build(&doc, &symbols);
    assert_eq!(after.revision(), doc.revision());
    assert_eq!(after.net_count(), 4);

    // R1.1 and R2.2 unchanged
    let a_r1_1 = after.net_of_pin(r1, 0).unwrap().row();
    assert_eq!(after.net_key(a_r1_1).to_string(), "net:pin:0.0");
    assert_eq!(after.net_display_name(a_r1_1), "N(R1.1)");

    // R1.2 keeps its key net:pin:0.1 and name N(R1.2)
    let a_r1_2 = after.net_of_pin(r1, 1).unwrap().row();
    assert_eq!(after.net_key(a_r1_2).to_string(), "net:pin:0.1");
    assert_eq!(after.net_display_name(a_r1_2), "N(R1.2)");
    assert_eq!(after.net_pins(a_r1_2), &[(r1, 1)]);

    // R2.1 becomes its own net with key net:pin:1.0 and name N(R2.1)
    let a_r2_1 = after.net_of_pin(r2, 0).unwrap().row();
    assert_ne!(a_r1_2, a_r2_1);
    assert_eq!(after.net_key(a_r2_1).to_string(), "net:pin:1.0");
    assert_eq!(after.net_display_name(a_r2_1), "N(R2.1)");
    assert_eq!(after.net_pins(a_r2_1), &[(r2, 0)]);

    let a_r2_2 = after.net_of_pin(r2, 1).unwrap().row();
    assert_eq!(after.net_key(a_r2_2).to_string(), "net:pin:1.1");
    assert_eq!(after.net_display_name(a_r2_2), "N(R2.2)");
}

/// CTC-name-01 built from scratch: labels `VCC` and `V3V3` shorted by a wire yield one net with
/// key `net:"V3V3"`, display name `V3V3` (lowest by byte order), and candidate `VCC`.
#[test]
fn ctc_name_01_shorted_labels_lowest_byte_order_and_candidates() {
    let symbols = SymbolLibrary::builtin();
    let mut doc = Doc::new();

    let a = Pt { x: 0, y: 0 };
    let b = Pt { x: 2 * STEP, y: 0 };
    let l_vcc = add_label(&mut doc, a, "VCC");
    let l_v3v3 = add_label(&mut doc, b, "V3V3");
    add_wire(&mut doc, a, b);

    let circuit = Circuit::build(&doc, &symbols);
    assert_eq!(circuit.net_count(), 1);
    let row = circuit.net_rows().next().unwrap();

    let expected_key = CircuitKey::Label(Arc::from("V3V3"));
    assert_eq!(circuit.net_key(row), &expected_key);
    assert_eq!(circuit.net_key(row).to_string(), r#"net:"V3V3""#);
    assert_eq!(circuit.net_display_name(row), "V3V3");
    let candidates: Vec<&str> = circuit.net_candidate_names(row).collect();
    assert_eq!(candidates, vec!["VCC"]);
    assert_eq!(circuit.net_labels(row), &[l_v3v3, l_vcc]);

    // Reverse name lookup finds the net under both its display name and its candidate name.
    assert_eq!(circuit.nets_by_name("V3V3"), &[row]);
    assert_eq!(circuit.nets_by_name("VCC"), &[row]);
    assert_eq!(
        circuit.net_by_key(&expected_key).map(|r| r.row()),
        Some(row)
    );
}

/// Unit test for the label key form (CK1/CK2): lowest label text wins over pins/wires, duplicate
/// texts deduplicate candidates, and labels sort by (text, pos, id).
#[test]
fn key_form_label_net_overrides_pins_and_dedups_candidates() {
    let symbols = SymbolLibrary::builtin();
    let mut doc = Doc::new();

    let r1 = place_symbol(
        &mut doc,
        &symbols,
        "Resistor-IEEE-Standard",
        Pt { x: 0, y: 0 },
        Orient::IDENTITY,
        "R1",
        "1k",
    );
    let p1 = doc.pins(r1).unwrap()[0];
    let p2 = Pt {
        x: p1.x,
        y: p1.y + STEP,
    };
    add_wire(&mut doc, p1, p2);
    let l_z = add_label(&mut doc, p1, "Z_SIG");
    let l_a_hi = add_label(&mut doc, p2, "A_SIG");
    let l_a_lo = add_label(&mut doc, p1, "A_SIG");
    let l_m = add_label(&mut doc, p2, "M_SIG");

    let circuit = Circuit::build(&doc, &symbols);
    let row = circuit.net_of_pin(r1, 0).unwrap().row();

    assert_eq!(circuit.net_key(row).to_string(), r#"net:"A_SIG""#);
    assert_eq!(circuit.net_display_name(row), "A_SIG");
    let candidates: Vec<&str> = circuit.net_candidate_names(row).collect();
    assert_eq!(candidates, vec!["M_SIG", "Z_SIG"]);
    // CD18 label order: text bytes, then (x, y), then LabelId.
    assert_eq!(circuit.net_labels(row), &[l_a_lo, l_a_hi, l_m, l_z]);
    assert_eq!(circuit.nets_by_name("A_SIG"), &[row]);
    assert_eq!(circuit.nets_by_name("M_SIG"), &[row]);
    assert_eq!(circuit.nets_by_name("Z_SIG"), &[row]);
}

/// Unit test for the pin key form (CK1/CK2/CD18): natural designator order (`R2` < `R10`),
/// position tie-breaker, coincident-pin tie-breakers (symbol, value, orientation, bounds), and
/// missing library pin metadata fallback (`N(R3#1)`).
#[test]
fn key_form_pin_net_anchor_and_cd18_tie_breakers() {
    let symbols = SymbolLibrary::builtin();

    // 1. Natural designator order: R2 beats R10 even when R10 is inserted first.
    {
        let mut doc = Doc::new();
        let r10 = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 0, y: 0 },
            Orient::IDENTITY,
            "R10",
            "1k",
        );
        let r2 = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 6 * STEP, y: 0 },
            Orient::IDENTITY,
            "R2",
            "1k",
        );
        let r10_pin2 = doc.pins(r10).unwrap()[1];
        let r2_pin1 = doc.pins(r2).unwrap()[0];
        add_wire(&mut doc, r10_pin2, r2_pin1);

        let circuit = Circuit::build(&doc, &symbols);
        let row = circuit.net_of_pin(r10, 1).unwrap().row();
        assert_eq!(circuit.net_key(row), &CircuitKey::Pin { part: r2, pin: 0 });
        assert_eq!(circuit.net_display_name(row), "N(R2.1)");
        assert_eq!(circuit.net_pins(row), &[(r2, 0), (r10, 1)]);
    }

    // 2. Coincident duplicate-designator pins break ties by symbol -> value -> orient -> bounds.
    {
        let mut doc = Doc::new();
        // Same designator "R3", same pin 1 position (0, 0), different values ("2k" inserted before "1k").
        let p_2k = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 2 * STEP, y: 0 },
            Orient::IDENTITY,
            "R3",
            "2k",
        );
        let p_1k = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 2 * STEP, y: 0 },
            Orient::IDENTITY,
            "R3",
            "1k",
        );
        assert_eq!(doc.pins(p_2k).unwrap()[0], doc.pins(p_1k).unwrap()[0]);

        let circuit = Circuit::build(&doc, &symbols);
        let row = circuit.net_of_pin(p_2k, 0).unwrap().row();
        assert_eq!(
            circuit.net_key(row),
            &CircuitKey::Pin { part: p_1k, pin: 0 }
        );
        assert_eq!(circuit.net_pins(row), &[(p_1k, 0), (p_2k, 0)]);
    }

    // 3. Coincident pins with equal symbol & value break ties by orientation raw encoding.
    {
        let mut doc = Doc::new();
        // Place two identical resistors with pin 1 at the exact same world point (0, 0),
        // one at ROT_90 (raw 1) and one at IDENTITY (raw 0).
        let p_rot = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 0, y: 2 * STEP },
            Orient::ROT_90,
            "R3",
            "1k",
        );
        let p_id = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 2 * STEP, y: 0 },
            Orient::IDENTITY,
            "R3",
            "1k",
        );
        assert_eq!(doc.pins(p_rot).unwrap()[0], Pt::ZERO);
        assert_eq!(doc.pins(p_id).unwrap()[0], Pt::ZERO);

        let circuit = Circuit::build(&doc, &symbols);
        let row = circuit.net_of_pin(p_rot, 0).unwrap().row();
        assert_eq!(
            circuit.net_key(row),
            &CircuitKey::Pin { part: p_id, pin: 0 }
        );
        assert_eq!(circuit.net_pins(row), &[(p_id, 0), (p_rot, 0)]);
    }

    // 4. Coincident pins with equal designator, number, position, symbol, value & orient break
    // ties by world body bounds.
    {
        let mut doc = Doc::new();
        let sym = symbols.by_name("Resistor-IEEE-Standard").unwrap();
        let big = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: sym,
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: Arc::from("R3"),
                    value: Arc::from("1k"),
                },
                local_pins: vec![Pt::ZERO],
                local_bounds: Rect::new(Pt { x: 0, y: 0 }, Pt { x: STEP, y: STEP }).unwrap(),
            })
            .unwrap();
        let small = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: sym,
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: Arc::from("R3"),
                    value: Arc::from("1k"),
                },
                local_pins: vec![Pt::ZERO],
                local_bounds: Rect::new(Pt { x: -STEP, y: 0 }, Pt { x: STEP, y: STEP }).unwrap(),
            })
            .unwrap();
        let [ItemId::Part(p_big)] = big.added[..] else {
            panic!()
        };
        let [ItemId::Part(p_small)] = small.added[..] else {
            panic!()
        };

        let circuit = Circuit::build(&doc, &symbols);
        let row = circuit.net_of_pin(p_big, 0).unwrap().row();
        assert_eq!(circuit.net_pins(row), &[(p_small, 0), (p_big, 0)]);
    }

    // 5. Legacy part with more stored pins than its symbol definition uses `#<index>` (CK13).
    {
        let mut doc = Doc::new();
        let sym = symbols.by_name("Ground-COM-General").unwrap();
        let placed = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: sym,
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: Arc::from("R3"),
                    value: Arc::from(""),
                },
                local_pins: vec![Pt::ZERO, Pt { x: STEP, y: 0 }],
                local_bounds: Rect::new(Pt::ZERO, Pt { x: STEP, y: STEP }).unwrap(),
            })
            .unwrap();
        let [ItemId::Part(p)] = placed.added[..] else {
            panic!()
        };
        let circuit = Circuit::build(&doc, &symbols);
        let row = circuit.net_of_pin(p, 1).unwrap().row();
        assert_eq!(circuit.pin_number(circuit.pin_row(p, 1).unwrap()), None);
        assert_eq!(circuit.net_key(row), &CircuitKey::Pin { part: p, pin: 1 });
        assert_eq!(circuit.net_display_name(row), "N(R3#1)");
    }
}

/// Unit test for the bare wire-island key form (CK1/CK2/CD18): positive-y wire beats positive-x
/// wire sharing the same lower endpoint regardless of wire direction or insertion order.
#[test]
fn key_form_wire_island_anchor_and_direction_independence() {
    let symbols = SymbolLibrary::builtin();

    // Order A: positive-x inserted first, positive-y reversed (b -> a) inserted second.
    let mut doc_a = Doc::new();
    let w_x = add_wire(&mut doc_a, Pt::ZERO, Pt { x: STEP, y: 0 });
    let w_y = add_wire(&mut doc_a, Pt { x: 0, y: STEP }, Pt::ZERO);

    let ca = Circuit::build(&doc_a, &symbols);
    assert_eq!(ca.net_count(), 1);
    let row_a = ca.net_rows().next().unwrap();
    // Positive-y wire has lower=(0,0), upper=(0,STEP) < (STEP,0), so w_y is anchor.
    assert_eq!(ca.net_key(row_a), &CircuitKey::Wire(w_y));
    assert_eq!(ca.net_key(row_a).to_string(), format!("net:wire:{}", w_y.0));
    assert_eq!(ca.net_display_name(row_a), format!("N(wire:{})", w_y.0));
    assert_ne!(w_x, w_y);

    // Order B: positive-y inserted first, positive-x inserted second.
    let mut doc_b = Doc::new();
    let w_y_b = add_wire(&mut doc_b, Pt::ZERO, Pt { x: 0, y: STEP });
    let _w_x_b = add_wire(&mut doc_b, Pt { x: STEP, y: 0 }, Pt::ZERO);

    let cb = Circuit::build(&doc_b, &symbols);
    let row_b = cb.net_rows().next().unwrap();
    assert_eq!(cb.net_key(row_b), &CircuitKey::Wire(w_y_b));
    assert_eq!(cb.net_display_name(row_b), format!("N(wire:{})", w_y_b.0));
}

/// CK3, CK4, CK6, CK7 and CD18 net ordering:
/// - All pins start with `PinRole::Unspecified` until C3.1.
/// - `Ground-COM-General` and `"0"` label produce declared ground reference + hub.
/// - `"GND"` alone produces heuristic ground + hub, with `is_ground_reference == false`.
/// - Signal net hub cutoff is strictly > 32 pins (`HUB_PIN_THRESHOLD`).
/// - Net rows are ordered: pin nets -> label-only nets -> wire-only islands.
#[test]
fn ck3_roles_ck4_ground_ck7_hubs_and_cd18_net_groups() {
    let symbols = SymbolLibrary::builtin();
    let mut doc = Doc::new();

    // Wire-only island (added first, must still sort LAST under CD18).
    let w_only = add_wire(
        &mut doc,
        Pt {
            x: 20 * STEP,
            y: 20 * STEP,
        },
        Pt {
            x: 21 * STEP,
            y: 20 * STEP,
        },
    );

    // Label-only net "A" (added second, must sort after pin nets and before wire-only islands).
    let l_only = add_label(
        &mut doc,
        Pt {
            x: 15 * STEP,
            y: 15 * STEP,
        },
        "A",
    );

    // Ground symbol net.
    let gnd_part = place_symbol(
        &mut doc,
        &symbols,
        "Ground-COM-General",
        Pt { x: 0, y: 0 },
        Orient::IDENTITY,
        "",
        "",
    );

    // Zero-label net.
    let zero_at = Pt { x: 10 * STEP, y: 0 };
    let zero_label = add_label(&mut doc, zero_at, "0");

    // Heuristic ground-name-only net ("GND" without ground part or "0").
    let gnd_name_at = Pt {
        x: 10 * STEP,
        y: 5 * STEP,
    };
    add_label(&mut doc, gnd_name_at, "GND");

    let circuit = Circuit::build(&doc, &symbols);

    // Every pin has PinRole::Unspecified until C3.1.
    for part_row in circuit.part_rows() {
        for pin_row in circuit.part_pin_rows(part_row) {
            assert_eq!(circuit.pin_role(pin_row), PinRole::Unspecified);
        }
    }

    // Ground symbol classification and evidence.
    let gnd_part_row = circuit.part_row(gnd_part).unwrap();
    assert_eq!(circuit.part_symbol_class(gnd_part_row), SymbolClass::Ground);
    let gnd_sym_net = circuit.net_of_pin(gnd_part, 0).unwrap().row();
    assert_eq!(circuit.net_kind(gnd_sym_net), NetKind::Ground);
    assert_eq!(
        circuit.net_kind_evidence(gnd_sym_net),
        EvidenceTier::Declared
    );
    assert!(circuit.net_is_ground_reference(gnd_sym_net));
    assert!(circuit.net_is_hub(gnd_sym_net));
    assert_eq!(
        circuit.net_ground_evidence(gnd_sym_net),
        &[GroundEvidence::GroundSymbol(gnd_part)]
    );

    // Zero-label net evidence.
    let zero_net = circuit.nets_by_name("0")[0];
    assert_eq!(circuit.net_kind(zero_net), NetKind::Ground);
    assert_eq!(circuit.net_kind_evidence(zero_net), EvidenceTier::Declared);
    assert!(circuit.net_is_ground_reference(zero_net));
    assert!(circuit.net_is_hub(zero_net));
    assert_eq!(
        circuit.net_ground_evidence(zero_net),
        &[GroundEvidence::ZeroLabel(zero_label)]
    );

    // "GND" alone: heuristic ground, NOT a ground reference (CTC-gnd-01, CK4).
    let gnd_name_net = circuit.nets_by_name("GND")[0];
    assert_eq!(circuit.net_kind(gnd_name_net), NetKind::Ground);
    assert_eq!(
        circuit.net_kind_evidence(gnd_name_net),
        EvidenceTier::Heuristic
    );
    assert!(!circuit.net_is_ground_reference(gnd_name_net));
    assert!(circuit.net_ground_evidence(gnd_name_net).is_empty());
    assert!(circuit.net_is_hub(gnd_name_net));

    // CD18 net group order: pin net first, then label-only nets ("0", "A", "GND"), then wire-only.
    let rows: Vec<_> = circuit.net_rows().collect();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0], gnd_sym_net);
    assert_eq!(circuit.net_display_name(rows[1]), "0");
    assert_eq!(circuit.net_display_name(rows[2]), "A");
    assert_eq!(circuit.net_labels(rows[2]), &[l_only]);
    assert_eq!(circuit.net_display_name(rows[3]), "GND");
    assert_eq!(circuit.net_key(rows[4]), &CircuitKey::Wire(w_only));
    assert_eq!(circuit.net_kind(rows[4]), NetKind::Signal);
    assert!(!circuit.net_is_hub(rows[4]));
}

/// CK7 hub threshold boundary: 32 signal pins is not a hub; 33 signal pins is a hub.
#[test]
fn ck7_hub_threshold_at_32_vs_33_pins() {
    let symbols = SymbolLibrary::builtin();
    let mut doc = Doc::new();

    // Place 32 resistors whose pin 1 all sit on (0, 0) (so they share one net via abutting pins).
    let mut first = PartId(0);
    for i in 0..HUB_PIN_THRESHOLD {
        let p = place_symbol(
            &mut doc,
            &symbols,
            "Resistor-IEEE-Standard",
            Pt { x: 2 * STEP, y: 0 },
            Orient::IDENTITY,
            &format!("R{}", i + 1),
            "1k",
        );
        if i == 0 {
            first = p;
        }
    }

    let c32 = Circuit::build(&doc, &symbols);
    let n32 = c32.net_of_pin(first, 0).unwrap().row();
    assert_eq!(c32.net_pins(n32).len(), 32);
    assert_eq!(c32.net_kind(n32), NetKind::Signal);
    assert!(!c32.net_is_hub(n32));

    // Add a 33rd resistor on the same net.
    place_symbol(
        &mut doc,
        &symbols,
        "Resistor-IEEE-Standard",
        Pt { x: 2 * STEP, y: 0 },
        Orient::IDENTITY,
        "R33",
        "1k",
    );

    let c33 = Circuit::build(&doc, &symbols);
    let n33 = c33.net_of_pin(first, 0).unwrap().row();
    assert_eq!(c33.net_pins(n33).len(), 33);
    assert!(c33.net_is_hub(n33));
}

/// Empty document and zero-pin parts build cleanly without panicking.
#[test]
fn empty_doc_and_zero_pin_part() {
    let symbols = SymbolLibrary::builtin();
    let mut doc = Doc::new();
    let empty = Circuit::build(&doc, &symbols);
    assert_eq!(empty.revision(), 0);
    assert_eq!(empty.part_count(), 0);
    assert_eq!(empty.pin_count(), 0);
    assert_eq!(empty.net_count(), 0);
    assert_eq!(empty.net_of_pin(PartId(0), 0), None);
    assert!(empty.nets_by_name("VCC").is_empty());

    let sym = symbols.by_name("Resistor-IEEE-Standard").unwrap();
    let change = doc
        .apply(Command::PlacePart {
            part: Part {
                symbol: sym,
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: Arc::from("H1"),
                value: Arc::from("hole"),
            },
            local_pins: vec![],
            local_bounds: Rect::new(Pt::ZERO, Pt::ZERO).unwrap(),
        })
        .unwrap();
    let [ItemId::Part(p)] = change.added[..] else {
        panic!()
    };

    let c = Circuit::build(&doc, &symbols);
    assert_eq!(c.part_count(), 1);
    assert_eq!(c.pin_count(), 0);
    assert_eq!(c.net_count(), 0);
    let row = c.part_row(p).unwrap();
    assert_eq!(c.part_designator(row), "H1");
    assert_eq!(c.part_value(row), "hole");
    assert_eq!(c.part_pin_rows(row).len(), 0);
    assert_eq!(c.net_of_pin(p, 0), None);
}
