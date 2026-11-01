#![no_main]
//! Fuzzes `lztans::decode`, the only public entry point taking untrusted bytes.
//!
//! The first two input bytes choose the destination length (the size is stored outside the
//! stream, so it is caller-controlled); the rest is the compressed stream. Lengths are
//! kept below 800,000 bytes so that the harness's own buffer stays small: three blocks
//! of 262,144 bytes are enough to reach window restarts and multi-block paths.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let sel = u16::from_le_bytes([data[0], data[1]]) as usize;
    let len = match sel >> 14 {
        // Block-aligned prefixes: the documented valid case.
        0 => (1 + (sel & 3)) * 262_144,
        // Small sizes, where most single-sub-chunk streams live.
        1 => sel & 0x3FFF,
        // Medium sizes crossing the 131,072-byte sub-chunk boundary.
        2 => 100_000 + (sel & 0x3FFF) * 8,
        // Unaligned sizes slightly off a block boundary.
        _ => 262_144 + (sel & 0x3FFF) - 0x2000,
    };
    let mut out = vec![0u8; len];
    let _ = lztans::decode(&data[2..], &mut out);
});
