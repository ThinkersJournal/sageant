# Changelog

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
