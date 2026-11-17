//! `Change` contract for ids that survive with new geometry (canvas-plan.md section 9, D2):
//! they are in neither `added` nor `removed`, and `dirty` covers their old and new bounds on
//! apply, undo and redo.
use omnia_doc::{Change, Command, Doc, ItemId, Pt, Rect, Wire, WireId};

fn h(x0: i32, x1: i32, y: i32) -> Wire {
    Wire {
        a: Pt { x: x0, y },
        b: Pt { x: x1, y },
    }
}

fn covers(change: &Change, wire: &Wire) {
    let dirty = change.dirty.expect("a real change has a dirty region");
    let bounds = Rect::from_points(wire.a, wire.b).unwrap();
    assert!(
        dirty.contains_rect(bounds),
        "dirty {dirty:?} must cover {bounds:?}"
    );
}

fn not_listed(change: &Change, id: WireId) {
    assert!(!change.added.contains(&ItemId::Wire(id)));
    assert!(!change.removed.contains(&ItemId::Wire(id)));
}

#[test]
fn merge_survivor_is_covered_by_dirty_on_apply_undo_redo() {
    let mut doc = Doc::new();
    doc.apply(Command::AddWires(vec![h(0, 1000, 0)])).unwrap();
    let before = *doc.wire(WireId(0)).unwrap();

    let change = doc
        .apply(Command::AddWires(vec![h(1000, 2000, 0)]))
        .unwrap();
    let after = *doc.wire(WireId(0)).unwrap();
    assert_eq!(
        after,
        h(0, 2000, 0),
        "collinear add merges into the lowest id"
    );
    assert_eq!(doc.wires().count(), 1);
    not_listed(&change, WireId(0));
    covers(&change, &before);
    covers(&change, &after);

    let undo = doc.undo().unwrap();
    assert_eq!(*doc.wire(WireId(0)).unwrap(), before);
    not_listed(&undo, WireId(0));
    covers(&undo, &before);
    covers(&undo, &after);

    let redo = doc.redo().unwrap();
    assert_eq!(*doc.wire(WireId(0)).unwrap(), after);
    not_listed(&redo, WireId(0));
    covers(&redo, &before);
    covers(&redo, &after);
}

#[test]
fn split_keeper_is_covered_by_dirty_on_apply_undo_redo() {
    let mut doc = Doc::new();
    doc.apply(Command::AddWires(vec![h(0, 2000, 1000)]))
        .unwrap();
    let before = *doc.wire(WireId(0)).unwrap();

    // A vertical wire ending on the middle of wire 0 splits it (INV2).
    let tee = Wire {
        a: Pt { x: 1000, y: 1000 },
        b: Pt { x: 1000, y: 2000 },
    };
    let change = doc.apply(Command::AddWires(vec![tee])).unwrap();
    let after = *doc.wire(WireId(0)).unwrap();
    assert_ne!(
        after, before,
        "the first piece keeps the id with shorter geometry"
    );
    not_listed(&change, WireId(0));
    covers(&change, &before);
    covers(&change, &after);

    let undo = doc.undo().unwrap();
    assert_eq!(*doc.wire(WireId(0)).unwrap(), before);
    not_listed(&undo, WireId(0));
    covers(&undo, &before);

    let redo = doc.redo().unwrap();
    assert_eq!(*doc.wire(WireId(0)).unwrap(), after);
    not_listed(&redo, WireId(0));
    covers(&redo, &before);
}
