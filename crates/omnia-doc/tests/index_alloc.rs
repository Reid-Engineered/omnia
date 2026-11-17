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
    // A connected 64-wire staircase preserves the original net workload.
    // Alternating axes make every meeting a real corner (T6.1 / WD1).
    let w_ch = doc
        .apply(Command::AddWires(
            (0..64)
                .map(|i| {
                    let a = Pt {
                        x: ((i + 1) / 2) * 10,
                        y: (i / 2) * 10,
                    };
                    let b = if i % 2 == 0 {
                        Pt {
                            x: a.x + 10,
                            y: a.y,
                        }
                    } else {
                        Pt {
                            x: a.x,
                            y: a.y + 10,
                        }
                    };
                    Wire { a, b }
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
    let first_wire = match w_ch.added[0] {
        ItemId::Wire(id) => id,
        _ => unreachable!(),
    };
    let mut original_net = Vec::new();
    doc.net_wires(doc.net_of(first_wire).unwrap(), &mut original_net);
    assert_eq!(
        original_net.len(),
        64,
        "the allocation workload is a connected 64-wire net"
    );
    assert_eq!(doc.net_at(Pt::ZERO), doc.net_of(first_wire));
    let mut moved = p_ch.added.clone();
    // Move the complete connected workload; partial translation used to overlap
    // the untouched staircase and depended on non-normal geometry to keep 64 wires.
    moved.extend_from_slice(&w_ch.added);
    moved.extend_from_slice(&l_ch.added);
    doc.apply(Command::Move {
        follow: omnia_doc::prefs::FollowPolicy::new(false, 10, false).unwrap(),
        items: moved.clone(),
        delta: Pt { x: 1000, y: 1000 },
    })
    .unwrap();
    doc.apply(Command::Reorient {
        follow: omnia_doc::prefs::FollowPolicy::new(false, 10, false).unwrap(),
        items: moved,
        pivot: Pt { x: 1000, y: 1000 },
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

        let (_, query_within_allocs) = count_allocs(|| doc.query_within(area, &mut items_out));
        assert_eq!(query_within_allocs, 0);
        assert!(!items_out.is_empty());

        let (_, query_touching_allocs) = count_allocs(|| doc.query_touching(area, &mut items_out));
        assert_eq!(query_touching_allocs, 0);
        assert!(!items_out.is_empty());

        let (_, net_allocs) = count_allocs(|| doc.net_wires(net, &mut wires_out));
        assert_eq!(net_allocs, 0);
        assert_eq!(wires_out.len(), 64);
    }
}

fn dense_doc_with_external_pins() -> omnia_doc::Doc {
    use omnia_doc::{Doc, Label, Orient, Part, SymbolId, Wire};

    let pitch = 6 * GRID_NM;
    let half = 2 * GRID_NM;
    let mut parts = Vec::with_capacity(3_600);
    let mut wires = Vec::with_capacity(3_600);
    let mut labels = Vec::with_capacity(60);
    let mut id = 0usize;
    for row in -30..30 {
        labels.push(Label {
            at: Pt {
                x: 0,
                y: row * pitch,
            },
            text: "ROW".into(),
        });
        for col in -30..30 {
            let (x, y) = (col * pitch, row * pitch);
            parts.push((
                Part {
                    symbol: SymbolId(0),
                    at: Pt { x, y },
                    orient: Orient::IDENTITY,
                    designator: "U".into(),
                    value: "".into(),
                },
                // External pins extend beyond the symbol body along X.
                vec![
                    Pt {
                        x: -half - GRID_NM,
                        y: 0,
                    },
                    Pt {
                        x: half + GRID_NM,
                        y: 0,
                    },
                ],
                rect(-half, -half, half, half),
            ));
            let len = if id.is_multiple_of(10) {
                20 * INDEX_CELL_NM
            } else {
                pitch - 2 * half
            };
            wires.push(Wire {
                a: Pt { x: x + half, y },
                b: Pt {
                    x: x + half + len,
                    y,
                },
            });
            id += 1;
        }
    }
    Doc::from_items(parts, wires, labels).unwrap()
}

#[test]
fn doc_query_within_and_touching_large_grid_allocate_nothing_with_capacity() {
    let doc = dense_doc_with_external_pins();
    let areas = [
        // Normalised 4K-ish viewport.
        rect(-96 * GRID_NM, -54 * GRID_NM, 96 * GRID_NM, 54 * GRID_NM),
        // Drawn in reverse (bottom-right to top-left).
        Rect {
            min: Pt {
                x: 96 * GRID_NM,
                y: 54 * GRID_NM,
            },
            max: Pt {
                x: -96 * GRID_NM,
                y: -54 * GRID_NM,
            },
        },
        // Degenerate zero-area horizontal line along row 0's wires.
        Rect {
            min: Pt { x: 0, y: 0 },
            max: Pt {
                x: 100 * GRID_NM,
                y: 0,
            },
        },
        // Whole sheet.
        rect(-1_000_000_000, -1_000_000_000, 1_000_000_000, 1_000_000_000),
    ];

    let mut out = Vec::with_capacity(7_260);
    let capacity = out.capacity();
    let mut within_counts = Vec::new();
    let mut touching_counts = Vec::new();
    for _ in 0..3 {
        for &area in &areas {
            let ((), allocs) = count_allocs(|| doc.query_within(area, &mut out));
            assert_eq!(allocs, 0, "query_within over {area:?} allocated");
            assert_eq!(out.capacity(), capacity);
            within_counts.push(out.len());

            let ((), allocs) = count_allocs(|| doc.query_touching(area, &mut out));
            assert_eq!(allocs, 0, "query_touching over {area:?} allocated");
            assert_eq!(out.capacity(), capacity);
            touching_counts.push(out.len());
        }
    }
    assert!(within_counts[0] > 100, "{within_counts:?}");
    assert_eq!(within_counts[0], within_counts[1]);
    assert_eq!(within_counts[3], 7_260);
    assert!(touching_counts[0] >= within_counts[0]);
    assert_eq!(touching_counts[0], touching_counts[1]);
    assert_eq!(touching_counts[3], 7_260);

    // Control: with zero capacity, growing `out` is counted.
    let mut empty = Vec::new();
    let ((), allocs) = count_allocs(|| doc.query_within(areas[0], &mut empty));
    assert!(allocs > 0);
    let mut empty2 = Vec::new();
    let ((), allocs) = count_allocs(|| doc.query_touching(areas[0], &mut empty2));
    assert!(allocs > 0);
}

#[test]
fn doc_query_within_allocates_nothing_with_capacity() {
    use omnia_doc::{Doc, Label, Orient, Part, SymbolId, Wire};

    let mut parts = Vec::new();
    // 5 parts whose bodies (-10..=10, -10..=10) sit inside the box (0..=100, -10..=10),
    // even though their external pins stick outside the box (x = -50 and x = 150)!
    for i in 0..5 {
        parts.push((
            Part {
                symbol: SymbolId(0),
                at: Pt {
                    x: 10 + i * 20,
                    y: 0,
                },
                orient: Orient::IDENTITY,
                designator: "R".into(),
                value: "".into(),
            },
            vec![Pt { x: -60, y: 0 }, Pt { x: 60, y: 0 }],
            rect(-5, -5, 5, 5),
        ));
    }
    // 5 more parts whose bodies lie at y = 30..=40 (outside the box), with a pin stub at y = 0
    // (inside the box) so the index envelope intersects the box while symbol bodies miss it.
    for i in 0..5 {
        parts.push((
            Part {
                symbol: SymbolId(0),
                at: Pt {
                    x: 10 + i * 20,
                    y: 35,
                },
                orient: Orient::IDENTITY,
                designator: "C".into(),
                value: "".into(),
            },
            vec![Pt { x: 0, y: -35 }],
            rect(-5, -5, 5, 5),
        ));
    }
    let doc = Doc::from_items(
        parts,
        (0..100).map(|x| Wire {
            a: Pt { x, y: -20 },
            b: Pt { x, y: 20 },
        }),
        [Label {
            at: Pt { x: 5, y: 0 },
            text: "NET".into(),
        }],
    )
    .unwrap();

    // Box (0, -10)..(100, 10) intersects all 100 wires and all 10 part envelopes, yet encloses
    // only the 5 inner parts (bodies wholly inside despite external pins) plus the 1 label = 6!
    let mut out = Vec::with_capacity(6);
    for area in [
        rect(0, -10, 100, 10),
        Rect {
            min: Pt { x: 100, y: 10 },
            max: Pt { x: 0, y: -10 },
        },
    ] {
        let (_, allocs) = count_allocs(|| doc.query_within(area, &mut out));
        assert_eq!(allocs, 0);
        assert_eq!(out.len(), 6);
        assert_eq!(out.capacity(), 6);
    }

    // Point box at (5, 0) encloses only the label (the overlapping part at (10, 0) spans 5..=15
    // so it is not wholly inside (5, 0)).
    let (_, allocs) = count_allocs(|| doc.query_within(rect(5, 0, 5, 0), &mut out));
    assert_eq!(allocs, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out.capacity(), 6);

    let mut full = Vec::with_capacity(111);
    for _ in 0..3 {
        let (_, allocs) = count_allocs(|| {
            doc.query_within(
                Rect {
                    min: Pt { x: 200, y: 50 },
                    max: Pt { x: -50, y: -50 },
                },
                &mut full,
            )
        });
        assert_eq!(allocs, 0);
        assert_eq!(full.len(), 111);
    }
}

#[test]
fn doc_query_touching_allocates_nothing_with_capacity() {
    use omnia_doc::{Doc, Label, Orient, Part, SymbolId, Wire};

    let mut parts = Vec::new();
    // 4 parts with bodies inside (0..=100, -10..=10) and external pins sticking out.
    for i in 0..4 {
        parts.push((
            Part {
                symbol: SymbolId(0),
                at: Pt {
                    x: 15 + i * 20,
                    y: 0,
                },
                orient: Orient::IDENTITY,
                designator: "R".into(),
                value: "".into(),
            },
            vec![Pt { x: -50, y: 0 }, Pt { x: 50, y: 0 }],
            rect(-5, -5, 5, 5),
        ));
    }
    // 6 parts whose bodies sit outside (y = 30..=40) with pin stubs reaching into y = 0.
    for i in 0..6 {
        parts.push((
            Part {
                symbol: SymbolId(0),
                at: Pt {
                    x: 10 + i * 15,
                    y: 35,
                },
                orient: Orient::IDENTITY,
                designator: "C".into(),
                value: "".into(),
            },
            vec![Pt { x: 0, y: -35 }],
            rect(-5, -5, 5, 5),
        ));
    }
    let doc = Doc::from_items(
        parts,
        (0..20).map(|x| Wire {
            a: Pt { x: 200 + x, y: -20 },
            b: Pt { x: 200 + x, y: 20 },
        }),
        [Label {
            at: Pt { x: 5, y: 0 },
            text: "NET".into(),
        }],
    )
    .unwrap();

    // Box (0, -10)..(100, 10) hits all 10 part envelopes in the spatial index, yet `query_touching`
    // matches only the 4 parts whose symbol bodies touch the box plus the 1 label = 5!
    let mut out = Vec::with_capacity(5);
    for area in [
        rect(0, -10, 100, 10),
        Rect {
            min: Pt { x: 100, y: 10 },
            max: Pt { x: 0, y: -10 },
        },
    ] {
        let (_, allocs) = count_allocs(|| doc.query_touching(area, &mut out));
        assert_eq!(allocs, 0);
        assert_eq!(out.len(), 5);
        assert_eq!(out.capacity(), 5);
    }

    // Degenerate point box at (5, 0) touches only the label at (5, 0) (the part at x=15 spans
    // 10..=20, so its pin at -35 does not trigger part selection).
    let (_, allocs) = count_allocs(|| doc.query_touching(rect(5, 0, 5, 0), &mut out));
    assert_eq!(allocs, 0);
    assert_eq!(out.len(), 1);
    assert_eq!(out.capacity(), 5);

    let mut full = Vec::with_capacity(31);
    for _ in 0..3 {
        let (_, allocs) = count_allocs(|| {
            doc.query_touching(
                Rect {
                    min: Pt { x: 300, y: 50 },
                    max: Pt { x: -50, y: -50 },
                },
                &mut full,
            )
        });
        assert_eq!(allocs, 0);
        assert_eq!(full.len(), 31);
    }
}
