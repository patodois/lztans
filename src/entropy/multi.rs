//! Multi-array container (shared by type-5 arrays and Leviathan sub-chunks).

use super::decode_array;
use crate::bits::{Mb, Mf};
use crate::error::{corrupt, Error, Result};

/// Decode a container that starts at `src[0]` (its count byte) and whose region
/// extends to the end of `src`. Produces `n_out` output arrays whose total size
/// is at most `limit`. Returns the arrays and the number of bytes occupied.
pub(crate) fn decode_multi(
    src: &[u8],
    n_out: usize,
    limit: usize,
    depth: u32,
) -> Result<(Vec<Vec<u8>>, usize)> {
    if src.len() < 4 {
        return Err(Error::Truncated);
    }
    let b = src[0];
    if b & 0x80 == 0 {
        return corrupt("multi-array: count byte without bit 7");
    }
    let n_in = usize::from(b & 0x3F);
    let mut pos = 1usize;
    if n_in == 0 {
        let mut outs = Vec::with_capacity(n_out);
        let mut left = limit;
        for _ in 0..n_out {
            let (a, n) = decode_array(&src[pos..], left, depth + 1)?;
            left -= a.len();
            pos += n;
            outs.push(a.into_owned());
        }
        return Ok((outs, pos));
    }
    let mut sources: Vec<Vec<u8>> = Vec::with_capacity(n_in);
    let mut total = 0usize;
    for _ in 0..n_in {
        let (a, n) = decode_array(&src[pos..], limit - total, depth + 1)?;
        total += a.len();
        pos += n;
        sources.push(a.into_owned());
    }
    if src.len() < pos + 2 {
        return Err(Error::Truncated);
    }
    let q = usize::from(src[pos]) | (usize::from(src[pos + 1]) << 8);
    pos += 2;
    let vb = q & 0x3FFF;
    let combined = q & 0x8000 != 0;

    let index: Vec<u8>;
    let exps: Vec<u8>;
    let num_lens: usize;
    if combined {
        let (a, n) = decode_array(&src[pos..], total, depth + 1)?;
        pos += n;
        index = a.iter().map(|t| t & 15).collect();
        exps = a.iter().map(|t| t >> 4).collect();
        num_lens = index.len();
    } else {
        let (a, n) = decode_array(&src[pos..], total, depth + 1)?;
        pos += n;
        if a.len() < n_out {
            return corrupt("multi-array: fewer intervals than output arrays");
        }
        num_lens = a.len() - n_out;
        let (e, n2) = decode_array(&src[pos..], num_lens, depth + 1)?;
        pos += n2;
        if e.len() != num_lens {
            return corrupt("multi-array: exponent count");
        }
        if e.iter().any(|&x| x > 16) {
            return corrupt("multi-array: exponent above 16");
        }
        index = a.into_owned();
        exps = e.into_owned();
    }
    if index.len() > total {
        return corrupt("multi-array: too many intervals");
    }
    if vb > src.len() - pos {
        return Err(Error::Truncated);
    }
    let streams = &src[pos..pos + vb];
    pos += vb;
    match index.last() {
        Some(0) => {}
        _ => return corrupt("multi-array: last index is not zero"),
    }
    let mut fwd = Mf::new(streams);
    let mut bwd = Mb::new(streams);
    let mut lens: Vec<usize> = Vec::with_capacity(num_lens);
    for (i, &e) in exps.iter().take(num_lens).enumerate() {
        let e = u32::from(e);
        let r = if i % 2 == 0 { fwd.read(e) } else { bwd.read(e) };
        lens.push((1usize << e) + r as usize);
    }
    if fwd.overrun() || bwd.overrun() {
        return corrupt("multi-array: interval length streams overrun");
    }

    let mut cursors = vec![0usize; n_in];
    let mut ci = 0usize;
    let mut cl = 0usize;
    let mut outs: Vec<Vec<u8>> = Vec::with_capacity(n_out);
    for _ in 0..n_out {
        let mut o: Vec<u8> = Vec::new();
        loop {
            let s = *index
                .get(ci)
                .ok_or(Error::Corrupt("multi-array: index list ended"))?;
            ci += 1;
            if s == 0 {
                break;
            }
            let s = usize::from(s);
            if s > n_in {
                return corrupt("multi-array: source index out of range");
            }
            let len = *lens
                .get(cl)
                .ok_or(Error::Corrupt("multi-array: length list ended"))?;
            cl += 1;
            let c = cursors[s - 1];
            let srcarr = &sources[s - 1];
            if len > srcarr.len() - c {
                return corrupt("multi-array: interval beyond its source");
            }
            o.extend_from_slice(&srcarr[c..c + len]);
            cursors[s - 1] = c + len;
        }
        if combined {
            cl += 1;
        }
        outs.push(o);
    }
    if ci != index.len() || cl != num_lens {
        return corrupt("multi-array: unused intervals");
    }
    for (c, s) in cursors.iter().zip(sources.iter()) {
        if *c != s.len() {
            return corrupt("multi-array: source array not fully used");
        }
    }
    Ok((outs, pos))
}
