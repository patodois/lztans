//! Huffman arrays (array types 2 and 4).

use crate::bits::{Lb, Lf, Mf};
use crate::error::{corrupt, Result};

pub(crate) const MAX_LEN: usize = 11;
const TABLE_SIZE: usize = 1 << MAX_LEN;

#[inline]
pub(crate) fn zigzag(v: u32) -> i32 {
    ((v >> 1) as i32) ^ -((v & 1) as i32)
}

/// Unary count: zero bits before a one bit.
pub(crate) fn read_unary(r: &mut Mf<'_>) -> Result<u32> {
    let mut total = 0u32;
    loop {
        match r.leading_zeros() {
            Some(z) => {
                r.consume(z + 1);
                total += z;
                if total >= 4096 {
                    return corrupt("unary count too large");
                }
                return Ok(total);
            }
            None => {
                r.consume(32);
                total += 32;
                if total >= 4096 {
                    return corrupt("unary count too large");
                }
            }
        }
    }
}

/// Truncated binary code for an alphabet of `x >= 2` values.
pub(crate) fn read_truncated_binary(r: &mut Mf<'_>, x: u32) -> u32 {
    let y = 32 - (x - 1).leading_zeros(); // ceil(log2 x)
    let u = (1u32 << y) - x;
    let v = r.peek(y);
    if (v >> 1) >= u {
        r.consume(y);
        v - u
    } else {
        r.consume(y - 1);
        v >> 1
    }
}

/// Symbol-range rule shared by Huffman and tANS table descriptions. Returns the
/// coded symbols in ascending order (exactly `ns` of them).
pub(crate) fn read_symbol_ranges(r: &mut Mf<'_>, ns: usize, extras: &[u32]) -> Result<Vec<u8>> {
    let mut syms: Vec<u8> = Vec::with_capacity(ns);
    let mut sym: usize = 0;
    let mut used: usize = 0;
    let mut j = 0usize;
    let x = extras.len();
    if x % 2 == 1 {
        let v = extras[0];
        j = 1;
        if v >= 8 {
            return corrupt("symbol range: initial gap exponent");
        }
        sym = r.read(v + 1) as usize + (1usize << (v + 1)) - 1;
    }
    for _ in 0..x / 2 {
        let v1 = extras[j];
        j += 1;
        if v1 >= 9 {
            return corrupt("symbol range: count exponent");
        }
        let count = r.read(v1) as usize + (1usize << v1);
        let v2 = extras[j];
        j += 1;
        if v2 >= 8 {
            return corrupt("symbol range: gap exponent");
        }
        let gap = r.read(v2 + 1) as usize + (1usize << (v2 + 1)) - 1;
        if sym + count > 256 {
            return corrupt("symbol range beyond 255");
        }
        for s in sym..sym + count {
            syms.push(s as u8);
        }
        used += count;
        sym += count + gap;
    }
    if sym >= 256 || used >= ns || sym + (ns - used) > 256 {
        return corrupt("symbol range: inconsistent");
    }
    for s in sym..sym + (ns - used) {
        syms.push(s as u8);
    }
    Ok(syms)
}

/// Lists of symbols per code length (index 1..=11).
struct LengthLists {
    lists: [Vec<u8>; MAX_LEN + 1],
}

impl LengthLists {
    fn new() -> Self {
        LengthLists {
            lists: Default::default(),
        }
    }
}

/// Number of leading zero bits (at most 31) as a `Result`.
fn lz(r: &Mf<'_>) -> Result<u32> {
    match r.leading_zeros() {
        Some(z) => Ok(z),
        None => corrupt("too many leading zero bits"),
    }
}

/// Doubled-gamma code of the old dense description.
fn read_gd(r: &mut Mf<'_>) -> Result<u32> {
    if r.peek(8) == 0 {
        return corrupt("doubled-gamma code starts with eight zero bits");
    }
    let z = lz(r)?;
    r.consume(z + 1);
    let rest = r.read(z + 1);
    Ok((1u32 << (z + 1)) + rest - 1)
}

fn read_new(r: &mut Mf<'_>, lists: &mut LengthLists) -> Result<usize> {
    let k = r.read(2);
    let ns = r.read(8) as usize + 1;
    let xcount = if ns == 256 {
        0
    } else {
        let x = 2 * (257 - ns).min(ns) as u32;
        read_truncated_binary(r, x) as usize
    };
    let mut u = Vec::with_capacity(ns + xcount);
    for _ in 0..ns + xcount {
        let v = read_unary(r)?;
        if v >= 256 {
            return corrupt("huffman: unary count >= 256");
        }
        u.push(v);
    }
    let (codes, extras) = u.split_at(ns);
    let mut lens: Vec<usize> = Vec::with_capacity(ns);
    let mut c: Vec<u32> = codes.to_vec();
    if k > 0 {
        for ci in c.iter_mut() {
            let f = r.read(k);
            *ci = (*ci << k) | f;
        }
    }
    let mut run: i32 = 30;
    for &ci in &c {
        let d = zigzag(ci);
        let len = d + (run >> 2) + 1;
        if !(1..=MAX_LEN as i32).contains(&len) {
            return corrupt("huffman: code length out of range");
        }
        lens.push(len as usize);
        run += d;
    }
    let syms = read_symbol_ranges(r, ns, extras)?;
    // Within one length, symbols are listed in ascending order; `syms` is ascending.
    for (s, l) in syms.iter().zip(lens.iter()) {
        lists.lists[*l].push(*s);
    }
    Ok(ns)
}

fn read_old_sparse(r: &mut Mf<'_>, lists: &mut LengthLists) -> Result<usize> {
    let ns = r.read(8) as usize;
    if ns == 0 {
        return corrupt("huffman: empty sparse description");
    }
    if ns == 1 {
        return corrupt("huffman: single-symbol description is not supported");
    }
    let cb = r.read(3);
    if cb > 4 {
        return corrupt("huffman: sparse length width");
    }
    for _ in 0..ns {
        let s = r.read(8) as u8;
        let len = r.read(cb) as usize + 1;
        if len > MAX_LEN {
            return corrupt("huffman: sparse code length");
        }
        lists.lists[len].push(s);
    }
    Ok(ns)
}

fn read_old_dense(r: &mut Mf<'_>, lists: &mut LengthLists) -> Result<usize> {
    let k = r.read(2);
    let zmax = 20u32 >> k;
    let mut skip = r.read(1) == 1;
    let mut sym: usize = 0;
    let mut coded: usize = 0;
    let mut avg4: i32 = 32;
    while sym < 256 {
        if skip {
            skip = false;
        } else {
            let g = read_gd(r)? as usize;
            sym += g;
            if sym >= 256 {
                break;
            }
        }
        let n = read_gd(r)? as usize;
        if sym + n > 256 {
            return corrupt("huffman: dense group beyond 255");
        }
        coded += n;
        for _ in 0..n {
            let z = lz(r)?;
            if z > zmax {
                return corrupt("huffman: dense code too long");
            }
            r.consume(z + 1);
            let f = r.read(k);
            let v = (z << k) | f;
            let len = zigzag(v) + ((avg4 + 2) >> 2);
            if !(1..=MAX_LEN as i32).contains(&len) {
                return corrupt("huffman: dense code length out of range");
            }
            avg4 = len + ((3 * avg4 + 2) >> 2);
            lists.lists[len as usize].push(sym as u8);
            sym += 1;
        }
    }
    if sym != 256 || coded < 2 {
        return corrupt("huffman: dense description inconsistent");
    }
    Ok(coded)
}

/// Decoding table: entry `idx` holds `symbol | (len << 8)`.
pub(crate) struct HuffTable {
    entries: Vec<u16>,
}

fn build_table(lists: &LengthLists) -> Result<HuffTable> {
    let mut entries = vec![0u16; TABLE_SIZE];
    let mut slot = 0usize;
    for len in 1..=MAX_LEN {
        let width = 1usize << (MAX_LEN - len);
        for &s in &lists.lists[len] {
            if slot + width > TABLE_SIZE {
                return corrupt("huffman: code space overfull");
            }
            let entry = u16::from(s) | ((len as u16) << 8);
            for sl in slot..slot + width {
                let idx = (sl as u32).reverse_bits() >> (32 - MAX_LEN as u32);
                entries[idx as usize] = entry;
            }
            slot += width;
        }
    }
    if slot != TABLE_SIZE {
        return corrupt("huffman: code space not full");
    }
    Ok(HuffTable { entries })
}

/// Read a code description from `r` and build the table.
fn read_description(r: &mut Mf<'_>) -> Result<HuffTable> {
    let mut lists = LengthLists::new();
    let a = r.read(1);
    if a == 1 {
        if r.read(1) != 0 {
            return corrupt("huffman: invalid description selector");
        }
        read_new(r, &mut lists)?;
    } else if r.read(1) == 1 {
        read_old_dense(r, &mut lists)?;
    } else {
        read_old_sparse(r, &mut lists)?;
    }
    if r.overrun() {
        return corrupt("huffman: description runs past the array");
    }
    build_table(&lists)
}

#[inline(always)]
fn dec_f(r: &mut Lf<'_>, t: &[u16]) -> u8 {
    r.refill();
    let e = t[(r.bits() & (TABLE_SIZE as u64 - 1)) as usize];
    r.consume(u32::from(e >> 8));
    e as u8
}

#[inline(always)]
fn dec_b(r: &mut Lb<'_>, t: &[u16]) -> u8 {
    r.refill();
    let e = t[(r.bits() & (TABLE_SIZE as u64 - 1)) as usize];
    r.consume(u32::from(e >> 8));
    e as u8
}

/// Decode a three-stream payload (`region` starts with the u16 `m`).
fn decode_three(region: &[u8], out: &mut [u8], table: &HuffTable) -> Result<()> {
    if region.len() < 2 {
        return corrupt("huffman: payload too short");
    }
    let m = usize::from(region[0]) | (usize::from(region[1]) << 8);
    if 2 + m > region.len() {
        return corrupt("huffman: stream A length");
    }
    let a_bytes = &region[2..2 + m];
    let bc = &region[2 + m..];
    let t = &table.entries[..];
    let mut a = Lf::new(a_bytes);
    let mut b = Lf::new(bc);
    let mut c = Lb::new(bc);
    let n = out.len();
    let mut chunks = out.chunks_exact_mut(3);
    for ch in &mut chunks {
        ch[0] = dec_f(&mut a, t);
        ch[1] = dec_b(&mut c, t);
        ch[2] = dec_f(&mut b, t);
    }
    let rem = chunks.into_remainder();
    if !rem.is_empty() {
        rem[0] = dec_f(&mut a, t);
        if rem.len() > 1 {
            rem[1] = dec_b(&mut c, t);
        }
    }
    let _ = n;
    if a.byte_pos() != m || b.byte_pos() + c.bytes_used() != bc.len() {
        return corrupt("huffman: bit streams not exactly consumed");
    }
    Ok(())
}

/// Decode a Huffman array body (`body` includes the code description).
pub(crate) fn decode(body: &[u8], dst_size: usize, six_streams: bool) -> Result<Vec<u8>> {
    let mut r = Mf::new(body);
    let table = read_description(&mut r)?;
    let p = r.byte_pos();
    let rest = &body[p.min(body.len())..];
    let mut out = vec![0u8; dst_size];
    if !six_streams {
        decode_three(rest, &mut out, &table)?;
    } else {
        if rest.len() < 3 {
            return corrupt("huffman: payload too short");
        }
        let m = usize::from(rest[0]) | (usize::from(rest[1]) << 8) | (usize::from(rest[2]) << 16);
        let rest = &rest[3..];
        if m > rest.len() {
            return corrupt("huffman: six-stream split");
        }
        let (left, right) = rest.split_at(m);
        let half = dst_size.div_ceil(2);
        let (o1, o2) = out.split_at_mut(half);
        decode_three(left, o1, &table)?;
        decode_three(right, o2, &table)?;
    }
    Ok(out)
}
