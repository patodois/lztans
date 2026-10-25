# lztans

A safe, dependency-free Rust decoder for a family of block-compressed streams that combine an LZ77
stage with Huffman, tANS and run-length entropy coding. It decodes the block formats commonly called
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
* Prefixes: a destination whose length is a multiple of 262,144 bytes and at most the true size decodes
  to the first bytes of the full output (spec 1.7); other lengths below the true size are not
  guaranteed and usually return an error.
* Streams are decoded in full into the caller's buffer; multi-block streams (256 KiB blocks, window
  restarts, stored blocks, memset blocks) are handled.
* Not supported, rejected with `Error::Unsupported`: LZNA, BitKnit, and blocks that set the checksum flag.
* Minimum supported Rust version 1.88 (checked: 1.88 builds and passes the tests, 1.87 does not).
* `Error` is `#[non_exhaustive]`.

## Clean-room provenance

The code was written from a prose format specification (`spec/` of the spec writer's repository,
**spec commit `bb20c71`**) plus test vectors, by an implementer who never saw any existing decoder source.
See `QUESTIONS.md` for the points where the specification was unclear and the assumption taken, and
the `// SPEC-QUESTION:` markers in the code. Details of the procedure and its limits are in the spec
writer's `CLEANROOM.md`; they apply to this crate unchanged (in particular: it cannot prove that the
authors' training data did not contain similar code, and patents and trademarks are out of scope).

## Testing

```
cargo test                                   # unit tests, synthetic vectors, fuzz-style tests
LZTANS_CORPUS=/path/to/corpus cargo test     # additionally the 584 game-data vectors (kept out of git)
LZTANS_FUZZ_ITERS=100000 LZTANS_FUZZ_SEED=7 cargo test --release --test fuzz -- --nocapture
cargo run --release --example bench -- /path/to/corpus
```

`tests/vectors/` holds the synthetic vectors (generated, contain no game data) with an `expect` field
per entry: 87 must decode to their SHA-256, 10 must be rejected, 1 (an unaligned prefix) is
unspecified and only must not panic. The aligned-prefix vectors test the prefix rule. The corpus is game data and is never committed.

## Licence

MIT OR Apache-2.0.
