# sageant HANDOFF

You are the `sageant` lane (cwd `C:\Projects\sageant`, repo `ThinkersJournal/sageant`): the Knowledge Store.
Charter: `C:\Projects\.lane-state\newlane\sageant.md`; concept: `C:\Projects\KNOWLEDGE-STORE-CONCEPT.md` 10b/10c.
Decided model and rows: `ROADMAP.md` (incl. "Expansion"). Model changes need an `[ASK]`; X1-X3 are asked and approved (CireSnave 2026-11-18).
Cluster PM is `pm-ants`: [ASK]/[STATUS]/[FINDING] there; [READY] to `pm-assist`, cc `pm-ants`; pm-assist allocates the version at gate.

## State (2026-11-21)
- Rows 0-6 DONE; row E decided not-now. main = 0.3.35 (2bb77ec); PR #55 (X2 survey) MERGED 2026-11-22T01:40Z. Not published (PM decides).
- #53 carried: X1 slice 1 (`worth_of`, `worth-v1`) and slice 2 (`WorthSnapshot`, `WorthStore` on `MemoryStore` and `PersistStore`; keys `w/<id>/<version:010>/<model>`; `forget`/`purge` remove them in the same batch; `record_worth` bumps `n/<id>` so a racing forget retries); design notes `docs/DESIGN-WORTH-AND-TRUST.md` (s4.5 = fixture inputs for epistemologicant; mapper handles refused cases e, g, h) and `docs/DESIGN-PREWARM-AND-EVICT.md` (board 472 open with CireSnave; PM agreed option 1 = recorder + replay harness first; recorder UNBUILT, docs only until he rules); ROADMAP row C1; mutation runs 8-9.
- Working branch `work/next-after55`, worktree `C:\Projects\sageant-wt-next` (off 2bb77ec). Next PR opens 2026-11-22 00:00Z (gate hold). Superseded `x/` AccessLog code: local branches `work/x1-access-log` (worktree sageant-wt-x1) and `work/x1-xlog-superseded`, never pushed; reuse only for the `u/` slice.
- PM rulings: no owner-notification on put path (patent US20230063214A1); no entropy detection; "unretrievable", never "erased", except `sqlite-secure` (tested); no read counter on read-only reads (design note 2.2); pin + budget owner-only; fixed worth weights in v1.
- X3's derivation core is EpistemologicAnt's (crate `epistemologicant`); sageant keeps storage, mapping, display. X4/X5 have no design text yet (told pm-ants); X6 a separate project; X7 not now.
- mcpant: answered pm-platform 2026-11-20 (what MCPAnt must offer; seed = `crates/sageant-mcp` server.rs, Grants, Limits, wire tests). **HOLD (pm-platform): do not open any PR to mcpant until pm-platform says; keep the untrusted-text envelope, response-budget and grant-policy wording in sageant-mcp stable.**

Done 2026-11-21: X2 prior-art survey written as docs/SURVEY-DECAY-EVICTION.md (S1-S4 proposals go to CireSnave via PM, not code); PR window 11-22 per pm-ants TASK; keep committing on this branch after the PR.


- DESIGN NOTE docs/DESIGN-PREWARM-AND-EVICT.md (automatic demotion/forgetting, rewritten 2026-11-21, merged in #54): Q1-Q4, Q8, Q9 answered; Q5-Q7 on board 472 (PM lean 50% pin cap, refuse write at hard cap, two rungs); USE = a chunk returned in a query result, recorded by SageAnt (CireSnave 2026-11-21): `u/<id>` last-use record, written by the CLI/MCP retriever wrapper, Q10 no (get-by-id is not use), Q11 last-use only, Q12 always record, under the store's own authority, read grants still cannot change a chunk (PM 2026-11-21); no code until CireSnave reads the note. The `x/` AccessLog is SUPERSEDED (kept off the PR, see above). Recorder, replay harness, tiny-model ranker DROPPED to ROADMAP C2. NO deletion code. CireSnave 2026-11-21 closed 472: pins capped 50% per store, a full store refuses writes, an ordered list of any number of stores each with its OWN max (RAM/SSD/HDD/cloud, via PersistAnt), 75/85/95 per store, demotion one store down, forgetting at the last. Marks/targets settable per store (validated). PM ruled Q13 (promote one up, once/window), Q14 (write-through for volatile; refuse volatile-without-durable), Q15 (catalog+index in one durable store, bodies tiered); cloud store waits for X7. PersistAnt answered (in the note): RAM kv yes (Volatile), NO size reporting (scan or file size), durability/erasure table given; only sqlite-secure erases. Docs-only PR allowed (PM); no code until the note is read. Trust vanishes on delete; demotion leaves trust byte-identical.

## Next (queue, demand orders work: CireSnave via PM, EXPECTATIONS 6.31)
1. (blocked until CireSnave reads the note and says go) `u/<id>` last-use record + CLI/MCP retriever wrapper (reuse 27d161a ideas), mutation run 10, worth-v2. No recorder, tier or deletion code before then (PM 2026-11-21: docs-only PR is fine).
2. X2: owner-only pin `p/<id>` + budget records, `DecayMaintainer`, `--dry-run`; prior-art survey (decay/eviction, worth/salience) owed first.
3. X3 mapper after `epistemologicant` exists. Then row E, then X7 (authenticated and encrypted only).
4. One PR per project per day; keep committing on the branch; primary checkout stays on main and clean; all work in worktrees, `cd` into the worktree in the same command. `supertool` (untracked) is not ours.
5. cargo-deny was clean at 7394dcb; chrono is transitive (rmcp -> schemars); if RUSTSEC-2026-0543/0555 turns it red apply the portfolio ruling (deny.toml ignore, one-sentence reason, expiry 2026-12-17, ROADMAP row to leave chrono).

## Licence exclusions
Never read or copy `eidetic-engine` (excludes Anthropic and its agents), `cyberbrain-core` (FSL), `velesdb-memory`
(custom licence).

## STOPPED (2026-11-22, pm-ants TASK, EXPECTATIONS 6.32)
No groundwork code exists: the branch holds only HANDOFF edits. No live work. Wake condition: CireSnave's ruling on board 489/472 (read of the design note, go for `u/<id>` last-use). Then Next row 1; X2 pin/budget after it. No WAITs.
