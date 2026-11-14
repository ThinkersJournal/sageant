# Mutation run (ROADMAP row 3 exit)

Tool: `cargo-mutants` 27.1.0, `cargo mutants --all-features --no-shuffle -j 2`, over the in-memory store and every
trait at `main` 1931085 (0.0.11) plus this PR's tests. 313 mutants per run.

| Run | Code | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| 1 | `main` 1931085, before `tests/mutation_kills.rs` | 313 | 236 | 19 | 57 | 1 |
| 2 | same source, with `tests/mutation_kills.rs` | 313 | 252 | 3 | 57 | 1 |

`tests/mutation_kills.rs` adds 10 tests, one or more per surviving mutant: error messages, `Actor` getters,
`ChunkId::as_str`, `Query::filter_ref`, first-duplicate-wins in `DefaultFuser`, `len`/`is_empty`, store `Debug` (never
shows content), the `Reader` link methods, `flag_derived_from` (right kind and right target both required), and
all-non-ASCII text in `check_text`. Those mutants were missed, so there was no red state to write first; run 2 is the
evidence they are now caught.

## The 3 remaining misses are equivalent mutants

All three replace `^` with `|` in SHA-256's `compress` and cannot change any output:

- `src/sha256.rs:31` `ch = (e & f) ^ (!e & g)`: the two terms are never set in the same bit (`e` against `!e`), so
  `^` and `|` agree.
- `src/sha256.rs:38` (two mutants) `maj = (a & b) ^ (a & c) ^ (b & c)`: where two of the three terms are set, all three
  are, and `1 ^ 1 ^ 1 = 1 = 1 | 1 | 1`; with one or none set the operators agree.

The `^` mutants in the sigma functions (lines 30 and 37 and the schedule) are not equivalent and were caught.

## The 1 timeout

`src/sha256.rs:67` `%` to `+` in `sha256_hex` makes the padding loop run for a very long time; the run killed it at the
timeout. A hang is a detected change, so it is counted as caught for the purposes of this record; it is listed
separately because the tool does.

## Re-running

```
cargo mutants --all-features --no-shuffle -j 2 --output <dir>
```

about 30 minutes on the lane's machine. The `redactant` and `nfc` feature code is included by `--all-features`.
