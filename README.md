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
  256 KiB, sub-chunks produce at most 128 KiB, and nesting is capped at 24 levels. Measured working
  memory (heap above the caller's output buffer, `examples/peak.rs`): at most 376 KiB over all 648
  corpus streams, and at most about 0.3 MiB over several million mutated and random inputs
  (`tests/fuzz.rs`). No larger hostile case has been constructed, so no tighter worst-case figure is
  claimed.
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
was used only as a black box: to produce expected hashes for the test vectors and to compare accept/reject
behaviour on mutated inputs. Points where the description was unclear, and the assumption taken in each
case, are recorded in
[QUESTIONS.md](https://github.com/patodois/lztans/blob/main/QUESTIONS.md) and marked with
`// SPEC-QUESTION:` in the code. The procedure cannot prove that nothing similar was in the authors'
training data, and it does not address patents or trademarks; "Kraken", "Mermaid", "Selkie" and
"Leviathan" are used here as format names only.

## Testing

```
cargo test                                   # unit tests, synthetic vectors, fuzz-style tests
LZTANS_CORPUS=/path/to/corpus cargo test     # additionally the 648 game-data vectors (kept out of git)
LZTANS_FUZZ_ITERS=100000 LZTANS_FUZZ_SEED=7 cargo test --release --test fuzz -- --nocapture
cargo run --release --example bench -- /path/to/corpus
```

`tests/vectors/` holds the synthetic vectors (generated, contain no game data) with an `expect` field
per entry: 87 must decode to their SHA-256, 10 must be rejected, 1 (an unaligned prefix) is
unspecified and only must not panic. The aligned-prefix vectors test the prefix rule. `syn-nested-rle-work*.bin` hold RLE arrays whose nested work is longer than their output (`tests/nested_rle.rs`). The corpus is game data and is never committed.

## Licence

MIT OR Apache-2.0.
