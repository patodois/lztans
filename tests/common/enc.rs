//! Test-side encoder for entropy-coded arrays: builds valid arrays from the
//! rules the decoder in `src/entropy/` applies, together with the bytes they
//! must decode to.
//!
//! These builders follow this crate's own decoder, so a round trip through them
//! guards against regressions; it does not prove conformance to the format
//! (the game-data corpus does that, see `tests/vectors.rs`).

use super::{long, stored_long, stored_short, Rng};

/// MSB-first bit writer.
#[derive(Default, Clone)]
pub struct Msb {
    pub bits: Vec<bool>,
}

impl Msb {
    pub fn put(&mut self, v: u32, n: u32) {
        for i in (0..n).rev() {
            self.bits.push((v >> i) & 1 == 1);
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.bits.len().div_ceil(8)];
        for (i, &b) in self.bits.iter().enumerate() {
            if b {
                out[i / 8] |= 0x80 >> (i % 8);
            }
        }
        out
    }
}

/// LSB-first bit writer.
#[derive(Default, Clone)]
pub struct Lsb {
    pub bits: Vec<bool>,
}

impl Lsb {
    pub fn put(&mut self, v: u32, n: u32) {
        for i in 0..n {
            self.bits.push((v >> i) & 1 == 1);
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.bits.len().div_ceil(8)];
        for (i, &b) in self.bits.iter().enumerate() {
            if b {
                out[i / 8] |= 1 << (i % 8);
            }
        }
        out
    }
}

pub fn reversed(mut v: Vec<u8>) -> Vec<u8> {
    v.reverse();
    v
}

/// `n` distinct random symbols.
fn distinct_symbols(rng: &mut Rng, n: usize) -> Vec<u8> {
    let mut syms: Vec<u8> = Vec::new();
    while syms.len() < n {
        let s = rng.next() as u8;
        if !syms.contains(&s) {
            syms.push(s);
        }
    }
    syms
}

// ------------------------------------------------------------ Huffman (types 2 and 4)

/// Three-stream payload: u16 size of stream A, A, B forward, C backward.
fn huffman_three(data: &[u8], code: &[(u32, u32); 256]) -> Vec<u8> {
    let (mut a, mut b, mut c) = (Lsb::default(), Lsb::default(), Lsb::default());
    for (i, &s) in data.iter().enumerate() {
        let (val, len) = code[s as usize];
        let w = match i % 3 {
            0 => &mut a,
            1 => &mut c,
            _ => &mut b,
        };
        for k in (0..len).rev() {
            w.put((val >> k) & 1, 1);
        }
    }
    let ab = a.bytes();
    let mut out = vec![ab.len() as u8, (ab.len() >> 8) as u8];
    out.extend(ab);
    out.extend(b.bytes());
    out.extend(reversed(c.bytes()));
    out
}

/// A Huffman array (old sparse code description) with random content: type 4
/// (six streams) when `six`, else type 2. Returns `(array, decoded bytes)`, or
/// `None` when the array would not be smaller than its output.
pub fn huffman(rng: &mut Rng, out_len: usize, six: bool) -> Option<(Vec<u8>, Vec<u8>)> {
    let k = rng.range(2, 60);
    // Code lengths of a complete code: split a leaf until there are k.
    let mut lens = vec![1u32, 1];
    while lens.len() < k {
        let i = rng.below(lens.len());
        if lens[i] < 11 {
            lens[i] += 1;
            let l = lens[i];
            lens.push(l);
        }
    }
    let syms = distinct_symbols(rng, k);
    let mut d = Msb::default();
    d.put(0, 1); // old description
    d.put(0, 1); // sparse form
    d.put(k as u32, 8);
    d.put(4, 3); // 4 bits per length
    for (s, l) in syms.iter().zip(&lens) {
        d.put(u32::from(*s), 8);
        d.put(l - 1, 4);
    }
    // Canonical codes: by length, then in description order.
    let mut code = [(0u32, 0u32); 256];
    let mut slot = 0u32;
    for len in 1..=11u32 {
        for (s, l) in syms.iter().zip(&lens) {
            if *l == len {
                code[*s as usize] = (slot >> (11 - len), len);
                slot += 1 << (11 - len);
            }
        }
    }
    assert_eq!(slot, 2048);
    let data: Vec<u8> = (0..out_len).map(|_| syms[rng.below(k)]).collect();
    let mut body = d.bytes();
    if six {
        let half = out_len.div_ceil(2);
        let left = huffman_three(&data[..half], &code);
        let right = huffman_three(&data[half..], &code);
        body.extend_from_slice(&[
            left.len() as u8,
            (left.len() >> 8) as u8,
            (left.len() >> 16) as u8,
        ]);
        body.extend(left);
        body.extend(right);
    } else {
        body.extend(huffman_three(&data, &code));
    }
    if body.len() >= out_len {
        return None;
    }
    Some((long(if six { 4 } else { 2 }, out_len, &body), data))
}

// ------------------------------------------------------------ tANS (type 1, sparse weight table)

#[derive(Clone, Copy, Default)]
struct Ent {
    symbol: u8,
    nbits: u32,
    base: usize,
}

/// Decoding table for `weights`, laid out as the decoder lays it out.
fn tans_table(tb: u32, weights: &[(u8, u32)]) -> Vec<Ent> {
    let size = 1usize << tb;
    let mut w: Vec<(u8, u32)> = weights.to_vec();
    w.sort_by_key(|e| e.0);
    let singles: Vec<u8> = w.iter().filter(|e| e.1 == 1).map(|e| e.0).collect();
    let s_states = size - singles.len();
    let mut table = vec![Ent::default(); size];
    for (i, &s) in singles.iter().enumerate() {
        table[s_states + i] = Ent {
            symbol: s,
            nbits: tb,
            base: 0,
        };
    }
    let mut start = [0usize; 4];
    let mut acc = 0;
    for (j, st) in start.iter_mut().enumerate() {
        *st = acc;
        acc += s_states / 4 + usize::from(j < s_states % 4);
    }
    let mut cursor = start;
    let mut p = 0usize;
    for &(sym, wt) in w.iter().filter(|e| e.1 > 1) {
        let wt = wt as usize;
        let mut k = 0;
        for (j, cur) in cursor.iter_mut().enumerate() {
            let off = (j + 4 - p % 4) % 4;
            let cj = if wt > off { (wt - off).div_ceil(4) } else { 0 };
            for _ in 0..cj {
                let wv = wt + k;
                let nbits = tb - wv.ilog2();
                table[*cur] = Ent {
                    symbol: sym,
                    nbits,
                    base: (wv << nbits) & (size - 1),
                };
                *cur += 1;
                k += 1;
            }
        }
        p += wt;
    }
    table
}

/// A tANS array (type 1, sparse weight table) with random content. Returns
/// `(array, decoded bytes)`, or `None` when the array would not be smaller than
/// its output.
pub fn tans(rng: &mut Rng, out_len: usize) -> Option<(Vec<u8>, Vec<u8>)> {
    if out_len < 5 {
        return None;
    }
    let tb = rng.range(8, 11) as u32;
    let size = 1u32 << tb;
    let (count, mdb, deltas, last) = loop {
        let count = rng.range(1, 8);
        let mdb = rng.range(1, tb as usize) as u32;
        let cap = mdb.min(if rng.below(2) == 0 { 2 } else { 16 });
        let deltas: Vec<u32> = (0..count).map(|_| rng.below(1 << cap) as u32).collect();
        let (mut w, mut total) = (0u32, 0u32);
        let mut ok = true;
        for &d in &deltas {
            w += d;
            total += w;
            ok &= w != 0 && total < size;
        }
        if ok && size - total >= w && size - total > 1 {
            break (count, mdb, deltas, size - total);
        }
    };
    let syms = distinct_symbols(rng, count + 1);
    let mut d = Msb::default();
    d.put(0, 1); // reserved
    d.put(tb - 8, 2);
    d.put(0, 1); // sparse table
    d.put(count as u32 - 1, 3);
    d.put(mdb, 4);
    let mut weights = Vec::new();
    let mut w = 0;
    for i in 0..count {
        d.put(u32::from(syms[i]), 8);
        d.put(deltas[i], mdb);
        w += deltas[i];
        weights.push((syms[i], w));
    }
    d.put(u32::from(syms[count]), 8);
    weights.push((syms[count], last));
    let table = tans_table(tb, &weights);
    // For each symbol, the table entry that a state `y` must come from.
    let mut from: Vec<Vec<usize>> = vec![Vec::new(); 256];
    for (x, e) in table.iter().enumerate() {
        let f = &mut from[e.symbol as usize];
        if f.is_empty() {
            f.resize(size as usize, usize::MAX);
        }
        f[e.base..e.base + (1usize << e.nbits)].fill(x);
    }
    let main = out_len - 5;
    let mut data: Vec<u8> = (0..main).map(|_| syms[rng.below(syms.len())]).collect();
    data.extend(rng.bytes(5));
    // Encode backwards from the final states (the last five output bytes).
    let mut st = [0usize; 5];
    for (k, s) in st.iter_mut().enumerate() {
        *s = data[main + k] as usize;
    }
    let mut steps: Vec<(u32, u32)> = vec![(0, 0); main];
    for i in (0..main).rev() {
        let j = i % 5;
        let y = st[j];
        let x = from[data[i] as usize][y];
        assert_ne!(x, usize::MAX, "tans: no state");
        steps[i] = ((y - table[x].base) as u32, table[x].nbits);
        st[j] = x;
    }
    let (mut f, mut b) = (Lsb::default(), Lsb::default());
    f.put(st[0] as u32, tb);
    b.put(st[1] as u32, tb);
    f.put(st[2] as u32, tb);
    b.put(st[3] as u32, tb);
    f.put(st[4] as u32, tb);
    for (i, &(v, n)) in steps.iter().enumerate() {
        if (i / 5) % 2 == 0 {
            f.put(v, n);
        } else {
            b.put(v, n);
        }
    }
    let mut body = d.bytes();
    body.extend(f.bytes());
    body.extend(reversed(b.bytes()));
    if body.len() >= out_len || body.len() < 8 {
        return None;
    }
    Some((long(1, out_len, &body), data))
}

// ------------------------------------------------------------ RLE (type 3)

/// An RLE array with random commands, part of its work sometimes in a nested
/// array. Returns `(array, decoded bytes)` or `None`.
pub fn rle(rng: &mut Rng, depth: u32) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut lits: Vec<u8> = Vec::new();
    // Commands in execution order, each as the bytes it occupies.
    let mut cmds: Vec<Vec<u8>> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    let mut fill = 0u8;
    for _ in 0..rng.range(1, 12) {
        match rng.below(6) {
            0 => {
                fill = rng.next() as u8;
                lits.push(fill);
                cmds.push(vec![0x01]);
            }
            1 => {
                let (copy, run) = (rng.below(16), rng.range(3, 15));
                let l = rng.bytes(copy);
                out.extend(&l);
                lits.extend(l);
                out.extend(std::iter::repeat_n(fill, run));
                cmds.push(vec![((run << 4) | (15 - copy)) as u8]);
            }
            2 => {
                let (copy, run) = (rng.below(64), rng.below(128));
                let t = 4096 + (run << 6 | copy);
                let l = rng.bytes(copy);
                out.extend(&l);
                lits.extend(l);
                out.extend(std::iter::repeat_n(fill, run));
                cmds.push(vec![t as u8, (t >> 8) as u8]);
            }
            3 => {
                let n = rng.range(1, 3);
                out.extend(std::iter::repeat_n(fill, n * 128));
                let v = 0x8FF + n;
                cmds.push(vec![v as u8, (v >> 8) as u8]);
            }
            4 => {
                let n = rng.range(1, 3);
                let l = rng.bytes(n * 64);
                out.extend(&l);
                lits.extend(l);
                let v = 511 + n;
                cmds.push(vec![v as u8, (v >> 8) as u8]);
            }
            _ => {
                let l = rng.bytes(15);
                out.extend(&l);
                lits.extend(l);
                cmds.push(vec![0x00]);
            }
        }
    }
    let mut w = lits.clone();
    for c in cmds.iter().rev() {
        w.extend(c);
    }
    let mut body;
    if rng.below(2) == 0 || depth >= 3 {
        body = vec![0u8];
        body.extend(&w);
    } else {
        // Part of the work in a nested array.
        let cut = rng.below(w.len() + 1);
        let arr = array(rng, &w[..cut], depth + 1);
        if arr[0] == 0 {
            return None;
        }
        body = arr;
        body.extend(&w[cut..]);
    }
    if body.len() >= out.len() || out.is_empty() || body.len() == 1 {
        return None;
    }
    Some((long(3, out.len(), &body), out))
}

// ------------------------------------------------------------ sliced container (type 5 and Leviathan)

/// Encode a sliced container (count byte first) from a plan: `plan[k]` lists the
/// intervals of output array `k` as `(source index, bytes)`. `combined` selects
/// the combined index/exponent list.
pub fn sliced_from_plan(
    rng: &mut Rng,
    plan: &[Vec<(usize, Vec<u8>)>],
    n_in: usize,
    combined: bool,
    depth: u32,
) -> Vec<u8> {
    let mut srcs: Vec<Vec<u8>> = vec![Vec::new(); n_in];
    let mut index: Vec<u8> = Vec::new();
    let mut exps: Vec<u8> = Vec::new();
    let (mut fwd, mut bwd) = (Msb::default(), Msb::default());
    // Position in the exponent list (in the combined list terminators count too).
    let mut slot = 0usize;
    for intervals in plan {
        for (s, bytes) in intervals {
            let len = bytes.len();
            assert!(len >= 1 && len <= if combined { 65535 } else { 131071 });
            srcs[*s].extend(bytes);
            let e = len.ilog2();
            index.push(*s as u8 + 1);
            exps.push(e as u8);
            let w = if slot.is_multiple_of(2) {
                &mut fwd
            } else {
                &mut bwd
            };
            w.put((len - (1 << e)) as u32, e);
            slot += 1;
        }
        index.push(0);
        if combined {
            exps.push(0);
            slot += 1;
        }
    }
    let mut v = vec![0x80 | n_in as u8];
    for s in &srcs {
        v.extend(array(rng, s, depth + 1));
    }
    let mut streams = fwd.bytes();
    streams.extend(reversed(bwd.bytes()));
    // Unused bytes between the two streams are allowed.
    let mid = fwd.bytes().len();
    for _ in 0..rng.below(3) {
        streams.insert(mid, 0);
    }
    let q = streams.len() | if combined { 0x8000 } else { 0 };
    v.extend_from_slice(&[q as u8, (q >> 8) as u8]);
    if combined {
        let merged: Vec<u8> = index.iter().zip(&exps).map(|(i, e)| i | e << 4).collect();
        v.extend(array(rng, &merged, depth + 1));
    } else {
        v.extend(array(rng, &index, depth + 1));
        v.extend(array(rng, &exps, depth + 1));
    }
    v.extend(streams);
    v
}

/// A container (count byte first) that yields `outs`. Unless `min_two`, it is
/// sometimes the form without source arrays (one array per output).
pub fn sliced(rng: &mut Rng, outs: &[Vec<u8>], depth: u32, min_two: bool) -> Vec<u8> {
    let total: usize = outs.iter().map(|o| o.len()).sum();
    let n_out = outs.len();
    if !min_two && (rng.below(3) == 0 || total < 4 * n_out + 8 || depth >= 3) {
        let mut v = vec![0x80u8];
        for o in outs {
            v.extend(array(rng, o, depth + 1));
        }
        return v;
    }
    let combined = rng.below(2) == 0;
    let n_in = rng.range(if min_two { 2 } else { 1 }, 4);
    let max_iv = (total.saturating_sub(n_out)).max(1);
    let cap = if combined { 65535 } else { 131071 };
    let mut n_iv = 0usize;
    let mut plan = Vec::new();
    for o in outs {
        let mut intervals = Vec::new();
        let mut at = 0usize;
        while at < o.len() {
            let rest = o.len() - at;
            let len = if n_iv + 1 >= max_iv / 2 {
                rest
            } else {
                rng.range(1, rest.min(700))
            }
            .min(cap);
            intervals.push((rng.below(n_in), o[at..at + len].to_vec()));
            at += len;
            n_iv += 1;
        }
        plan.push(intervals);
    }
    sliced_from_plan(rng, &plan, n_in, combined, depth)
}

/// A type-5 sliced container of about `n` bytes made of runs of one fill byte
/// (one source array, which an RLE array holds in a few bytes) and short random
/// pieces (one or two other sources), so the container is smaller than its
/// output. Returns `(array, decoded bytes, combined list?)` or `None`.
pub fn sliced_runs(rng: &mut Rng, n: usize) -> Option<(Vec<u8>, Vec<u8>, bool)> {
    let fill = rng.next() as u8;
    let n_in = rng.range(2, 3);
    let combined = rng.below(2) == 0;
    let mut data = Vec::new();
    let mut intervals = Vec::new();
    while data.len() < n {
        let run = vec![fill; rng.range(16, 300)];
        data.extend(&run);
        intervals.push((0, run));
        let k = rng.range(1, 8);
        let piece = rng.bytes(k);
        data.extend(&piece);
        intervals.push((rng.range(1, n_in - 1), piece));
    }
    let body = sliced_from_plan(rng, &[intervals], n_in, combined, 1);
    if body.len() >= data.len() || data.len() > 1 << 18 {
        return None;
    }
    Some((long(5, data.len(), &body), data, combined))
}

// ------------------------------------------------------------ any array holding given bytes

/// An array that decodes to `data`: RLE for constant data, sometimes a
/// concatenation or a sliced container, otherwise stored.
pub fn array(rng: &mut Rng, data: &[u8], depth: u32) -> Vec<u8> {
    let n = data.len();
    if n >= 8 && data.iter().all(|&b| b == data[0]) && rng.below(4) != 0 {
        return long(3, n, &data[..1]);
    }
    if depth < 4 && n >= 40 {
        match rng.below(5) {
            0 => {
                let parts = rng.range(2, 5);
                let mut body = vec![parts as u8];
                let mut at = 0;
                for i in 0..parts {
                    let end = if i + 1 == parts {
                        n
                    } else {
                        at + rng.below(n - at + 1)
                    };
                    body.extend(array(rng, &data[at..end], depth + 1));
                    at = end;
                }
                if body.len() < n {
                    return long(5, n, &body);
                }
            }
            1 => {
                let body = sliced(rng, &[data.to_vec()], depth, true);
                if body.len() < n && body.len() >= 6 {
                    return long(5, n, &body);
                }
            }
            _ => {}
        }
    }
    if n <= 0xFFF && rng.below(2) == 0 {
        stored_short(data)
    } else {
        stored_long(data)
    }
}

/// An array with freely chosen content of about `n` bytes (tANS, Huffman, RLE or
/// otherwise stored). Returns `(array, decoded bytes)`.
pub fn free_array(rng: &mut Rng, n: usize, depth: u32) -> (Vec<u8>, Vec<u8>) {
    for _ in 0..4 {
        let r = match rng.below(6) {
            0 => tans(rng, n.max(5)),
            1 => huffman(rng, n.max(2), false),
            2 => huffman(rng, n.max(2), true),
            3 => rle(rng, depth),
            _ => None,
        };
        if let Some(x) = r {
            return x;
        }
    }
    let d = rng.bytes(n);
    (array(rng, &d, depth), d)
}
