//! RLE arrays whose nested work array is longer than the RLE's own output.
//!
//! The work array nested in an RLE body is bounded by the array grammar (262,144
//! bytes), not by the output size: fill changes consume work without emitting
//! output. Every command still has to fit in the output that remains.

mod common;

use common::{block, long, rle, short, stream};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

const MAX_ARRAY: usize = 1 << 18;

fn vector(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/vectors")
            .join(name),
    )
    .unwrap()
}

fn sha(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The inner array: 262,144 bytes of work made of 131,071 literals, a command
/// that emits 127 copies of the fill byte, and 131,071 fill changes. Each fill
/// change takes one literal and emits nothing.
fn max_work_array() -> Vec<u8> {
    const F: usize = 131_071;
    // Commands in execution order, each as the bytes it occupies in the work
    // array (the last byte of a two-byte command is its high byte).
    let ops: [&[u8]; 10] = [
        &[0x01],       // fill = next literal
        &[0xFE, 0x0C], // 1023 * 128 copies of the fill
        &[0xC0, 0x2F], // 127 copies of the fill
        &[0x01],
        &[0x40, 0x10], // one copy
        &[0x01],
        &[0x40, 0x10],
        &[0x01],
        &[0xFE, 0x0C],
        &[0xC0, 0x2F],
    ];
    let mut body = vec![0u8, 0x41, 0xC0, 0x2F, 0x01];
    for op in ops.iter().rev() {
        body.extend_from_slice(op);
    }
    rle(2 * F + 2, &body)
}

fn run(data: &[u8], size: usize) -> (Result<usize, lztans::Error>, Vec<u8>) {
    let mut out = vec![0u8; size];
    (lztans::decode(data, &mut out), out)
}

#[test]
fn work_longer_than_output_decodes() {
    let data = vector("syn-nested-rle-work.bin");
    let (res, out) = run(&data, 64);
    assert_eq!(res, Ok(data.len()));
    assert_eq!(
        sha(&out),
        "d53eda7a637c99cc7fb566d96e9fa109bf15c478410a3f5eb4d4c4e26cd081f6"
    );
}

#[test]
fn work_at_the_grammar_limit_decodes() {
    let data = vector("syn-nested-rle-work-max.bin");
    assert_eq!(data, stream(&rle(127, &max_work_array())));
    let (res, out) = run(&data, 127);
    assert_eq!(res, Ok(data.len()));
    assert_eq!(out, vec![b'A'; 127]);
    assert_eq!(
        sha(&out),
        "026134f6117e45a37c5c2dc2f330bdd274c6dc087526b91ecec4d6dac9bb7346"
    );
}

/// The commands must still fit in the output: one byte too many or too few is refused.
#[test]
fn commands_beyond_the_output_are_refused() {
    let inner = max_work_array();
    for size in [126usize, 128] {
        let (res, out) = run(&stream(&rle(size, &inner)), size);
        assert!(res.is_err(), "output size {size} must be refused");
        assert_eq!(out.len(), size);
    }
}

/// A work array may not be declared above 262,144 bytes. The stored form's size
/// field is 18 bits wide with the top bits required to be clear, so 262,144 is not
/// expressible there; the stream is refused whatever follows the header.
#[test]
fn work_array_above_the_limit_is_refused() {
    let mut body = vec![0x04, 0x00, 0x00]; // type 0, 3-byte form, size 262,144
    body.extend_from_slice(&[0x41; 40]);
    let data = stream(&rle(100, &body));
    let (res, _) = run(&data, 100);
    assert!(matches!(res, Err(lztans::Error::Corrupt(_))), "{res:?}");
    // The largest size the long header form can declare is exactly 262,144.
    assert_eq!(rle(1 << 18, &[0x41])[..5], [0x3F, 0xFF, 0xFC, 0x00, 0x01]);
}

/// Input that ends inside the nested work is an error, not a panic.
#[test]
fn truncated_work_is_refused() {
    let mut cut = stream(&rle(127, &max_work_array()));
    cut.truncate(cut.len() - 1);
    let (res, _) = run(&cut, 127);
    assert!(res.is_err());
}

/// An RLE array of 11 bytes that emits 16 zero bytes after expanding a nested
/// array of 262,144 bytes and walking it as 131,072 fill changes.
fn amplifier() -> Vec<u8> {
    let mut body = long(3, MAX_ARRAY, &[0x01]);
    body.extend_from_slice(&[0x00, 0x14]); // executed first: 16 copies of the fill byte (0)
    short(3, 16, &body)
}

/// Nested type-5 concatenations over `amplifier()`, `fan[i]` parts per level.
fn amplifier_tree(fan: &[usize]) -> (Vec<u8>, usize) {
    if fan.is_empty() {
        return (amplifier(), 16);
    }
    let (sub, sub_out) = amplifier_tree(&fan[1..]);
    let mut body = vec![fan[0] as u8];
    for _ in 0..fan[0] {
        body.extend(&sub);
    }
    (long(5, sub_out * fan[0], &body), sub_out * fan[0])
}

/// Valid by the array grammar, but its arrays decode to 2^31 bytes in one quantum:
/// refused by the work budget instead of costing seconds of CPU per block.
#[test]
fn work_amplification_is_refused() {
    let one = amplifier();
    assert_eq!(one.len(), 11);
    // A few amplifiers are within the budget and decode.
    let few = block(0x06, &long(5, 32, &[&[2u8][..], &one, &one].concat()));
    let (res, out) = run(&few, 32);
    assert_eq!(res, Ok(few.len()));
    assert_eq!(out, vec![0u8; 32]);
    // 16,384 of them in one quantum (two sub-chunks of 8,192) are not.
    let (sub, sub_out) = amplifier_tree(&[2, 64, 64]);
    assert_eq!(sub_out, 131_072);
    let data = block(0x06, &[&sub[..], &sub[..]].concat());
    assert_eq!(data.len(), 181_801);
    let (res, _) = run(&data, 262_144);
    assert!(matches!(res, Err(lztans::Error::Corrupt(_))), "{res:?}");
}
