//! Allocator wrapper that tracks live, peak and largest heap allocations.
//!
//! A test file that measures heap use registers it with
//! `#[global_allocator] static ALLOC: common::alloc::Counting = common::alloc::Counting;`.
//! The counters are process-wide.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct Counting;

/// Heap bytes in use.
pub static LIVE: AtomicUsize = AtomicUsize::new(0);
/// Highest value of `LIVE` since the last `start`.
pub static PEAK: AtomicUsize = AtomicUsize::new(0);
/// Largest single allocation since the last `start`.
pub static BIGGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let live = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
            BIGGEST.fetch_max(l.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l);
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = System.realloc(p, l, new);
        if !q.is_null() {
            if new >= l.size() {
                let live = LIVE.fetch_add(new - l.size(), Ordering::Relaxed) + new - l.size();
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(l.size() - new, Ordering::Relaxed);
            }
            BIGGEST.fetch_max(new, Ordering::Relaxed);
        }
        q
    }
}

/// Start a measurement: resets the peak and the largest allocation and returns
/// the bytes in use now.
pub fn start() -> usize {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    BIGGEST.store(0, Ordering::Relaxed);
    base
}

/// Peak bytes in use above `base` since `start` returned it.
pub fn peak_above(base: usize) -> usize {
    PEAK.load(Ordering::Relaxed) - base
}

/// Largest single allocation since the last `start`.
pub fn biggest() -> usize {
    BIGGEST.load(Ordering::Relaxed)
}
