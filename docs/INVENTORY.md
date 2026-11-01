# Row 1: read-only inventory of `weavatrix-memory` and `areev-store`

Read-only: nothing was copied, built or run. Counts come from `grep`/`wc` over the unpacked `.crate`
tarballs, so test counts are `#[test]` attributes, not passing tests. Not read, by licence: `eidetic-engine`,
`cyberbrain-core`, `velesdb-memory`.

## Provenance (measured 2026-11-01)

| | weavatrix-memory | areev-store |
|---|---|---|
| Version read | 0.3.3 | 1.13.2 |
| `.crate` sha256 | `351408eca9005872581af9b15903935392dd72887fd30e5663b40e5f1a13faf2` | `5e903cd55da407b13d7946a632c13e183d3a51c7031d9bdb4670e2defdc742ac` |
| Matches crates.io API checksum | yes | yes |
| VCS commit (`.cargo_vcs_info.json`) | `905c933ed70c05d3905f309bffee2e2ddcda3209` | `fddbeb273c1349b17604eb2427d2d26eddb791f6`, path `crates/areev-store` |
| Licence (Cargo.toml) | MIT | MIT OR Apache-2.0 |
| Licence text shipped in the tarball | `LICENSE`, (c) 2026 Sergii Ziborov | **none** (no LICENSE/NOTICE file; metadata only) |
| Size | 7,768 lines in `src/` (63 files) | 35,253 lines in the tarball; `src/lib.rs` alone is 12,615 |
| `unsafe` | `#![forbid(unsafe_code)]` | not forbidden; one `unsafe` (`ManuallyDrop::take`, `src/db.rs:400`) |
| `#[test]` attributes | 81 (77 in `tests/`) | 308 |
| Rust MSRV | 1.89 | 1.90 |

## weavatrix-memory: shape

An event-sourced, bitemporal **fact graph** for coding agents. The unit is a `MemoryFact`
(`source --relation--> target`, `valid_from/valid_until`, `observed_at`, `recorded_at`, `confidence`,
`evidence[]`, `supersedes`), not a text chunk. Modules: `domain` (fact, node, evidence), `store` (in-memory and
file event store, expected-version appends, subscriptions), `projection` (replay, snapshots), `snapshot`
(compact format `WMEMCB02`), `context` (budgeted context compiler, RRF fusion over retrieval channels
`Literal/Lexical/Semantic/Hybrid`), `extraction` (entity linking from provider output), `analytics`
(belief revision, drift, gaps, consolidation), `codec` (optional lz4 and XChaCha20).

- **Dependencies:** `serde` and `weavatrix-graph` (MIT, same author) are required; the rest are optional features
  (`lz4_flex`, `chacha20poly1305`, `getrandom`, `zeroize`, `mmap-guard`, `blazingly-json`). The lockfile has 130
  packages; it includes dev-dependencies (`cqrs-es`, `tokio`, `agentic-memory`), and how many are normal
  dependencies was not separated.
- **Determinism:** time is caller-supplied (`Timestamp` = unix micros, no clock calls found in `src`). The one
  `RandomState` (`projection/memory/index.rs:52`) seeds an in-process id lookup index. `HashMap` is used in the
  linker and projection state; whether any of it reaches output order was **not** checked.
- **Useful ideas, not code:** confidence as integer basis points (0..=10,000), no floats; `Evidence {kind, source,
  locator, digest}`; self-supersession rejected at construction; retraction history that never rewrites what was
  known at an earlier cut; integer RRF (`k=60`, scale 1e9).
- **Gaps against the SageAnt model:** no chunk text, no origin enum, no `part-of` hierarchy, no
  archive/forget/purge, no collections. Retraction is a time bound on a fact, not content removal.

## areev-store: shape

A content-addressed **storage engine plus memory API** on Turso/SQLite (optional Postgres). Grains are
immutable content-addressed blobs; around them sit dictionary-encoded triple indexes, an op-log with HLC
ordering and tombstones, CAS blobs, hybrid BM25 + structural recall, encryption, anonymisation, retention
holds, GDPR-style erasure and subject reports, attestation, fork/merge, telemetry. Everything hangs off one
`Areev` struct in a 12.6k-line `lib.rs`.

- **Dependencies (required):** `turso =0.7.2` (MIT; pinned exactly, while crates.io is at 0.9.0-pre.2),
  `areev-core 1.13` (MIT OR Apache-2.0, same repo), `tokio`, `aes-gcm`, `argon2`, `ed25519-dalek`, `hkdf`, `sha2`,
  `serde_json`, `getrandom`, `zeroize`, `hex`. Postgres and TLS are features.
- **Overlap:** it is the PersistAnt layer (storage, indexes, blobs, transactions) fused with a memory layer.
  Adopting it would put a second storage engine under SageAnt, against "SageAnt sits atop PersistAnt".
- **Useful ideas, not code:** `forget` deletes index rows outright rather than flagging them, so nothing
  forgotten is recallable; erasure reports erased content hashes and reclaimed vocabulary; retention holds;
  `verification_status` on model-extracted facts ("unverified" until checked), queryable; `include_superseded`
  as an explicit recall switch; keyed hashes that carry no identity.
- **Gaps:** model is subject/relation/object grains, not text chunks with a parent hierarchy; `confidence` is an
  `f64` in 0..1; the origin/confidence split is not there.
- **Cleanliness:** one 12.6k-line file, a Postgres module of 3.4k lines. A "trimmed copy" would be a rewrite of the storage layer, not a trim.

## Fork vs fresh

| Option | What it is | Verdict |
|---|---|---|
| A. Fork weavatrix-memory | Keep `domain/store/projection/snapshot`, replace fact with chunk | Most of what is reusable is the event-store and snapshot plumbing, which PersistAnt already owns. Adds a required `weavatrix-graph` dependency, against the zero-dependency core. The fact graph would be removed. |
| B. Fork areev-store | Trim to the memory API | Not trimmable (see above); wrong storage engine; no licence file to carry; one `unsafe`. |
| C. Fresh, borrowing ideas | Write the chunk model, traits and in-memory store from the decided model; take only the ideas listed above, with credit in the contract note | Matches the decided model exactly, zero required dependencies, no copied files to trace. |

**Recommendation: C, fresh.** Neither crate's data model is a text chunk, and the parts worth having
(basis-point confidence, evidence links, never-rewrite-history retraction, forget-deletes-the-index,
explicit superseded switch) are designs, which can be reimplemented without copying a file. Because nothing is
copied, no derived-file table is needed; the contract note will credit the ideas by crate, version and
commit above. If the PM prefers to copy anything later (for instance the integer RRF), that file gets the
full provenance treatment first.

## Not established

- Behaviour of either crate when built or tested; passing test counts.
- Whether `HashMap` iteration in weavatrix-memory reaches any output order.
- Dependency licences beyond `weavatrix-graph`, `areev-core` and `turso`.
- Patents (row 5).
