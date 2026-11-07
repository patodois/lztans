//! Heap use of the decoder on RLE arrays that nest a work array of the largest size
//! the array grammar allows, from inputs of a few dozen bytes.
//!
//! This file has one test on purpose: the allocator counters are process-wide.

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

/// Type-3 array, long header form.
fn rle(out_len: usize, body: &[u8]) -> Vec<u8> {
    assert!(body.len() < out_len && out_len <= 1 << 18);
    let d = out_len - 1;
    let c = ((d & 0x3FFF) << 18) | body.len();
    let mut v = vec![0x30 | (d >> 14) as u8];
    v.extend_from_slice(&(c as u32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

fn stream(array: &[u8]) -> Vec<u8> {
    let n = array.len() - 1;
    let mut v = vec![0x8C, 0x06, (n >> 16) as u8, (n >> 8) as u8, n as u8];
    v.extend_from_slice(array);
    v
}

/// Heap bytes in use at the peak while decoding, above what was live before, with a
/// destination buffer that is allocated before the measurement starts.
fn peak_of(data: &[u8], size: usize) -> (usize, bool, bool) {
    // One canary byte after the destination: nothing may write past `size`.
    let mut buf = vec![0xEEu8; size + 1];
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let ok = lztans::decode(data, &mut buf[..size]).is_ok();
    let extra = PEAK.load(Ordering::Relaxed) - base;
    (extra, ok, buf[size] == 0xEE)
}

#[test]
fn nested_work_at_the_limit_stays_within_the_format_bound() {
    const MAX_ARRAY: usize = 1 << 18;
    // The format's bound for one RLE level: the nested array (at most 262,144 bytes)
    // plus the rest of the RLE body (shorter than the RLE's own output), plus the
    // output being built (the RLE's own output).
    let bound = |out_len: usize| MAX_ARRAY + 2 * out_len;

    // 1. A valid stream: work of exactly 262,144 bytes, output of 127 bytes.
    let valid = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/vectors/syn-nested-rle-work-max.bin"
    ))
    .unwrap();
    let (extra, ok, canary) = peak_of(&valid, 127);
    println!(
        "valid, work 262144, output 127: peak {extra} bytes, input {} bytes",
        valid.len()
    );
    assert!(ok && canary);
    assert!(extra <= bound(127), "{extra}");

    // 2. The same stream with the output one byte short: refused, still within the bound.
    let short = stream(&rle(126, &valid[5 + 5..]));
    let (extra, ok, canary) = peak_of(&short, 126);
    println!("refused, output 126: peak {extra} bytes");
    assert!(!ok && canary);
    assert!(extra <= bound(126), "{extra}");

    // 3. A nested array declared at the limit from a one-byte body, with nothing
    //    consuming its work: refused after reserving at most the format's bound.
    let huge = rle(MAX_ARRAY, &[0x41]);
    let mut body = huge.clone();
    body.extend_from_slice(&[0x01; 60]);
    let adversarial = stream(&rle(100, &body));
    let (extra, ok, canary) = peak_of(&adversarial, 100);
    println!(
        "refused, declared work 262144 from {} input bytes, output 100: peak {extra} bytes",
        adversarial.len()
    );
    assert!(!ok && canary);
    assert!(extra <= bound(100), "{extra}");
}
