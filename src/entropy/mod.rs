//! Entropy-coded byte arrays (section 3 of the format description).

pub(crate) mod huffman;
mod multi;
mod rle;
mod tans;

use crate::error::{corrupt, Error, Result};
use std::borrow::Cow;

pub(crate) use multi::decode_multi;

/// Maximum nesting of arrays inside arrays (RLE, recursive, multi-array).
const MAX_DEPTH: u32 = 24;

/// Decode one array from the start of `src`. `limit` is the largest decoded size
/// the caller accepts. Returns the bytes and the number of input bytes used.
pub(crate) fn decode_array(src: &[u8], limit: usize, depth: u32) -> Result<(Cow<'_, [u8]>, usize)> {
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
    let (src_size, dst_size, hdr) = if h0 >= 0x80 {
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
    if dst_size > limit {
        return corrupt("array larger than its limit");
    }
    let body = src.get(hdr..hdr + src_size).ok_or(Error::Truncated)?;
    let data = match ty {
        1 => tans::decode(body, dst_size)?,
        2 => huffman::decode(body, dst_size, false)?,
        3 => rle::decode(body, dst_size, depth)?,
        4 => huffman::decode(body, dst_size, true)?,
        _ => decode_recursive(body, dst_size, depth)?,
    };
    Ok((Cow::Owned(data), hdr + src_size))
}

/// Type-5 arrays: a list of nested arrays, or a multi-array container.
fn decode_recursive(body: &[u8], dst_size: usize, depth: u32) -> Result<Vec<u8>> {
    let b = *body.first().ok_or(Error::Truncated)?;
    let n = usize::from(b & 0x7F);
    if n < 2 || body.len() < 6 {
        return corrupt("recursive array: count or size too small");
    }
    if b & 0x80 == 0 {
        let mut out: Vec<u8> = Vec::with_capacity(dst_size);
        let mut pos = 1usize;
        for _ in 0..n {
            let (a, used) = decode_array(&body[pos..], dst_size - out.len(), depth + 1)?;
            out.extend_from_slice(&a);
            pos += used;
        }
        if out.len() != dst_size || pos != body.len() {
            return corrupt("recursive array: size mismatch");
        }
        Ok(out)
    } else {
        let (mut outs, used) = decode_multi(body, 1, dst_size, depth)?;
        let out = outs.pop().unwrap_or_default();
        if out.len() != dst_size || used != body.len() {
            return corrupt("multi-array: size mismatch");
        }
        Ok(out)
    }
}
