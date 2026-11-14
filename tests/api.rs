//! Behaviour that the documentation of `decode` and `Error` promises.

mod common;

use common::{block, long, stored_long, stored_short};
use lztans::{decode, Error};

/// An inner length that runs past its enclosing region, in an input that is
/// complete: the stream is corrupt, not truncated.
#[test]
fn region_overrun_in_a_complete_input_is_corrupt() {
    // A type-5 array whose second part, a stored array, claims 50 bytes where 16 remain.
    let mut body = vec![0x02u8, 0x80, 50];
    body.extend_from_slice(&[0x41; 10]);
    body.extend(stored_short(&[0x42; 4]));
    let s = block(0x06, &long(5, 100, &body));
    assert_eq!(s.len(), 29);
    assert!(matches!(
        decode(&s, &mut [0u8; 100]),
        Err(Error::Corrupt(_))
    ));

    // A Kraken sub-chunk whose body ends right after its command array.
    let mut body = vec![0x55u8; 8];
    body.extend(stored_long(&[0x41; 2]));
    body.extend(stored_long(&[]));
    let s = block(0x06, &common::lz_sub(1, &body));
    assert!(matches!(
        decode(&s, &mut [0u8; 100]),
        Err(Error::Corrupt(_))
    ));
}

/// Input that ends before the stream does is still `Truncated`.
#[test]
fn input_cut_short_is_truncated() {
    let mut body = vec![0x02u8];
    body.extend(long(3, 60, &[0x41]));
    body.extend(long(3, 40, &[0x42]));
    let s = block(0x06, &long(5, 100, &body));
    let mut out = [0u8; 100];
    assert_eq!(decode(&s, &mut out), Ok(s.len()));
    for cut in [1, 2, 4, 5, 6, 12, s.len() - 1] {
        assert_eq!(
            decode(&s[..cut], &mut out),
            Err(Error::Truncated),
            "cut at {cut}"
        );
    }
}

/// A stored block of four bytes.
const ABCD: [u8; 6] = [0x4C, 0x06, b'a', b'b', b'c', b'd'];

/// `dst` longer than the stream, input ending where the stream ends: an error here.
#[test]
fn longer_dst_with_input_ending_at_the_stream_end() {
    let mut out = [0u8; 5];
    assert_eq!(decode(&ABCD, &mut out), Err(Error::Truncated));
    // A memset last block is the documented exception: it fills the extra bytes.
    let memset = [0x8C, 0x06, 0x07, 0xFF, 0xFF, 0x41];
    let mut out = [0u8; 1005];
    assert_eq!(decode(&memset, &mut out), Ok(memset.len()));
    assert!(out.iter().all(|&b| b == 0x41));
}

/// `dst` longer than the stream, more input after it: the call can succeed with bytes
/// that do not belong to the stream, and only the returned count shows it.
#[test]
fn longer_dst_with_input_after_the_stream() {
    let mut padded = ABCD.to_vec();
    padded.extend_from_slice(&[0; 16]);
    let mut out = [0u8; 5];
    let used = decode(&padded, &mut out).unwrap();
    assert_eq!(&out[..4], b"abcd");
    assert_ne!(
        used,
        ABCD.len(),
        "the count differs from the compressed size"
    );

    let two = [ABCD, ABCD].concat();
    let mut out = [0u8; 6];
    let used = decode(&two, &mut out).unwrap();
    assert_eq!(&out[..4], b"abcd");
    assert_ne!(
        used,
        ABCD.len(),
        "the count differs from the compressed size"
    );

    // With the right length the count is the compressed size and later input is ignored.
    let mut out = [0u8; 4];
    assert_eq!(decode(&two, &mut out), Ok(ABCD.len()));
    assert_eq!(&out, b"abcd");
}
