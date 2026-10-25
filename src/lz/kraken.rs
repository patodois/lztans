//! Kraken LZ sub-chunks.

use super::{copy_delta, copy_match, copy_raw, raw_prefix, read_side_streams, Place};
use crate::entropy::decode_array;
use crate::error::{corrupt, Error, Result};

pub(crate) fn decode(body: &[u8], mode: u32, out: &mut [u8], pl: &Place) -> Result<()> {
    if mode > 1 {
        return corrupt("kraken: invalid mode");
    }
    let dst_count = pl.dst_count();
    let mut pos = raw_prefix(body, out, pl)?;
    let mut p = pl.start + pos;
    let first = *body.get(pos).ok_or(Error::Truncated)?;
    if first & 0x80 != 0 {
        return corrupt("kraken: literal array with bit 7 set");
    }
    let (lit, n) = decode_array(&body[pos..], dst_count, 0)?;
    pos += n;
    let (cmd, n) = decode_array(&body[pos..], dst_count, 0)?;
    pos += n;
    if body.len() - pos < 3 {
        return Err(Error::Truncated);
    }
    let x = body[pos];
    let scale = if x & 0x80 != 0 {
        pos += 1;
        u32::from(x) - 127
    } else {
        0
    };
    let (oc, n) = decode_array(&body[pos..], cmd.len(), 0)?;
    pos += n;
    let ox = if scale >= 2 {
        let (a, n) = decode_array(&body[pos..], oc.len(), 0)?;
        pos += n;
        if a.len() != oc.len() {
            return corrupt("kraken: offset extras length");
        }
        Some(a)
    } else {
        None
    };
    let (lc, n) = decode_array(&body[pos..], dst_count >> 2, 0)?;
    pos += n;
    let side = read_side_streams(&body[pos..], &oc, ox.as_deref(), scale, &lc)?;

    let delta = mode == 0;
    let end = pl.end;
    let mut recent = [8usize; 3];
    let mut lo = 8usize;
    let mut lit_cur = 0usize;
    let mut off_cur = 0usize;
    let mut len_cur = 0usize;
    for &f in cmd.iter() {
        let mut lit_n = usize::from(f & 3);
        let mut mlen = usize::from((f >> 2) & 15);
        let oidx = usize::from(f >> 6);
        if lit_n == 3 {
            lit_n = *side
                .lens
                .get(len_cur)
                .ok_or(Error::Corrupt("kraken: lengths exhausted"))? as usize;
            len_cur += 1;
        }
        if delta {
            copy_delta(out, p, &lit, &mut lit_cur, lit_n, lo, pl, end)?;
        } else {
            copy_raw(out, p, &lit, &mut lit_cur, lit_n, end)?;
        }
        p += lit_n;
        let dist = if oidx == 3 {
            let d = *side
                .offsets
                .get(off_cur)
                .ok_or(Error::Corrupt("kraken: offsets exhausted"))?;
            off_cur += 1;
            recent = [d, recent[0], recent[1]];
            d
        } else {
            let d = recent[oidx];
            match oidx {
                1 => recent.swap(0, 1),
                2 => recent = [recent[2], recent[0], recent[1]],
                _ => {}
            }
            d
        };
        lo = dist;
        if mlen == 15 {
            mlen = 14
                + *side
                    .lens
                    .get(len_cur)
                    .ok_or(Error::Corrupt("kraken: lengths exhausted"))? as usize;
            len_cur += 1;
        } else {
            mlen += 2;
        }
        copy_match(out, p, dist, mlen, pl, end)?;
        p += mlen;
    }
    let rest = end - p;
    if delta {
        copy_delta(out, p, &lit, &mut lit_cur, rest, lo, pl, end)?;
    } else {
        copy_raw(out, p, &lit, &mut lit_cur, rest, end)?;
    }
    if lit_cur != lit.len() || off_cur != side.offsets.len() || len_cur != side.lens.len() {
        return corrupt("kraken: unused literals, offsets or lengths");
    }
    Ok(())
}
