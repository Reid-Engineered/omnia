//! D3: `SpatialIndex::query` allocates nothing when `out` already has room.
//!
//! A test-local counting allocator counts allocations made on this thread inside a scope (the
//! approach of `omnia-bench`'s `alloc` module and `omnia-canvas`'s `sheet_alloc` test).

use omnia_doc::{GRID_NM, INDEX_CELL_NM, ItemId, PartId, Pt, Rect, SpatialIndex, WireId};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

fn record() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}

// SAFETY: every method forwards to `System` unchanged; the counters never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: forwarded contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: forwarded contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        // SAFETY: forwarded contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn count_allocs<R>(f: impl FnOnce() -> R) -> (R, u64) {
    ALLOCS.with(|n| n.set(0));
    COUNTING.with(|c| c.set(true));
    let r = f();
    COUNTING.with(|c| c.set(false));
    (r, ALLOCS.with(Cell::get))
}

fn rect(x0: i32, y0: i32, x1: i32, y1: i32) -> Rect {
    Rect::from_points(Pt { x: x0, y: y0 }, Pt { x: x1, y: y1 }).unwrap()
}

#[test]
fn query_allocates_nothing_with_capacity() {
    let mut index = SpatialIndex::new();
    // A 60 x 60 block of parts plus one wire per part, a tenth of them long.
    let pitch = 6 * GRID_NM;
    let half = 2 * GRID_NM;
    let mut id = 0;
    for row in -30..30 {
        for col in -30..30 {
            let (x, y) = (col * pitch, row * pitch);
            index
                .insert(
                    ItemId::Part(PartId(id)),
                    rect(x - half, y - half, x + half, y + half),
                )
                .unwrap();
            let len = if id % 10 == 0 {
                20 * INDEX_CELL_NM
            } else {
                pitch - 2 * half
            };
            index
                .insert(
                    ItemId::Wire(WireId(id)),
                    rect(x + half, y, x + half + len, y),
                )
                .unwrap();
            id += 1;
        }
    }

    // A 4K viewport at 100% zoom (K3: 20 px per grid step), plus smaller and larger areas.
    let areas = [
        rect(-96 * GRID_NM, -54 * GRID_NM, 96 * GRID_NM, 54 * GRID_NM),
        rect(0, 0, GRID_NM, GRID_NM),
        rect(-1_000_000_000, -1_000_000_000, 1_000_000_000, 1_000_000_000),
    ];
    let mut out = Vec::with_capacity(2 * 3_600);
    let capacity = out.capacity();
    let mut found = Vec::new();
    for _ in 0..3 {
        for &area in &areas {
            let ((), allocs) = count_allocs(|| index.query(area, &mut out));
            assert_eq!(allocs, 0, "query over {area:?} allocated");
            assert_eq!(out.capacity(), capacity);
            found.push(out.len());
        }
    }
    // The areas really found items, the whole sheet all of them.
    assert!(found[0] > 100, "{found:?}");
    assert_eq!(found[2], 7_200);

    // Control: with no room, growing `out` is counted, so the zero above is real.
    let mut empty = Vec::new();
    let ((), allocs) = count_allocs(|| index.query(areas[0], &mut empty));
    assert!(allocs > 0);
}

#[test]
fn doc_query_allocates_nothing_with_capacity() {
    let doc = omnia_doc::Doc::from_items(
        [],
        (0..100).map(|x| omnia_doc::Wire {
            a: Pt { x, y: 0 },
            b: Pt { x, y: 1 },
        }),
        [],
    )
    .unwrap();
    let mut out = Vec::with_capacity(100);
    for area in [rect(0, 0, 100, 100), rect(0, 0, 0, 0)] {
        let (_, allocs) = count_allocs(|| doc.query(area, &mut out));
        assert_eq!(allocs, 0);
        assert!(!out.is_empty());
    }
}

#[test]
fn doc_query_and_net_wires_allocate_nothing_after_commands() {
    use omnia_doc::{Command, Doc, Label, Orient, Part, SymbolId, TextField, Wire};

    let mut doc = Doc::new();
    let p_ch = doc
        .apply(Command::PlacePart {
            part: Part {
                symbol: SymbolId(0),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: "R1".into(),
                value: "1k".into(),
            },
            local_pins: vec![Pt::ZERO, Pt { x: 20, y: 0 }],
            local_bounds: rect(-10, -10, 30, 10),
        })
        .unwrap();
    // Gaps keep the 64 wires separate: T6.1 merges collinear segments that
    // join with nothing else at the meeting point, which would leave only a
    // couple of added wires here and defeat the slicing below.
    let w_ch = doc
        .apply(Command::AddWires(
            (0..64)
                .map(|i| Wire {
                    a: Pt { x: i * 20, y: 0 },
                    b: Pt {
                        x: i * 20 + 10,
                        y: 0,
                    },
                })
                .collect(),
        ))
        .unwrap();
    let l_ch = doc
        .apply(Command::AddLabel(Label {
            at: Pt::ZERO,
            text: "BUS".into(),
        }))
        .unwrap();
    let mut moved = p_ch.added.clone();
    moved.extend_from_slice(&w_ch.added[..16]);
    moved.extend_from_slice(&l_ch.added);
    doc.apply(Command::Move {
        items: moved.clone(),
        delta: Pt { x: 100, y: 100 },
    })
    .unwrap();
    doc.apply(Command::Reorient {
        items: moved,
        pivot: Pt { x: 100, y: 100 },
        by: Orient::ROT_90,
    })
    .unwrap();
    doc.apply(Command::SetText {
        item: l_ch.added[0],
        field: TextField::Label,
        text: "VCC".into(),
    })
    .unwrap();
    doc.undo().unwrap();
    doc.redo().unwrap();
    doc.undo().unwrap();
    doc.undo().unwrap();

    let mut items_out = Vec::with_capacity(256);
    let mut wires_out = Vec::with_capacity(128);
    let area = rect(-1_000, -1_000, 2_000, 2_000);
    let net = match w_ch.added[32] {
        ItemId::Wire(id) => doc.net_of(id).unwrap(),
        _ => unreachable!(),
    };

    for _ in 0..3 {
        let (_, query_allocs) = count_allocs(|| doc.query(area, &mut items_out));
        assert_eq!(query_allocs, 0);
        assert!(!items_out.is_empty());

        let (_, net_allocs) = count_allocs(|| doc.net_wires(net, &mut wires_out));
        assert_eq!(net_allocs, 0);
        assert!(!wires_out.is_empty());
    }
}
