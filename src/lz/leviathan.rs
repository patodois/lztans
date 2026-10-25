//! Leviathan LZ sub-chunks.

use super::{copy_match, raw_prefix, read_side_streams, Place};
use crate::entropy::{decode_array, decode_multi};
use crate::error::{corrupt, Error, Result};
use std::borrow::Cow;

/// Literal arrays of a sub-chunk with their cursors.
struct Literals<'a> {
    mode: u32,
    arrays: Vec<Cow<'a, [u8]>>,
    cur: Vec<usize>,
}

impl<'a> Literals<'a> {
    fn take(&mut self, idx: usize) -> Result<u8> {
        let a = &self.arrays[idx];
        let c = self.cur[idx];
        let b = *a
            .get(c)
            .ok_or(Error::Corrupt("leviathan: literal array exhausted"))?;
        self.cur[idx] = c + 1;
        Ok(b)
    }

    /// Copy a run of `n` literals to `out[p..]`; `dist` is the delta distance.
    fn run(&mut self, out: &mut [u8], p: usize, n: usize, dist: usize, pl: &Place) -> Result<()> {
        if n == 0 {
            return Ok(());
        }
        if n > pl.end - p {
            return corrupt("leviathan: literal run past the sub-chunk");
        }
        match self.mode {
            0 | 1 => {
                let mut c = self.cur[0];
                if self.mode == 1 {
                    super::copy_raw(out, p, &self.arrays[0], &mut c, n, pl.end)?;
                } else {
                    super::copy_delta(out, p, &self.arrays[0], &mut c, n, dist, pl, pl.end)?;
                }
                self.cur[0] = c;
                Ok(())
            }
            2 => {
                if dist == 0 || p < pl.base || dist > p - pl.base {
                    return corrupt("leviathan: delta literal reaches before the window");
                }
                let first = self.take(1)?.wrapping_add(out[p - dist]);
                out[p] = first;
                let mut c = self.cur[0];
                super::copy_delta(out, p + 1, &self.arrays[0], &mut c, n - 1, dist, pl, pl.end)?;
                self.cur[0] = c;
                Ok(())
            }
            3 | 5 => {
                if dist == 0 || p < pl.base || dist > p - pl.base {
                    return corrupt("leviathan: delta literal reaches before the window");
                }
                let m = if self.mode == 3 { 4 } else { 16 };
                for i in 0..n {
                    let q = p + i;
                    let b = self.take((q - pl.start) % m)?;
                    out[q] = b.wrapping_add(out[q - dist]);
                }
                Ok(())
            }
            _ => {
                if p == 0 {
                    return corrupt("leviathan: no previous byte");
                }
                for i in 0..n {
                    let q = p + i;
                    let idx = usize::from(out[q - 1] >> 4);
                    out[q] = self.take(idx)?;
                }
                Ok(())
            }
        }
    }

    fn all_used(&self) -> bool {
        self.arrays
            .iter()
            .zip(self.cur.iter())
            .all(|(a, &c)| a.len() == c)
    }
}

pub(crate) fn decode(body: &[u8], mode: u32, out: &mut [u8], pl: &Place) -> Result<()> {
    if mode > 5 {
        return corrupt("leviathan: invalid mode");
    }
    let dst_count = pl.dst_count();
    let mut pos = raw_prefix(body, out, pl)?;
    let mut p = pl.start + pos;

    let x = *body.get(pos).ok_or(Error::Truncated)?;
    let scale = if x & 0x80 != 0 {
        pos += 1;
        u32::from(x) - 127
    } else {
        0
    };
    let (oc, n) = decode_array(&body[pos..], dst_count / 3, 0)?;
    pos += n;
    let ox = if scale >= 2 {
        let (a, n) = decode_array(&body[pos..], dst_count / 3, 0)?;
        pos += n;
        if a.len() != oc.len() {
            return corrupt("leviathan: offset extras length");
        }
        Some(a)
    } else {
        None
    };
    let (lc, n) = decode_array(&body[pos..], dst_count / 5, 0)?;
    pos += n;

    let arrays: Vec<Cow<'_, [u8]>> = match mode {
        0 | 1 => {
            let (a, n) = decode_array(&body[pos..], dst_count, 0)?;
            pos += n;
            vec![a]
        }
        _ => {
            let n_out = match mode {
                2 => 2,
                3 => 4,
                _ => 16,
            };
            let (v, n) = decode_multi(&body[pos..], n_out, dst_count, 0)?;
            pos += n;
            v.into_iter().map(Cow::Owned).collect()
        }
    };
    let mut lits = Literals {
        mode,
        cur: vec![0; arrays.len()],
        arrays,
    };

    // Commands: a single array or eight lanes.
    let cmd_first = *body.get(pos).ok_or(Error::Truncated)?;
    let lanes: Vec<Cow<'_, [u8]>>;
    if cmd_first & 0x80 == 0 {
        let (a, n) = decode_array(&body[pos..], dst_count, 0)?;
        pos += n;
        lanes = vec![a];
    } else {
        if cmd_first != 0x83 {
            return corrupt("leviathan: bad command lane marker");
        }
        pos += 1;
        let (v, n) = decode_multi(&body[pos..], 8, dst_count, 0)?;
        pos += n;
        lanes = v.into_iter().map(Cow::Owned).collect();
    }
    let side = read_side_streams(&body[pos..], &oc, ox.as_deref(), scale, &lc)?;

    let total_cmds: usize = lanes.iter().map(|l| l.len()).sum();
    let multi = lanes.len() == 8;
    let mut lane_cur = [0usize; 8];
    let end = pl.end;
    let mut recent = [8usize; 7];
    let mut lo = 8usize;
    let mut off_cur = 0usize;
    let mut front = 0usize;
    let mut back = side.lens.len();

    for k in 0..total_cmds {
        let cmd = if multi {
            let lane = (p - pl.start) & 7;
            let c = *lanes[lane]
                .get(lane_cur[lane])
                .ok_or(Error::Corrupt("leviathan: command lane exhausted"))?;
            lane_cur[lane] += 1;
            c
        } else {
            lanes[0][k]
        };
        let oidx = usize::from(cmd >> 5);
        let litc = usize::from((cmd >> 3) & 3);
        let mlc = usize::from(cmd & 7);
        let mut lit_n = litc;
        if litc == 3 {
            if front >= back {
                return corrupt("leviathan: lengths exhausted");
            }
            lit_n = side.lens[front] as usize;
            front += 1;
        }
        lits.run(out, p, lit_n, lo, pl)?;
        p += lit_n;
        let dist = if oidx == 7 {
            let d = *side
                .offsets
                .get(off_cur)
                .ok_or(Error::Corrupt("leviathan: offsets exhausted"))?;
            off_cur += 1;
            recent = [
                d, recent[0], recent[1], recent[2], recent[3], recent[4], recent[5],
            ];
            d
        } else {
            let d = recent[oidx];
            recent.copy_within(0..oidx, 1);
            recent[0] = d;
            d
        };
        lo = dist;
        let mlen = if mlc == 7 {
            if front >= back {
                return corrupt("leviathan: lengths exhausted");
            }
            back -= 1;
            side.lens[back] as usize + 6
        } else {
            mlc + 2
        };
        copy_match(out, p, dist, mlen, pl, end)?;
        p += mlen;
    }
    lits.run(out, p, end - p, lo, pl)?;
    if !lits.all_used() || off_cur != side.offsets.len() || front != back {
        return corrupt("leviathan: unused literals, offsets or lengths");
    }
    Ok(())
}
