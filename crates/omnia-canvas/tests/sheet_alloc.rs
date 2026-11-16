//! B10/INV7: `SheetView`'s paint body makes no heap allocation once warm.
//!
//! A test-local counting allocator counts allocations made on this thread inside a scope
//! (the approach of `omnia-bench`'s `alloc` module). The sink stands in for
//! `Window::paint_quad`, which is GPUI's code and outside B10's scope (Q5).

use gpui::{Bounds, point, px, size};
use omnia_canvas::{Camera, SheetView, camera};
use omnia_doc::Pt;
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

fn count_allocs(f: impl FnOnce()) -> u64 {
    ALLOCS.with(|n| n.set(0));
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    ALLOCS.with(Cell::get)
}

#[test]
fn counter_sees_allocations() {
    let n = count_allocs(|| drop(std::hint::black_box(vec![1u8; 64])));
    assert_eq!(n, 1);
}

#[test]
fn paint_makes_no_allocation_after_the_first_frame() {
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: size(px(1920.0), px(1080.0)),
    };
    let mut camera = Camera::new(Pt::ZERO, 1.0, bounds);
    let mut quads = 0u64;
    let first = count_allocs(|| {
        SheetView::new(camera.clone(), &omnia_theme::GRAPHITE)
            .paint_quads(bounds, 1.0, |_| quads += 1);
    });
    println!("first frame: {first} allocations, {quads} quads");

    // Later frames: pan and zoom across K4, through faded levels, at three scale factors.
    let mut frames = 0;
    let mut zoom = 1.0f64;
    for scale_factor in [1.0f32, 1.25, 1.5] {
        for step in 0..200 {
            let factor = if step < 100 { 0.93 } else { 1.0 / 0.93 };
            // Anchor near the centre and alternate the pan so the view stays on the sheet.
            camera.zoom_about(point(960.3, 540.7), factor);
            let sign = if step % 2 == 0 { 1.0 } else { -1.0 };
            camera.pan_by(point(3.25 * sign, -1.5 * sign + 0.125));
            zoom = camera.zoom();
            let view = SheetView::new(camera.clone(), &omnia_theme::GRAPHITE);
            let mut painted = 0u64;
            let allocs = count_allocs(|| view.paint_quads(bounds, scale_factor, |_| painted += 1));
            assert_eq!(
                allocs, 0,
                "frame {frames}: zoom {zoom}, scale factor {scale_factor}"
            );
            assert!(
                painted > 1,
                "frame {frames}: zoom {zoom}, visible {:?}",
                camera.visible()
            );
            frames += 1;
        }
    }
    assert!((camera::MIN_ZOOM..=camera::MAX_ZOOM).contains(&zoom));
}
