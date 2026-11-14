# Changelog

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
