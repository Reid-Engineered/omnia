//! Edit operations to `Command`s, one to one (mcp-plan.md section 9.2, AD2, AD11).
//!
//! One operation becomes its `Command`; more than one become a single `Command::Batch` in
//! request order, which `Doc::apply` runs atomically as one undo step (AD5). Nothing here
//! predicts ids or checks geometry: every rule of the document is the document's to apply, so a
//! request is refused with exactly the `CommandError` the commit would give. The one check made
//! here is the symbol name of `place_part`, which only the library can resolve (`unknown_symbol`).
//!
//! The session's wiring preferences supply what the schema does not carry yet: the follow policy
//! of `move` and `reorient` (WK10, with the modifier released until `invert_follow` arrives with
//! A2.2) and `delete`'s `with_wires` (WK10 `delete_wires_with_part`, until A2.3 adds the field).

use std::sync::Arc;

use omnia_doc::{Command, ItemId, Label, Wire, prefs::WiringPrefs};
use omnia_symbols::SymbolLibrary;

use crate::{
    AgentError, ErrorKind,
    schema::{EditOp, ItemRef},
};

fn ids(items: &[ItemRef]) -> Vec<ItemId> {
    items.iter().map(|item| item.0).collect()
}

/// The `Command` of one edit operation; `index` is its position in the request, for messages.
pub fn edit_command(
    index: usize,
    op: &EditOp,
    symbols: &SymbolLibrary,
    prefs: &WiringPrefs,
) -> Result<Command, AgentError> {
    let follow = prefs.follow_policy(false);
    Ok(match op {
        EditOp::PlacePart {
            symbol,
            at,
            orient,
            designator,
            value,
        } => {
            let id = symbols.by_name(symbol).ok_or_else(|| {
                // AD14: the name is document-supplied text and is not repeated.
                AgentError::with_message(
                    ErrorKind::UnknownSymbol,
                    format!("operation {index}: no symbol in the library has this name"),
                )
            })?;
            symbols.place_command(
                id,
                at.0,
                orient.0,
                Arc::from(designator.as_str()),
                Arc::from(value.as_str()),
            )
        }
        EditOp::AddWires { wires } => {
            Command::AddWires(wires.iter().map(|[a, b]| Wire { a: a.0, b: b.0 }).collect())
        }
        EditOp::AddLabel { at, text } => Command::AddLabel(Label {
            at: at.0,
            text: Arc::from(text.as_str()),
        }),
        EditOp::Move { items, delta } => Command::Move {
            items: ids(items),
            delta: delta.0,
            follow,
        },
        EditOp::Reorient { items, pivot, by } => Command::Reorient {
            items: ids(items),
            pivot: pivot.0,
            by: by.0,
            follow,
        },
        EditOp::Delete { items } => Command::Delete {
            items: ids(items),
            with_wires: prefs.delete_wires_with_part,
        },
        EditOp::SetText { item, field, text } => Command::SetText {
            item: item.0,
            field: field.0,
            text: Arc::from(text.as_str()),
        },
    })
}

/// The one `Command` of a request's operations: the operation's own command, or a `Batch` of
/// them in request order (AD2). The decoder guarantees 1 to AK3 operations.
pub fn to_command(
    ops: &[EditOp],
    symbols: &SymbolLibrary,
    prefs: &WiringPrefs,
) -> Result<Command, AgentError> {
    let mut commands = ops
        .iter()
        .enumerate()
        .map(|(i, op)| edit_command(i, op, symbols, prefs))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if commands.len() == 1 {
        commands.pop().expect("one command")
    } else {
        Command::Batch(commands)
    })
}

#[cfg(test)]
mod tests {
    use omnia_doc::{Orient, PIN_LATTICE_NM, Part, PartId, Pt, TextField, WireId};

    use super::*;
    use crate::{
        ErrorCode,
        schema::{OrientName, Point, TextFieldName},
    };

    const L: i32 = PIN_LATTICE_NM;

    fn p(x: i32, y: i32) -> Point {
        Point(Pt { x: x * L, y: y * L })
    }

    #[test]
    fn each_operation_maps_to_its_command() {
        let symbols = SymbolLibrary::builtin();
        let prefs = WiringPrefs::default();
        let follow = prefs.follow_policy(false);
        let part = ItemRef(ItemId::Part(PartId(3)));
        let wire = ItemRef(ItemId::Wire(WireId(7)));
        let id = symbols.by_name("Resistor-IEEE-Standard").unwrap();
        let cases = [
            (
                EditOp::PlacePart {
                    symbol: "Resistor-IEEE-Standard".into(),
                    at: p(1, 2),
                    orient: OrientName(Orient::ROT_90),
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                symbols.place_command(id, p(1, 2).0, Orient::ROT_90, "R1".into(), "10k".into()),
            ),
            (
                EditOp::AddWires {
                    wires: vec![[p(0, 0), p(3, 0)]],
                },
                Command::AddWires(vec![Wire {
                    a: p(0, 0).0,
                    b: p(3, 0).0,
                }]),
            ),
            (
                EditOp::AddLabel {
                    at: p(1, 1),
                    text: "VCC".into(),
                },
                Command::AddLabel(Label {
                    at: p(1, 1).0,
                    text: "VCC".into(),
                }),
            ),
            (
                EditOp::Move {
                    items: vec![part, wire],
                    delta: p(2, 0),
                },
                Command::Move {
                    items: vec![part.0, wire.0],
                    delta: p(2, 0).0,
                    follow,
                },
            ),
            (
                EditOp::Reorient {
                    items: vec![part],
                    pivot: p(0, 0),
                    by: OrientName(Orient::FLIP_X),
                },
                Command::Reorient {
                    items: vec![part.0],
                    pivot: p(0, 0).0,
                    by: Orient::FLIP_X,
                    follow,
                },
            ),
            (
                EditOp::Delete { items: vec![wire] },
                Command::Delete {
                    items: vec![wire.0],
                    with_wires: false,
                },
            ),
            (
                EditOp::SetText {
                    item: part,
                    field: TextFieldName(TextField::Value),
                    text: "4k7".into(),
                },
                Command::SetText {
                    item: part.0,
                    field: TextField::Value,
                    text: "4k7".into(),
                },
            ),
        ];
        for (op, expected) in &cases {
            assert_eq!(&edit_command(0, op, &symbols, &prefs).unwrap(), expected);
            // One operation is its own command, never a batch of one (AD2).
            assert_eq!(
                &to_command(std::slice::from_ref(op), &symbols, &prefs).unwrap(),
                expected
            );
        }
        let all: Vec<EditOp> = cases.iter().map(|(op, _)| op.clone()).collect();
        let Command::Batch(batch) = to_command(&all, &symbols, &prefs).unwrap() else {
            panic!("several operations are one batch");
        };
        assert_eq!(
            batch,
            cases.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>()
        );
        let Command::PlacePart { part, .. } = &cases[0].1 else {
            unreachable!()
        };
        assert_eq!(
            part,
            &Part {
                symbol: id,
                at: p(1, 2).0,
                orient: Orient::ROT_90,
                designator: "R1".into(),
                value: "10k".into(),
            }
        );
    }

    #[test]
    fn an_unknown_symbol_is_refused_without_echoing_its_name() {
        let symbols = SymbolLibrary::builtin();
        let ops = [
            EditOp::AddLabel {
                at: p(0, 0),
                text: "A".into(),
            },
            EditOp::PlacePart {
                symbol: "No-Such-Symbol".into(),
                at: p(0, 0),
                orient: OrientName(Orient::IDENTITY),
                designator: "U1".into(),
                value: "x".into(),
            },
        ];
        let err = to_command(&ops, &symbols, &WiringPrefs::default()).unwrap_err();
        assert_eq!(err.code(), ErrorCode::UnknownSymbol);
        assert!(err.message.starts_with("operation 1:"), "{}", err.message);
        assert!(!err.message.contains("No-Such-Symbol"));
    }
}
