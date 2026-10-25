//! Decodes the synthetic vectors (always) and the game-data corpus (when its
//! directory is given in `LZTANS_CORPUS`) and compares SHA-256 digests.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// One synthetic entry: file, size, expectation, digest.
struct Entry {
    id: String,
    file: String,
    size: usize,
    expect: String,
    sha: Option<String>,
}

/// Minimal reader for the pretty-printed `synthetic.json`.
fn read_synthetic(dir: &Path) -> Vec<Entry> {
    let text = fs::read_to_string(dir.join("synthetic.json")).unwrap();
    let mut out = Vec::new();
    let (mut id, mut file, mut size, mut expect, mut digest) = (None, None, None, None, None);
    for line in text.lines() {
        let line = line.trim().trim_end_matches(',');
        if let Some(v) = line.strip_prefix("\"id\": ") {
            id = Some(v.trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("\"file\": ") {
            file = Some(v.trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("\"decompressed_size\": ") {
            size = Some(v.parse::<usize>().unwrap());
        } else if let Some(v) = line.strip_prefix("\"expect\": ") {
            expect = Some(v.trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("\"sha256\": ") {
            digest = Some(if v == "null" {
                None
            } else {
                Some(v.trim_matches('"').to_string())
            });
        } else if line.starts_with('}') {
            if let (Some(id), Some(file), Some(size), Some(expect), Some(sha)) = (
                id.take(),
                file.take(),
                size.take(),
                expect.take(),
                digest.take(),
            ) {
                out.push(Entry {
                    id,
                    file,
                    size,
                    expect,
                    sha,
                });
            }
        }
    }
    out
}

#[test]
fn synthetic_vectors() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors");
    let rows = read_synthetic(&dir);
    assert_eq!(rows.len(), 98);
    let (mut decoded, mut rejected, mut unspecified) = (0, 0, 0);
    for e in rows {
        let data = fs::read(dir.join(&e.file)).unwrap();
        let mut out = vec![0u8; e.size];
        let res = lztans::decode(&data, &mut out);
        match e.expect.as_str() {
            "decode" => {
                res.unwrap_or_else(|err| panic!("{}: {err}", e.id));
                assert_eq!(sha(&out), e.sha.unwrap(), "{}", e.id);
                decoded += 1;
            }
            "reject" => {
                assert!(res.is_err(), "{} must be rejected", e.id);
                rejected += 1;
            }
            // Only has to return without panicking.
            "unspecified" => unspecified += 1,
            other => panic!("unknown expectation {other}"),
        }
    }
    assert_eq!((decoded, rejected, unspecified), (87, 10, 1));
}

/// Aligned prefixes (spec 1.7) decode to the first bytes of the full output.
#[test]
fn aligned_prefixes() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors");
    let data = fs::read(dir.join("syn-prefix-base.bin")).unwrap();
    let rows = read_synthetic(&dir);
    let full = rows
        .iter()
        .find(|e| e.id == "syn-prefix-base")
        .expect("base entry");
    let mut whole = vec![0u8; full.size];
    lztans::decode(&data, &mut whole).unwrap();
    assert_eq!(sha(&whole), *full.sha.as_ref().unwrap());
    let mut checked = 0;
    for e in rows
        .iter()
        .filter(|e| e.id.starts_with("syn-prefix-aligned"))
    {
        assert_eq!(e.size % 262_144, 0);
        let mut out = vec![0u8; e.size];
        let used = lztans::decode(&data, &mut out).unwrap();
        assert!(used < data.len(), "later blocks must not be examined");
        assert_eq!(out[..], whole[..e.size], "{}", e.id);
        checked += 1;
    }
    assert!(checked >= 2);
    // Zero bytes need no input; every other size, aligned or not, must not panic.
    assert_eq!(lztans::decode(&[], &mut []), Ok(0));
    for size in [
        1usize,
        1000,
        100_000,
        262_143,
        262_145,
        400_000,
        full.size - 1,
    ] {
        let mut out = vec![0u8; size];
        let _ = lztans::decode(&data, &mut out);
    }
}

fn read_manifest(path: &Path, size_col: usize, sha_col: usize) -> Vec<(String, usize, String)> {
    let text = fs::read_to_string(path).unwrap();
    text.lines()
        .skip(1)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (
                c[1].to_string(),
                c[size_col].parse().unwrap(),
                c[sha_col].to_string(),
            )
        })
        .collect()
}

#[test]
fn corpus_vectors() {
    let Some(dir) = std::env::var_os("LZTANS_CORPUS") else {
        eprintln!("LZTANS_CORPUS not set: skipping the corpus");
        return;
    };
    let dir = PathBuf::from(dir);
    let mut n = 0;
    for m in ["manifest.tsv", "entropy_manifest.tsv"] {
        for (file, size, digest) in read_manifest(&dir.join(m), 2, 3) {
            let data = fs::read(dir.join(&file)).unwrap();
            let mut out = vec![0u8; size];
            let used = lztans::decode(&data, &mut out).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(used <= data.len());
            assert_eq!(sha(&out), digest, "{file}");
            n += 1;
        }
    }
    eprintln!("corpus vectors matched: {n}");
    assert_eq!(n, 648);
}
