# SageAnt roadmap

SageAnt is the Knowledge Store: a storable, organised collection of annotated, sorted, retrievable
knowledge chunks, with manual retrieval tooling and pluggable interfaces for automated retrieval.
It sits above PersistAnt (storage). Status: pre-1.0, published on crates.io at 0.3.8; rows 0 to 6 are done (the crates: `sageant`, `sageant-lexical`, `sageant-persistant`, `sageant-mcp`, `sageant-cli`).

## Decided model (changes need an `[ASK]`)

- One chunk model for logs, memory and documents. A chunk is UTF-8 text (NFC) and immutable; an edit is a
  new version with `supersedes`.
- Size is a measured property with a per-collection cap, never a type. A document or dictionary is a parent
  chunk with child chunks linked `part-of`. Non-text is text with a `format` tag; binary only as blob
  references, later.
- Trust is split: `origin` (user, tool, web, another agent, model) and `confidence` (unconfirmed,
  corroborated by independent origins, contradicted; evidence as links). Injection safety is policy, not a
  stored opinion: retrieved content is data and carries its origin label.
- Deletion: `archive` (kept, out of retrieval), `forget` (content removed, minimal tombstone; the default),
  `purge` (everything gone, only by explicit person action). The audit log never holds content. Secrets are
  never stored; store a vault reference.

## Rows

| # | Row | Status |
|---|-----|--------|
| 0 | ROADMAP, HANDOFF, version 0.0.1, CHANGELOG | done (#1) |
| 1 | Read-only inventory of `weavatrix-memory` (MIT) and `areev-store` (MIT OR Apache-2.0); fork-vs-fresh recommendation | done (#2; recommendation: fresh) |
| 2 | Contract note, options first: data model, retriever trait, promoter/demoter/maintainer traits, MCP tool surface, errors, determinism, PersistAnt mapping. PM reads before any code | done (#3; PM answered 6 questions) |
| 3 | Core plus in-memory reference store; value-asserting tests, mutation checks, no `unsafe`, zero required dependencies | done (#4 to #12; mutation run in `docs/MUTATION.md`) |
| 4 | PersistAnt-backed store, lexical retriever, MCP tool server, CLI; ease gate for a stranger | done (#13 to #25: 0.2.0 to 0.3.4; recorded workspace mutation run in `docs/MUTATION.md`; first run verified from a real `cargo install`; `rmcp` 3.10.0) |
| 5 | Patent sweep before any release | done (#27: `docs/PATENT-SWEEP.md`, 19 hits read, 18 clear, 1 design-around, 0 needs-review; AI reading, not legal advice) |
| 6 | First release | done (0.3.8, all five crates on crates.io; 0.3.7 shipped only `sageant`, `sageant-lexical` and `sageant-persistant` because crates.io refused a 22-character keyword, see CHANGELOG) |
| E | MCP over HTTP | not started: threat-model draft in `docs/THREAT-MODEL-MCP-HTTP.md`, waiting for a CireSnave decision (recommendation: not now) |

## Constraints

- Not to be read or copied: `eidetic-engine` (licence excludes Anthropic and its agents), `cyberbrain-core`
  (FSL), `velesdb-memory` (custom licence).
- Anything copied from an MIT/Apache crate needs full provenance: tarball checksum, commit, notices,
  derived-file table.
- SortAnt (rank fusion) and TokenAnt (chunk sizing) are not released. Define the small trait needed, with a
  plain default (score descending, then input order), so each can be swapped in later.
- crates.io dependencies only; no `unsafe`; no `#[ignore]`.
