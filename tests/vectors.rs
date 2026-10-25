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

/// Minimal reader for the pretty-printed `synthetic.json`.
fn read_synthetic(dir: &Path) -> Vec<(String, usize, Option<String>)> {
    let text = fs::read_to_string(dir.join("synthetic.json")).unwrap();
    let mut out = Vec::new();
    let (mut file, mut size, mut digest) = (None, None, None);
    for line in text.lines() {
        let line = line.trim().trim_end_matches(',');
        if let Some(v) = line.strip_prefix("\"file\": ") {
            file = Some(v.trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("\"decompressed_size\": ") {
            size = Some(v.parse::<usize>().unwrap());
        } else if let Some(v) = line.strip_prefix("\"sha256\": ") {
            digest = Some(if v == "null" {
                None
            } else {
                Some(v.trim_matches('"').to_string())
            });
        } else if line.starts_with('}') {
            if let (Some(f), Some(s), Some(d)) = (file.take(), size.take(), digest.take()) {
                out.push((f, s, d));
            }
        }
    }
    out
}

#[test]
fn synthetic_vectors() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors");
    let rows = read_synthetic(&dir);
    assert_eq!(rows.len(), 77);
    let mut rejected = 0;
    for (file, size, digest) in rows {
        let data = fs::read(dir.join(&file)).unwrap();
        let mut out = vec![0u8; size];
        let res = lztans::decode(&data, &mut out);
        match digest {
            Some(d) => {
                res.unwrap_or_else(|e| panic!("{file}: {e}"));
                assert_eq!(sha(&out), d, "{file}");
            }
            None => {
                assert!(res.is_err(), "{file} must be rejected");
                rejected += 1;
            }
        }
    }
    assert_eq!(rejected, 1);
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
    assert_eq!(n, 584);
}
