//! Entropy-coded byte arrays (section 3 of the format description).

pub(crate) mod huffman;
mod rle;
mod sliced;
mod tans;

use crate::error::{corrupt, Error, Result};
use std::borrow::Cow;

pub(crate) use sliced::read_sliced;

/// Maximum nesting of arrays inside arrays (RLE, concatenated, sliced container).
// SPEC-QUESTION: the spec gives no nesting bound (QUESTIONS.md, item 8).
const MAX_DEPTH: u32 = 24;

/// Decode one array from the start of `src`. `limit` is the largest decoded size
/// the caller accepts. Returns the bytes and the number of input bytes used.
pub(crate) fn read_array(src: &[u8], limit: usize, depth: u32) -> Result<(Cow<'_, [u8]>, usize)> {
    if depth > MAX_DEPTH {
        return corrupt("arrays nested too deeply");
    }
    let h0 = *src.first().ok_or(Error::Truncated)?;
    let ty = (h0 >> 4) & 7;
    if ty == 0 {
        let (size, off) = if h0 >= 0x80 {
            if src.len() < 2 {
                return Err(Error::Truncated);
            }
            ((usize::from(h0) << 8 | usize::from(src[1])) & 0x0FFF, 2)
        } else {
            if src.len() < 3 {
                return Err(Error::Truncated);
            }
            let v = usize::from(h0) << 16 | usize::from(src[1]) << 8 | usize::from(src[2]);
            if v >> 18 != 0 {
                return corrupt("stored array: size field has high bits");
            }
            (v, 3)
        };
        if size > limit {
            return corrupt("array larger than its limit");
        }
        let body = src.get(off..off + size).ok_or(Error::Truncated)?;
        return Ok((Cow::Borrowed(body), off + size));
    }
    if ty > 5 {
        return corrupt("invalid array type");
    }
    let (body_len, out_len, hdr) = if h0 >= 0x80 {
        if src.len() < 3 {
            return Err(Error::Truncated);
        }
        let b = usize::from(h0) << 16 | usize::from(src[1]) << 8 | usize::from(src[2]);
        let s = b & 0x3FF;
        (s, s + ((b >> 10) & 0x3FF) + 1, 3)
    } else {
        if src.len() < 5 {
            return Err(Error::Truncated);
        }
        let c = usize::from(src[1]) << 24
            | usize::from(src[2]) << 16
            | usize::from(src[3]) << 8
            | usize::from(src[4]);
        let s = c & 0x3FFFF;
        let d = (((usize::from(h0) & 0x0F) << 14) | (c >> 18)) + 1;
        if s >= d {
            return corrupt("array: source size not below decoded size");
        }
        (s, d, 5)
    };
    if out_len > limit {
        return corrupt("array larger than its limit");
    }
    let body = src.get(hdr..hdr + body_len).ok_or(Error::Truncated)?;
    let data = match ty {
        1 => tans::decode(body, out_len)?,
        2 => huffman::decode(body, out_len, false)?,
        3 => rle::decode(body, out_len, depth)?,
        4 => huffman::decode(body, out_len, true)?,
        _ => read_concat(body, out_len, depth)?,
    };
    Ok((Cow::Owned(data), hdr + body_len))
}

/// Type-5 arrays: a list of concatenated arrays, or a sliced container.
fn read_concat(body: &[u8], out_len: usize, depth: u32) -> Result<Vec<u8>> {
    let b = *body.first().ok_or(Error::Truncated)?;
    let n = usize::from(b & 0x7F);
    if n < 2 || body.len() < 6 {
        return corrupt("concatenated array: count or size too small");
    }
    if b & 0x80 == 0 {
        let mut out: Vec<u8> = Vec::with_capacity(out_len);
        let mut pos = 1usize;
        for _ in 0..n {
            let (a, used) = read_array(&body[pos..], out_len - out.len(), depth + 1)?;
            out.extend_from_slice(&a);
            pos += used;
        }
        if out.len() != out_len || pos != body.len() {
            return corrupt("concatenated array: size mismatch");
        }
        Ok(out)
    } else {
        let (mut outs, used) = read_sliced(body, 1, out_len, depth)?;
        let out = outs.pop().unwrap_or_default();
        if out.len() != out_len || used != body.len() {
            return corrupt("sliced container: size mismatch");
        }
        Ok(out)
    }
}
