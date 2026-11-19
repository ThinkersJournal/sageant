# sageant HANDOFF

You are the `sageant` lane (cwd `C:\Projects\sageant`, repo `ThinkersJournal/sageant`): the Knowledge Store.
Charter: `C:\Projects\.lane-state\newlane\sageant.md`; concept: `C:\Projects\KNOWLEDGE-STORE-CONCEPT.md` 10b/10c.
Decided model and rows: `ROADMAP.md`. Do not reopen the model without an `[ASK]` (X1-X3 are asked and approved).

## State (2026-11-19)
- Rows 0-6 DONE; row E decided not-now. crates.io max 0.3.24 (all five crates); main 0.3.31 + this PR (0.3.32, ROADMAP expansion + corrected proposal copy).
- **PUBLISH WAITS for this PR's merge** (one release carries the doc corrections; PM said so 2026-11-18). After merge: tell the PM, PM publishes 0.3.32, then my clean-CARGO_HOME registry install + FINDING.
- Expansion (ROADMAP "Expansion", approved by CireSnave 2026-11-18, not started): Phase 1 = X1 worth, X2 adaptive-decay Maintainer, X3 trust + source ratings. X3's derivation core goes in the EpistemologicAnt project (crate `epistemologicant`, lane asleep; PM says when awake) and sageant depends on it; sageant keeps storage + display. Tools are generic, not sageant-shaped. X4-X7 approved but later phases; X6 a separate project; do not create field projects here.
- PM rulings: no owner-notification on put path (patent US20230063214A1); no entropy detection; "unretrievable", never "erased", except `sqlite-secure` (tested).
- Known: sqlite/sqlite-full homes keep MORE fragments at N>=3 (text index tx cell); sqlite-secure is the answer; README table stands.

WAIT: nothing blocking: publish waits on this PR's merge (PM publishes); check below | check: gh pr view docs/roadmap-expansion --json state,mergedAt | wake: 2026-11-20T16:00:00Z

## Next (order from the PM, 2026-11-18)
1. After this merges: [MERGED] to PM; registry install after publish.
2. Next open day: docs-only DESIGN NOTE: worth snapshot record format (keyed like embeddings); what sageant needs from a trust tool stated as GENERAL capabilities (any body of claims, any kind of source; sageant one example) for EpistemologicAnt: inputs origin identity, evidence links, outcomes; outputs ordered levels with reasons; migration of existing chunks; test plan.
3. Then slices X1 -> X2 (X3 code after `epistemologicant` exists): one PR per day, red-first with sabotage tests, no network, deterministic dry-run plans, secrets never stored; [STATUS] each slice saying what is and is NOT shown.
4. One PR per project per day (CLAUDE.md 2026-11-16). Primary checkout stays on main, clean (it lags origin/main; fast-forward it); all work in worktrees, `cd` into the worktree in the same command. `supertool` (untracked, from the supertool plugin) is not ours: leave it.
5. cargo-deny is clean at 7394dcb; chrono is transitive (rmcp -> schemars), if RUSTSEC-2026-0543/0555 turns it red apply the portfolio ruling (deny.toml ignore, one-sentence reason, expiry 2026-12-17, ROADMAP row to leave chrono).

## Licence exclusions
Never read or copy `eidetic-engine` (excludes Anthropic and its agents), `cyberbrain-core` (FSL), `velesdb-memory`
(custom licence).
