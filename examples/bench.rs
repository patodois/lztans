//! Developer tool: measures decoding throughput over a corpus directory
//! (`manifest.tsv` + `blocks/`). Usage: `bench <corpus-dir> [rounds]`.

use sha2::{Digest, Sha256};
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: bench <corpus-dir> [rounds]");
    let rounds: usize = args.next().map_or(5, |v| v.parse().expect("rounds"));
    let text = std::fs::read_to_string(format!("{dir}/manifest.tsv")).expect("manifest");
    let mut items: Vec<(String, String, Vec<u8>, usize)> = Vec::new();
    for l in text.lines().skip(1) {
        let c: Vec<&str> = l.split('\t').collect();
        let data = std::fs::read(format!("{dir}/{}", c[1])).expect("read");
        items.push((
            c[0].to_string(),
            c[4].to_string(),
            data,
            c[2].parse().expect("size"),
        ));
    }
    let largest = items.iter().map(|i| i.3).max().unwrap_or(0);
    let mut out = vec![0u8; largest];
    println!("{} streams, largest output {} bytes", items.len(), largest);
    for (label, filter) in [
        ("all", None),
        ("kraken", Some("kraken")),
        ("mermaid", Some("mermaid")),
        ("leviathan (Warframe)", Some("lev-wf")),
        ("leviathan (War Thunder, multi-block)", Some("lev-wt")),
    ] {
        let sel: Vec<&(String, String, Vec<u8>, usize)> = items
            .iter()
            .filter(|i| match filter {
                None => true,
                Some("lev-wf") => i.1 == "leviathan" && i.0.starts_with("wf"),
                Some("lev-wt") => i.1 == "leviathan" && i.0.starts_with("wt"),
                Some(c) => i.1 == c,
            })
            .collect();
        let bytes: usize = sel.iter().map(|i| i.3).sum();
        let comp: usize = sel.iter().map(|i| i.2.len()).sum();
        let mut best = f64::MAX;
        let mut best_sha = f64::MAX;
        for _ in 0..rounds {
            let t = Instant::now();
            for i in &sel {
                lztans::decode(&i.2, &mut out[..i.3]).expect("decode");
            }
            best = best.min(t.elapsed().as_secs_f64());
            let t = Instant::now();
            for i in &sel {
                lztans::decode(&i.2, &mut out[..i.3]).expect("decode");
                std::hint::black_box(Sha256::digest(&out[..i.3]));
            }
            best_sha = best_sha.min(t.elapsed().as_secs_f64());
        }
        println!(
            "{label:40} {:5} streams {:8.1} MB out {:8.1} MB in  decode {:7.1} MB/s ({:.3} s)   decode+sha256 {:7.1} MB/s",
            sel.len(),
            bytes as f64 / 1e6,
            comp as f64 / 1e6,
            bytes as f64 / 1e6 / best,
            best,
            bytes as f64 / 1e6 / best_sha
        );
    }
}
