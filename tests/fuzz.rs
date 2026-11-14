//! Fuzz-style robustness test: mutated and random inputs must be rejected or
//! decoded without panics, each in under 10 s, with the decoder's heap below
//! 4 MiB and no single allocation above 2 MiB (the largest crafted case,
//! in `hostile_headers`, takes 1.5 MiB and one allocation of 1 MiB).
//!
//! `LZTANS_FUZZ_SEED` seeds the generator. `LZTANS_FUZZ_ITERS` sets the number of cases per seed (default 3000);
//! `LZTANS_CORPUS` adds the game-data corpus as seed material when available.

mod common;

use common::alloc;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOC: alloc::Counting = alloc::Counting;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn mutate(rng: &mut Rng, data: &mut Vec<u8>) {
    for _ in 0..1 + rng.below(4) {
        if data.is_empty() {
            data.push(rng.next() as u8);
            continue;
        }
        match rng.below(10) {
            0..=3 => {
                let p = if rng.below(2) == 0 {
                    rng.below(data.len())
                } else {
                    rng.below(data.len().min(48))
                };
                data[p] ^= 1 << rng.below(8);
            }
            4 | 5 => {
                let p = rng.below(data.len().min(64));
                data[p] = [0x00, 0x01, 0x7F, 0x80, 0x83, 0xC0, 0xF0, 0xFF][rng.below(8)];
            }
            6 => {
                let p = rng.below(data.len());
                data[p] = rng.next() as u8;
            }
            7 => data.truncate(rng.below(data.len())),
            8 => {
                let p = rng.below(data.len());
                data.insert(p, rng.next() as u8);
            }
            _ => {
                let p = rng.below(data.len());
                data.remove(p);
            }
        }
    }
}

struct Seed {
    name: String,
    data: Vec<u8>,
    size: usize,
    synthetic: bool,
}

fn load_seeds() -> Vec<Seed> {
    let mut seeds = Vec::new();
    let vec_dir = common::vectors::dir();
    let mut names: Vec<_> = fs::read_dir(&vec_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "bin"))
        .collect();
    names.sort();
    // Sizes come from the JSON (the first entry of a file that has several).
    let mut sizes = std::collections::HashMap::new();
    for e in common::vectors::read_synthetic(&vec_dir) {
        sizes.entry(e.file).or_insert(e.size);
    }
    for p in names {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if let Some(&size) = sizes.get(&name) {
            seeds.push(Seed {
                name,
                data: fs::read(&p).unwrap(),
                size,
                synthetic: true,
            });
        }
    }
    // Every file the JSON names was found.
    assert_eq!(seeds.len(), sizes.len());
    if let Some(dir) = std::env::var_os("LZTANS_CORPUS") {
        let dir = Path::new(&dir);
        for m in ["manifest.tsv", "entropy_manifest.tsv"] {
            let text = fs::read_to_string(dir.join(m)).unwrap();
            for l in text.lines().skip(1) {
                let c: Vec<&str> = l.split('\t').collect();
                let data = fs::read(dir.join(c[1])).unwrap();
                if data.len() <= 100_000 {
                    seeds.push(Seed {
                        name: c[1].to_string(),
                        data,
                        size: c[2].parse().unwrap(),
                        synthetic: false,
                    });
                }
            }
        }
    }
    seeds
}

struct Stats {
    cases: usize,
    ok: usize,
    slowest: Duration,
    peak: usize,
    biggest: usize,
}

fn run_case(data: &[u8], size: usize, st: &mut Stats, what: &str) {
    let mut out = vec![0u8; size];
    let base = alloc::start();
    let t = Instant::now();
    let r = lztans::decode(data, &mut out);
    let el = t.elapsed();
    st.slowest = st.slowest.max(el);
    st.peak = st.peak.max(alloc::peak_above(base));
    st.biggest = st.biggest.max(alloc::biggest());
    st.cases += 1;
    if r.is_ok() {
        st.ok += 1;
    }
    assert!(el < Duration::from_secs(10), "{what}: took {el:?}");
}

#[test]
fn mutated_and_random_inputs() {
    let iters: usize = std::env::var("LZTANS_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000);
    let seeds = load_seeds();
    let synthetic = seeds.iter().filter(|s| s.synthetic).count();
    assert!(synthetic > 0);
    let mut st = Stats {
        cases: 0,
        ok: 0,
        slowest: Duration::ZERO,
        peak: 0,
        biggest: 0,
    };
    let seed: u64 = std::env::var("LZTANS_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0x9E37_79B9_7F4A_7C15);
    let mut rng = Rng(seed | 1);

    // Pristine seeds first.
    for s in &seeds {
        run_case(&s.data, s.size, &mut st, &s.name);
    }
    // Mutations of valid streams, sometimes with a wrong output size.
    for i in 0..iters * 4 {
        // A third of the cases start from the synthetic streams, whose arrays
        // are mostly stored, so mutations reach the LZ command loops.
        let s = if rng.below(3) == 0 {
            &seeds[rng.below(synthetic)]
        } else {
            &seeds[rng.below(seeds.len())]
        };
        let mut d = s.data.clone();
        mutate(&mut rng, &mut d);
        let mut size = s.size;
        if rng.below(20) == 0 {
            size = match rng.below(4) {
                0 => size.saturating_sub(1 + rng.below(100)),
                1 => size + 1 + rng.below(100),
                2 => rng.below(400_000),
                _ => size / 2,
            };
        }
        run_case(&d, size, &mut st, &format!("mutant {i} of {}", s.name));
    }
    // Random bytes behind a plausible block header.
    for i in 0..iters {
        let len = rng.below(600);
        let mut d: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        if d.len() >= 2 {
            d[0] = 0x8C;
            d[1] = [0x06, 0x0A, 0x0C][rng.below(3)];
        }
        if d.len() >= 5 && rng.below(2) == 0 {
            d[2] = 0;
            d[3] = 0;
            d[4] = (len as u8).saturating_sub(5);
        }
        let size = [8, 64, 1000, 70_000, 131_072, 131_073, 262_144, 300_000][rng.below(8)];
        run_case(&d, size, &mut st, &format!("random {i}"));
    }
    // Pure noise.
    for i in 0..iters {
        let len = rng.below(2000);
        let d: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        run_case(&d, rng.below(300_000), &mut st, &format!("noise {i}"));
    }
    eprintln!(
        "fuzz: {} cases, {} decoded, slowest {:?}, peak extra heap {} KiB, biggest single allocation {} KiB",
        st.cases,
        st.ok,
        st.slowest,
        st.peak / 1024,
        st.biggest / 1024
    );
    // The output buffer is allocated before the measurement starts. The largest
    // working set constructed so far is 1.5 MiB (a 47-byte Kraken sub-chunk whose
    // 131,072 offsets are read before any command runs, one 1 MiB allocation);
    // adding up the arrays a Kraken sub-chunk can hold at once gives about 1.7 MiB.
    // These bounds leave room above that and catch anything of a larger order.
    assert!(st.peak < 4 << 20, "peak heap {} bytes", st.peak);
    assert!(
        st.biggest <= 2 << 20,
        "single allocation {} bytes",
        st.biggest
    );
}

#[test]
fn hostile_headers() {
    // Streams that claim huge sizes or deep nesting with almost no data.
    let mut st = Stats {
        cases: 0,
        ok: 0,
        slowest: Duration::ZERO,
        peak: 0,
        biggest: 0,
    };
    let cases: Vec<(Vec<u8>, usize)> = vec![
        // Entropy-only sub-chunk: type-5 arrays nested to the depth limit and beyond.
        (
            {
                let mut d = vec![0x8C, 0x06, 0x00, 0x03, 0xFF];
                for _ in 0..4000 {
                    d.extend_from_slice(&[0x50, 0x00, 0x01, 0x00, 0x00, 0x00, 0x20]);
                }
                d
            },
            131_072,
        ),
        (vec![0x8C, 0x0C, 0x07, 0xFF, 0xFF, 0x41], 262_144),
        (vec![0x8C, 0x06, 0x03, 0xFF, 0xFF, 0x00], 262_144),
        (
            vec![
                0x8C, 0x0C, 0x00, 0x00, 0x08, 0x8F, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            131_072,
        ),
    ];
    for (i, (d, size)) in cases.iter().enumerate() {
        run_case(d, *size, &mut st, &format!("hostile {i}"));
    }
    // The largest working set constructed so far: a 47-byte Kraken sub-chunk
    // whose 131,072 offset codes (with scale extras) are expanded before any command.
    let mut body = vec![0x11u8; 8];
    body.extend(common::long(3, 131_064, &[0x41]));
    body.extend(common::long(3, 131_072, &[0x00]));
    body.push(0x81);
    body.extend(common::long(3, 131_072, &[0x00]));
    body.extend(common::long(3, 131_072, &[0x00]));
    body.extend(common::stored_short(&[]));
    body.extend_from_slice(&[0x80; 4]);
    let kraken = common::block(0x06, &common::lz_sub(1, &body));
    assert_eq!(kraken.len(), 47);
    run_case(&kraken, 131_072, &mut st, "kraken offsets");
    assert_eq!(st.ok, 1, "only the memset block is valid");
    assert!(st.peak < 4 << 20, "peak heap {} bytes", st.peak);
    assert!(st.biggest <= 2 << 20, "single allocation {}", st.biggest);
    eprintln!(
        "hostile headers: peak extra heap {} KiB, biggest single allocation {} KiB",
        st.peak / 1024,
        st.biggest / 1024
    );
}
