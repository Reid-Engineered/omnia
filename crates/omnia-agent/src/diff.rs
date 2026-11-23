//! The dirty-bounded diff of an edit (mcp-plan.md section 9.3), over public APIs only.
//!
//! `Change.dirty` covers the old and new index bounds of every item an edit touched, including
//! items that kept their id while their fields changed (a moved part, a merge's surviving wire,
//! a split's first piece). So the diff needs no scan of the whole document: `added` are the
//! change's added ids read in the after document, `removed` its removed ids read in the before
//! document, and `modified` are the other ids `Doc::query` finds in `dirty` in either document
//! whose fields differ. Each list is sorted by kind and then id (AD8), and every entry carries
//! the item's full fields (A1.3).
//!
//! The diff is one page (AK4): at most [`crate::DEFAULT_PAGE_SIZE`] entries, filled from
//! `added`, then `removed`, then `modified`; `truncated` says whether any were left out. The
//! change itself always lists every added and removed id.

use std::collections::BTreeSet;

use omnia_doc::{Change, Doc};
use omnia_symbols::SymbolLibrary;

use crate::{
    AgentError, DEFAULT_PAGE_SIZE,
    read::item,
    schema::{Diff, Item, ItemRef, LabelModified, Modified, PartModified, WireModified},
};

/// The diff from `before` to `after`, where `change` is what the edit returned.
pub fn diff(
    before: &Doc,
    after: &Doc,
    change: &Change,
    symbols: &SymbolLibrary,
) -> Result<Diff, AgentError> {
    let added_ids: BTreeSet<ItemRef> = change.added.iter().map(|&id| id.into()).collect();
    let removed_ids: BTreeSet<ItemRef> = change.removed.iter().map(|&id| id.into()).collect();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut modified = Vec::new();
    for &id in &added_ids {
        if let Some(i) = item(after, symbols, id.0)? {
            added.push(i);
        }
    }
    for &id in &removed_ids {
        if let Some(i) = item(before, symbols, id.0)? {
            removed.push(i);
        }
    }
    if let Some(dirty) = change.dirty {
        let mut found = Vec::new();
        let mut touched = BTreeSet::new();
        for doc in [before, after] {
            doc.query(dirty, &mut found);
            touched.extend(found.iter().map(|&id| ItemRef(id)));
        }
        for id in touched {
            if added_ids.contains(&id) || removed_ids.contains(&id) {
                continue;
            }
            match (item(before, symbols, id.0)?, item(after, symbols, id.0)?) {
                (Some(b), Some(a)) if a != b => modified.push(pair(b, a)),
                (Some(_), Some(_)) | (None, None) => {}
                // A change reports membership as a net delta, so an id that is live on only
                // one side is reported where it belongs.
                (None, Some(a)) => added.push(a),
                (Some(b), None) => removed.push(b),
            }
        }
        added.sort_by_key(Item::id);
        removed.sort_by_key(Item::id);
    }
    Ok(page(added, removed, modified))
}

/// A modified entry from the before and after of the same item.
fn pair(before: Item, after: Item) -> Modified {
    match (before, after) {
        (Item::Part(b), Item::Part(a)) => Modified::Part(PartModified {
            id: a.id,
            before: b.part,
            after: a.part,
        }),
        (Item::Wire(b), Item::Wire(a)) => Modified::Wire(WireModified {
            id: a.id,
            before: b.wire,
            after: a.wire,
        }),
        (Item::Label(b), Item::Label(a)) => Modified::Label(LabelModified {
            id: a.id,
            before: b.label,
            after: a.label,
        }),
        _ => unreachable!("an item id names one kind"),
    }
}

/// Keeps at most one page of entries, in the order added, removed, modified.
fn page(mut added: Vec<Item>, mut removed: Vec<Item>, mut modified: Vec<Modified>) -> Diff {
    let total = added.len() + removed.len() + modified.len();
    let mut room = usize::try_from(DEFAULT_PAGE_SIZE).unwrap_or(usize::MAX);
    let mut keep = |len: usize| {
        let k = len.min(room);
        room -= k;
        k
    };
    let (a, r, m) = (keep(added.len()), keep(removed.len()), keep(modified.len()));
    added.truncate(a);
    removed.truncate(r);
    modified.truncate(m);
    Diff {
        added,
        removed,
        modified,
        truncated: a + r + m < total,
    }
}

/// The same diff computed by comparing every item of both documents, with no use of the
/// change's lists or dirty rectangle. Tests use it as the reference.
#[cfg(test)]
pub(crate) fn brute_force(
    before: &Doc,
    after: &Doc,
    symbols: &SymbolLibrary,
) -> Result<Diff, AgentError> {
    let ids = |doc: &Doc| -> BTreeSet<ItemRef> {
        doc.parts()
            .map(|(id, _)| ItemRef(omnia_doc::ItemId::Part(id)))
            .chain(
                doc.wires()
                    .map(|(id, _)| ItemRef(omnia_doc::ItemId::Wire(id))),
            )
            .chain(
                doc.labels()
                    .map(|(id, _)| ItemRef(omnia_doc::ItemId::Label(id))),
            )
            .collect()
    };
    let all: BTreeSet<ItemRef> = ids(before).union(&ids(after)).copied().collect();
    let (mut added, mut removed, mut modified) = (Vec::new(), Vec::new(), Vec::new());
    for id in all {
        match (item(before, symbols, id.0)?, item(after, symbols, id.0)?) {
            (None, Some(a)) => added.push(a),
            (Some(b), None) => removed.push(b),
            (Some(b), Some(a)) if a != b => modified.push(pair(b, a)),
            _ => {}
        }
    }
    Ok(page(added, removed, modified))
}

#[cfg(test)]
mod tests {
    use omnia_doc::{Command, ItemId, Label, Orient, PIN_LATTICE_NM, Pt, Wire, WireId};

    use super::*;
    use crate::preview::run;

    const L: i32 = PIN_LATTICE_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * L, y: y * L }
    }

    fn wire(a: Pt, b: Pt) -> Wire {
        Wire { a, b }
    }

    #[test]
    fn merge_split_move_and_text_are_reported_with_full_fields() {
        let symbols = SymbolLibrary::builtin();
        let mut doc = Doc::new();
        let r = symbols.by_name("Resistor-IEEE-Standard").unwrap();
        doc.apply(symbols.place_command(r, pt(0, 0), Orient::IDENTITY, "R1".into(), "1k".into()))
            .unwrap();
        doc.apply(Command::AddWires(vec![
            wire(pt(10, 0), pt(14, 0)),
            wire(pt(20, 0), pt(20, 6)),
        ]))
        .unwrap();
        doc.apply(Command::AddLabel(Label {
            at: pt(-10, -10),
            text: "A".into(),
        }))
        .unwrap();
        let cases = [
            // A collinear extension merges into the surviving lowest id (modified) and drops the
            // new piece.
            Command::AddWires(vec![wire(pt(14, 0), pt(18, 0))]),
            // A wire ending on another's interior splits it.
            Command::AddWires(vec![wire(pt(16, 3), pt(20, 3))]),
            Command::Move {
                items: vec![ItemId::Part(omnia_doc::PartId(0))],
                delta: pt(0, 4),
                follow: omnia_doc::prefs::WiringPrefs::default().follow_policy(false),
            },
            Command::SetText {
                item: ItemId::Label(omnia_doc::LabelId(0)),
                field: omnia_doc::TextField::Label,
                text: "B".into(),
            },
            Command::Delete {
                items: vec![ItemId::Wire(WireId(1))],
                with_wires: false,
            },
            Command::Batch(vec![
                Command::AddLabel(Label {
                    at: pt(3, 3),
                    text: "C".into(),
                }),
                Command::Delete {
                    items: vec![ItemId::Wire(WireId(0))],
                    with_wires: false,
                },
            ]),
        ];
        for cmd in cases {
            let (after, change) = run(&doc, cmd.clone()).unwrap();
            let d = diff(&doc, &after, &change, &symbols).unwrap();
            assert_eq!(d, brute_force(&doc, &after, &symbols).unwrap(), "{cmd:?}");
            assert!(
                !(d.added.is_empty() && d.removed.is_empty() && d.modified.is_empty()),
                "{cmd:?} changes something"
            );
            assert!(!d.truncated);
        }
    }

    #[test]
    fn a_large_edit_is_truncated_to_one_page() {
        let symbols = SymbolLibrary::builtin();
        let doc = Doc::new();
        let n = DEFAULT_PAGE_SIZE as i32 + 7;
        let labels = (0..n)
            .map(|i| {
                Command::AddLabel(Label {
                    at: pt(i % 100, i / 100),
                    text: "N".into(),
                })
            })
            .collect();
        let (after, change) = run(&doc, Command::Batch(labels)).unwrap();
        let d = diff(&doc, &after, &change, &symbols).unwrap();
        assert!(d.truncated);
        assert_eq!(d.added.len(), DEFAULT_PAGE_SIZE as usize);
        assert_eq!(change.added.len(), n as usize, "the change lists every id");
        assert_eq!(d, brute_force(&doc, &after, &symbols).unwrap());
        // The page is the lowest ids.
        let first: Vec<ItemRef> = d.added.iter().map(Item::id).collect();
        let mut sorted = first.clone();
        sorted.sort();
        assert_eq!(first, sorted);
        assert_eq!(first[0], ItemRef(ItemId::Label(omnia_doc::LabelId(0))));
    }

    #[test]
    fn a_no_op_has_an_empty_diff() {
        let symbols = SymbolLibrary::builtin();
        let doc = Doc::new();
        let (after, change) = run(&doc, Command::AddWires(Vec::new())).unwrap();
        assert!(change.dirty.is_none());
        let d = diff(&doc, &after, &change, &symbols).unwrap();
        assert_eq!(
            d,
            Diff {
                added: vec![],
                removed: vec![],
                modified: vec![],
                truncated: false
            }
        );
    }
}
