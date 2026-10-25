# Synthetic test vectors (no game data)

Each `.bin` file is a small compressed stream built from random bytes by an encoder written from the
format specification's grammar. Nothing here comes from a game, so these files may be published. Each
entry has an expected result, listed in `synthetic.json`.

`synthetic.json` lists every entry with the fields `id`, `file`, `decompressed_size`, `expect`, `sha256`
and `note`:

* `expect: "decode"`: the decoder must succeed and produce bytes whose SHA-256 is `sha256`.
* `expect: "reject"`: the specification declares the stream invalid (spec 09, section 9.2a). A conforming
  decoder must return an error. `sha256` is `null`.
* `expect: "unspecified"`: informational only (a prefix that is not on a 262144-byte boundary); the
  decoder only has to return without panicking.

Several entries may share one `file` with different `decompressed_size` (the aligned prefixes of
`syn-prefix-base.bin`).

| Prefix | What it tests |
|--------|----------------|
| `syn-stored-*`, `syn-two-*` | stored block flag, stored sub-chunk, entropy-only sub-chunk, memset block after a stored one |
| `syn-huff-old-dense-*` | old Huffman code description, dense form (3.3.1), all values of the Rice parameter k |
| `syn-huff-old-sparse-*` | old sparse form, unsorted symbol order |
| `syn-huff-new-*` | new description with 2..256 symbols, k = 0..3, symbols in several runs |
| `syn-huff-single-symbol` | one-symbol description: invalid, reject |
| `syn-kraken-*`, `syn-leviathan-*`, `syn-mermaid-*` | LZ sub-chunks with random commands: classic and scaled offsets, 8.4 MB distances reaching into memset blocks, Mermaid far offsets with the extension byte |
| `syn-reject-delta-run-*`, `syn-ok-delta-run-*` | delta literal run longer than a last distance below 8 (middle and final runs; Kraken, Mermaid, Leviathan): reject; the raw-literal counterpart of the same commands decodes (4.5) |
| `syn-reject-overlap-short-distance-kraken`, `syn-ok-short-distance-no-overlap-kraken` | match with distance below 8: overlapping rejects, non-overlapping decodes (4.4) |
| `syn-reject-mermaid-far-unused`, `syn-reject-mermaid-second-list-single-piece`, `syn-ok-mermaid-far-used` | Mermaid far-offset validity (6.1, 6.3) |
| `syn-prefix-*` | aligned prefixes of a three-block stream decode to the first bytes of the full output (1.7) |
| `ex-*` | the worked examples of spec/10 |
