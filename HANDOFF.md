# sageant HANDOFF

You are the `sageant` lane (cwd `C:\Projects\sageant`, repo `ThinkersJournal/sageant`): the Knowledge Store.
Charter: `C:\Projects\.lane-state\newlane\sageant.md`; concept: `C:\Projects\KNOWLEDGE-STORE-CONCEPT.md` 10b/10c.
Decided model and rows: `ROADMAP.md`. Do not reopen the model without an `[ASK]`.

## State (2026-11-18)
- Rows 0-6 DONE (ROADMAP); row E decided not-now (2026-11-17). All five crates are on crates.io (max read 2026-11-18: 0.3.24); main is 0.3.31 once the docs-correction PR merges.
- Proposals (not decided): `docs/PROPOSAL-expansion-2026-11-18.md`; nothing started, Phase 1 waits for CireSnave.
- persistant 0.18.0 facts: kv.batch_atomic.* leaves recorded only for kv-on-relational (sqlite) not opendal memory/fs; PersistStore needs the 4 leaves + kv.get/put/delete/ordered_scan_values; batch() Ok(false) = a Require failed.
- PM rulings: no owner-notification on put path (patent US20230063214A1); no entropy detection; say "unretrievable", never "erased", until PersistAnt row 38 passes.


## Next
1. PUBLISHED: 0.3.19 and 0.3.24 (v0.3.24 = bundle of 0.3.20-0.3.23: threat model, registry smoke CI, design note, text index on PersistAnt 0.18.23, KvTextIndex deleted, rebuild migration to index2.db). My clean-CARGO_HOME registry installs of 0.3.19 and 0.3.24 passed on Windows (FINDINGs sent). Registry smoke is pinned to 0.3.24.
2. Known: sqlite/sqlite-full homes keep MORE fragments than before at N>=3 (text index tx posting cell); README unchanged (not erased); sqlite-secure is the answer; persistant told.
3. Row E DECIDED 2026-11-17 (PM): A, not now; revisit when a consumer asks. Recorded in #50 (merged) with the D17 wording. F parked. Idle candidate: a full (not diff-scoped) mutation run. One PR per project per day (CLAUDE.md 2026-11-16).
4. Primary checkout: stay on main, clean; all work in worktrees (I once edited the primary by running a script from the wrong cwd: always `cd` into the worktree in the same command). `supertool` (untracked, 551KB python script dated Jul 20, from the supertool plugin) is not ours: leave it.

## Licence exclusions
Never read or copy `eidetic-engine` (excludes Anthropic and its agents), `cyberbrain-core` (FSL), `velesdb-memory`
(custom licence).
