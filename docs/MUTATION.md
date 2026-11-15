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

# Mutation run (ROADMAP row 4 exit)

Tool: `cargo-mutants` 27.1.0, `cargo mutants --all-features --no-shuffle -j 2 --workspace`, over the whole workspace
(`sageant`, `sageant-lexical`, `sageant-persistant`) at `main` d580c81 (0.3.1). 712 mutants, 62 minutes on the lane's
machine.

| Run | Code | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| 3 | `main` d580c81, before the kill tests below | 712 | 551 | 36 | 119 | 6 |
| 3b | same source plus this PR's tests, only the 160 mutants in the functions that had misses or hangs | 160 | 143 | 0 | 12 | 5 |

(Runs 1 and 2 above are row 3's, over the core only.) Run 3b used `--re` over the names of the functions below; the
mutants outside that filter were not re-run, because the new tests cannot make a caught mutant uncaught.

## The 36 misses, and what killed them

All were missed, so there was no red state to write first; run 3b is the evidence they are now caught.

| Misses | Where | Killed by |
|---|---|---|
| 6 | `Filter::confidence_filter`, `includes_superseded`, `limit_value` (public accessors the stores rely on) | `tests/mutation_kills_row4.rs`: each accessor, set and unset |
| 2 | `Cursor::finish` (`Ok(())`) and `Chunk::decode`'s `||` (version 0, empty collection, empty kind) | the same file: exact `Corrupt("fields")` and `Corrupt("trailing bytes")` for chunk, link, audit entry and tombstone |
| 1 | `MemoryIndex::drop_doc` (`!` deleted from the `retain`) | `crates/sageant-lexical/tests/mutation_kills.rs`: re-indexing or removing one document leaves other documents' postings |
| 6 | `ln_ratio_micro`: range guards and the small-ratio branch | the same file: each guard alone, and ratios below 1/4 against known values |
| 3 | `bm25_milli`: the empty-corpus guard and `2 * n + 1` | a fixed-answer index for each guard, and a float BM25 cross-check with the term in three of six documents |
| 2 | `PersistStore::load` and `history`: `\|\|` to `&&` in the key-versus-record check | `crates/sageant-persistant/tests/mutation_kills.rs`: another chunk's record, then the same chunk's other version, stored under a key |
| 1 | `PersistStore::check_cap`: `>` to `>=` | a write exactly at the cap is allowed, one byte over is not |
| 2 | `Debug` for `PersistStore` and `KvTextIndex` | Debug output names the type and backend and never shows content |
| 3 | `Reader for PersistStore`: `history`, `links_from`, `links_to` | served through `&dyn Reader` and compared with what was stored |
| 7 | `dump_all_for_test` | exact key list after one put. This also exposed a real weakness: the forget test's "nothing left in storage" check would have passed on an empty dump |

3 more are the SHA-256 equivalents already recorded in run 2 (`src/sha256.rs:31` and `:38` twice, `^` to `|`); they
are not in run 3b's filter and remain missed for the reason given there.

## The 6 timeouts

`src/sha256.rs:67` `%` to `+` in `sha256_hex` makes the padding loop run for a very long time (as in run 2). Five more
are in `PersistStore::list`'s regrouping loop (`<` to `<=`, `<` to `==`, `<` to `>`, `==` to `!=`, `+=` to `*=`): they
stop the loop advancing, so the listing hangs. A hang is a detected change and is counted as caught for this record;
the tool lists it separately, and so do we. Run 3b reproduced the five `list` hangs.

## Re-running

```
cargo mutants --all-features --no-shuffle -j 2 --workspace --output <dir>
```

about an hour on the lane's machine for the workspace.
