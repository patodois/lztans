//! LZ sub-chunk decoding shared by the three codecs.

pub(crate) mod kraken;
pub(crate) mod leviathan;
pub(crate) mod mermaid;

use crate::bits::{Mb, Mf};
use crate::error::{corrupt, Error, Result};

/// Where a sub-chunk sits inside the output buffer.
#[derive(Clone, Copy)]
pub(crate) struct Place {
    /// Absolute index of the window start.
    pub base: usize,
    /// Absolute index of the sub-chunk's first output byte.
    pub start: usize,
    /// Absolute index one past the sub-chunk's last output byte.
    pub end: usize,
}

impl Place {
    pub fn sub_len(&self) -> usize {
        self.end - self.start
    }
}

/// Window start and the exclusive upper bound a literal run may reach.
#[derive(Clone, Copy)]
pub(crate) struct Win {
    pub base: usize,
    pub limit: usize,
}

/// Offsets and lengths decoded from the two bit streams.
pub(crate) struct SideStreams {
    /// Match distances (positive).
    pub offsets: Vec<usize>,
    /// Length entries (each at least 3).
    pub lens: Vec<u32>,
}

/// Decode the offset and length bit streams (`src` is everything after the last
/// array of the sub-chunk body).
pub(crate) fn read_side_streams(
    src: &[u8],
    oc: &[u8],
    ox: Option<&[u8]>,
    scale: u32,
    lc: &[u8],
) -> Result<SideStreams> {
    if src.is_empty() {
        return Err(Error::Truncated);
    }
    let mut fwd = Mf::new(src);
    let mut bwd = Mb::new(src);

    // Number of long lengths.
    let z = bwd
        .leading_zeros()
        .ok_or(Error::Corrupt("long-length count"))?;
    if z > 10 {
        return corrupt("too many long lengths");
    }
    bwd.consume(z);
    let v = bwd.read(z + 1);
    let n_long = (v - 1) as usize;
    if n_long > 512 {
        return corrupt("too many long lengths");
    }

    let mut offsets = Vec::with_capacity(oc.len());
    for (i, &c) in oc.iter().enumerate() {
        macro_rules! rd {
            ($k:expr) => {
                if i % 2 == 0 {
                    fwd.read($k)
                } else {
                    bwd.read($k)
                }
            };
        }
        let dist: u64;
        if scale == 0 {
            if c < 0xF0 {
                let n = u32::from(c >> 4) + 4;
                let m = (1u64 << n) + u64::from(rd!(n));
                dist = (m << 4) + u64::from(c & 15) - 248;
            } else {
                let n = u32::from(c - 0xF0) + 4;
                let m = (1u64 << n) + u64::from(rd!(n));
                let low = u64::from(rd!(12));
                dist = 8_322_816 + (m << 12) + low;
            }
        } else {
            let nb = u32::from(c >> 3);
            if nb > 26 {
                return corrupt("scaled offset: too many bits");
            }
            let o = (u64::from(8 + (c & 7)) << nb) | u64::from(rd!(nb));
            let od = o - 8;
            let d = if scale == 1 {
                od
            } else {
                let x = ox
                    .and_then(|a| a.get(i))
                    .copied()
                    .ok_or(Error::Corrupt("offset extras"))?;
                od * u64::from(scale) + u64::from(x)
            };
            if d == 0 {
                return corrupt("scaled offset: zero distance");
            }
            dist = d;
        }
        offsets.push(usize::try_from(dist).map_err(|_| Error::Corrupt("offset too large"))?);
    }

    let mut longs: Vec<u32> = Vec::with_capacity(n_long);
    for i in 0..n_long {
        let r_zero = if i % 2 == 0 {
            fwd.leading_zeros()
        } else {
            bwd.leading_zeros()
        };
        let z = r_zero.ok_or(Error::Corrupt("long length code"))?;
        if z > 12 {
            return corrupt("long length code too long");
        }
        let v = if i % 2 == 0 {
            fwd.consume(z);
            fwd.read(z + 7)
        } else {
            bwd.consume(z);
            bwd.read(z + 7)
        };
        longs.push(v - 64);
    }

    if fwd.overrun() || bwd.overrun() || fwd.byte_pos() + bwd.bytes_used() != src.len() {
        return corrupt("offset/length bit streams not exactly consumed");
    }

    let mut lens = Vec::with_capacity(lc.len());
    let mut li = 0usize;
    for &b in lc {
        let v = if b == 255 {
            let x = *longs
                .get(li)
                .ok_or(Error::Corrupt("long lengths exhausted"))?;
            li += 1;
            x + 255
        } else {
            u32::from(b)
        };
        lens.push(v + 3);
    }
    if li != longs.len() {
        return corrupt("unused long lengths");
    }
    Ok(SideStreams { offsets, lens })
}

/// Copy a match of `len` bytes from `dist` bytes back to position `p`.
#[inline]
pub(crate) fn copy_match(
    out: &mut [u8],
    p: usize,
    dist: usize,
    len: usize,
    pl: &Place,
    limit: usize,
) -> Result<()> {
    if p < pl.base || dist == 0 || dist > p - pl.base {
        return corrupt("match reaches before the window");
    }
    if len > limit - p {
        return corrupt("match runs past the end of its sub-chunk");
    }
    if dist < 8 && len > dist {
        // Spec 4.4: an overlapping match with distance below 8 is invalid.
        return corrupt("overlapping match with distance below 8");
    }
    if dist >= len {
        if len <= 16 {
            // Short copies are common; a plain loop beats a library call.
            let (head, tail) = out.split_at_mut(p);
            let src = &head[p - dist..p - dist + len];
            for (d, s) in tail[..len].iter_mut().zip(src) {
                *d = *s;
            }
        } else {
            out.copy_within(p - dist..p - dist + len, p);
        }
    } else {
        let mut done = 0usize;
        let src = p - dist;
        while done < len {
            let n = dist.min(len - done);
            out.copy_within(src + done..src + done + n, p + done);
            done += n;
        }
    }
    Ok(())
}

/// Copy `n` raw literals taken from `lit[*cur..]` to `out[p..]`.
#[inline]
pub(crate) fn copy_raw(
    out: &mut [u8],
    p: usize,
    lit: &[u8],
    cur: &mut usize,
    n: usize,
    limit: usize,
) -> Result<()> {
    if n > limit - p || n > lit.len() - *cur {
        return corrupt("literal run beyond its array or sub-chunk");
    }
    out[p..p + n].copy_from_slice(&lit[*cur..*cur + n]);
    *cur += n;
    Ok(())
}

/// Copy `n` delta literals: `out[p+i] = lit[cur+i] + out[p+i-dist]`.
#[inline]
pub(crate) fn copy_delta(
    out: &mut [u8],
    p: usize,
    lit: &[u8],
    cur: &mut usize,
    n: usize,
    dist: usize,
    win: Win,
) -> Result<()> {
    if n == 0 {
        return Ok(());
    }
    if n > win.limit - p || n > lit.len() - *cur {
        return corrupt("literal run beyond its array or sub-chunk");
    }
    if dist < 8 && n > dist {
        // Spec 4.5: a delta run longer than a last distance below 8 is invalid.
        return corrupt("delta literal run longer than a last distance below 8");
    }
    if dist == 0 || p < win.base || dist > p - win.base {
        return corrupt("delta literal reaches before the window");
    }
    let src = &lit[*cur..*cur + n];
    if dist >= n {
        let (head, tail) = out.split_at_mut(p);
        let prev = &head[p - dist..p - dist + n];
        for ((d, s), q) in tail[..n].iter_mut().zip(src).zip(prev) {
            *d = s.wrapping_add(*q);
        }
    } else {
        for (i, s) in src.iter().enumerate() {
            out[p + i] = s.wrapping_add(out[p + i - dist]);
        }
    }
    *cur += n;
    Ok(())
}

/// At window position 0 a sub-chunk starts with eight raw bytes. Copies them and
/// returns how many body bytes they occupied (0 or 8).
pub(crate) fn raw_prefix(body: &[u8], out: &mut [u8], pl: &Place) -> Result<usize> {
    if pl.start != pl.base {
        return Ok(0);
    }
    if pl.sub_len() < 8 {
        return corrupt("sub-chunk too short for its eight raw bytes");
    }
    let raw = body.get(..8).ok_or(Error::Truncated)?;
    out[pl.start..pl.start + 8].copy_from_slice(raw);
    Ok(8)
}
