# SageAnt roadmap

SageAnt is the Knowledge Store: a storable, organised collection of annotated, sorted, retrievable
knowledge chunks, with manual retrieval tooling and pluggable interfaces for automated retrieval.
It sits above PersistAnt (storage). Status: pre-1.0, published on crates.io (see the crates' pages for the current version); rows 0 to 6 are done (the crates: `sageant`, `sageant-lexical`, `sageant-persistant`, `sageant-mcp`, `sageant-cli`).

## Decided model (changes need an `[ASK]`)

- One chunk model for logs, memory and documents. A chunk is UTF-8 text (NFC) and immutable; an edit is a
  new version with `supersedes`.
- Size is a measured property with a per-collection cap, never a type. A document or dictionary is a parent
  chunk with child chunks linked `part-of`. Non-text is text with a `format` tag; binary only as blob
  references, later.
- Trust is split: `origin` (user, tool, web, another agent, model) and `confidence` (unconfirmed,
  corroborated by independent origins, contradicted; evidence as links). Injection safety is policy, not a
  stored opinion: retrieved content is data and carries its origin label.
  This is today's model; the graded, derived trust rating and source ratings that will sit beside it are row X3 below.
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
| E | MCP over HTTP | decided 2026-11-17 (PM): not now; revisit when a consumer asks. Stays a threat-model document (`docs/THREAT-MODEL-MCP-HTTP.md`); nothing is built |

## Constraints

- Not to be read or copied: `eidetic-engine` (licence excludes Anthropic and its agents), `cyberbrain-core`
  (FSL), `velesdb-memory` (custom licence).
- Anything copied from an MIT/Apache crate needs full provenance: tarball checksum, commit, notices,
  derived-file table.
- SortAnt (rank fusion) and TokenAnt (chunk sizing) are not released. Define the small trait needed, with a
  plain default (score descending, then input order), so each can be swapped in later.
- crates.io dependencies only; no `unsafe`; no `#[ignore]`.

## Expansion (agreed direction, not started)

Source: Pip's proposal, `docs/PROPOSAL-expansion-2026-11-18.md` (with the PM's review at its end), which CireSnave agreed with on
2026-11-18 ("I agree with your additions to Pip's proposal. Go ahead and add the proposal and your additions to SageAnt's
roadmap"). **Nothing here is built and no row is started.** Approval is not an order of work: phases run in dependency order
(Phase 1 = X1 to X3, Phase 2 = X4 and X5, Phase 3 = X6 and X7), each starts with a prior-art survey and a design note, and the
standing rules below bind.

**Approvals (CireSnave, 2026-11-18, verbatim):**
- X1 to X3: "The SageAnt lane can proceed with that work whenever. It has my approval." This answers the `[ASK]` for those three
  decided-model changes.
- X4 to X7: "Cross-graph addressing, sharing, replication, and transport are covered by my approval."

**Decided points (CireSnave, 2026-11-18):**

1. **No extra privacy guard in the knowledge base.** "A user's private data should never be directly shared with an LLM ... That's
   why we created with-secret ... adding an additional privacy guard is redundant and shouldn't be integrated into the knowledge
   base." Sharing is governed only by what exists: entries start private, and writing to a shared or public collection needs the
   existing per-collection write grant.
2. **Public sharing is a note, not a mechanism.** "Minds communicate. Once a mind has communicated something, there is no way to
   stop that something from circulating ... we should add a note that anything shared publicly should be assumed to remain among
   public knowledge forever." The README carries the note. `forget` and `purge` act on this store only; nothing is built to
   propagate a purge.
3. **Knowledge must earn trust.** "All knowledge should include trust ratings. All knowledge should have to build that trust."
   Today there is `origin` plus a three-state `confidence` (unconfirmed, corroborated by independent origins, contradicted), and a
   chunk starts unconfirmed. The direction is a graded, **derived** trust rating on every retrieval result, from corroboration by
   independent origins, each origin's track record, age and stability, and challenge history. Nothing starts trusted; imported or
   cross-graph knowledge enters at the bottom locally (a remote rating is a claim, not trust). Trust is distinct from worth (what
   to keep) and from confidence (the belief state).
4. **Sources carry a rating too.** CireSnave: "I do my best to never steer you wrong and over time you would know that about me.
   That should allow knowledge entries from me to start somewhat higher than a knowledge entry from a knowledge source we don't
   know. Similarly, a source that has constantly been feeding us misinformation ... should have a lower trust rating and thus
   knowledge entries from that source should start lower than those from an unknown source." So an entry starts from its source's
   earned rating (known-good above unknown above known-bad) and still has to build its own trust by corroboration.

**Defaults, changeable by CireSnave at any time** (he approved after reading the PM's recommendations, and did not answer the three
choices one by one): a few ordered levels shown with their reasons; the owner gets a small day-one prior above an unknown source and
then earns it like everyone else; the rating is shown on every retrieval result.

**Proposed, not decided (the PM's design for how a source is rated):** a source is an identity (a person, a tool with a version, a
web domain, an agent, a model), not only a class. Its rating is derived from its record (its entries later corroborated or
contradicted by independent origins), per topic with a fallback to overall, shrunk toward the unknown baseline while evidence is
thin, and recency-weighted so a source can recover. A source can never corroborate itself, nor raise its own rating through sources
that only cite each other. A source's prior is capped below "corroborated": it is a starting point, never a substitute.

**Where the pieces live (CireSnave's standing rule, 2026-11-18):** "If something needs a tool that falls in one of those fields, we
should build that tool in those projects and use it as a dependency for the project that needs it." So the derivation core of X3
(corroboration, the independence test, per-topic source track records, shrinkage, no self-corroboration) is epistemology and is
built in the **EpistemologicAnt** project (crate `epistemologicant`), which sageant will depend on; alignment of graphs for X4 and
X5 will likewise belong to **OntologicAnt** later. Such tools are built generic over any use of their type, not shaped to sageant.
SageAnt keeps the storage (derived snapshot records) and the display. X1 and X2 are sageant's own. Those projects are separate and
asleep until the PM wakes them; the names are not final. Sageant creates none of them.

| # | Row | Status |
|---|-----|--------|
| X1 | Worth, a derived signal (access frequency, neighbour count, recency), stored as a snapshot record keyed like embeddings, never a mutable field on an immutable chunk; distinct from confidence and trust | approved 2026-11-18; planned, not started; design note first |
| X2 | Adaptive-decay `Maintainer` (a new implementation of the existing trait): comfortable, pressured and emergency regimes, inflow balancing, a recovery margin, worth-ordered eviction. It plans before it applies, archives before it forgets (reversible), never touches pinned chunks, and may never purge on the "reproducible from its decomposition" rule (not verifiable without a model; it conflicts with the lossy-step rule). Acceptance: a store filled to its cap under synthetic inflow stays under the cap, loses nothing unrecoverable, and gives deterministic dry-run plans | approved 2026-11-18; planned, not started; Phase 1; local only, no network; a crate inside sageant |
| X3 | Trust rating and source ratings (points 3 and 4 above). Derivation in EpistemologicAnt; sageant stores the derived snapshot and shows it | approved 2026-11-18 (with source ratings); planned, not started; design note first (sageant's requirements, stated generically, for the EpistemologicAnt lane) |
| X4 | Cross-graph link addressing: a link target that names another graph plus a chunk, with a stub. Stubs are untrusted text (origin-labelled data, as today). Needs a stable address (graph id, chunk id, version, content hash) | approved 2026-11-18; planned, not started; Phase 2; prior-art survey, then a design note |
| X5 | Sharing: a chunk version referenced from a shared collection, so the existing per-collection grants apply. No new per-entry grant scope, no new privacy guard | approved 2026-11-18; planned, not started; Phase 2 |
| X6 | Caching, replication, LSH placement and membership: a **separate project** (its own name, lane and survey of content-addressed stores and DHTs), not part of sageant. The communal base is a separate deployment of the same store software, not a mode of a private store | approved 2026-11-18 (the work); not started; Phase 3; waits for a name from CireSnave, a survey and its own lane |
| X7 | Transport between graphs: **not now**. Row E stays "not now"; any network transport must be authenticated and encrypted (CireSnave 2026-11-17: no workarounds for insecure protocols). Synapse as a transport is unverified for peers outside this machine | approved 2026-11-18; not started; Phase 3; the PM decides after the survey, limited to authenticated, encrypted transports |
| C1 | Working-set cache advisor: a tiny local model ranks an LRU's eviction candidates and proposes pre-warm queries (CireSnave idea 2026-11-20, via the PM). **Idea, not approved.** Sageant has no cache or LRU today; the design note `docs/DESIGN-PREWARM-AND-EVICT.md` sets out options and recommends a recorder and replay harness first (with the offline-optimum ceiling), then the eviction ranker, then pre-warm; no model in sageant's core | proposed 2026-11-20; docs only; awaiting the PM's answers in the note's section 7 |

**Related, not an overlap:** the "Ant suite" in the proposal's out-of-scope section (EpistemologicAnt, OntologicAnt, SemanticAnt;
names not final) are comprehensive libraries, each covering a whole field. AnalysAnt's lenses (`analysant-epistemic`,
`-ontological`, `-semantic`) are likely to use them; they are not the same thing. The content library these fields support is the
goal the expansion serves, not part of it.
