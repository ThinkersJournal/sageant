# Changelog

## 0.3.23

- **The search index is now PersistAnt's text index** (`persistant ~0.18.22`, feature `text-index`, its blocking `KvHandle::text()` and `remove_prefix_erased`) instead of our hand-written `KvTextIndex`, which is **deleted** (never part of a stable API beyond 0.3.x; `PersistTextIndex` implements the same `sageant_lexical::TextIndex`). A document is `<chunk id>/<version>` in the index `sageant`; we keep our tokeniser and BM25. **Ranking is unchanged**: a differential test over 40 chunks x 3 versions (empty and one-word texts, replacement, removal, Unicode terms) on `sqlite-secure`, `sqlite` and `sqlite-full` finds postings, document lengths, stats and BM25 hits identical to the in-memory reference (and, before the old index was deleted, to it too); a home written by 0.3.19 gave the same scores (499 and 420) before and after the move.
- **Migration** (the index is derived data, so it is rebuilt, not translated): the first command after the upgrade finds the old `index.db` (or no `index2.db`), says `rebuilding the search index in the new format (one time)`, builds `index2.db` from the store, and then deletes `index.db` (and its journal files). A crash before the delete leaves `index.db`, so the next run rebuilds again. **Rollback:** a 0.3.19 binary on the same home finds no `index.db` and rebuilds its own with `sageant reindex` or its first search. We delete the old file at once rather than keeping it for a second open (the design note's first idea) because an old index nobody updates would keep the terms of every chunk forgotten after the upgrade; a test forgets a chunk after the upgrade and reads every file under the home.
- **Erasure is not better on plain and WAL sqlite, and the README table stands** (unretrievable, not erased): with two versions per chunk, terms of removed chunks are still in the files at N = 3 (2 chunks), 8 (1) and 30 (1) on `sqlite` and `sqlite-full`, where the old index left none in this shape (its leftovers appeared in a different shape in 0.3.18): the residue is the stale posting-key cell of PersistAnt's `tx` keys. `sqlite-secure` leaves nothing at N = 1, 3, 8, 30. A test pins that something is left at scale on the two other backends.
- Mutation run 7, diff-scoped (26 mutants): 17 caught, 2 missed, 7 unviable; after two kill tests one equivalent survivor (`docs/MUTATION.md`).

## 0.3.22

- Docs: `docs/DESIGN-TEXT-INDEX-MIGRATION.md`, the design note for replacing `KvTextIndex` with PersistAnt's text index (what is deleted, migration of existing homes by rebuild into a new file, rollback, acceptance tests). Design only; one open question for the persistant lane (a blocking text-index handle).
## 0.3.21

- CI: `registry install smoke` on Linux, macOS and Windows: a clean `CARGO_HOME`, `cargo install sageant-cli --version <pinned published> --locked` from crates.io, then the same first-run smoke. Pinned to 0.3.19 (bump at each publish). CI only; no code change.

## 0.3.20

- Docs: `docs/THREAT-MODEL-MCP-HTTP.md`, the threat-model draft for ROADMAP row E (MCP over HTTP), for a CireSnave decision. Nothing is built. It lists assets, attackers, fourteen threats with what the current stdio design already does and what an HTTP listener would need, what a stolen token could do under each option, three options (A do not build, B loopback-only with per-client tokens, C network-facing with TLS and OAuth), the recommendation (A now; B only on a stated need with every control as a release condition), the five decisions needed, and the refutation tests a build would need. AI-assisted analysis, not a penetration test; the current MCP authorisation spec is to be read before building.

## 0.3.19

- PersistAnt `~0.18.21` (crates.io, newest; the revised publish that records `kv.delete_erased` and `text.index.erased` as off on `sqlite` and `sqlite-full`, on on `sqlite-secure`). **Forget is atomic again:** the two-step forget of 0.3.17 (an atomic step, then `delete_erased` per key, finished at the next open) is replaced by one batch that carries `BatchOp::DeleteErased` for the content keys (and, in `KvTextIndex::remove`, for the postings and forward lists), so there is no window in which a forgotten chunk's bytes are still stored. A backend that cannot overwrite refuses the whole batch before changing anything; the stores then apply it with plain deletes and stop asking. `PersistStore::finish_erasing`, `KvTextIndex::finish_erasing`, `Erasure::EraseOnDelete` and the crash test hooks are gone (never published). `storage_needs()` prefers only `storage.secure_delete`. `Erasure` is now `Zeroed` or `Unretrievable`, and `backend_erasure("sqlite" | "sqlite-full")` reports `Unretrievable`, as PersistAnt's matrix now says.
- Measured with 0.18.21 (every file read back after forgetting N chunks of about 700 bytes, and N indexed documents removed): `sqlite-secure` leaves nothing at N = 1, 3, 8, 30 (store and index). `sqlite` and `sqlite-full` with the overwrite in the batch: nothing at N = 1 and 3, but 3 chunks and 1 document still have text in the files at N = 8 and 30; with the overwrite off, plain deletes leave text even at N = 3 (the control). So the batch is a real improvement on plain deletes and still not a guarantee: only `sqlite-secure` is "erased"; the README table is the single source. A test pins the `sqlite` and `sqlite-full` limit.
- A test simulates a backend refusing erased batches (store and index): the forget still succeeds and the stores stop asking (checked through test-only getters). Mutation run 6, diff-scoped (41 mutants): 22 caught, 6 missed, 13 unviable; after kill tests, 2 survivors, both equivalent (`docs/MUTATION.md`).

## 0.3.18

- **Correction: `sqlite` and `sqlite-full` (WAL) do not erase on forget; 0.3.17 said they did.** My mutation run 5 failed a test that forgot three chunks, which showed the claim was too broad. Measured with PersistAnt 0.18.15, reading every database file back after forgetting N chunks of about 700 bytes: `sqlite-secure` leaves 0 copies for N = 1, 3, 8 and 30; plain `sqlite` and `sqlite-full` leave 0 for N = 1 but copies of some of the forgotten text for N >= 3 (2 at N = 3, 3 at N = 8 and 30). `kv.delete_erased` overwrites a key's stored cell and zeroes freed pages and the WAL, but not stale fragments SQLite left in pages it split or rebalanced earlier. 0.3.17's tests used one chunk, so they could not see it. Only `sqlite-secure` is claimed to erase, now checked at 1, 3, 8 and 30 chunks. `sageant forget` and `sageant info` say "unretrievable, not erased" for `sqlite` and `sqlite-full` (and for any backend that cannot erase), with how to move to a secure store (`export`, then `import` into a new home); the README table, `docs/CONTRACT.md` D18 and the API docs carry the correction (`Erasure::EraseOnDelete` is documented as a best effort, `Erasure::guarantees_erasure()` is true only for `Zeroed`). A test pins the measured limit (forgetting eight chunks leaves copies on `sqlite` and `sqlite-full`), so if it ever stops being true the table is re-measured before it changes. The two-step forget with `kv.delete_erased` stays as defence in depth, not as a guarantee. Do not publish a release that carries the 0.3.17 claim.
- Mutation run 5, limited to code changed since run 4 (`--in-diff`, 93 mutants, 26 minutes): 65 caught, 10 missed, 18 unviable. Kill tests for the new code: `finish_erasing` (store and index) counts exactly what it erased and is a no-op on a backend that zeroes on delete; `KvTextIndex` gets the same test hook as `PersistStore` to stop a removal after its atomic step. The remaining survivors are `add`'s `outside` and `unreadable` counters, unreachable without a race or a failing disk (as in run 4). See `docs/MUTATION.md`.

## 0.3.17

- **RETRACTED in 0.3.18: the claim that `sqlite` and `sqlite-full` erase. See 0.3.18.** (Original text:) PersistAnt `~0.18.15`. **`sqlite` and `sqlite-full` (WAL) now erase on forget**, not only `sqlite-secure`. The blocking key-value handle in 0.18.15 exposes `delete_erased`, which overwrites a key's stored bytes (key included) before removing it. I measured on all three SQLite backends: a plain delete leaves the bytes, and an erased delete of a *different* or *absent* key does not scrub them, so the erased delete has to be applied to the live key. `PersistStore::forget` and `purge` therefore work in two steps on a backend that has `kv.delete_erased` but not `storage.secure_delete`: one atomic batch makes the chunk unreadable (tombstone, head, links), then each stored copy is erased while still live. A chunk with no head is never listed and has no history even while its bytes are being erased; a forget interrupted between the steps is finished the next time the store is opened (`PersistStore::finish_erasing`, also called by `purge`). `KvTextIndex::remove` does the same for postings and forward lists (the terms are in their keys; postings first, forward list last, so a stop leaves a forward list that finds the rest; `KvTextIndex::finish_erasing` runs at open). New API: `Erasure` (`Zeroed`, `EraseOnDelete`, `Unretrievable`), `backend_erasure(id)`, `Chosen::erasure()`; `storage_needs()` prefers `kv.delete_erased` after `storage.secure_delete`.
- Evidence: `crates/sageant-persistant/tests/erase.rs` reads every database file after a forget, on `sqlite-secure`, `sqlite` and `sqlite-full`, for a chunk that was superseded and archived first (rewriting its records) and for its index terms: the text is gone; with erasing switched off (`open_without_erasing_for_test`) the same steps DO leave it (the control); a simulated crash between the steps shows nothing readable and the next open finishes the erasing. The CLI test forgets through a real MCP session on all three backends and reads the files. `sageant forget` and `sageant info` say "erased" for these backends and keep "unretrievable" for any backend that cannot erase (unit-tested wording). The README table is still the single source.
- Not done: `BatchOp::DeleteErased` (one atomic erased batch) is not in 0.18.15, so the two steps are not atomic as a whole. A reader holding a snapshot can make an erased delete on WAL return an error after the key is gone; the error says the erasing is finished at the next open.

## 0.3.16

- Mutation run 4 over the whole workspace (`docs/MUTATION.md`): 1010 mutants, 740 caught, 108 missed, 155 unviable, 7 timeouts; kill tests for the survivors in `sageant-cli`, `sageant-mcp` and `sageant-persistant`, and three small refactors so the logic could be tested directly: `within_budget` (the response-size loop shared by `list`, `search` and `history` in `sageant-mcp`), `resolves_inside` (the folder-containment check in `sageant add`) and `chosen_from` (`sageant-persistant`). Re-runs of the affected functions: 130 mutants, 108 caught, 18 missed; the survivors are named in the record as equivalent or unreachable (the three SHA-256 mutants, `Grants::read_only`, a `fit` sentinel, a rebuild condition whose result is identical, and `add`'s counters for paths that need a race, a special file or a failing disk). Two real gaps in my own tests were found and closed: the atomicity guard on chunks in the middle of a cycle path, and a purge-hook test that passed with a hook that did nothing. No change to behaviour.

## 0.3.15

- PersistAnt `~0.18.12` (crates.io, newest). `storage_needs()` now *prefers* `storage.secure_delete` (never requires it): a backend that zeroes freed content ranks first, so a new store gets PersistAnt's `sqlite-secure`. `Chosen::secure_delete()` and `backend_secure_delete(id)` report whether a backend has it. **Erased is now claimed, for those backends only:** new tests (`crates/sageant-persistant/tests/erase.rs`) read the raw database files after a forget and do not find the chunk's text or its search terms on `sqlite-secure`, and the same steps on plain `sqlite` DO find them (the control that shows the test can see). `sageant forget` says "erased from the store file" only when the home's backend has secure delete (and says copies kept by a file system, snapshot or backup are not touched); otherwise it says "unretrievable" and how to move to a secure store (`export`, then `import` into a new home). `sageant info` prints `secure delete: yes|no`. The main README has the one table of which backends erase and which only make a chunk unretrievable (the other READMEs link to it); `docs/CONTRACT.md` D18 records the change.
- Fix (introduced in 0.3.11): `sageant serve` opened the search index with a hard-coded `sqlite` instead of the home's recorded backend, so a forget through MCP did not zero the index terms on a secure store. A new test forgets through a real MCP session and reads the raw files; it fails with the old line and passes now.
- Not done: PersistAnt's `kv.delete_erased` is not used, because the blocking key-value handle in 0.18.12 does not expose it; `sqlite-secure` zeroes on every delete and overwrite, including in a batch, which is what these stores use. A WAL database (`sqlite-full`) does not have `storage.secure_delete`.

## 0.3.14

- `sageant-cli`: `sageant add FOLDER [--collection C] [--kind K] [--ext txt,md] [--max-bytes N]` puts the text files of a folder (recursively; `txt` and `md` by default; 1 MiB limit) into the store, one chunk per file with the folder-relative path (forward slashes) as its source and origin User; adding the same folder again adds nothing; it prints what it added and what it skipped (links, outside the folder, too large, unreadable, other files). **It never reads outside the folder:** the walk uses real directory entries only, so a symbolic link to a file or directory is skipped and counted, never followed (which also rules out loops); a Windows directory junction, which the platform does not report as a link, is caught because every directory and every file must still resolve to a path inside the folder's resolved path, checked again at the moment of reading. The refutation test makes a directory link (a symlink, else a junction) to a folder holding a marker, and shows the marker is never read; I ran the same test against a deliberately naive walk that follows links and it fails with "outside content was read". A symlink-to-file test runs on Unix only (it needs no privilege there).

## 0.3.13

- CI: a new `install smoke` job on Linux, Windows and macOS runs a real `cargo install --path crates/sageant-cli --locked` and then `tools/first-run-smoke.sh`, which, with every data-directory variable pointed at an empty temporary directory and `SAGEANT_HOME` unset, checks `--version`, `where`, `put` (the id alone on stdout), `search`, `get` by id prefix and `list`, and that something was created under the data directory. It is the first-run-with-no-setup gate as an automated check, using each platform's default data directory. The script can be run by hand against any `sageant` binary. It is not a required check until the PM adds it to branch protection.

## 0.3.12

- `sageant-cli`: backup and restore, and the missing way back from archive. `sageant export [--out FILE]` writes every version of every chunk and every link as JSON lines (first line `{"sageant_export":1}`); forgotten chunks, tombstones and the audit log are not in it. `sageant import FILE` reads it back: the whole file is checked first (a bad file imports nothing and the error names the line), a version-1 record whose id does not match its content is refused, what is already present is skipped so importing twice adds nothing, versions merge by appending, archived chunks stay archived, links are re-made, and the search index is updated. Provenance (origin, source, time) is carried as written. A link made on an older version of its source is re-made on the source's latest version. `sageant unarchive ID` brings a chunk back into search and listings. Seven new CLI tests, red first (five failed against the missing commands; one more was added for the lying-id refusal).

## 0.3.11

- Storage is chosen through PersistAnt, not hard-wired (the portfolio rule). `sageant-persistant` declares what it needs as `storage_needs()` (key-value get, put, delete and ordered value scans, the four atomic-batch leaves, durability that survives the process; PersistAnt 0.18.0, newest on crates.io) and `PersistStore::open_chosen(endpoint)` / `KvTextIndex::open_chosen(endpoint)` open the first backend PersistAnt's `providers` lists for that need whose endpoint kind is a path, returning a `Chosen` (`backend()` id and `why()` lines). `explain_backend(id)` gives the same reasons for a recorded id, or an error saying it no longer qualifies. No silent fallback: if none qualifies the error says what each backend lacks. A server (URL) endpoint is not chosen for you (the backend depends on the URL scheme); name it with `open`. Erase-on-forget is not asked for because PersistAnt 0.18.0 records no secure-delete capability; "forgotten" stays "unretrievable".
- `sageant-cli`: a home's first command lets PersistAnt choose and records the id in `<home>/backend`; every later command opens that recorded id, so a future change in PersistAnt's ranking can never make an existing store be read by a different engine. A store made by 0.3.8 or earlier (it has `store.db` and no `backend` file) is recorded as `sqlite`. An unknown or unopenable recorded backend is refused with a message naming the file and saying the data is untouched. New `sageant info` prints the data directory, the backend and PersistAnt's reasons. A bad id prefix now says how many chunks exist.

## 0.3.10

- `sageant-persistant`: the two documented atomicity limits are closed. Each chunk now has a link generation (`n/<id>`, bumped when a link is added from or to it); a writer that read links requires the generations it read to be unchanged at commit and otherwise reads again (up to 16 times). (1) The `part-of` / `derived-from` cycle check is now atomic with the write: two writers adding opposite links at once cannot both win, and a longer cycle closed by a racing edge is caught; the link batch also requires the target still has the version it was checked at, so a link to a chunk forgotten mid-link is refused (`Forgotten`) instead of left dangling. (2) `forget` and `purge` now retry when a link lands between reading a chunk's links and committing, so no outgoing link of a forgotten chunk survives and a chunk derived from it is still flagged for review. New `tests/atomicity.rs` makes each race deterministic with a test-only hook (six tests, red before the fix, green after) plus a threaded stress test.
- Known limits that remain: a write that loses 16 retries in a row reports a `Backend` contention error; `Conflict` semantics are unchanged.

## 0.3.9

- ROADMAP: rows 5 (patent sweep) and 6 (first release, 0.3.8 on crates.io) are done; the status line is current. `ROADMAP.md` is excluded from the published packages, so this changes nothing a crates.io user receives; the version moves because the repository changed.

## 0.3.8

- Release fix: 0.3.7 reached crates.io for `sageant`, `sageant-lexical` and `sageant-persistant` only. crates.io refused `sageant-mcp` at upload because its keyword `model-context-protocol` is 22 characters (the limit is 20), and `sageant-cli` was not attempted. `cargo publish --dry-run` does not check keyword length, so the 0.3.7 release PR did not catch it (my miss). The keyword is now `llm`; no other package had a problem. The git tag `v0.3.7` (on 589c821) stays where it was put and does not mean all five crates shipped at 0.3.7; 0.3.8 is the first version of all five crates. New test `tests/package_metadata.rs` holds crates.io's metadata limits for every package (at most 5 keywords of at most 20 ASCII characters, known category slugs, description, readme, licence files, no `publish = false`), so a bad keyword now fails `cargo test` instead of the upload. No code changed.

## 0.3.7

- First release to crates.io (the five names were reserved at 0.0.0): `sageant`, `sageant-lexical`, `sageant-persistant`, `sageant-mcp`, `sageant-cli`. `publish = false` is removed. Packaging: the root package excludes the lane's working files (`HANDOFF.md`, `ROADMAP.md`, `deny.toml`, `.github`); every crate now ships `LICENSE-APACHE`, `LICENSE-MIT` and its own `README.md`, and has `readme`, `keywords`, `categories`, `repository` and an accurate `description` (the root crate's still said "Planned"). The README says `cargo install sageant-cli`. No code changed.

## 0.3.6

- Row 5: the patent sweep (`docs/PATENT-SWEEP.md`) over 0.3.x at 31bd649: 14 ideas, 10 queries, 19 hits whose claims were read; 18 clear as read, 1 design-around (US20230063214A1, the earlier ruling: no owner notification on the put path), none needs review. AI-assisted, not legal advice. No code changed.

## 0.3.5

- ROADMAP: row 4 is done and row 5 (patent sweep before any release) is next; the stale "stub crate, no API" status line is replaced. No code changed.

## 0.3.4

- Row 4: new crate `sageant-cli`, the `sageant` command. No setup step: the first command creates the store in the user data directory (or `SAGEANT_HOME` / `--home`). `put`, `get`, `list`, `search`, `history`, `links`, `link`, `archive`, `forget`, `purge`, `reindex`, `where` and `serve` (MCP on standard input and output, read-only unless `--allow-write <collection>`). An id prefix is enough where it is unique. `forget` says "unretrievable", never "erased". `purge` refuses unless standard input is a terminal and the person types `purge <first 8 characters of the id>`; it is audited as a typed confirmation. Verified from a real `cargo install --path crates/sageant-cli --root <dir> --locked`: the installed binary stored, searched, read and listed on a fresh home with no setup. The README is now accurate (it was a placeholder).

## 0.3.3

- Row 4: new crate `sageant-mcp`, the MCP server (CONTRACT D13, D14). `Tools<S, I>` implements the ten tools (`sageant_get`, `_list`, `_search`, `_links`, `_history`, `_put`, `_supersede`, `_link`, `_archive`, `_forget`) over any `Reader + Writer` and `TextIndex`, as plain functions tested without a transport; `McpServer` wires them to MCP with `rmcp` 3.10.0 (crates.io, MSRV 1.88, Apache-2.0). There is no purge tool. Reads are on; a write works only on a collection the server was given a grant for (`Grants::writable`); a client writes as `Origin::Agent`, attributed to the configured actor. Every chunk is served in a JSON envelope (id, version, collection, kind, origin, confidence, status, source, created time) whose `content` field is the only place the stored text appears, never in prose or in an error (the D14 test: hostile text round-trips exactly, and a wire test does the same over a real MCP session). Responses are capped by an estimated token budget and say when they were cut. Errors carry a stable code (`not_found`, `forgotten`, `not_authorised`, `secret_refused`, ...) and never content.
- Still to come: `sageant-cli` (first run from `cargo install`, typed-confirmation purge, `serve`).

## 0.3.2

- Row 4: the recorded mutation run over the whole workspace (`docs/MUTATION.md`): 712 mutants, 551 caught, 36 missed, 119 unviable, 6 timeouts. New kill tests (`tests/mutation_kills_row4.rs`, `crates/sageant-lexical/tests/mutation_kills.rs`, `crates/sageant-persistant/tests/mutation_kills.rs`) catch every miss except the 3 known SHA-256 equivalents; a rerun of the 160 mutants in the affected functions shows 0 missed. No library code changed.

## 0.3.1

- Row 4: `sageant_persistant::KvTextIndex`, a durable `sageant_lexical::TextIndex` on a PersistAnt key-value backend, so BM25 retrieval survives a restart. Keys `x/<term>/<id>/<version>` (term frequency), `d/<id>/<version>` (token count), `f/<id>/<version>/<term>` (a document's terms, for re-indexing and removal) and `m/ix` (corpus totals); every change is one atomic batch with a `Require` on the totals, so concurrent writers cannot desynchronise them (a lost race is retried). A differential test drives it and `MemoryIndex` through the same index, re-index and remove steps and compares postings, lengths and totals; another shows a `LexicalRetriever` over either returns identical hits. `sageant-persistant` now depends on `sageant-lexical`.
- Still to come: `sageant-mcp`, `sageant-cli`, then the recorded mutation run.

## 0.3.0

- **Breaking.** `sageant_lexical::TextIndex` methods (`index`, `remove`, `postings`, `doc_len`, `stats`) now return `Result<_, Error>`, because a key-value-backed index can fail. `MemoryIndex` never fails; callers add `?` or `.unwrap()`. `LexicalRetriever::retrieve` passes an index failure to its caller instead of reading it as "no matches". The kv-backed index in `sageant-persistant` is the next slice.

## 0.2.2

- Row 4: `sageant-persistant` slice 3 finished, caps, the secret screen and the `Writer` impl. `PersistStore` gets `set_byte_cap` (`TooLarge`), `set_screen` (the screen must be `Send + Sync`; a refusal is audited by class only and stores nothing), `put_overriding` and `supersede_overriding` (an override names the class and a person; audited as `Overridden`), and implements `Writer`, attributing every audit entry of a call to the caller's `role/agent` label exactly as `MemoryStore` does. A differential test drives both stores through the same `Writer` sequence (put, supersede, link, archive and unarchive, note, forget, purge, invalid actor) and compares results and audit logs. One precedence difference remains: a supersede of an unknown id with non-NFC text reports `NotFound` where `MemoryStore` reports `NotNormalised`.
- Core: `SecretOverride::class/by`, `Actor::is_valid/label` are public so another store can attribute and audit the same way.
- Still to come: the kv text index, `sageant-mcp`, `sageant-cli`, then the recorded mutation run.

## 0.2.1

- Row 4: `sageant-persistant` slice 3, the audit log and the removal lifecycle. Every `PersistStore` write now appends a content-free entry to the audit log in the same atomic batch (`a/<seq>`, sequence held in `m/aseq`; a lost race for the sequence number is retried), with the same actors and reasons as `MemoryStore`; a refused write appends nothing. New: `audit`, `archive`, `unarchive` (every version, a new version of an archived chunk stays archived), `note`, `forget` (every version, source, hash and outgoing links removed in one batch, minimal tombstone left, links pointing at it stay and read as removed, chunks derived from it flagged for review), `purge` (also removes the tombstone), `tombstone`, `needs_review`, `link_target`. Reads of a forgotten chunk say `Forgotten`, not `NotFound`; putting the same text again ends the tombstone. A differential test compares both stores on listings, tombstones, review flags, link targets, confidence, every refusal and the audit log; a dump of every stored key proves a forgotten chunk's content is gone. Forget makes a chunk unretrievable; it does not claim the backend's old pages are erased.
- Core: canonical stored forms for audit entries and tombstones (`AuditEvent::encode/decode/new`, `Tombstone::encode/decode/new`), `Action::name/parse`, `Chunk::with_status`, and `PersonAuthority::how` is public so a store can audit who authorised a purge.
- Still to come in the crate: caps and the secret screen, the `Writer` impl, the kv text index.

## 0.2.0

- **Breaking.** `Reader::list`, `Promoter::promote` and `Maintainer::plan` now return `Result<_, Error>`, and `MemoryIndex::from_reader` returns `Result<Self, Error>`, because a key-value store can fail to list. `MemoryStore` never fails; callers add `?` or `.unwrap()`. Nothing else changes.
- `PersistStore` now implements `Reader`, so every `Retriever`, `Promoter` and `Maintainer` runs over it unchanged (new test `crates/sageant-persistant/tests/reader.rs`; fallible-reader tests in `tests/fallible.rs`).
- Still to come in the crate: archive, forget, purge, audit log, caps and the secret screen, the `Writer` impl, the kv text index.

## 0.1.4

- Row 4: `sageant-persistant` slice 2, links and derived confidence. `PersistStore::link`, `links_from`, `links_to`, `confidence`, and `list` now accepts a confidence filter (it was refused in 0.1.3). A link is stored twice in one atomic batch (`l/<from>/<kind>/<to>` and `r/<to>/<kind>/<from>`) with a `Require` that it is new and that `from` still has the version it was made on. Refusals match `MemoryStore` exactly (not found, self-link, duplicate, `part-of`/`derived-from` cycles) and change nothing; a differential test compares both stores on links, reverse links, confidence and the confidence filter, and on every refusal. Known limit, documented: the cycle check reads existing links and is not atomic with the write, so two writers adding opposite `part-of` links at the same instant could both pass it.
- Core: the link rules are now pure public functions both stores use, `derive_confidence` and `closes_cycle` (`MemoryStore` was refactored onto them, all its tests unchanged); `LinkKind::name`/`parse` and `Link::new`.
- Still to come in the crate: archive, forget, purge, audit log, caps and the secret screen, the `Reader`/`Writer` impls (`Reader::list` must become fallible first, a breaking change), the kv text index.

## 0.1.3

- Row 4: new crate `sageant-persistant` (first slice: chunks). `PersistStore::open(backend_id, endpoint)` over a PersistAnt ~0.18 key-value backend (feature `registry-sqlite`; SQLite is the embedded default). It needs `kv.get`, `kv.put`, `kv.delete`, `kv.ordered_scan_values` and the four `kv.batch_atomic.*` leaves with durability `Process`, and PersistAnt refuses a backend lacking any, by name, at open (in 0.18 that is `memory` and `fs`: their batch atomicity has no recorded test). `put`, `supersede`, `supersede_expecting` (optimistic concurrency: `Conflict{expected, found}`), `get`, `get_version`, `history` and `list` (same order and filters as `Reader::list`; a differential test compares it with `MemoryStore`). Each write is one atomic batch with a `Require` on the prior state; every read verifies the stored record. Not yet in the crate: links, confidence, archive, forget, purge, audit, caps and the secret screen, the `Reader`/`Writer` impls (a confidence filter is refused until links exist), the kv text index.
- Core, needed by the crate: `Chunk::first_version` and `Chunk::next_version` build and validate versions exactly as `MemoryStore` does (which now uses them); `Error::Backend(String)`; `Filter::matches`, `confidence_filter`, `includes_superseded` and `limit_value` are public so every store applies the same listing rules.

## 0.1.2

- Row 4: stored records (CONTRACT D17). `Chunk::encode`/`decode` and `Link::encode`/`decode`: a canonical, versioned, zero-dependency byte format (header line, then `name byte_len` fields in one fixed order), owned by the core so a backend never builds a `Chunk` by hand. Decoding verifies the content hash, that the text is one the store accepts (ASCII, or NFC with the `nfc` feature), the id of a first version, and that the record is exactly canonical; anything else is the new `Error::Corrupt(why)`. `ChunkId::parse` (64 lowercase hex or `Invalid("id")`). The format is pinned by a golden-bytes test. No behaviour change to the in-memory store.

## 0.1.1

- Row 4 starts. The repository is now a cargo workspace (root `sageant` plus `crates/`, CONTRACT s1), one version for every crate, minimum Rust version raised from 1.88 to 1.94 (persistant 0.18 needs it; standing rule). CI runs clippy, test and doc with `--workspace`. New crate `sageant-lexical` (`publish = false` for now): `tokenize`, `term_counts`, the `TextIndex` trait (owned returns so a kv backend can hold postings, CONTRACT D17), `MemoryIndex`, `ln_ratio_micro` (integer-only natural log) and `LexicalRetriever` (BM25, k1 1.2, b 0.75, integer arithmetic only, scores in thousandths; the reader decides visibility so a stale posting never surfaces a forgotten or archived chunk). Integer scores agree with a float reference to 2 thousandths on the test corpus. The core crate is unchanged. Not yet: `sageant-persistant`, `sageant-mcp`, `sageant-cli`.

## 0.1.0

- **Breaking.** `Reader::get`, `get_version`, `history`, `links_from` and `links_to` return owned values (`Chunk`, `Vec<Chunk>`, `Vec<Link>`) instead of references (PM ruling 2026-11-14, `row4-reader-shape`): a backend that keeps its data in storage cannot lend references, and the store trait must not force a whole store into memory. `MemoryStore`'s own inherent methods still borrow, so code on a concrete `MemoryStore` is unchanged; only code written against `&dyn Reader` or `impl Reader` changes. `ROADMAP.md` statuses for rows 2 to 4 brought up to date.

## 0.0.12

- Row 3 exit: recorded mutation run (`docs/MUTATION.md`). `cargo-mutants` over the in-memory store and traits: 313 mutants; first run 19 missed, after `tests/mutation_kills.rs` (10 tests) 3 missed, all equivalent (`^` to `|` in SHA-256 `ch` and `maj`), 1 timeout (padding loop). No source change.

## 0.0.11

- core slice 10 (ROADMAP row 3, in progress): maintenance interfaces (CONTRACT D12). `Maintainer::plan(&dyn Reader) -> Plan` (reads only; `Plan` is data: `Step`s of a `Change` {Archive, Link, Supersede} with a typed `Why` {Duplicate, Expired, Consolidated, Custom}) and `Maintainer::apply` (default method: each step through a `&mut dyn Writer` under an `Actor`, the `Why` label as the audit reason; a failed step is reported in `Applied::failed` and does not stop the rest); `ExpireMaintainer` (reference: archive active chunks created before a time); `Promoter` (a retriever with no write power) and `Demoter` (writes a derived chunk, never edits the original; takes the item by reference plus the writer). No model calls: summarising and embedding are the caller's. `Writer::note` and `Action::Note` record the reason of a link or supersede step as an audit entry of its own. Not yet: the recorded mutation run, row 4.

## 0.0.10

- core slice 9 (ROADMAP row 3, in progress): retrieval types. `Retriever` (takes `&dyn Reader` and a `Query`: text, `Filter`, limit, opaque context), `Hit` (chunk ref, optional integer score, typed `Reason`: Lexical, Recency, LinkNeighbour, Custom), `order_hits` (score desc, unscored last, then id asc, version desc: the stated rule until SortAnt), `Fuser` and `DefaultFuser` (score desc then input order; a duplicate chunk version keeps its best score and reason). No tag reason (no tags in the model). No concrete retriever yet: the lexical one is row 4. Also `docs/LICENCES.md`: third-party licence record. Not yet: maintainer, Promoter/Demoter, mutation run.

## 0.0.9

- core slice 8 (ROADMAP row 3, in progress): feature `nfc` (adds the optional `unicode-normalization` 0.1.26 dependency; the core stays zero-dependency without it). With it, `put` and `supersede` accept non-ASCII text that is already NFC and refuse the rest with `NotNormalised{at}` (the first byte that differs from the NFC form); `normalise_nfc` and `is_nfc` are the helpers. Without it nothing changes: only ASCII is accepted. Not yet: retrievers, maintainer.

## 0.0.8

- core slice 7 (ROADMAP row 3, in progress): the `Writer` trait (put, supersede, link, set_status, forget, purge) implemented by `MemoryStore`, object-safe; one `Actor` (role, agent, both non-empty) on every write, audited as `role/agent`; `Action::Unarchive` and `MemoryStore::unarchive`. `purge` through the trait still needs a `PersonAuthority` and is attributed to it. The per-write secrets override stays an explicit separate call, not on the trait. Inherent store methods are unchanged. Not yet: nfc feature, retrievers, maintainer.\n\n## 0.0.7

- core slice 6 (ROADMAP row 3, in progress): the `Reader` trait (get, get_version, history, links_from, links_to, confidence, `list`) implemented by `MemoryStore`, object-safe so a retriever can take `&dyn Reader`; `Filter` (collection, kind, origin, status, derived confidence, created from/until, `include_archived`, `include_superseded`, `limit`), ordered by `(ChunkId` ascending, version descending). No tag filter (the model has no tags); confidence is an exact match, not a minimum. Not yet: `Writer` and `Actor`, nfc feature, retrievers.

## 0.0.6

- core slice 5 (ROADMAP row 3, in progress): audited per-write secrets override. `SecretOverride::allow_class(class, by)` with `put_overriding` and `supersede_overriding`: covers one write and only the class the screen flagged; the audit log gets `Action::Overridden` (`secret-override:<class>`, actor = who allowed it, never the text); an override naming nobody is `Invalid`. Plain `put` and `supersede` are unchanged. Not yet: Reader/Writer traits, nfc feature, retrievers.

## 0.0.5

- core slice 4 (ROADMAP row 3, in progress): `archive`, `forget` (content, source, hash and outgoing links removed; minimal `Tombstone`; returns `Removed::Unretrievable`, there is no "erased" until PersistAnt row 38 passes), `purge` (needs `PersonAuthority`; also removes the tombstone), `Error::Forgotten`, links to removed targets read `LinkTarget::Removed`, derived chunks flagged `needs_review`, removed evidence ignored by `confidence`, and a content-free audit log (`audit()`; secret refusals are logged as `Refused` with the class only). Not yet: per-put secrets override, retrievers, Reader/Writer traits, nfc feature.

## 0.0.4

- core slice 1 (ROADMAP row 3, in progress): `MemoryStore` with `put`, `supersede`, `get`, `get_version`, `history`; `Chunk`, `Origin`, `Confidence`, `Error`; own SHA-256 (checked against `sha256sum`); caps; NFC is verified for ASCII only and anything else is refused with `NotNormalised` until the `nfc` helper exists (CONTRACT D4). Slice 2: `SecretScreen` trait run on content and source of every write (`Error::SecretRefused{class}`, never the text); feature `redactant` adds `RegisteredSecrets` over `redactant::Scrubber` (registered secrets only, no detector; no per-put override or audit log yet). Slice 3: typed append-only links (`MemoryStore::link`, `links_from`, `links_to`; `LinkKind`; cycles refused in `part-of` and `derived-from`; `Error::LinkCycle`, `DuplicateLink`) and `MemoryStore::confidence` derived from corroborating or contradicting links. No deletion, retrievers or audit log yet. Still `publish = false`.

## 0.0.3

- docs: `docs/CONTRACT.md` (ROADMAP row 2): contract note, options first (D1-D18), PersistAnt facts as reported at `persistant@b6a37fb`. Draft for the PM; no code.

## 0.0.2

- docs: `docs/INVENTORY.md` (ROADMAP row 1): read-only inventory of `weavatrix-memory` 0.3.3 and `areev-store` 1.13.2, recommendation: fresh. Read at weavatrix-memory `905c933`, areev-store `fddbeb2`.

## 0.0.1

- docs: `ROADMAP.md` (rows 0-5) and the lane charter in `HANDOFF.md`. No code; the crate is still a stub (`publish = false`).
