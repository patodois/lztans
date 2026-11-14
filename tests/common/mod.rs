//! Helpers shared by the integration tests.
//!
//! Each test file includes this module with `mod common;` and uses only part of
//! it, hence the `dead_code` allowance.

#![allow(dead_code)]

pub mod alloc;
pub mod enc;
pub mod lz;
pub mod vectors;

/// Small deterministic generator (xorshift64); tests seed it with fixed values.
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in `0..n` (`0` when `n == 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    /// Uniform in `lo..=hi`.
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }
    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

/// Array header, long form (5 bytes), array types 1 to 5.
pub fn long(ty: u8, out_len: usize, body: &[u8]) -> Vec<u8> {
    assert!(
        (1..=5).contains(&ty) && body.len() < out_len && out_len <= 1 << 18,
        "long array header: body {} bytes, output {out_len}",
        body.len()
    );
    let d = out_len - 1;
    let c = ((d & 0x3FFF) << 18) | body.len();
    let mut v = vec![(ty << 4) | (d >> 14) as u8];
    v.extend_from_slice(&(c as u32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// Array header, short form (3 bytes), array types 1 to 5.
pub fn short(ty: u8, out_len: usize, body: &[u8]) -> Vec<u8> {
    let s = body.len();
    assert!(s <= 1023 && out_len > s && out_len - s - 1 <= 1023);
    let b = 0x80_0000 | (usize::from(ty) << 20) | ((out_len - s - 1) << 10) | s;
    let mut v = vec![(b >> 16) as u8, (b >> 8) as u8, b as u8];
    v.extend_from_slice(body);
    v
}

/// Stored array, 2-byte header (at most 4095 bytes).
pub fn stored_short(data: &[u8]) -> Vec<u8> {
    assert!(data.len() <= 0xFFF);
    let mut v = vec![0x80 | (data.len() >> 8) as u8, data.len() as u8];
    v.extend_from_slice(data);
    v
}

/// Stored array, 3-byte header.
pub fn stored_long(data: &[u8]) -> Vec<u8> {
    assert!(data.len() < 1 << 18);
    let n = data.len();
    let mut v = vec![(n >> 16) as u8, (n >> 8) as u8, n as u8];
    v.extend_from_slice(data);
    v
}

/// One block (restart flag set, given codec id) holding one quantum with this payload.
pub fn block(codec: u8, payload: &[u8]) -> Vec<u8> {
    assert!(!payload.is_empty() && payload.len() <= 0x3_FFFF);
    let n = payload.len() - 1;
    let mut v = vec![0x8C, codec, (n >> 16) as u8, (n >> 8) as u8, n as u8];
    v.extend_from_slice(payload);
    v
}

/// RLE array (type 3) in the long header form.
pub fn rle(out_len: usize, body: &[u8]) -> Vec<u8> {
    long(3, out_len, body)
}

/// A Kraken block whose only sub-chunk is the given array (entropy-only form).
pub fn stream(array: &[u8]) -> Vec<u8> {
    block(0x06, array)
}

/// LZ sub-chunk: 3-byte header (LZ flag, mode, body size) and body.
pub fn lz_sub(mode: u8, body: &[u8]) -> Vec<u8> {
    let h = 0x80_0000 | (usize::from(mode) << 19) | body.len();
    let mut v = vec![(h >> 16) as u8, (h >> 8) as u8, h as u8];
    v.extend_from_slice(body);
    v
}
