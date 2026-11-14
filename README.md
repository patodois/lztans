# lztans

lztans is a safe, dependency-free Rust decoder for developers reading block-compressed streams that combine LZ77 with Huffman, tANS and run-length entropy coding. It decodes the block formats commonly called
**Kraken**, **Mermaid / Selkie** and **Leviathan** (format names only: this crate is not affiliated with
the owners of those names and is not derived from any implementation of them).

```rust
let mut out = vec![0u8; decompressed_size]; // the size is stored outside the stream
let consumed = lztans::decode(&compressed, &mut out)?;
```

* `#![forbid(unsafe_code)]`; malformed input returns `lztans::Error`, never panics.
* Allocations are bounded by the sizes the stream's headers can prove: every array is at most
  256 KiB, sub-chunks produce at most 128 KiB, nesting is capped at 24 levels, and the arrays of one
  quantum may decode to at most 16 MiB in total (QUESTIONS.md, item 13). Measured working
  memory (heap above the caller's output buffer): at most 372 KiB over all 648 corpus streams
  (`examples/peak.rs`) and 252 KiB over 1.5 million mutated and random inputs (`tests/fuzz.rs`).
  Crafted inputs reach more: 1.0 MiB from 239 bytes (nested concatenations around a sliced
  container, `tests/nested_rle_memory.rs`) and 1.5 MiB from 47 bytes (a Kraken sub-chunk whose
  131,072 offsets are expanded before any command runs, `tests/fuzz.rs`). Adding up what one Kraken
  sub-chunk can hold at once gives about 1.7 MiB; that is an estimate, not a proven bound.
* An array nested in an RLE array is bounded by the array grammar (262,144 bytes), not by the RLE's own
  output size, because fill changes consume work without emitting output. Each command is still checked
  against the output that remains before it expands anything, so a small input cannot make the decoder
  write past the output or reserve more than that bound (`tests/nested_rle_memory.rs`).
* Prefixes: a destination whose length is a multiple of 262,144 bytes and at most the true size decodes
  to the first bytes of the full output (spec 1.7); other lengths below the true size are not
  guaranteed and usually return an error.
* Streams are decoded in full into the caller's buffer; multi-block streams (256 KiB blocks, window
  restarts, stored blocks, memset blocks) are handled.
* Not supported, rejected with `Error::Unsupported`: LZNA, BitKnit, and blocks that set the checksum flag.
* Minimum supported Rust version 1.88 (checked: 1.88 builds and passes the tests, 1.87 does not).
* `Error` is `#[non_exhaustive]`.

## How this was built

The decoder was written from a prose description of the format (spec commit `03de3f4`) and a set of
test vectors, by an implementer who did not read any existing decoder's source code. A reference decoder
(the `oozextract` crate, run as a program) was used only as a black box: to produce expected hashes for
the test vectors and to compare accept/reject behaviour on mutated inputs.

The description came from an earlier, separate step with a different author. That author read the source
of `oozextract` 0.5.6 (published under the MIT licence) as a reference for facts about the format,
measured the streams of two games, and wrote the rules down as prose and arithmetic, without code. Before
the description was handed over, a decoder written only from its words reproduced the reference's output
on the test streams. The implementer of this crate never saw that source, and the description's author
wrote none of this crate. Both are language models. The description is not published in this repository.

Points where the description was unclear, and the assumption taken in each
case, are recorded in
[QUESTIONS.md](https://github.com/patodois/lztans/blob/main/QUESTIONS.md) and marked with
`// SPEC-QUESTION:` in the code. The procedure cannot prove that nothing similar was in the authors'
training data, and it does not address patents or trademarks; "Kraken", "Mermaid", "Selkie" and
"Leviathan" are used here as format names only.

## Testing

```
cargo test                                   # unit tests, synthetic vectors, round trips, fuzz-style tests
LZTANS_CORPUS=/path/to/corpus cargo test     # additionally the 648 game-data vectors (kept out of git)
LZTANS_FUZZ_ITERS=100000 LZTANS_FUZZ_SEED=7 cargo test --release --test fuzz -- --nocapture
cargo run --release --example bench -- /path/to/corpus
```

`tests/vectors/` holds the synthetic vectors (generated, contain no game data) with an `expect` field
per entry: `decode` (must decode to its SHA-256), `reject` (must be refused) or `unspecified` (an
unaligned prefix that only must not panic). The aligned-prefix vectors test the prefix rule. `syn-nested-rle-work*.bin` hold RLE arrays whose nested work is longer than their output (`tests/nested_rle.rs`). The corpus is game data and is never committed.

## Licence

MIT OR Apache-2.0.
