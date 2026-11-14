//! Round trips through a test-side encoder for the forms that no committed vector
//! holds: tANS arrays, 6-stream Huffman, type-5 arrays (concatenated and sliced
//! containers), Leviathan literal modes 0 to 5 with one or eight command lanes and
//! long lengths through the 255 escape, and Mermaid with entropy-coded near offsets.
//!
//! The streams are built from this decoder's own rules (`tests/common/enc.rs`,
//! `tests/common/lz.rs`): they guard against regressions in those paths and do not
//! prove conformance to the format. The game-data corpus does that
//! (`tests/vectors.rs`, `LZTANS_CORPUS`). Seeds are fixed, so every run builds the
//! same streams.

mod common;

use common::enc::{free_array, huffman, rle, sliced_runs, tans};
use common::lz::{leviathan, mermaid};
use common::{block, long, lz_sub, Rng};

const KRAKEN: u8 = 0x06;
const MERMAID: u8 = 0x0A;
const LEVIATHAN: u8 = 0x0C;

/// `stream` must decode to `expect` and consume all of its bytes.
fn round_trip(stream: &[u8], expect: &[u8], what: &str) {
    let mut out = vec![0u8; expect.len()];
    let res = lztans::decode(stream, &mut out);
    assert_eq!(res, Ok(stream.len()), "{what}");
    if let Some(i) = out.iter().zip(expect).position(|(a, b)| a != b) {
        panic!("{what}: first wrong byte at {i} of {}", expect.len());
    }
}

/// Run `make` until it has produced `want` cases, giving up after `10 * want` tries.
fn cases<T>(want: usize, mut make: impl FnMut() -> Option<T>) -> Vec<T> {
    let mut v = Vec::new();
    for _ in 0..10 * want {
        if v.len() == want {
            break;
        }
        if let Some(x) = make() {
            v.push(x);
        }
    }
    assert_eq!(v.len(), want, "the generator gave up too often");
    v
}

#[test]
fn tans_arrays() {
    let mut rng = Rng::new(0x7A45_0001);
    for (i, (arr, data)) in cases(400, || {
        let n = rng.range(5, 3000);
        tans(&mut rng, n)
    })
    .into_iter()
    .enumerate()
    {
        round_trip(&block(KRAKEN, &arr), &data, &format!("tans case {i}"));
    }
}

#[test]
fn huffman_arrays_three_and_six_streams() {
    let mut rng = Rng::new(0x4855_0002);
    for six in [false, true] {
        for (i, (arr, data)) in cases(300, || {
            let n = rng.range(200, 3000);
            huffman(&mut rng, n, six)
        })
        .into_iter()
        .enumerate()
        {
            assert_eq!(arr[0] >> 4, if six { 4 } else { 2 });
            round_trip(
                &block(MERMAID, &arr),
                &data,
                &format!("huffman six={six} case {i}"),
            );
        }
    }
}

#[test]
fn rle_arrays() {
    let mut rng = Rng::new(0x524C_0003);
    for (i, (arr, data)) in cases(500, || rle(&mut rng, 0)).into_iter().enumerate() {
        round_trip(&block(LEVIATHAN, &arr), &data, &format!("rle case {i}"));
    }
}

/// Type-5 arrays that list their parts: each part is a tANS, Huffman, RLE or
/// stored array (which may itself be a concatenation or a sliced container).
#[test]
fn concatenated_arrays() {
    let mut rng = Rng::new(0x434F_0004);
    for (i, (arr, data)) in cases(400, || {
        let parts = rng.range(2, 6);
        let mut body = vec![parts as u8];
        let mut data = Vec::new();
        for _ in 0..parts {
            let n = rng.range(5, 800);
            let (a, x) = free_array(&mut rng, n, 1);
            body.extend(a);
            data.extend(x);
        }
        (body.len() < data.len() && data.len() <= 131_072)
            .then(|| (long(5, data.len(), &body), data))
    })
    .into_iter()
    .enumerate()
    {
        round_trip(
            &block(KRAKEN, &arr),
            &data,
            &format!("concatenation case {i}"),
        );
    }
}

/// Type-5 sliced containers with two or three source arrays, both index list forms.
#[test]
fn sliced_containers() {
    let mut rng = Rng::new(0x534C_0005);
    let mut combined = 0;
    let all = cases(400, || {
        let n = rng.range(300, 6000);
        sliced_runs(&mut rng, n)
    });
    for (i, (arr, data, c)) in all.iter().enumerate() {
        round_trip(&block(KRAKEN, arr), data, &format!("sliced case {i}"));
        combined += usize::from(*c);
    }
    assert!(
        combined > 100 && combined < 300,
        "{combined} of 400 combined"
    );
}

/// Every literal mode, with one command array and with eight lanes; the first
/// sub-chunk of a block (window start) and a second one after a stored sub-chunk.
#[test]
fn leviathan_literal_modes_and_lanes() {
    let mut rng = Rng::new(0x4C45_0006);
    let first = rng.bytes(131_072);
    for mode in 0..6u8 {
        for eight in [false, true] {
            let what = format!("leviathan mode {mode} eight lanes {eight}");
            for (i, lev) in cases(40, || {
                let n = rng.range(40, 3000);
                leviathan(&mut rng, &[], n, mode, eight)
            })
            .into_iter()
            .enumerate()
            {
                assert_eq!(lev.eight_lanes, eight);
                round_trip(
                    &block(LEVIATHAN, &lz_sub(mode, &lev.body)),
                    &lev.out,
                    &format!("{what} case {i}"),
                );
            }
            for (i, lev) in cases(4, || {
                let n = rng.range(64, 6000);
                leviathan(&mut rng, &first, n, mode, eight)
            })
            .into_iter()
            .enumerate()
            {
                let mut payload = lz_sub(0, &first);
                payload.extend(lz_sub(mode, &lev.body));
                round_trip(
                    &block(LEVIATHAN, &payload),
                    &lev.out,
                    &format!("{what}, second sub-chunk, case {i}"),
                );
            }
        }
    }
}

/// Literal and match lengths above 257 travel through the 255 escape.
#[test]
fn leviathan_long_lengths() {
    let mut rng = Rng::new(0x4C4F_0007);
    let mut with_long = 0;
    let mut long_total = 0;
    for i in 0..120 {
        let mode = (i % 6) as u8;
        let Some(lev) = ({
            let n = rng.range(8000, 40_000);
            let eight = rng.below(2) == 0;
            leviathan(&mut rng, &[], n, mode, eight)
        }) else {
            continue;
        };
        round_trip(
            &block(LEVIATHAN, &lz_sub(mode, &lev.body)),
            &lev.out,
            &format!("long lengths case {i}"),
        );
        with_long += usize::from(lev.long_lengths > 0);
        long_total += lev.long_lengths;
    }
    assert!(with_long >= 60, "{with_long} streams with long lengths");
    assert!(long_total >= 200, "{long_total} long lengths");
}

/// Near offsets in two entropy-coded arrays; far offset counts with and without
/// the 4095 escape.
#[test]
fn mermaid_entropy_coded_near_offsets() {
    let mut rng = Rng::new(0x4D45_0008);
    let (mut near, mut escaped) = (0, 0);
    for (i, m) in cases(400, || {
        let n = rng.range(60, 5000);
        mermaid(&mut rng, n)
    })
    .into_iter()
    .enumerate()
    {
        round_trip(
            &block(MERMAID, &lz_sub(1, &m.body)),
            &m.out,
            &format!("mermaid case {i}"),
        );
        near += m.near;
        escaped += usize::from(m.far_escape);
    }
    assert!(near >= 3000, "{near} near offsets");
    assert!(escaped >= 60, "{escaped} far counts through the escape");
}
