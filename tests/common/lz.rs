//! Test-side encoder for LZ sub-chunks (Leviathan with every literal mode,
//! Mermaid with entropy-coded near offsets), built from the rules the decoder in
//! `src/lz/` applies, together with the bytes they must decode to.
//!
//! Like `enc.rs`, these builders follow this crate's own decoder: they guard
//! against regressions and do not prove conformance.

use super::enc::{array, sliced, Msb};
use super::{stored_long, Rng};

/// An array whose header has bit 7 clear (long form or 3-byte stored form).
fn force_long(rng: &mut Rng, data: &[u8]) -> Vec<u8> {
    let a = array(rng, data, 1);
    if a[0] & 0x80 != 0 {
        stored_long(data)
    } else {
        a
    }
}

/// A Leviathan sub-chunk built by [`leviathan`].
pub struct Leviathan {
    /// Sub-chunk body (without the 3-byte sub-chunk header).
    pub body: Vec<u8>,
    /// The window followed by the sub-chunk's output.
    pub out: Vec<u8>,
    /// Commands in eight lanes (`true`) or in one array.
    pub eight_lanes: bool,
    /// Lengths sent through the 255 escape (long lengths).
    pub long_lengths: usize,
}

/// A Leviathan sub-chunk of `sub_len` output bytes in literal mode `mode`
/// (0 to 5), placed after `window` (empty: the sub-chunk starts the window).
/// `eight_lanes` chooses the command layout. `None` when the body would not be
/// smaller than its output.
pub fn leviathan(
    rng: &mut Rng,
    window: &[u8],
    sub_len: usize,
    mode: u8,
    eight_lanes: bool,
) -> Option<Leviathan> {
    let start = window.len();
    let end = start + sub_len;
    let mut out = window.to_vec();
    let mut body = Vec::new();
    if start == 0 {
        let raw = rng.bytes(8);
        out.extend(&raw);
        body.extend(&raw);
    }
    let n_arrays = match mode {
        0 | 1 => 1,
        2 => 2,
        3 => 4,
        _ => 16,
    };
    let mut lits: Vec<Vec<u8>> = vec![Vec::new(); n_arrays];
    // Emit `n` literals: choose the array and whether the byte is a delta.
    let emit = |out: &mut Vec<u8>, lits: &mut Vec<Vec<u8>>, n: usize, lo: usize, rng: &mut Rng| {
        for i in 0..n {
            let q = out.len();
            let b = rng.next() as u8;
            let (idx, delta) = match mode {
                0 => (0, true),
                1 => (0, false),
                2 => (usize::from(i == 0), true),
                3 => ((q - start) % 4, true),
                5 => ((q - start) % 16, true),
                _ => (usize::from(out[q - 1] >> 4), false),
            };
            lits[idx].push(b);
            out.push(if delta {
                b.wrapping_add(out[q - lo])
            } else {
                b
            });
        }
    };
    let mut lanes: Vec<Vec<u8>> = vec![Vec::new(); if eight_lanes { 8 } else { 1 }];
    let mut recent = [8usize; 7];
    let mut lo = 8usize;
    let (mut lit_lens, mut match_lens): (Vec<usize>, Vec<usize>) = (Vec::new(), Vec::new());
    let mut offsets: Vec<usize> = Vec::new();
    let (max_off, max_len) = (sub_len / 3, sub_len / 5);
    loop {
        let p = out.len();
        let rem = end - p;
        if rem < 12 || rng.below(60) == 0 {
            break;
        }
        let can_len = lit_lens.len() + match_lens.len() + 2 <= max_len;
        // Literals.
        let mut lit_n = rng.below(3);
        if can_len && rng.below(4) == 0 {
            lit_n = if rng.below(12) == 0 {
                rng.range(258, 900)
            } else {
                rng.range(3, 30)
            };
        }
        lit_n = lit_n.min(rem - 9);
        if lit_n >= 3 && !can_len {
            lit_n = 2;
        }
        let litc = lit_n.min(3);
        emit(&mut out, &mut lits, lit_n, lo, rng);
        if litc == 3 {
            lit_lens.push(lit_n);
        }
        let p2 = out.len();
        let rem = end - p2;
        // Match.
        let mut mlen = rng.range(2, 8);
        if can_len && rng.below(3) == 0 {
            mlen = if rng.below(8) == 0 {
                rng.range(264, 4000)
            } else {
                rng.range(9, 120)
            };
        }
        mlen = mlen.min(rem);
        if mlen >= 9 && !can_len {
            mlen = 8;
        }
        let avail = p2; // the window starts at 0
        let (oidx, dist);
        if rng.below(10) < 5 && offsets.len() < max_off && avail >= 8 {
            oidx = 7;
            dist = if rng.below(3) == 0 {
                rng.range(8, avail)
            } else {
                rng.range(8, avail.min(64))
            };
            offsets.push(dist);
            recent = [
                dist, recent[0], recent[1], recent[2], recent[3], recent[4], recent[5],
            ];
        } else {
            let mut i = rng.below(7);
            if recent[i] > avail {
                i = (0..7).find(|&k| recent[k] <= avail)?;
            }
            oidx = i;
            dist = recent[i];
            recent.copy_within(0..i, 1);
            recent[0] = dist;
        }
        lo = dist;
        let mlc = if mlen >= 9 {
            match_lens.push(mlen);
            7
        } else {
            mlen - 2
        };
        for _ in 0..mlen {
            let q = out.len();
            out.push(out[q - dist]);
        }
        let cmd = (oidx << 5 | litc << 3 | mlc) as u8;
        let lane = if eight_lanes { (p - start) & 7 } else { 0 };
        lanes[lane].push(cmd);
    }
    let rest = end - out.len();
    emit(&mut out, &mut lits, rest, lo, rng);

    // Length entries: literal lengths from the front, match lengths from the back.
    let mut entries: Vec<usize> = lit_lens.clone();
    entries.extend(match_lens.iter().rev().map(|m| m - 6));
    let mut lc = Vec::new();
    let mut longs: Vec<u32> = Vec::new();
    for e in entries {
        let v = e - 3;
        if v >= 255 {
            lc.push(255u8);
            longs.push((v - 255) as u32);
        } else {
            lc.push(v as u8);
        }
    }
    // Offset and long-length bit streams.
    let (mut fwd, mut bwd) = (Msb::default(), Msb::default());
    let v = longs.len() as u32 + 1;
    let z = 31 - v.leading_zeros();
    bwd.put(0, z);
    bwd.put(v, z + 1);
    let mut oc = Vec::new();
    for (i, &d) in offsets.iter().enumerate() {
        let w = if i % 2 == 0 { &mut fwd } else { &mut bwd };
        let t = d + 248;
        let (m, low) = (t >> 4, t & 15);
        let n = m.ilog2();
        if !(4..=18).contains(&n) {
            return None;
        }
        oc.push(((n - 4) << 4) as u8 | low as u8);
        w.put((m - (1 << n)) as u32, n);
    }
    for (i, &l) in longs.iter().enumerate() {
        let w = if i % 2 == 0 { &mut fwd } else { &mut bwd };
        let x = l + 64;
        let z = (32 - x.leading_zeros()) - 7;
        w.put(0, z);
        w.put(x, z + 7);
    }
    body.extend(force_long(rng, &oc));
    body.extend(array(rng, &lc, 1));
    if n_arrays == 1 {
        body.extend(array(rng, &lits[0], 1));
    } else {
        body.extend(sliced(rng, &lits, 1, false));
    }
    if eight_lanes {
        body.push(0x83);
        body.extend(sliced(rng, &lanes, 1, false));
    } else {
        body.extend(force_long(rng, &lanes[0]));
    }
    body.extend(fwd.bytes());
    let mut b = bwd.bytes();
    b.reverse();
    body.extend(b);
    if body.len() >= sub_len {
        return None;
    }
    Some(Leviathan {
        body,
        out,
        eight_lanes,
        long_lengths: longs.len(),
    })
}

/// A Mermaid sub-chunk built by [`mermaid`].
pub struct Mermaid {
    /// Sub-chunk body (mode 1, raw literals).
    pub body: Vec<u8>,
    /// The sub-chunk's output.
    pub out: Vec<u8>,
    /// Near offsets (entropy-coded in two arrays).
    pub near: usize,
    /// Far offsets, and whether their count went through the 4095 escape.
    pub far: usize,
    pub far_escape: bool,
}

/// A single-piece Mermaid sub-chunk (mode 1) at the window start, with its near
/// offsets entropy-coded as two byte arrays. `None` when the body would not be
/// smaller than its output.
pub fn mermaid(rng: &mut Rng, sub_len: usize) -> Option<Mermaid> {
    assert!((40..=65536).contains(&sub_len));
    let mut out = rng.bytes(8);
    let mut body = out.clone();
    let (mut lit, mut cmd, mut near): (Vec<u8>, Vec<u8>, Vec<usize>) =
        (Vec::new(), Vec::new(), Vec::new());
    let mut n_far = 0usize;
    loop {
        let rem = sub_len - out.len();
        if rem < 40 || rng.below(50) == 0 {
            break;
        }
        if rng.below(4) == 0 {
            // Far match back to the window start.
            let c = rng.range(3, 23);
            let dist = out.len();
            for _ in 0..c + 5 {
                let q = out.len();
                out.push(out[q - dist]);
            }
            cmd.push(c as u8);
            n_far += 1;
        } else {
            // Literals, then a near match.
            let (n, mlen) = (rng.below(8), rng.range(3, 15));
            let l = rng.bytes(n);
            out.extend(&l);
            lit.extend(l);
            let dist = rng.range(8, out.len().min(65535));
            near.push(dist);
            for _ in 0..mlen {
                let q = out.len();
                out.push(out[q - dist]);
            }
            cmd.push((mlen << 3 | n) as u8);
        }
    }
    let l = rng.bytes(sub_len - out.len());
    out.extend(&l);
    lit.extend(l);
    body.extend(array(rng, &lit, 1));
    body.extend(array(rng, &cmd, 1));
    body.extend_from_slice(&[0xFF, 0xFF]); // near offsets as two arrays
    let hi: Vec<u8> = near.iter().map(|d| (d >> 8) as u8).collect();
    let lo: Vec<u8> = near.iter().map(|d| *d as u8).collect();
    body.extend(array(rng, &hi, 1));
    body.extend(array(rng, &lo, 1));
    let far_escape = n_far > 0 && rng.below(2) == 0;
    if far_escape {
        body.extend_from_slice(&[0x00, 0xF0, 0xFF, n_far as u8, (n_far >> 8) as u8]);
    } else {
        let t = n_far << 12;
        body.extend_from_slice(&[t as u8, (t >> 8) as u8, (t >> 16) as u8]);
    }
    // Far offsets of 0: the source is the window start.
    body.extend(std::iter::repeat_n(0u8, 3 * n_far));
    if body.len() >= sub_len || n_far >= 4095 {
        return None;
    }
    Some(Mermaid {
        body,
        out,
        near: near.len(),
        far: n_far,
        far_escape,
    })
}
