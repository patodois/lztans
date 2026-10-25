//! The four bit-stream conventions of the format.
//!
//! * [`Mf`]: most significant bit first, forward.
//! * [`Mb`]: most significant bit first, backward (starts at the last byte).
//! * [`Lf`]: least significant bit first, forward (buffered, for hot loops).
//! * [`Lb`]: least significant bit first, backward (buffered, for hot loops).
//!
//! All readers work on a byte region and supply zero bits when read past the
//! region. The number of bits consumed is tracked exactly so callers can verify
//! that streams end where they should.

#[inline(always)]
fn load8_be(data: &[u8], idx: usize) -> u64 {
    if let Some(s) = data.get(idx..idx.wrapping_add(8)) {
        let mut b = [0u8; 8];
        b.copy_from_slice(s);
        u64::from_be_bytes(b)
    } else {
        let mut v = 0u64;
        for i in 0..8 {
            let byte = data.get(idx.wrapping_add(i)).copied().unwrap_or(0);
            v = (v << 8) | u64::from(byte);
        }
        v
    }
}

/// Eight bytes ending `idx` bytes before the end of `data`; the byte nearest the
/// end is the most significant one of the result. Missing bytes are zero.
#[inline(always)]
fn load8_from_end(data: &[u8], idx: usize) -> u64 {
    let len = data.len();
    if idx <= len && len - idx >= 8 {
        let a = len - idx - 8;
        let mut b = [0u8; 8];
        b.copy_from_slice(&data[a..a + 8]);
        u64::from_le_bytes(b)
    } else {
        let mut v = 0u64;
        for i in 0..8 {
            let byte = if idx + i < len {
                data[len - 1 - idx - i]
            } else {
                0
            };
            v = (v << 8) | u64::from(byte);
        }
        v
    }
}

/// MSB-first forward reader.
#[derive(Clone)]
pub(crate) struct Mf<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> Mf<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Mf { data, bit: 0 }
    }

    #[inline(always)]
    fn window(&self) -> u64 {
        load8_be(self.data, self.bit >> 3) << (self.bit & 7)
    }

    /// Next `k` bits (0..=32) without consuming them.
    #[inline(always)]
    pub fn peek(&self, k: u32) -> u32 {
        if k == 0 {
            0
        } else {
            (self.window() >> (64 - k)) as u32
        }
    }

    #[inline(always)]
    pub fn consume(&mut self, k: u32) {
        self.bit += k as usize;
    }

    #[inline(always)]
    pub fn read(&mut self, k: u32) -> u32 {
        let v = self.peek(k);
        self.consume(k);
        v
    }

    /// Number of zero bits before the next one bit (at most 31), or `None` if
    /// the next 32 bits are all zero.
    #[inline(always)]
    pub fn leading_zeros(&self) -> Option<u32> {
        let w = self.peek(32);
        if w == 0 {
            None
        } else {
            Some(w.leading_zeros())
        }
    }

    /// Byte position just after the bits consumed so far.
    pub fn byte_pos(&self) -> usize {
        self.bit.div_ceil(8)
    }

    /// True if more bits were consumed than the region holds.
    pub fn overrun(&self) -> bool {
        self.bit > self.data.len() * 8
    }
}

/// MSB-first backward reader.
#[derive(Clone)]
pub(crate) struct Mb<'a> {
    data: &'a [u8],
    bit: usize,
}

impl<'a> Mb<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Mb { data, bit: 0 }
    }

    #[inline(always)]
    fn window(&self) -> u64 {
        load8_from_end(self.data, self.bit >> 3) << (self.bit & 7)
    }

    #[inline(always)]
    pub fn peek(&self, k: u32) -> u32 {
        if k == 0 {
            0
        } else {
            (self.window() >> (64 - k)) as u32
        }
    }

    #[inline(always)]
    pub fn consume(&mut self, k: u32) {
        self.bit += k as usize;
    }

    #[inline(always)]
    pub fn read(&mut self, k: u32) -> u32 {
        let v = self.peek(k);
        self.consume(k);
        v
    }

    #[inline(always)]
    pub fn leading_zeros(&self) -> Option<u32> {
        let w = self.peek(32);
        if w == 0 {
            None
        } else {
            Some(w.leading_zeros())
        }
    }

    /// Number of bytes consumed from the end (a partly used byte counts).
    pub fn bytes_used(&self) -> usize {
        self.bit.div_ceil(8)
    }

    pub fn overrun(&self) -> bool {
        self.bit > self.data.len() * 8
    }
}

/// LSB-first forward reader with a 64-bit buffer.
pub(crate) struct Lf<'a> {
    data: &'a [u8],
    /// Number of bytes loaded so far (may exceed `data.len()`: zero fill).
    pos: usize,
    buf: u64,
    cnt: u32,
}

impl<'a> Lf<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let mut r = Lf {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        };
        r.refill();
        r
    }

    /// Make at least 56 bits available.
    #[inline(always)]
    pub fn refill(&mut self) {
        if let Some(s) = self.data.get(self.pos..self.pos.wrapping_add(8)) {
            let mut b = [0u8; 8];
            b.copy_from_slice(s);
            self.buf |= u64::from_le_bytes(b) << self.cnt;
            let take = (63 - self.cnt) >> 3;
            self.pos += take as usize;
            self.cnt += take * 8;
        } else {
            self.refill_slow();
        }
    }

    #[cold]
    #[inline(never)]
    fn refill_slow(&mut self) {
        while self.cnt <= 56 {
            let byte = self.data.get(self.pos).copied().unwrap_or(0);
            self.buf |= u64::from(byte) << self.cnt;
            self.cnt += 8;
            self.pos += 1;
        }
    }

    /// Low bits of the buffer (call after `refill`).
    #[inline(always)]
    pub fn bits(&self) -> u64 {
        self.buf
    }

    #[inline(always)]
    pub fn consume(&mut self, k: u32) {
        self.buf >>= k;
        self.cnt = self.cnt.saturating_sub(k);
    }

    #[inline(always)]
    pub fn read(&mut self, k: u32) -> u32 {
        self.refill();
        let v = (self.buf & ((1u64 << k) - 1)) as u32;
        self.consume(k);
        v
    }

    pub fn bits_used(&self) -> usize {
        self.pos * 8 - self.cnt as usize
    }

    pub fn byte_pos(&self) -> usize {
        self.bits_used().div_ceil(8)
    }
}

/// LSB-first backward reader with a 64-bit buffer.
pub(crate) struct Lb<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u64,
    cnt: u32,
}

impl<'a> Lb<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let mut r = Lb {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        };
        r.refill();
        r
    }

    #[inline(always)]
    pub fn refill(&mut self) {
        let len = self.data.len();
        if len >= self.pos + 8 {
            let a = len - self.pos - 8;
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.data[a..a + 8]);
            self.buf |= u64::from_be_bytes(b) << self.cnt;
            let take = (63 - self.cnt) >> 3;
            self.pos += take as usize;
            self.cnt += take * 8;
        } else {
            self.refill_slow();
        }
    }

    #[cold]
    #[inline(never)]
    fn refill_slow(&mut self) {
        let len = self.data.len();
        while self.cnt <= 56 {
            let byte = if self.pos < len {
                self.data[len - 1 - self.pos]
            } else {
                0
            };
            self.buf |= u64::from(byte) << self.cnt;
            self.cnt += 8;
            self.pos += 1;
        }
    }

    #[inline(always)]
    pub fn bits(&self) -> u64 {
        self.buf
    }

    #[inline(always)]
    pub fn consume(&mut self, k: u32) {
        self.buf >>= k;
        self.cnt = self.cnt.saturating_sub(k);
    }

    #[inline(always)]
    pub fn read(&mut self, k: u32) -> u32 {
        self.refill();
        let v = (self.buf & ((1u64 << k) - 1)) as u32;
        self.consume(k);
        v
    }

    pub fn bits_used(&self) -> usize {
        self.pos * 8 - self.cnt as usize
    }

    /// Bytes consumed from the end of the region (a partly used byte counts).
    pub fn bytes_used(&self) -> usize {
        self.bits_used().div_ceil(8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mf_example() {
        let mut r = Mf::new(&[0xB5, 0x40]);
        assert_eq!(r.read(3), 5);
        assert_eq!(r.read(7), 85);
    }

    #[test]
    fn lf_example() {
        let mut r = Lf::new(&[0xB5, 0x40]);
        assert_eq!(r.read(3), 5);
        assert_eq!(r.read(7), 22);
    }

    #[test]
    fn backward_readers_mirror_forward() {
        let fwd = [0xB5u8, 0x40];
        let bwd = [0x40u8, 0xB5];
        let mut a = Mb::new(&bwd);
        let mut b = Mf::new(&fwd);
        for k in [3, 7, 4, 9] {
            assert_eq!(a.read(k), b.read(k));
        }
        let mut c = Lb::new(&bwd);
        let mut d = Lf::new(&fwd);
        for k in [3, 7, 4, 9] {
            assert_eq!(c.read(k), d.read(k));
        }
    }

    #[test]
    fn zero_fill_and_positions() {
        let mut r = Mf::new(&[0xFF]);
        assert_eq!(r.read(8), 0xFF);
        assert_eq!(r.read(16), 0);
        assert!(r.overrun());
        let mut r = Lf::new(&[0xFF, 1]);
        r.read(9);
        assert_eq!(r.byte_pos(), 2);
        assert_eq!(r.read(7), 0);
        let mut r = Lb::new(&[1, 0xFF, 7]);
        r.read(3);
        assert_eq!(r.bytes_used(), 1);
    }
}
