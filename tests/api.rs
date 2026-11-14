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
