//! Mermaid / Selkie LZ sub-chunks.

use super::{copy_delta, copy_match, copy_raw, raw_prefix, Place, Win};
use crate::entropy::decode_array;
use crate::error::{corrupt, Error, Result};

const PIECE: usize = 65536;

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn u16(&mut self) -> Result<usize> {
        let b = self
            .data
            .get(self.pos..self.pos + 2)
            .ok_or(Error::Truncated)?;
        self.pos += 2;
        Ok(usize::from(b[0]) | usize::from(b[1]) << 8)
    }
    fn u24(&mut self) -> Result<usize> {
        let b = self
            .data
            .get(self.pos..self.pos + 3)
            .ok_or(Error::Truncated)?;
        self.pos += 3;
        Ok(usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16)
    }
    fn u8(&mut self) -> Result<usize> {
        let b = *self.data.get(self.pos).ok_or(Error::Truncated)?;
        self.pos += 1;
        Ok(usize::from(b))
    }
}

pub(crate) fn decode(body: &[u8], mode: u32, out: &mut [u8], pl: &Place) -> Result<()> {
    if mode > 1 {
        return corrupt("mermaid: invalid mode");
    }
    let delta = mode == 0;
    let dst_count = pl.dst_count();
    let mut pos = raw_prefix(body, out, pl)?;
    let (lit, n) = decode_array(&body[pos..], dst_count, 0)?;
    pos += n;
    let (cmd, n) = decode_array(&body[pos..], dst_count, 0)?;
    pos += n;
    let mut cur = Cursor { data: body, pos };
    let split = if dst_count > PIECE {
        let s = cur.u16()?;
        if s > cmd.len() {
            return corrupt("mermaid: command split beyond the command array");
        }
        s
    } else {
        cmd.len()
    };
    let n16 = cur.u16()?;
    let near: Vec<usize>;
    if n16 == 0xFFFF {
        let (hi, n) = decode_array(&body[cur.pos..], dst_count >> 1, 0)?;
        cur.pos += n;
        let (lo, n) = decode_array(&body[cur.pos..], dst_count >> 1, 0)?;
        cur.pos += n;
        if hi.len() != lo.len() {
            return corrupt("mermaid: near offset arrays differ in length");
        }
        near = hi
            .iter()
            .zip(lo.iter())
            .map(|(&h, &l)| usize::from(l) | usize::from(h) << 8)
            .collect();
    } else {
        let bytes = body
            .get(cur.pos..cur.pos + 2 * n16)
            .ok_or(Error::Truncated)?;
        near = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| usize::from(c[0]) | usize::from(c[1]) << 8)
            .collect();
        cur.pos += 2 * n16;
    }
    let t = cur.u24()?;
    let (mut n1, mut n2) = (0usize, 0usize);
    if t != 0 {
        n1 = t >> 12;
        n2 = t & 0xFFF;
        if n1 == 4095 {
            n1 = cur.u16()?;
        }
        if n2 == 4095 {
            n2 = cur.u16()?;
        }
    }
    let window_pos = pl.start - pl.base;
    let mut far: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for (piece, count) in [n1, n2].into_iter().enumerate() {
        let bound = window_pos + piece * PIECE;
        if count > body.len() - cur.pos {
            return Err(Error::Truncated);
        }
        let mut v = Vec::with_capacity(count);
        for _ in 0..count {
            let mut x = cur.u24()?;
            if bound >= 0xC0_0000 - 1 && x >= 0xC0_0000 {
                x += cur.u8()? << 22;
            }
            if x > bound {
                return corrupt("mermaid: far offset beyond the window");
            }
            v.push(x);
        }
        far[piece] = v;
    }
    let lenbytes = &body[cur.pos..];

    // Command walk.
    let end = pl.end;
    let mut p = pl.start + pos_prefix(pl);
    let mut recent = 8usize;
    let mut lit_cur = 0usize;
    let mut near_cur = 0usize;
    let mut len_cur = 0usize;
    let pieces = if dst_count > PIECE { 2 } else { 1 };
    for (piece, fars) in far.iter().enumerate().take(pieces) {
        let piece_start = pl.start + piece * PIECE;
        let piece_end = if piece == 0 {
            pl.start + dst_count.min(PIECE)
        } else {
            end
        };
        let cmds = if piece == 0 {
            &cmd[..split]
        } else {
            &cmd[split..]
        };
        let win = Win {
            base: pl.base,
            limit: piece_end,
        };
        let mut far_cur = 0usize;
        for &c in cmds {
            let c = usize::from(c);
            if c >= 24 {
                let n = c & 7;
                if delta {
                    copy_delta(out, p, &lit, &mut lit_cur, n, recent, win)?;
                } else {
                    copy_raw(out, p, &lit, &mut lit_cur, n, piece_end)?;
                }
                p += n;
                if c < 128 {
                    recent = *near
                        .get(near_cur)
                        .ok_or(Error::Corrupt("mermaid: near offsets exhausted"))?;
                    near_cur += 1;
                }
                let mlen = (c >> 3) & 15;
                copy_match(out, p, recent, mlen, pl, piece_end)?;
                p += mlen;
            } else if c >= 3 {
                let v = *fars
                    .get(far_cur)
                    .ok_or(Error::Corrupt("mermaid: far offsets exhausted"))?;
                far_cur += 1;
                let mlen = c + 5;
                recent = far_dist(piece_start, v, p, pl)?;
                copy_match(out, p, recent, mlen, pl, piece_end)?;
                p += mlen;
            } else {
                let l = read_len(lenbytes, &mut len_cur)?;
                match c {
                    0 => {
                        let n = l + 64;
                        if delta {
                            copy_delta(out, p, &lit, &mut lit_cur, n, recent, win)?;
                        } else {
                            copy_raw(out, p, &lit, &mut lit_cur, n, piece_end)?;
                        }
                        p += n;
                    }
                    1 => {
                        recent = *near
                            .get(near_cur)
                            .ok_or(Error::Corrupt("mermaid: near offsets exhausted"))?;
                        near_cur += 1;
                        let mlen = l + 91;
                        copy_match(out, p, recent, mlen, pl, piece_end)?;
                        p += mlen;
                    }
                    _ => {
                        let v = *fars
                            .get(far_cur)
                            .ok_or(Error::Corrupt("mermaid: far offsets exhausted"))?;
                        far_cur += 1;
                        let mlen = l + 29;
                        recent = far_dist(piece_start, v, p, pl)?;
                        copy_match(out, p, recent, mlen, pl, piece_end)?;
                        p += mlen;
                    }
                }
            }
        }
        let rest = piece_end - p;
        if delta {
            copy_delta(out, p, &lit, &mut lit_cur, rest, recent, win)?;
        } else {
            copy_raw(out, p, &lit, &mut lit_cur, rest, piece_end)?;
        }
        p += rest;
    }
    // SPEC-QUESTION: unused literals / near offsets are rejected; the oracle
    // accepts them (QUESTIONS.md, item 4).
    if lit_cur != lit.len() {
        return corrupt("mermaid: unused literals");
    }
    if near_cur != near.len() {
        return corrupt("mermaid: unused near offsets");
    }
    if len_cur != lenbytes.len() {
        return corrupt("mermaid: unused length bytes");
    }
    Ok(())
}

fn pos_prefix(pl: &Place) -> usize {
    if pl.start == pl.base {
        8
    } else {
        0
    }
}

/// Distance of a far match whose source is `v` bytes before `piece_start`.
fn far_dist(piece_start: usize, v: usize, p: usize, pl: &Place) -> Result<usize> {
    let src = piece_start
        .checked_sub(v)
        .ok_or(Error::Corrupt("mermaid: far offset"))?;
    if src < pl.base || src >= p {
        return corrupt("mermaid: far match source outside the window");
    }
    Ok(p - src)
}

fn read_len(bytes: &[u8], cur: &mut usize) -> Result<usize> {
    let b = *bytes
        .get(*cur)
        .ok_or(Error::Corrupt("mermaid: length bytes exhausted"))?;
    if b <= 251 {
        *cur += 1;
        Ok(usize::from(b))
    } else {
        let e = bytes
            .get(*cur + 1..*cur + 3)
            .ok_or(Error::Corrupt("mermaid: length bytes exhausted"))?;
        *cur += 3;
        Ok(usize::from(b) + 4 * (usize::from(e[0]) | usize::from(e[1]) << 8))
    }
}
