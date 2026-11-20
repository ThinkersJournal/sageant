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

# Mutation run 4 (after the persistent store, MCP server and CLI)

Tool: `cargo-mutants` 27.1.0, `cargo mutants --all-features --no-shuffle -j 2 --workspace`, at `main` 1dcaf8c (0.3.15, Windows).
**1010 mutants, 3 hours.**

| Run | Code | Tested | Caught | Missed | Unviable | Timeout |
|---|---|---|---|---|---|---|
| 4 | `main` 1dcaf8c, before this PR's tests | 1010 | 740 | 108 | 155 | 7 |
| 4b | this PR's tests, only the 130 mutants in the functions that had misses (`--re`) | 130 | 108 | 18 | 2 | 2 |
| 4c | the `snippet` and `walk` mutants again after fixing one test and tightening another | 11 | 4 | 4 | 0 | 3 |

(Run 3, over 712 mutants before the CLI and the MCP server existed, is above.) The tests below were written **after** the run
named the survivors, so there was no red state for them; runs 4b and 4c are the evidence they now catch what they were
written for.

## The 108 misses, grouped, and what killed them

| Where | Misses | What was untested | Kill test |
|---|---|---|---|
| `sageant-cli` `add` and its `walk` | 21 | the summary counters; the "must still resolve inside the folder" guard (`starts_with` replaced by `true`) | exact summary lines for each kind of skip; the guard moved into `resolves_inside` and unit-tested with a `..` path that looks inside but resolves outside; a directory link is asserted to count as a link |
| `sageant-cli` `import`, `export`, `origin_from` | 11 | every origin except User and the import/export counters | every origin round-trips; exact "imported N chunks (M versions), L links; skipped S" and "exported N records" |
| `sageant-cli` `app.rs` | 15 | the default home, the 60-character cut, `reindex`'s count, the `[archived]` mark, `search`'s rebuild, the purge confirmation word, the audit actor from `USER`/`USERNAME` | tests for each (default home from the platform variables and the error when none is set; 60 vs 61 characters; `reindex` count; archived marker; search heals a deleted index; `xyzzy` refused; the actor is read from the environment via the audit log) |
| `sageant-mcp` | 43 | every error code but a few, `origin_from`/`confidence_from`/`status_name`, `fit`, the byte budget on `list`/`search`/`history`/`envelope`, `get_info`, `Debug`/`Display` | the budget loop became one function (`within_budget`) with exact boundary tests; `fit` tests at exact boundaries, multi-byte and escaped text; every `ToolError` code and message; list filters by each origin and each confidence; whole-envelope size under the budget; each search and history item capped at half the budget; the server advertises tools and its instructions over a real session |
| `sageant-persistant` | 15 | `bumped` (the link generation counter), `Chosen`, `explain_backend`, `choose_backend`'s two refusals, the purge hook, purge's conflict | generation values checked exactly after three links; a link landing mid-forget when the chunk already had links; a link that closes a cycle through a chunk in the middle of the path only; hooks run exactly once and the purge or forget really happens; Conflict on a purge or forget whose chunk got a new version; unit tests for `Chosen`, `explain_backend`, `choose_backend`, `bumped` |
| `src/sha256.rs` | 3 | (none: equivalent, see below) | |

The mutation run found one real weakness in my own tests of the new code: the cycle guard on chunks in the middle of a path
(the generation read for every chunk the check visits) was not exercised by any test, because every earlier race touched the
chunk being linked from. Another: the purge test passed even if its test hook did nothing.

## Survivors that are equivalent, or cannot be reached

- `src/sha256.rs:31` and `:38` (three mutants), `^` to `|` in `compress`: the same three as run 2, for the same reason.
- `Grants::read_only -> Default::default()`: `read_only()` *is* `Self::default()`.
- `fit`, `bounds.len() - 1` to `+ 1` or `/ 1` (two mutants): that value is only a sentinel for "does not fit" and the binary search
  never reads the element it points at.
- `search`, `&&` to `||` in the "rebuild the index" condition: it then rebuilds on every search of a non-empty store instead of
  only when the index is empty. Rebuilding is idempotent, so no output differs.
- `walk`, the `outside` and `other` counters (`+=` to `-=`/`*=`, four mutants, still missed in run 4c) and `add`'s `outside` counter
  and two `unreadable` counters on an I/O error: they count a directory entry that resolves outside the folder after the
  link check has passed, a file kind that is neither file, directory nor link (a socket, a device), or a read error. Each needs a
  race, a special file or a failing disk to reach; the safe behaviour (skip it) is tested, the number printed is not.

## Timeouts

Seven in run 4, counted as detected because a hang is a detected change (the tool lists them separately, and so do we):
`fit`'s binary-search loop (two) and `PersistStore::list`'s regrouping loop (five, as in run 3). Run 4b re-ran the two `fit`
ones; run 4c produced three more in `walk`.

## Limits of this record

- Run 4 was on Windows. The Unix-only symlink-to-file test (`add_never_follows_a_symlink_to_a_file_outside_the_folder`) and the
  Unix link-count test ran on the Linux and macOS CI jobs, not in this run.
- Runs 4b and 4c re-ran only the functions that had survivors, not all 1010 mutants: a new test cannot make a caught mutant
  uncaught, but a refactor (`within_budget`, `resolves_inside`, `chosen_from`) adds new mutants that run 4b only covered where
  they sit inside a re-run function name.
- The tests were written after the survivors were known.

## Re-running

```
cargo mutants --all-features --no-shuffle -j 2 --workspace --output <dir>
```

about three hours on the lane's machine for the workspace; add `--re '<function names>'` to re-run only some.

# Mutation run 5 (code changed since run 4)

`cargo mutants --all-features --no-shuffle -j 2 --workspace --in-diff <diff of 1dcaf8c..8e69825>` on `main` 8e69825 (0.3.17,
Windows). **This is not a full run:** it mutates only the lines changed since run 4's base (the #37 refactors, erase-on-forget in
0.3.15's successor #38 and their tests' neighbours). 93 mutants, 26 minutes: **65 caught, 10 missed, 18 unviable, 0 timeouts.**

| Misses | Where | What was untested | Kill test |
|---|---|---|---|
| 5 | `PersistStore::finish_erasing` (`+=` to `*=`), `KvTextIndex::finish_erasing` (`Ok(0)`, `Ok(1)`, `!=` to `==`, two counters) | that the function returns how many copies it erased, and that it does nothing on a backend that zeroes on delete | a store with three forgotten two-version chunks whose erasing was interrupted returns 6 and then 0; an index removal stopped after its atomic step returns 10 then 0; a `sqlite-secure` index returns 0 |
| 4 | `add`'s `outside` (walk, add) and `unreadable` counters | a count for a path that needs a race, a special file or a failing disk | none: equivalent in practice, as in run 4 |

The run's most important result was not a survivor but a failing kill test: the test that counted `finish_erasing` also
asserted the files were clean after forgetting three chunks, and they were not. That exposed that 0.3.17's claim that `sqlite`
and `sqlite-full` erase was wrong (SQLite leaves stale fragments after page splits); see the 0.3.18 CHANGELOG entry and the README
table. The claim is withdrawn and a test pins the measured limit.

Limits: diff-scoped, so unchanged code was not re-run; written after the survivors were known; Windows only.

# Mutation run 6 (the atomic erased forget, 0.3.19)

`cargo mutants --all-features --no-shuffle -j 2 --workspace --in-diff <changes against main bcc27f8>` (Windows). **Diff-scoped, not a
full run:** only the lines changed by the move to `BatchOp::DeleteErased` and PersistAnt 0.18.21. 41 mutants, 10 minutes: **22
caught, 6 missed, 13 unviable.** A re-run of the missed functions (9 mutants, 5 minutes) after the kill tests below: 7 caught, 2 missed.

| Missed | What was untested | Kill test |
|---|---|---|
| `refuse_erased_batches_for_test -> ()` (store, index) | the fallback tests passed even if the refusal was never simulated | `overwrites_on_forget_for_test` / `overwrites_on_remove_for_test`: a plain `sqlite` store starts out asking for overwriting deletes and after the refusal uses plain ones |
| `KvTextIndex::open`, `!=` to `==` | the overwrite default on a backend that does not zero | the index at 3 documents is fully overwritten on `sqlite` and `sqlite-full` (it is not with the overwrite off) |
| `KvTextIndex::batch`, `&&` to `||` | the simulated refusal fired for batches that ask for no overwrite | indexing a document while refusals are simulated still succeeds |

**Still missed, equivalent:** `match guard erased -> true` in `PersistStore::batch` and `KvTextIndex::batch` (two mutants): the fallback arm is
guarded by "this batch asked for an overwrite"; with the guard always true, a batch that asked for none and failed as unsupported would be
retried with the same plain operations and fail the same way.

Limits: diff-scoped; kill tests written after the survivors were known; Windows only.

# Mutation run 7 (search index on PersistAnt's text index, 0.3.23)

`cargo mutants --all-features --no-shuffle -j 2 --workspace --in-diff <changes against main 7576bd3>` (Windows). **Diff-scoped, not a full
run.** 26 mutants, 9 minutes: **17 caught, 2 missed, 7 unviable.** The two misses were both in `App::open`'s `rebuild` condition
(`old.exists() || !new.exists()`); two kill tests were written afterwards (a store whose index files are all gone is rebuilt by the first
command, whichever it is; an old index beside a new one means the new one may be half built and is rebuilt) and the function re-run
(3 mutants): 2 caught, 1 missed.

**Still missed, equivalent in behaviour:** `delete !` in `rebuild` (`old.exists() || new.exists()`). With no old file and no new file it
skips the rebuild, but `put` and `search` already rebuild an empty index from a non-empty store, so the result is the same (checked by
hand with the mutant: the second `put` after deleting `index2.db` still leaves a two-chunk search); with a new file and no old one it
rebuilds on every open, which costs time and changes no answer. Not a test gap in behaviour, a performance difference nothing observes.

Limits: diff-scoped; kill tests written after the survivors were known; Windows only.

# Mutation run 8 (worth-v1, X1 slice 1)

`cargo mutants -p sageant --all-features --no-shuffle -f src/worth.rs -j 2` (Windows, branch `work/phase1-design`). **File-scoped to
`src/worth.rs`, not a full run.** 30 mutants, 25 minutes: **28 caught, 0 missed, 2 unviable.** The tests (`tests/worth.rs`) were written
before the code and fail to compile without it (the red state); six hand-made sabotages of the implementation (evidence links counted
as loose, structural cap removed, a changed weight, link touches ignored, a changed stability cap, age measured from creation only) each
failed at least one test. No kill tests were needed afterwards.

Limits: file-scoped; Windows only; the 2 unviable mutants did not compile and say nothing about the tests.

# Mutation run 9 (worth snapshots, X1 slice 2)

Two filtered runs (Windows, branch `work/phase1-design`), `-j 2`, `--no-shuffle`, **not full runs**:
- `cargo mutants -p sageant --all-features --re '(?i)worth'`: 62 mutants, 7 minutes: **53 caught, 0 missed, 9 unviable**.
- `cargo mutants -p sageant-persistant --all-features --re '(?i)worth|scan_keys|removal_ops'`: 26 mutants, 15 minutes: **18 caught, 0 missed, 8 unviable**.

The tests (`tests/worth_store.rs`, `crates/sageant-persistant/tests/worth.rs`) were written before the code; they failed to compile
without it, which is a weaker red than a failing assertion. Five hand sabotages were each caught: `forget` leaving the `w/` keys, the
generation write removed from `record_worth` (a first attempt that rewrote the same value did not change state and was not caught; it
was a bad sabotage, redone), the `record_worth` head requirement removed, `MemoryStore::forget` keeping snapshots, and the key-versus-record
check removed. No kill tests were needed afterwards.

Limits: regex-filtered by mutant name, so code that does not carry "worth" in its name (the shared codec helpers, `scan_keys` aside) is not
covered here; Windows only; the unviable mutants did not compile and say nothing about the tests.
