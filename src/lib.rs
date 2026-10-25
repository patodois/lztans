//! # lztans
//!
//! A safe, dependency-free decoder for a family of block-compressed streams that
//! combine an LZ77 stage with Huffman, tANS and run-length entropy coding. It
//! decodes the block formats commonly called *Kraken*, *Mermaid* / *Selkie* and
//! *Leviathan* (these are format names only; this crate is not affiliated with
//! the owners of those names and contains no code derived from their
//! implementations - it was written from a prose format specification).
//!
//! The caller supplies the compressed bytes and a destination buffer whose
//! length is the known decompressed size:
//!
//! ```
//! # use lztans::decode;
//! // A stored block. Block header `4C 06`: low nibble 0xC, bit 6 set (stored,
//! // no compression), then codec id 0x06; the four output bytes follow verbatim.
//! let mut out = [0u8; 4];
//! let stream = [0x4C, 0x06, b'a', b'b', b'c', b'd'];
//! assert_eq!(decode(&stream, &mut out), Ok(6));
//! assert_eq!(&out, b"abcd");
//! ```
//!
//! Malformed input yields an [`Error`]; the decoder does not panic, does not use
//! `unsafe` and bounds every allocation by the sizes it can prove from the
//! stream's own headers (at most a few megabytes per sub-chunk).
//!
//! Not supported (rejected with [`Error::Unsupported`]): the LZNA and BitKnit
//! codecs and streams with checksums.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod bits;
mod entropy;
mod error;
mod lz;

pub use error::{Error, Result};

use lz::Place;

/// Output bytes per block.
const BLOCK: usize = 262_144;
/// Output bytes per sub-chunk.
const SUBCHUNK: usize = 131_072;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Codec {
    Kraken,
    Mermaid,
    Leviathan,
}

/// Decode one stream into `dst`, whose length must equal the decompressed size.
///
/// # Prefixes
///
/// A stream is made of blocks of 262,144 output bytes, and each block depends only on
/// earlier ones. So if `dst.len()` is a multiple of 262,144 and not larger than the
/// true decompressed size, the call is valid: it decodes the first `dst.len()` bytes
/// of the full output and does not look at the input after those blocks (useful to
/// probe the start of a long stream, or to grow an output buffer geometrically).
/// For any other length below the true size the result is not guaranteed: the last
/// block is then decoded with sizes different from those it was encoded with and the
/// call usually fails (it never panics). A length above the true size fails with
/// [`Error::Truncated`].
///
/// Returns the number of input bytes consumed. Input bytes after the end of the
/// stream are ignored. An empty `dst` decodes to nothing without looking at the
/// input.
///
/// # Errors
///
/// Returns an [`Error`] if the input is truncated, violates the format, or uses
/// a feature that is not supported. `dst` may be partly written in that case.
pub fn decode(src: &[u8], dst: &mut [u8]) -> Result<usize> {
    let total = dst.len();
    let mut ip = 0usize;
    let mut produced = 0usize;
    let mut base = 0usize;
    while produced < total {
        let left = (total - produced).min(BLOCK);
        let hdr = src.get(ip..ip + 2).ok_or(Error::Truncated)?;
        ip += 2;
        let (b0, b1) = (hdr[0], hdr[1]);
        if b0 & 0x0F != 0x0C || b0 & 0x30 != 0 {
            return Err(Error::Corrupt("bad block header"));
        }
        // SPEC-QUESTION: the checksum flag is rejected as the spec says, although
        // the oracle ignores it (QUESTIONS.md, item 1).
        if b1 & 0x80 != 0 {
            return Err(Error::Unsupported("checksummed blocks"));
        }
        // SPEC-QUESTION: the codec id is validated for stored blocks too, and
        // 0x05 / 0x0B are refused there (QUESTIONS.md, items 2 and 10).
        let codec = match b1 & 0x7F {
            0x06 => Codec::Kraken,
            0x0A => Codec::Mermaid,
            0x0C => Codec::Leviathan,
            0x05 | 0x0B => return Err(Error::Unsupported("LZNA and BitKnit codecs")),
            _ => return Err(Error::Corrupt("unknown codec id")),
        };
        if b0 & 0x80 != 0 {
            base = produced;
        }
        if b0 & 0x40 != 0 {
            let raw = src.get(ip..ip + left).ok_or(Error::Truncated)?;
            dst[produced..produced + left].copy_from_slice(raw);
            ip += left;
            produced += left;
            continue;
        }
        let q = src.get(ip..ip + 3).ok_or(Error::Truncated)?;
        ip += 3;
        let v = usize::from(q[0]) << 16 | usize::from(q[1]) << 8 | usize::from(q[2]);
        let size = v & 0x3_FFFF;
        if size != 0x3_FFFF {
            let payload = src.get(ip..ip + size + 1).ok_or(Error::Truncated)?;
            ip += size + 1;
            decode_quantum(payload, codec, dst, produced, left, base)?;
        } else if v >> 18 == 1 {
            let fill = *src.get(ip).ok_or(Error::Truncated)?;
            ip += 1;
            dst[produced..produced + left].fill(fill);
        } else {
            return Err(Error::Corrupt("bad quantum header"));
        }
        produced += left;
    }
    Ok(ip)
}

/// Decode the sub-chunks of one quantum into `dst[block_start..block_start+left]`.
fn decode_quantum(
    payload: &[u8],
    codec: Codec,
    dst: &mut [u8],
    block_start: usize,
    left: usize,
    base: usize,
) -> Result<()> {
    let mut q = 0usize;
    let mut d = 0usize;
    while d < left {
        let sub_len = (left - d).min(SUBCHUNK);
        if payload.len() - q < 4 {
            return Err(Error::Truncated);
        }
        let h = usize::from(payload[q]) << 16
            | usize::from(payload[q + 1]) << 8
            | usize::from(payload[q + 2]);
        let start = block_start + d;
        if h & 0x80_0000 == 0 {
            let (arr, used) = entropy::read_array(&payload[q..], sub_len, 0)?;
            if arr.len() != sub_len {
                return Err(Error::Corrupt("entropy sub-chunk has the wrong size"));
            }
            dst[start..start + sub_len].copy_from_slice(&arr);
            q += used;
        } else {
            let mode = ((h >> 19) & 0xF) as u32;
            let n = h & 0x7_FFFF;
            let body = payload.get(q + 3..q + 3 + n).ok_or(Error::Truncated)?;
            if n < sub_len {
                let pl = Place {
                    base,
                    start,
                    end: start + sub_len,
                };
                match codec {
                    Codec::Kraken => lz::kraken::decode(body, mode, dst, &pl)?,
                    Codec::Mermaid => lz::mermaid::decode(body, mode, dst, &pl)?,
                    Codec::Leviathan => lz::leviathan::decode(body, mode, dst, &pl)?,
                }
            } else if n == sub_len && mode == 0 {
                dst[start..start + sub_len].copy_from_slice(body);
            } else {
                return Err(Error::Corrupt("bad sub-chunk header"));
            }
            q += 3 + n;
        }
        d += sub_len;
    }
    if q != payload.len() {
        return Err(Error::Corrupt("quantum payload not exactly consumed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_output_needs_no_input() {
        assert_eq!(decode(&[], &mut []), Ok(0));
    }

    #[test]
    fn truncated_input_is_an_error() {
        let mut out = [0u8; 4];
        assert_eq!(decode(&[], &mut out), Err(Error::Truncated));
        assert_eq!(decode(&[0x4C, 0x06, 1], &mut out), Err(Error::Truncated));
    }
}
