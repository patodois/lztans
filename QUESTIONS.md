# Questions for the spec writer

Written during the first implementation (spec commit `22e0937`) and revised for spec commit
`bb20c71`, which settled items 3 and part of 4 and renamed several terms. Names below use the new terms. Each item says what
was unclear, which assumption the code makes (marked `// SPEC-QUESTION:` in the
source where it lives), and what the oracle showed when it could be asked.

## Open questions and assumptions

1. **Checksum flag (01 section 1.2, block header byte 1 bit 7).** The spec says to
   reject. The code rejects (`Error::Unsupported`). Oracle probe: a valid Kraken
   stream with only that bit set decodes to the *same* SHA-256 as without it, so
   the reference ignores the flag and reads no extra bytes (for a memset block
   too). If real data ever sets it, the spec's "3 bytes after each quantum
   header" belief would be contradicted by the reference's behaviour.

2. **Codec id of a stored block (1.2).** The spec rejects ids 0x05 and 0x0B. The
   oracle accepts a stored block (`byte0` bit 6) that carries id 0x05 (it copies
   the bytes) but rejects an unknown id such as 0x20. The code follows the spec
   text and rejects 0x05 / 0x0B even for stored blocks. Harmless for real data.

3. **Settled by `bb20c71`:** overlapping matches with distance below 8 (4.4) and delta literal runs
   longer than a last distance below 8 (4.5, every delta mode including final runs) are invalid and
   rejected. Also settled: Mermaid unused far offsets (6.3), a nonzero second far count in a
   single-piece sub-chunk (6.1), single-symbol Huffman descriptions (3.3.1). For Delta2 (Leviathan mode
   2) the code measures the *whole* run against the last distance (the spec says the rule covers "the
   first literal of a Delta2 run"; reading it as the whole run is an assumption, and it agrees with
   every vector).

4. **Unused literals at the end of a sub-chunk.** Kraken 5.3 and Mermaid 6.3 now state it as a rule;
   for Leviathan 7.4 calls it an observation only. The oracle does not check it for Leviathan or
   Mermaid (mutated streams with leftover literals decode to a hash); no Kraken mutant with leftover
   literals was ever accepted. The code requires exact use in all three (stricter than the oracle for
   Leviathan, where the spec leaves it open). Mermaid near offsets: the oracle accepted two mutants
   with unused near offsets; the spec requires full use and so does the code.

5. **Literal runs reaching past the end of their array (4.1, 5.2, 7.4).** The
   oracle accepts some mutated streams in which a literal run asks for more
   bytes than the literal array holds (it evidently reads neighbouring
   scratch memory). The spec says cursors must stay in their arrays; the code
   rejects. Same for Leviathan offsets/literal arrays "exhausted".

6. **Matches reaching before the window start, and zero distance (4.4, 4.2).** The
   oracle accepts some mutants where the source lies before the window start
   (it reads whatever precedes in its buffer) and scaled offsets with distance 0.
   The code rejects both (spec: invalid).

7. **Long Leviathan matches near the end of a sub-chunk (7.5).** The code does
   *not* apply the reference's "8 bytes must remain" rule, as the spec allows.
   Confirmed with the oracle: every mutant the oracle rejected but the code
   accepted (9 in 40 000) violates exactly this rule.

8. **Nesting depth of arrays (3.5 - 3.7).** The spec does not bound the nesting of
   RLE / type-5 / sliced containers. The code rejects nesting deeper than 24
   levels to keep stack use bounded. Real data nests at most a few levels.

9. **Call-site limit of sliced containers (3.7, 7.1).** Literal and command
   containers have no stated limit. The code requires the total decoded size of
   the source arrays to be at most `sub_len` (the sub-chunk size) in Leviathan
   sub-chunks and at most the array's `out_len` in type-5 arrays.

10. **Block header bits (1.2).** The code requires `byte0 & 0x0F == 0xC` and bits
    4-5 clear, and applies the codec check to every block, stored or not (see 2).

11. **Duplicate symbols in the old sparse Huffman description (3.3.1).** Not
    addressed by the spec. The code accepts duplicates (the code space check still
    applies). No disagreement with the oracle showed up in 43 000 differential mutants,
    which include mutated sparse descriptions.

12. **Mermaid far offsets and `bound` (6.1 step 6).** The text is read as: `P` is
    the window position (output index minus the window base) of the
    sub-chunk's first byte. Consistent with all vectors including `syn-mermaid-far-ext-*`.

## Wording defects noticed (none blocking)

* 3.3.1 new encoding: "ns == 256 gives X = 0" and the example in 10.2 agree, but
  the generic phrase "x = 2 * min(257 - ns, ns)" for `ns = 1` yields `x = 2`
  (a valid truncated-binary alphabet), which is moot because a one-symbol
  description is rejected later by the code-space rule; worth a sentence.
* 4.2 step 1 says "z leading zeros ... `n_long = v - 1`" but does not state the
  maximum `z` (it is 9 given the limit of 512); the code allows up to 10 and then
  enforces `n_long <= 512`.
* 3.7 step 3: the "region" of the source arrays is described as running to the
  end of the enclosing body; for the 8-lane command container in Leviathan
  (7.1 step 5) that region also contains the offset/length bit streams. The code
  uses that reading and it matches all vectors.
