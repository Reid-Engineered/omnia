//! W2.1: the empty-boundary path allocates nothing, even with selected wires.
use omnia_doc::prefs::FollowPolicy;
use omnia_doc::{
    Doc, FollowPlan, Isometry, ItemId, Label, Orient, Part, Pt, Rect, SymbolId, Wire, plan_follow,
};
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

#[test]
fn empty_boundary_zero_allocations_cold_and_reused() {
    let doc = Doc::from_items(
        [(
            Part {
                symbol: SymbolId(0),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
                designator: "R".into(),
                value: "".into(),
            },
            vec![Pt::ZERO],
            Rect::from_points(Pt::ZERO, Pt::ZERO).unwrap(),
        )],
        [Wire {
            a: Pt { x: 2540000, y: 0 },
            b: Pt { x: 5080000, y: 0 },
        }],
        std::iter::empty::<Label>(),
    )
    .unwrap();
    let items: Vec<_> = doc
        .parts()
        .map(|(id, _)| ItemId::Part(id))
        .chain(doc.wires().map(|(id, _)| ItemId::Wire(id)))
        .collect();
    let mut out = FollowPlan::default();
    let policy = FollowPolicy::new(true, 10, false).unwrap();
    for _ in 0..3 {
        let (result, allocs) = count_allocs(|| {
            plan_follow(
                &doc,
                &items,
                Isometry::Translate(Pt { x: 2540000, y: 0 }),
                policy,
                &mut out,
            )
        });
        assert_eq!(result, Ok(()));
        assert_eq!(allocs, 0);
        assert_eq!(out, FollowPlan::default());
    }
    let (_, allocs) = count_allocs(|| std::hint::black_box(vec![0u8; 512]));
    assert!(allocs > 0);
}
