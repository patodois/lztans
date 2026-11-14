# Agent guide

lztans decodes LZ77/Huffman/tANS block streams for Rust tools reading Kraken, Mermaid/Selkie and Leviathan formats.
It is a safe, dependency-free decoder used by the game-data readers `wf-unpack`, `wt-unpack` and `wt-formats`
(all under `github.com/patodois`).

## Toolchain and CI

- Rust 1.88 minimum; CI verifies it separately and uses stable Rust for normal checks.
- Release tests run on Linux, macOS and Windows; lint/docs jobs run on Linux.
- There are no Cargo feature switches in this package.
- Exact CI commands, including the MSRV library build:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
cargo test --release
LZTANS_FUZZ_ITERS=60000 cargo test
cargo build --lib
```

`cargo test --release` and `cargo build --lib` also run with Rust 1.88.0 in the MSRV job.

## Code map

- `src/lib.rs`, `error.rs`: decode entry point and errors.
- `src/bits.rs`, `entropy/`, `lz/`: bit readers, entropy arrays and LZ decoding.
- `tests/vectors/`, `tests/vectors.rs`: synthetic vectors and optional local corpus checks.
- `tests/common/`: shared test helpers and a test-side encoder; `tests/roundtrip.rs` round-trips the
  forms no committed vector holds (built from this decoder's rules: a regression guard, not conformance).
- `tests/api.rs`: what the docs of `decode` and `Error` promise.
- `tests/fuzz.rs`, `nested_rle.rs`, `nested_rle_memory.rs`: malformed inputs and nested-array limits.
- `examples/`: batch, benchmark and peak-memory utilities.
- `QUESTIONS.md`, `// SPEC-QUESTION:` comments: ambiguous format rules and the selected interpretations.

## Project rules

- Preserve the README's clean-room boundary: implement from the prose format description and vectors.
  The format description is kept outside this repository.
- Do not consult existing decoder source; use reference decoders only as black boxes for outputs/verdicts.
- The game-data corpus stays outside Git; set `LZTANS_CORPUS` to run its optional tests.
- Preserve `#![forbid(unsafe_code)]`; malformed input returns an error rather than panicking.
- Arrays are bounded at 256 KiB, sub-chunk output at 128 KiB, nesting at 24 levels, and the decoded
  size of all arrays of one quantum at 16 MiB (`QUESTIONS.md`, item 13).
- The caller provides the full output buffer; recorded working-memory measurements are not a tighter proven bound.
- Unsupported LZNA, BitKnit and checksum-flagged blocks must return `Error::Unsupported`.

## Corpus and vectors

- `LZTANS_CORPUS` names a folder holding `manifest.tsv` and `entropy_manifest.tsv`: tab-separated, one
  header line, then one row per stream. The tests read the second column (the stream's file, relative to
  the folder), the third (decompressed size) and the fourth (SHA-256 of the decoded bytes, lower-case
  hex); other columns are ignored. `tests/vectors.rs` expects 648 streams in total.
- To add a synthetic vector: put the `.bin` in `tests/vectors/` and add one entry to
  `tests/vectors/synthetic.json` (`id`, `file`, `decompressed_size`, `expect` = `decode`, `reject` or
  `unspecified`, `sha256` or `null`, `note`), one field per line like the existing entries; the tests
  derive every count from that file and fail on a `.bin` it does not list (the two
  `syn-nested-rle-work*.bin` vectors excepted, see `tests/nested_rle.rs`). A vector must contain no game
  data. A binary above 256 KiB also needs a pinned entry in `.public-tree-allow`. The encoder that built
  the committed vectors is not in this repository; `tests/common/` can build streams for new tests.

## Public files

- Never commit game files, extracted assets, renders, decompiled code or secrets.
- The public-tree gate needs Python 3.9+ and Git; run it from the repository root:
  `python3 tools/check_public_tree.py` (checks Git's index, including staged new files).
- CI checks committed changes with `python3 tools/check_public_tree.py --range BASE..HEAD`.

## Contributing, license and AI

- Open an issue to discuss a problem or send a pull request with the change.
- Before a pull request, run the applicable CI commands above and the public-tree gate.
- License: MIT OR Apache-2.0, at your option; see `LICENSE-MIT` and `LICENSE-APACHE`.

The code was written with AI coding agents (Anthropic's Claude and OpenAI's GPT models) under the maintainer's direction.
AI-assisted contributions are welcome when their use is disclosed and the changes are reviewed.
