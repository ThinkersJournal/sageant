# SageAnt roadmap

SageAnt is the Knowledge Store: a storable, organised collection of annotated, sorted, retrievable
knowledge chunks, with manual retrieval tooling and pluggable interfaces for automated retrieval.
It sits above PersistAnt (storage). Status: stub crate, no API.

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
| 0 | ROADMAP, HANDOFF, version 0.0.1, CHANGELOG | in PR |
| 1 | Read-only inventory of `weavatrix-memory` (MIT) and `areev-store` (MIT OR Apache-2.0); fork-vs-fresh recommendation | next |
| 2 | Contract note, options first: data model, retriever trait, promoter/demoter/maintainer traits, MCP tool surface, errors, determinism, PersistAnt mapping. PM reads before any code | blocked on 1 |
| 3 | Core plus in-memory reference store; value-asserting tests, mutation checks, no `unsafe`, zero required dependencies | blocked on 2 |
| 4 | PersistAnt-backed store, lexical retriever, MCP tool server, CLI; ease gate for a stranger | blocked on 3 |
| 5 | Patent sweep before any release | blocked on 4 |

## Constraints

- Not to be read or copied: `eidetic-engine` (licence excludes Anthropic and its agents), `cyberbrain-core`
  (FSL), `velesdb-memory` (custom licence).
- Anything copied from an MIT/Apache crate needs full provenance: tarball checksum, commit, notices,
  derived-file table.
- SortAnt (rank fusion) and TokenAnt (chunk sizing) are not released. Define the small trait needed, with a
  plain default (score descending, then input order), so each can be swapped in later.
- crates.io dependencies only; no `unsafe`; no `#[ignore]`.
