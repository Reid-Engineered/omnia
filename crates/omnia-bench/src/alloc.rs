//! Counting global allocator (plan section 9, B10, Q5).
//!
//! Every allocation in the process is counted in "total". Allocations made on a thread while
//! it is inside an [`ours`] scope, and not inside a nested [`not_ours`] scope, are also counted
//! in "ours". Q5: B10 gates "ours" only; "total" is reported, never gated.
//!
//! The binary installs [`CountingAlloc`] with `#[global_allocator]`. Without it every counter
//! stays at zero.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

/// Wraps [`System`] and counts allocations. Reallocations count as allocations.
pub struct CountingAlloc;

static TOTAL_ALLOCS: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);
static OURS_ALLOCS: AtomicU64 = AtomicU64::new(0);
static OURS_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    // Const-initialised and without a destructor, so reading it never allocates and works
    // during thread teardown (`try_with` covers the rest).
    static OURS_DEPTH: Cell<u32> = const { Cell::new(0) };
    static THREAD_ALLOCS: Cell<u64> = const { Cell::new(0) };
}

#[inline]
fn record_alloc(size: usize) {
    TOTAL_ALLOCS.fetch_add(1, Relaxed);
    TOTAL_BYTES.fetch_add(size as u64, Relaxed);
    let _ = THREAD_ALLOCS.try_with(|count| count.set(count.get() + 1));
    if OURS_DEPTH
        .try_with(|depth| depth.get() > 0)
        .unwrap_or(false)
    {
        OURS_ALLOCS.fetch_add(1, Relaxed);
        OURS_BYTES.fetch_add(size as u64, Relaxed);
    }
}

#[inline]
fn grow_live(by: usize) {
    let live = LIVE_BYTES.fetch_add(by, Relaxed) + by;
    PEAK_LIVE_BYTES.fetch_max(live, Relaxed);
}

// SAFETY: every method forwards to `System` with the caller's arguments unchanged; the
// counters are atomics and thread-locals that never allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded contract.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
            grow_live(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded contract.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
            grow_live(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded contract.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            record_alloc(new_size);
            if new_size >= layout.size() {
                grow_live(new_size - layout.size());
            } else {
                LIVE_BYTES.fetch_sub(layout.size() - new_size, Relaxed);
            }
        }
        new_ptr
    }
}

/// Process-wide counter values at one moment. Subtract two with [`AllocCounts::since`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocCounts {
    pub total_allocs: u64,
    pub total_bytes: u64,
    pub ours_allocs: u64,
    pub ours_bytes: u64,
}

impl AllocCounts {
    pub fn now() -> Self {
        Self {
            total_allocs: TOTAL_ALLOCS.load(Relaxed),
            total_bytes: TOTAL_BYTES.load(Relaxed),
            ours_allocs: OURS_ALLOCS.load(Relaxed),
            ours_bytes: OURS_BYTES.load(Relaxed),
        }
    }

    /// The counts accumulated between `earlier` and `self`.
    pub fn since(self, earlier: Self) -> Self {
        Self {
            total_allocs: self.total_allocs.wrapping_sub(earlier.total_allocs),
            total_bytes: self.total_bytes.wrapping_sub(earlier.total_bytes),
            ours_allocs: self.ours_allocs.wrapping_sub(earlier.ours_allocs),
            ours_bytes: self.ours_bytes.wrapping_sub(earlier.ours_bytes),
        }
    }
}

/// Heap bytes currently allocated through [`CountingAlloc`], and the most ever allocated.
pub fn heap_bytes() -> (usize, usize) {
    (LIVE_BYTES.load(Relaxed), PEAK_LIVE_BYTES.load(Relaxed))
}

struct DepthGuard(u32);

impl Drop for DepthGuard {
    fn drop(&mut self) {
        let _ = OURS_DEPTH.try_with(|depth| depth.set(self.0));
    }
}

fn with_depth<R>(depth: impl FnOnce(u32) -> u32, f: impl FnOnce() -> R) -> R {
    let previous = OURS_DEPTH.with(|d| {
        let previous = d.get();
        d.set(depth(previous));
        previous
    });
    let _restore = DepthGuard(previous);
    f()
}

/// Runs `f` with this thread's allocations counted as "ours". Wrap our own paint-path code
/// in it: element prepaint and paint bodies and the per-frame driver.
pub fn ours<R>(f: impl FnOnce() -> R) -> R {
    with_depth(|d| d.saturating_add(1), f)
}

/// Runs `f` with "ours" counting suspended. Wrap calls from our code into GPUI (for example
/// `window.paint_quad`) so GPUI's internal allocations land in "total" only (Q5).
pub fn not_ours<R>(f: impl FnOnce() -> R) -> R {
    with_depth(|_| 0, f)
}

/// Runs `f` and returns the number of allocations this thread made during it, in any scope.
pub fn count_allocs<R>(f: impl FnOnce() -> R) -> (R, u64) {
    let before = THREAD_ALLOCS.with(Cell::get);
    let result = f();
    let after = THREAD_ALLOCS.with(Cell::get);
    (result, after.wrapping_sub(before))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    // Each test reads the process-wide "ours" counter, which only this thread's scoped
    // allocations can move while no other test enters an `ours` scope concurrently. All
    // scope assertions live in this one test to keep that true under the parallel runner.
    #[test]
    fn scopes_separate_ours_from_total() {
        let ((), n) = count_allocs(|| {
            black_box(Vec::<u8>::with_capacity(16));
        });
        assert_eq!(n, 1, "count_allocs sees exactly one allocation");

        let ((), n) = count_allocs(|| black_box(()));
        assert_eq!(n, 0, "no allocation, no count");

        let before = AllocCounts::now();
        ours(|| {
            black_box(Vec::<u8>::with_capacity(32));
            not_ours(|| black_box(Vec::<u8>::with_capacity(64)));
            black_box(Box::new(7_u64));
        });
        let delta = AllocCounts::now().since(before);
        assert_eq!(
            delta.ours_allocs, 2,
            "the nested not_ours allocation is excluded"
        );
        assert_eq!(delta.ours_bytes, 32 + 8);
        assert!(delta.total_allocs >= 3, "total includes every allocation");

        let before = AllocCounts::now();
        black_box(Vec::<u8>::with_capacity(8));
        assert_eq!(
            AllocCounts::now().since(before).ours_allocs,
            0,
            "outside any scope"
        );

        // The scope is restored after a panic inside it.
        let caught = std::panic::catch_unwind(|| ours(|| panic!("inside scope")));
        assert!(caught.is_err());
        let before = AllocCounts::now();
        black_box(Vec::<u8>::with_capacity(8));
        assert_eq!(
            AllocCounts::now().since(before).ours_allocs,
            0,
            "depth restored"
        );
    }

    #[test]
    fn heap_bytes_tracks_live_allocations() {
        let (_, peak_before) = heap_bytes();
        let big = black_box(vec![0_u8; 1 << 20]);
        let (_, peak_after) = heap_bytes();
        assert!(peak_after >= peak_before.max(1 << 20));
        drop(big);
    }
}
