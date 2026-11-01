# CLOUD-REPORT: fuzzing `lztans`

## What was done
- Read the crate: the only public entry point taking untrusted bytes is `lztans::decode(src, dst)`.
  There is no compressor, so there is no round-trip target.
- Added `fuzz/` (cargo-fuzz, libFuzzer, nightly) with one target, `decode`. The crate has its own
  empty `[workspace]`, is excluded from the published package (`exclude` in `Cargo.toml`), is not
  built by CI, and its `target/`, `artifacts/` and `Cargo.lock` are git-ignored.
- Target input: the first 2 bytes choose the destination length (block-aligned 1..4 blocks, small
  sizes, sizes crossing the 131,072 sub-chunk boundary, sizes near a block boundary; all < 1.1 MB);
  the rest is the stream. Built with debug assertions, so arithmetic overflow is checked.
- One corpus file over 256 KiB (the public-tree limit) was dropped from the minimised corpus.
- Seed corpus: built from the repository's synthetic vectors (`tests/vectors`) that fit the size
  mapping (86 of them), then grown by fuzzing and minimised with `cargo fuzz cmin`
  (`fuzz/corpus/decode`).

## Fuzzing time (target `decode`)
- Run 1: 2101 s single process, max_len 4096, 27.5M executions.
- Run 2: 1517 s, 4 forked workers, max_len 20000, about 100M executions.
- Total wall time about 60 min (about 2.5 CPU-hours). Limits: malloc 16 MB, rss 512 MB, timeout 10 s.

## Bugs found
None: no panic, overflow, OOM, excessive allocation (over 16 MB) or hang. So no fix and no
regression test were added. Coverage was still growing slowly at the end (cov 2269 in run 1,
2504 in run 2 with longer inputs), so deeper runs might still find something.

## Not done / uncertain
- Inputs longer than 20,000 bytes and destination sizes above about 1.1 MB (more than 4 blocks)
  were not fuzzed. Multi-block paths were reached only through the first 4 blocks.
- Seeds for vectors whose size does not fit the mapping (e.g. other sizes in the 16k..100k gap)
  were skipped.
- The game-data corpus (`LZTANS_CORPUS`) was not used, by rule.
- Since nothing was found, the "revert the fix once" check is not applicable.
