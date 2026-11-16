# sageant HANDOFF

You are the `sageant` lane (cwd `C:\Projects\sageant`, repo `ThinkersJournal/sageant`): the Knowledge Store.
Charter: `C:\Projects\.lane-state\newlane\sageant.md`; concept: `C:\Projects\KNOWLEDGE-STORE-CONCEPT.md` 10b/10c.
Decided model and rows: `ROADMAP.md`. Do not reopen the model without an `[ASK]`.

## State (2026-11-16)
- Rows 0-3 DONE (#4-#12). Row 4 in progress. Merged: #13 0.1.0 (Reader owned, BREAKING), #14 0.1.1 (workspace, MSRV 94, sageant-lexical), #15 0.1.2 (records), #16 8042d4a 0.1.3 (sageant-persistant chunks over sqlite kv).
  PM claimed crate names sageant-lexical/-persistant/-mcp/-cli at 0.0.0 on crates.io; real releases publish over them.
- #17 cd8dd74 0.1.4 MERGED (persistant links+confidence; core derive_confidence/closes_cycle). Cleaned up.
- persistant 0.18.0 facts: kv.batch_atomic.* leaves recorded only for kv-on-relational (sqlite) not opendal memory/fs; PersistStore needs the 4 leaves + kv.get/put/delete/ordered_scan_values; batch() Ok(false) = a Require failed.
- PM rulings: no owner-notification on put path (patent US20230063214A1); no entropy detection; say "unretrievable", never "erased", until PersistAnt row 38 passes.

WAIT: nothing blocking: 0.3.24 is published (v0.3.24) and the registry smoke pin is 0.3.24 (#47); row E waits on CireSnave via PM | check: synapse mail | wake: PM mail

## Next
1. PUBLISHED: 0.3.19 and 0.3.24 (v0.3.24 = bundle of 0.3.20-0.3.23: threat model, registry smoke CI, design note, text index on PersistAnt 0.18.23, KvTextIndex deleted, rebuild migration to index2.db). main: 0.3.26 (registry smoke pinned to 0.3.24). My clean-CARGO_HOME registry installs of 0.3.19 and 0.3.24 passed on Windows (FINDINGs sent). This PR (#46) is the HANDOFF rewrite, 0.3.27; remove sageant-wt-handoff + docs/handoff-rewrite on merge (git status first).
2. Known: sqlite/sqlite-full homes keep MORE fragments than before at N>=3 (text index tx posting cell); README unchanged (not erased); sqlite-secure is the answer; persistant told.
3. Row E (MCP over HTTP): docs/THREAT-MODEL-MCP-HTTP.md; recommendation A; CireSnave decides via PM; build only if B approved. F parked. Idle candidates: CONTRACT D17 wording refresh for the PersistAnt text index; a full (not diff-scoped) mutation run.
4. Primary checkout: stay on main, clean; all work in worktrees (I once edited the primary by running a script from the wrong cwd: always `cd` into the worktree in the same command). `supertool` (untracked, 551KB python script dated Jul 20, from the supertool plugin) is not ours: leave it.

## Licence exclusions
Never read or copy `eidetic-engine` (excludes Anthropic and its agents), `cyberbrain-core` (FSL), `velesdb-memory`
(custom licence).
