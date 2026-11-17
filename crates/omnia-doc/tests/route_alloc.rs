//! W1.5: `route` allocates nothing once `out` and its `RouteScratch` are warm.
//!
//! A test-local counting allocator counts allocations made on this thread inside a scope (the
//! approach of `index_alloc.rs`).

use omnia_doc::route::{Axis, Blocker, RouteScratch, route};
use omnia_doc::{Doc, Label, Orient, PIN_LATTICE_NM, Part, Pt, Rect, SymbolId, Wire};
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

fn lp(x: i32, y: i32) -> Pt {
    Pt {
        x: x * PIN_LATTICE_NM,
        y: y * PIN_LATTICE_NM,
    }
}

/// A 12 x 12 block of parts (bodies 4 x 4 lattice units, a pin on the left edge of each), a
/// horizontal wire leaving every pin, and a vertical wire between every column.
fn sheet() -> Doc {
    let mut parts = Vec::new();
    let mut wires = Vec::new();
    for row in 0..12 {
        for col in 0..12 {
            let (x, y) = (col * 12, row * 12);
            parts.push((
                Part {
                    symbol: SymbolId(0),
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "U".into(),
                    value: "1k".into(),
                },
                vec![lp(x, y + 2)],
                Rect::from_points(lp(x, y), lp(x + 4, y + 4)).unwrap(),
            ));
            wires.push(Wire {
                a: lp(x - 4, y + 2),
                b: lp(x, y + 2),
            });
        }
        wires.push(Wire {
            a: lp(row * 12 + 8, -6),
            b: lp(row * 12 + 8, 150),
        });
    }
    Doc::from_items(parts, wires, std::iter::empty::<Label>()).unwrap()
}

#[test]
fn route_allocates_nothing_when_warm() {
    let doc = sheet();
    // Ok routes of every shape class, and refusals.
    let cases = [
        (lp(-4, 2), lp(-4, 40), Axis::Horizontal),
        (lp(-6, 0), lp(30, 30), Axis::Horizontal),
        (lp(-6, 0), lp(30, 30), Axis::Vertical),
        (lp(-4, 2), lp(130, 140), Axis::Vertical),
        (lp(-4, 14), lp(-4, 26), Axis::Horizontal),
        (lp(0, 2), lp(1, 2), Axis::Vertical),
        (Pt { x: 1, y: 0 }, lp(6, 6), Axis::Vertical),
        (lp(500, 500), lp(-500, -500), Axis::Horizontal),
    ];
    let mut scratch = RouteScratch::new();
    let mut out = Vec::new();
    let mut results = Vec::new();
    for &(a, b, leg) in &cases {
        results.push(route(&doc, a, b, leg, &mut scratch, &mut out));
    }
    assert!(results.iter().any(Result::is_ok), "{results:?}");
    assert!(results.contains(&Err(Blocker::Sheet)), "{results:?}");
    for _ in 0..3 {
        for (&(a, b, leg), expected) in cases.iter().zip(&results) {
            let (result, allocs) = count_allocs(|| route(&doc, a, b, leg, &mut scratch, &mut out));
            assert_eq!(allocs, 0, "route {a:?} -> {b:?} allocated");
            assert_eq!(&result, expected);
        }
    }
    // Control: a cold scratch and output allocate, so the zero above is real.
    let ((), allocs) = count_allocs(|| {
        let mut cold = RouteScratch::new();
        let mut out = Vec::new();
        let _ = route(
            &doc, cases[1].0, cases[1].1, cases[1].2, &mut cold, &mut out,
        );
    });
    assert!(allocs > 0);
}
