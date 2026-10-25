//! tANS arrays (array type 1).

use super::huffman::{read_symbol_ranges, read_truncated_binary, read_unary, zigzag};
use crate::bits::{Lb, Lf, Mf};
use crate::error::{corrupt, Result};

#[derive(Clone, Copy, Default)]
struct Entry {
    symbol: u8,
    nbits: u8,
    base: u16,
}

/// Read the weight table. Returns `(log2 table size, (symbol, weight) ascending)`.
fn read_weights(r: &mut Mf<'_>) -> Result<(u32, Vec<(u8, u32)>)> {
    if r.read(1) != 0 {
        return corrupt("tans: reserved bit set");
    }
    let lb = r.read(2) + 8;
    let table_size: u32 = 1 << lb;
    let golomb = r.read(1) == 1;
    let mut out: Vec<(u8, u32)> = Vec::new();
    if golomb {
        let q = r.read(3);
        let ns = r.read(8) as usize + 1;
        if ns < 2 {
            return corrupt("tans: fewer than two symbols");
        }
        let xcount = if ns == 256 {
            0
        } else {
            let x = 2 * (257 - ns).min(ns) as u32;
            read_truncated_binary(r, x) as usize
        };
        let mut u = Vec::with_capacity(ns + xcount);
        for _ in 0..ns + xcount {
            u.push(read_unary(r)?);
        }
        let (codes, extras) = u.split_at(ns);
        let syms = read_symbol_ranges(r, ns, extras)?;
        let mut avg: i64 = 6;
        let mut total: u64 = 0;
        for (i, &s) in syms.iter().enumerate() {
            let n_extra = codes[i] + q;
            if n_extra > 15 {
                return corrupt("tans: weight width");
            }
            let mut v: i64 = i64::from(r.read(n_extra)) + (1i64 << n_extra) - (1i64 << q);
            let a4 = avg >> 2;
            let mut limit = 2 * a4;
            if v <= limit {
                v = a4 + i64::from(zigzag(v as u32));
            }
            if limit > v {
                limit = v;
            }
            let w = v + 1;
            if w < 1 {
                return corrupt("tans: weight below one");
            }
            avg += limit - a4;
            total += w as u64;
            if total > u64::from(table_size) {
                return corrupt("tans: weights exceed table size");
            }
            out.push((s, w as u32));
        }
        if total != u64::from(table_size) {
            return corrupt("tans: weights do not fill the table");
        }
    } else {
        let count = r.read(3) as usize + 1;
        let bps = 32 - lb.leading_zeros(); // floor(log2 lb) + 1
        let mdb = r.read(bps);
        if mdb < 1 || mdb > lb {
            return corrupt("tans: sparse weight width");
        }
        let mut seen = [false; 256];
        let mut weight: u32 = 0;
        let mut total: u32 = 0;
        for _ in 0..count {
            let s = r.read(8) as usize;
            if seen[s] {
                return corrupt("tans: repeated symbol");
            }
            seen[s] = true;
            weight += r.read(mdb);
            if weight == 0 {
                return corrupt("tans: zero weight");
            }
            total += weight;
            if total >= table_size {
                return corrupt("tans: weights exceed table size");
            }
            out.push((s as u8, weight));
        }
        let s = r.read(8) as usize;
        if seen[s] {
            return corrupt("tans: repeated symbol");
        }
        let last = table_size - total;
        if last < weight || last <= 1 {
            return corrupt("tans: last weight");
        }
        out.push((s as u8, last));
        out.sort_by_key(|e| e.0);
    }
    if r.overrun() {
        return corrupt("tans: table runs past the array");
    }
    Ok((lb, out))
}

fn build_table(lb: u32, weights: &[(u8, u32)]) -> Result<Vec<Entry>> {
    let size = 1usize << lb;
    let mask = size - 1;
    let singles: Vec<u8> = weights.iter().filter(|e| e.1 == 1).map(|e| e.0).collect();
    let s_states = size - singles.len();
    let mut table = vec![Entry::default(); size];
    for (i, &sym) in singles.iter().enumerate() {
        table[s_states + i] = Entry {
            symbol: sym,
            nbits: lb as u8,
            base: 0,
        };
    }
    let a = s_states / 4;
    let rem = s_states % 4;
    let mut region_start = [0usize; 4];
    let mut region_end = [0usize; 4];
    let mut acc = 0usize;
    for j in 0..4 {
        region_start[j] = acc;
        acc += a + usize::from(j < rem);
        region_end[j] = acc;
    }
    let mut cursor = region_start;
    let mut p: usize = 0;
    for &(sym, w) in weights.iter().filter(|e| e.1 > 1) {
        let w = w as usize;
        let mut k = 0usize;
        for j in 0..4 {
            let off = (j + 4 - (p % 4)) % 4;
            let cj = if w > off { (w - off).div_ceil(4) } else { 0 };
            for _ in 0..cj {
                if cursor[j] >= region_end[j] {
                    return corrupt("tans: region overflow");
                }
                let ww = w + k;
                let nbits = lb - ww.ilog2();
                let base = ((ww << nbits) & mask) as u16;
                table[cursor[j]] = Entry {
                    symbol: sym,
                    nbits: nbits as u8,
                    base,
                };
                cursor[j] += 1;
                k += 1;
            }
        }
        p += w;
    }
    Ok(table)
}

pub(crate) fn decode(body: &[u8], dst_size: usize) -> Result<Vec<u8>> {
    if dst_size < 5 || body.len() < 8 {
        return corrupt("tans: array too small");
    }
    let mut r = Mf::new(body);
    let (lb, weights) = read_weights(&mut r)?;
    let table = build_table(lb, &weights)?;
    let p = r.byte_pos();
    if p >= body.len() {
        return corrupt("tans: no payload");
    }
    let payload = &body[p..];
    let mut f = Lf::new(payload);
    let mut b = Lb::new(payload);
    let mut st = [0usize; 5];
    st[0] = f.read(lb) as usize;
    st[1] = b.read(lb) as usize;
    st[2] = f.read(lb) as usize;
    st[3] = b.read(lb) as usize;
    st[4] = f.read(lb) as usize;
    let mut out = vec![0u8; dst_size];
    let main = dst_size - 5;
    let t = &table[..];
    let mut i = 0usize;
    while i < main {
        let from_f = (i / 5) % 2 == 0;
        let end = (i - i % 5 + 5).min(main);
        if from_f {
            while i < end {
                let j = i % 5;
                let e = t[st[j]];
                out[i] = e.symbol;
                f.refill();
                let bits = (f.bits() & ((1u64 << e.nbits) - 1)) as usize;
                f.consume(u32::from(e.nbits));
                st[j] = usize::from(e.base) + bits;
                i += 1;
            }
        } else {
            while i < end {
                let j = i % 5;
                let e = t[st[j]];
                out[i] = e.symbol;
                b.refill();
                let bits = (b.bits() & ((1u64 << e.nbits) - 1)) as usize;
                b.consume(u32::from(e.nbits));
                st[j] = usize::from(e.base) + bits;
                i += 1;
            }
        }
    }
    for (k, &s) in st.iter().enumerate() {
        if s > 255 {
            return corrupt("tans: final state is not a byte");
        }
        out[main + k] = s as u8;
    }
    if Some(p + f.byte_pos()) != body.len().checked_sub(b.bytes_used()) {
        return corrupt("tans: bit streams not exactly consumed");
    }
    Ok(out)
}
