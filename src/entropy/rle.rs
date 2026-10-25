//! RLE arrays (array type 3).

use super::read_array;
use crate::error::{corrupt, Result};

pub(crate) fn decode(body: &[u8], out_len: usize, depth: u32) -> Result<Vec<u8>> {
    if body.is_empty() {
        return corrupt("rle: empty body");
    }
    if body.len() == 1 {
        return Ok(vec![body[0]; out_len]);
    }
    // Work buffer: literals from the front, commands from the back.
    let owned;
    let w: &[u8] = if body[0] != 0 {
        let (arr, n) = read_array(body, out_len, depth + 1)?;
        let mut v = Vec::with_capacity(arr.len() + body.len() - n);
        v.extend_from_slice(&arr);
        v.extend_from_slice(&body[n..]);
        owned = v;
        &owned
    } else {
        &body[1..]
    };
    let mut out: Vec<u8> = Vec::with_capacity(out_len);
    let mut lit = 0usize;
    let mut end = w.len();
    let mut fill = 0u8;
    while lit < end {
        let cmd = w[end - 1];
        let (copy, run);
        if cmd == 0 || cmd > 0x2F {
            end -= 1;
            copy = usize::from(!cmd & 15);
            run = usize::from(cmd >> 4);
        } else if cmd >= 0x10 {
            if end - lit < 2 {
                return corrupt("rle: truncated command");
            }
            let t = (usize::from(cmd) << 8 | usize::from(w[end - 2])) - 4096;
            end -= 2;
            copy = t & 63;
            run = t >> 6;
        } else if cmd == 1 {
            end -= 1;
            if lit >= end {
                return corrupt("rle: missing literal for run byte");
            }
            fill = w[lit];
            lit += 1;
            continue;
        } else if cmd >= 9 {
            if end - lit < 2 {
                return corrupt("rle: truncated command");
            }
            let v = usize::from(cmd) << 8 | usize::from(w[end - 2]);
            end -= 2;
            copy = 0;
            run = (v - 0x8FF) * 128;
        } else {
            if end - lit < 2 {
                return corrupt("rle: truncated command");
            }
            let v = usize::from(cmd) << 8 | usize::from(w[end - 2]);
            end -= 2;
            copy = (v - 511) * 64;
            run = 0;
        }
        if copy > end - lit {
            return corrupt("rle: literal count beyond the data");
        }
        if copy + run > out_len - out.len() {
            return corrupt("rle: output overrun");
        }
        out.extend_from_slice(&w[lit..lit + copy]);
        lit += copy;
        out.resize(out.len() + run, fill);
    }
    if out.len() != out_len {
        return corrupt("rle: output size mismatch");
    }
    Ok(out)
}
