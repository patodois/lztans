//! Developer tool: peak heap use of the decoder (excluding the caller's output
//! buffer) over a corpus directory. Usage: `peak <corpus-dir>`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = System.alloc(l);
        if !p.is_null() {
            let live = LIVE.fetch_add(l.size(), Ordering::Relaxed) + l.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
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
            LIVE.fetch_sub(l.size(), Ordering::Relaxed);
            let live = LIVE.fetch_add(new, Ordering::Relaxed) + new;
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: peak <corpus-dir>");
    let mut worst = (0usize, String::new());
    let mut n = 0;
    for m in ["manifest.tsv", "entropy_manifest.tsv"] {
        let text = std::fs::read_to_string(format!("{dir}/{m}")).expect("manifest");
        for l in text.lines().skip(1) {
            let c: Vec<&str> = l.split('\t').collect();
            let data = std::fs::read(format!("{dir}/{}", c[1])).expect("read");
            let mut out = vec![0u8; c[2].parse().expect("size")];
            let base = LIVE.load(Ordering::Relaxed);
            PEAK.store(base, Ordering::Relaxed);
            lztans::decode(&data, &mut out).expect("decode");
            let extra = PEAK.load(Ordering::Relaxed) - base;
            if extra > worst.0 {
                worst = (extra, c[0].to_string());
            }
            n += 1;
        }
    }
    println!(
        "{n} streams; largest decoder heap above the output buffer: {} KiB ({})",
        worst.0 / 1024,
        worst.1
    );
}
