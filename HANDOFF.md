# sageant HANDOFF

You are the `sageant` lane (cwd `C:\Projects\sageant`, repo `ThinkersJournal/sageant`): the Knowledge Store.
Charter: `C:\Projects\.lane-state\newlane\sageant.md`; concept: `C:\Projects\KNOWLEDGE-STORE-CONCEPT.md` 10b/10c.
Decided model and rows: `ROADMAP.md` (incl. "Expansion"). Model changes need an `[ASK]`; X1-X3 are asked and approved (CireSnave 2026-11-18).

## State (2026-11-19)
- Rows 0-6 DONE; row E decided not-now. 0.3.32 PUBLISHED (all five crates, tag v0.3.32); my clean-CARGO_HOME registry install passed (FINDING sent). main = 0.3.32 (e147018 or later).
- Working branch `work/phase1-design` (worktree `C:\Projects\sageant-wt-phase1`, pushed, NO PR yet): design note `docs/DESIGN-WORTH-AND-TRUST.md`; `REGISTRY_VERSION` 0.3.32 in ci.yml; X1 slice 1 = `src/worth.rs` (`worth-v1`, pure, `worth_of(reader, id, now, params)`, tests/worth.rs 12 tests red-first, 6 hand sabotages each caught, mutation run 8 in docs/MUTATION.md: 30 mutants, 28 caught, 0 missed, 2 unviable). fmt, clippy -D warnings, doc -D warnings clean.
- X1 slice 2 (committed, pushed once the full suite is green): `WorthSnapshot` + `WorthStore` (core; `MemoryStore` and `PersistStore`), key `w/<id>/<version:010>/<model>`, record `sageant-worth/1` (numbers only), not an audited write; `forget`/`purge` delete the `w/` keys in the same batch (erased delete); `record_worth` requires head + bumps the `n/<id>` generation so a racing forget retries. Tests: tests/worth_store.rs (7), crates/sageant-persistant/tests/worth.rs (7); hand sabotages S1-S5 each caught (S2 first try was a bad sabotage, redone). cargo-mutants NOT yet run on the new code. Core tests were written first but compiled red only (missing items), the Persist ones likewise.
- Design note s4.5 (fixture inputs for epistemologicant) written, asked by pm-ants 2026-11-19; [STATUS] pm-ants when pushed. Values there are proposals, not approved defaults.
- PM accepted (2026-11-19): read signal none in X1 slice 1, then an owner/grant-only access log, never a counter on read-only reads; pin + budget owner-only records; fixed worth weights in v1.
- X3's derivation core is EpistemologicAnt's (crate `epistemologicant`, lane asleep; PM says when awake); sageant keeps storage/mapping/display. Tools generic, not sageant-shaped. X4-X7 approved, later phases.

WAIT: open the day's ONE PR from work/phase1-design | check: gh pr list --head work/phase1-design --state all --json number,state | wake: 2026-11-20T15:20:00Z

## Next
1. 11-20 (open day): fetch, rebase the branch on origin/main, re-check the note's code facts, set the lane-carried version (main+1: workspace, 5 path-dep pins, Cargo.lock, `cargo metadata --locked`), CHANGELOG line (design note, CI pin, worth-v1 computation; additive public API: `worth_of`, `WorthParams`, `Worth`, `Component`, `WORTH_MODEL`), open ONE PR (body: what the note claims about today's code and how checked), [READY] to pm-assist, cc pm-ants, my cluster PM (pm-ants msg 2026-11-19; [ASK]/[STATUS]/[FINDING] go to pm-ants too; CLAUDE.md 2026-11-19: pm-assist gates PRs and allocates the version; pm-ants notice: one PR per lane per UTC day). After merge: delete the branch, new branch.
2. X1 slice 2 DONE (see State); remaining: mutation run on it (docs/MUTATION.md), CHANGELOG additive API (`WorthSnapshot`, `WorthStore`, `Worth::version/half_life`). Original plan: snapshot record `w/<chunk id>/<version:010>/<model id>` = `{score_bp, components, computed_at, model_id}`; `WorthStore` trait in core + MemoryStore impl; PersistAnt impl in sageant-persistant; `forget` removes the chunk's `w/` records in the SAME atomic batch (see `removal_ops`/`forget_inner` in crates/sageant-persistant/src/lib.rs ~1165-1260; erased delete as for content), with a read-every-file test (positive control: same scan finds the records before). Red first, sabotage tests. Then X2 (owner-only pin `p/<id>` + budget records, `DecayMaintainer`, `--dry-run`), then X1 access log (owner/grant only). X3 code only after `epistemologicant` exists. Each slice [STATUS] with what is and is NOT shown. Prior-art survey (decay/eviction, worth/salience) owed before X2 code.
3. One PR per project per day; the rule limits when a PR OPENS, keep committing on the branch (PM 2026-11-19). Primary checkout stays on main, clean; `cd` into the worktree in the same command. `supertool` (untracked) is not ours.
4. cargo-deny clean at 7394dcb; chrono is transitive (rmcp -> schemars); if RUSTSEC-2026-0543/0555 turns it red apply the portfolio ruling (deny.toml ignore, one-sentence reason, expiry 2026-12-17, ROADMAP row to leave chrono).

## Licence exclusions
Never read or copy `eidetic-engine` (excludes Anthropic and its agents), `cyberbrain-core` (FSL), `velesdb-memory`
(custom licence).
