//! Reader for `tests/vectors/synthetic.json`.

use std::fs;
use std::path::{Path, PathBuf};

/// The directory of the committed vectors.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

/// One synthetic entry: file, size, expectation, digest.
pub struct Entry {
    pub id: String,
    pub file: String,
    pub size: usize,
    pub expect: String,
    pub sha: Option<String>,
}

/// Minimal line reader for the pretty-printed `synthetic.json` in `dir`: one
/// field per line; an entry is kept when its closing brace is reached with all
/// five fields seen.
pub fn read_synthetic(dir: &Path) -> Vec<Entry> {
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
