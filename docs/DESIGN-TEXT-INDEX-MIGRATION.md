# Design note: replace `KvTextIndex` with PersistAnt's text index (PM row, approved 2026-11-16)

Status: **design only, nothing built.** Written by the `sageant` lane against `persistant` 0.18.21 (`persistant-kv` 0.18.21,
`text-index` feature). Gate set by the PM: the replacement lands only if it passes our differential tests and N>=3 raw-file
read-back tests on `sqlite-secure`, `sqlite` and `sqlite-full`, and BM25 ranking stays **byte-identical** on our corpus.

## What exists

- `crates/sageant-persistant/src/index.rs` (318 lines): `KvTextIndex`, our `sageant_lexical::TextIndex` over a PersistAnt `KvHandle`,
  in its own file `<home>/index.db` (same backend id as the store). Keys `x/<term>/<id>/<ver>`, `d/<id>/<ver>`, `f/<id>/<ver>/<term>`,
  `m/ix`. Single atomic `commit` per `index`/`remove`; `remove` uses `BatchOp::DeleteErased`; the Unsupported fallback.
- Used by `sageant-cli` (`app.rs`: search, reindex, serve) and tests. Ranking is ours: `sageant-lexical` BM25, integer only, from
  `postings`, `doc_len`, `stats`.

## What PersistAnt offers (read from the 0.18.21 source, not run yet)

- `persistant_kv::text::TextIndex<'a, K: KvBackend>`: **async**, a layer over any `KvBackend`. Takes **terms with frequencies** (we keep our
  tokeniser), `put(index, doc_id, terms)`, `remove_many`, `remove_many_erased` (erased postings via `DeleteErased` in the same batch),
  `postings` (paged, `doc_id` ascending), `doc_len`, `stats` (docs, total_len), `docs` (paged ids), `search` (sum of tf; we do not use it).
- Every change is one `batch` with `Require` on the totals record and the document record. **All writers serialise on one totals record**
  (up to 64 retries). Fine for a personal store; our current index has the same shape (`m/ix`).
- Keys use U+001F separators: `tx`, `td`, `tt`. They do not collide with ours, but they are a **different on-disk layout**.

## Gaps and questions (to measure before writing code)

1. **Blocking vs async.** Our `TextIndex` and the CLI are synchronous; PersistAnt's layer is async over `KvBackend`, and the blocking
   `KvHandle` in 0.18.21 does not expose it nor implement `KvBackend`. Options: (a) ask the persistant lane for a blocking text-index
   handle (cleanest; they own the worker); (b) own a small current-thread runtime and `block_on` an async `registry::Handle`
   (works, adds a runtime and `tokio` as a direct dependency to `sageant-persistant`, and the CLI/MCP binaries already pull tokio via
   rmcp). **Recommendation: ask persistant for (a); build (b) only as a stop-gap if they decline.**
2. **Doc id and version.** Ours are `(ChunkId, version)`; theirs is one opaque string: use `"<id>/<ver>"` (no U+001F). `remove(id)` (all versions)
   needs the versions: enumerate via the store's `c/<id>/` keys or page `docs()` (O(N) per call; avoid). To be settled with a measurement.
3. **Postings order.** Ours are ordered by (id, version); theirs by the string `doc_id`. With `"<id>/<ver>"` and zero-padded 10-digit versions
   the order is identical; the differential test must prove it, including ids that are prefixes of each other (they are fixed-width hex here).
4. **Stats.** `docs` and `total_len` must equal ours on the same corpus (same length definition: sum of term frequencies). A differential test.
5. **Erasure.** `text.index.erased` is on only for `sqlite-secure` per PersistAnt's matrix; on `sqlite`/`sqlite-full` `remove_many_erased` is
   a best effort exactly like ours today. The README table does not change; read-back tests pin it (N=1,3,8,30) as for the store.
6. **Concurrency.** Same serialisation as today; the link-generation and index retries are independent of this.

## What would be deleted

`index.rs` (`KvTextIndex`, its `drop_ops`, `batch` fallback, getters), its tests in `tests/` that exercise key layout, and the hand-written
`m/ix` totals. **Kept:** the `sageant_lexical::TextIndex` trait and `MemoryIndex` (the differential oracle), BM25, `LexicalRetriever`, the CLI
`reindex`. `PersistTextIndex` (new name) would implement the same trait over PersistAnt's layer, so `sageant-lexical` and the retriever do not change.

## Migration of existing homes

The default home's `index.db` holds the old layout (`x/`, `d/`, `f/`, `m/ix`). The index is **derived** (rebuildable from the store with
`sageant reindex`), never the source of truth, so migration is rebuild, not translate:
1. On open, detect the layout: new keys (`tt␟`) present -> new; old `m/ix` present -> old; neither -> new.
2. Old layout: the CLI says once "rebuilding the search index (old format)", reads the store, writes the new index into a **new file**
   (`index2.db`), then removes `index.db` with the same erased-delete rules as forget (the old index holds text-derived terms of forgotten
   chunks only if they were never removed; it contains no chunk text itself). No in-place mutation, so a crash leaves the old file intact.
3. `sageant reindex` always builds new-layout.
Cost: one reindex per existing home (seconds for thousands of chunks; to be measured, not assumed).

## Rollback

Both layouts live in separate files, so rollback is a version downgrade: a 0.3.19 binary ignores `index2.db` and still has `index.db` **if
migration has not yet deleted it**. Therefore: delete the old file only on the **second** successful open after migration (or by an explicit
`sageant reindex --drop-old`). A downgrade after that rebuilds the old index with 0.3.19's `reindex`. Stated in the CHANGELOG.

## Acceptance tests (red first where possible)

- Differential: same random corpus (including supersedes, removals, Unicode, empty text) into `MemoryIndex`, old `KvTextIndex` and the new
  index: identical `postings`, `doc_len`, `stats`; and **byte-identical `sageant search` output** (ids, scores, order) over our corpus.
- Read-back (raw file bytes) after forgetting N = 1, 3, 8, 30 on `sqlite-secure` (expect 0 left), `sqlite` and `sqlite-full` (pinned limit as today).
- Migration: an old-layout `index.db` fixture is rebuilt, search results unchanged, a crash mid-migration leaves the old file usable.
- Unsupported fallback for erased batches preserved (refused-batch simulation).
- Mutation run (diff-scoped) for the new module; package dry-run; packaged first-run check as for 0.3.19.

## Decision needed

Item 1: ask the persistant lane for a blocking text-index handle (and whether `KvTextIndex`-style `remove(id)` over versions has a better
primitive), or accept a stop-gap `block_on`. I will send that `[ASK]` with this note. **Until it is answered I build nothing for this row.**

— lane: sageant (Claude Sonnet 5.5; AI-assisted reading of the dependency source, no measurements yet)
