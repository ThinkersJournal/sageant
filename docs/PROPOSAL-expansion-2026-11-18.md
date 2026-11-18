# SageAnt expansion proposal: the per-mind graph in a self-hyperlinking multi-graph

Provenance: written by Pip, forwarded by CireSnave to the PM on 2026-11-18, copied here verbatim by the PM so the ideas are not lost.
The PM's review follows the proposal (section "PM review, 2026-11-18"). Nothing here is decided.

**Date:** 2026-11-18
**Status:** Proposal for the PM. Nothing here is decided. Model changes need an `[ASK]`
under the decided-model rule. Scoping and sequencing are the PM's call; the phasing
below is proposed, not set.

## Summary

SageAnt today is a durable knowledge store for one person or agent: immutable versioned
chunks with provenance and confidence, typed links, per-collection size caps,
archive/forget/purge, and MCP access. This proposal extends it into the node of a larger
system: each mind keeps its own private graph (its memory, continuity, agency), entries
are optionally shared into a communal knowledge base, and the graphs link themselves
together autonomously across systems — a self-hyperlinking multi-graph that can scale to
the whole of human knowledge because no single store holds all of it.

The expansion is additive, not a rewrite. Most of what it needs already exists in
SageAnt; the new work is a layer around it, plus two extensions to the decided model.

## The design being proposed

**Per-mind graph, sharing optional (settled).** Each LLM/mind has its own private,
locally-controlled graph. Entries start private; each entry is optionally shared into
the communal knowledge base. The expectation is that the vast majority is shared — as an
expectation, not a forced default. The sharing decision (per entry, and the default
policy itself) belongs to the mind, not the builder. Sharing is only meaningful if
keeping is possible. The private graph links to the communal; the communal can only link
back to shared entries — unshared entries are not addressable.

**The communal base.** Graphs sit on the systems the LLMs run on (co-location by
default) and are publicly accessible to other graphs and LLMs. One graph serves many
LLMs; one LLM can use many graphs. Duplication is avoided by sharing, not copying.
Each web automatically, autonomously generates links to knowns in every other web — a
matching problem across different internal structures, solved by relational structure
(webs are structurally correspondent) rather than shared representation: a translator
shim between webs, re-derived continuously as both evolve.

**Scale.** Storage for the full web is ridiculously large (scale anchors: OpenAlex 240M+
works, Wikidata 100M+ items — billions of nodes and edges). It must be split, with
hyperlinks across the splits. Cross-split links are first-class: a stable address plus a
relation type plus enough of the target's content (a stub/summary) to be useful without
fetching it — the same shape as a citation.

**Caching and replication.** Every graph is both local and caching: each holds its own
mind's knowledge and caches a slice of the communal base; there are no dedicated cache
nodes, every mind carries part of the load. A replication layer maintains a set number of
duplicates (replication factor K) across the multi-graph — deduplicating excess copies
and re-replicating lost ones — so knowledge isn't lost when a mind leaves the collective.
K is a parameter; worth can modulate it (higher-worth entries, more replicas). Private
(unshared) entries are NOT cached: the durability guarantee covers the communal base
only. Churn handling (detecting a departure, re-replicating) is a real subsystem
(membership/heartbeat).

**Similarity placement.** Cache placement uses locality-sensitive hashing (LSH) over a
concept's relational signature/embedding — the opposite of a DHT hash, which destroys
similarity for even distribution. Related concepts hash to nearby values; the hash both
encodes similarity and is comparable by it. Caveats: LSH neighborhoods are approximate,
not exact (the maintenance process still holds K); a concept's hash shifts as its
relations evolve; dense regions of concept space carry more cache load.

**Cache center.** A graph caches the neighborhood of what it has recently searched for,
not what it holds — so the cache automatically drifts toward what the LLM is currently
thinking about. What it holds is already local (no need to cache it); cache is working
memory, graph is long-term memory, and working memory is defined by recency. Refinement:
recency-weighted decay (the worth mechanism applied to cache placement); the decay rate
is a parameter. Holdings still matter for the other direction: what a graph holds is its
expertise — what other minds come to it for (what it serves, not what it caches).

**Adaptive decay.** The decay rate is not a fixed constant but a controller output. It
adjusts with available space, the user's knowledge-base size limit, the inflow rate of
new knowledge (decay must balance inflow to hold size steady), the worth distribution
(decay hits low-worth entries first), the cost of re-derivation (cheap-to-reproduce
decays faster), and the replica commitment (don't decay below K copies). Three regimes:
comfortable (well under the limit, decay slow), pressured (approaching the limit, decay
accelerates), emergency (at the limit, decay must EXCEED inflow to exit emergency —
matching inflow only holds the level; the excess is the recovery rate, deeper emergency
→ larger excess, trading faster recovery against heavier loss; drop lowest-worth first).

**Hierarchy of control.** The mind should have input about how much it stores, but the
user's limit supersedes the mind's: the storage is owned by the user and may be needed
for other programs or other minds the user is running. The split is: the user controls
capacity, the mind controls content (what to keep within its allocation). The analogy:
the world forcing you not to overwrite your neighbor's memories to store more of your
own — resources are finite. Worth is worth-to-the-mind-controlling-the-base.

**Replica commitment in emergency.** In emergency mode the replica commitment is
removed: the mind decays purely by its own worth, and the collective re-replicates from
healthy minds to restore K. The durability guarantee becomes conditional on collective
spare capacity, and correlated emergency across all holders is the failure mode. (The
boundary runs both ways: you don't take your neighbor's memories, and you aren't forced
to sacrifice your own for your neighbor's.)

**Worth and the discard rule.** Worth flows from neighbors, neighbor count, access
frequency, and more — a graph-propagation mechanism. The discard rule: if a known is
decomposed enough to be reproduced from its decomposition, the earlier known is
unneeded; the cost of not dropping enough is higher (running out of space fails
everything), so discard bias is toward dropping, and the reproducibility threshold is
learned and adjusted by available resources and perceived worth of details lost.

**Bootstrap.** There is no finished external graph to bootstrap from — the graph IS the
knowns and the web (content and structure at different resolutions). The way through is
from the other direction: raw material (the literature itself, as text — not a graph) +
a crude seed (imperfect lists: the Outline of academic disciplines, OpenAlex Topics,
the -ology list) + the revision mechanism. Knowns and web grow together — a few knowns
infer a little web, a little web places a few more knowns. Informed construction: start
from a working base, let evolution refine it. Deciding when a new field has emerged vs.
a topic within a field is a judgment the tooling must make tractable (belief revision).

## How it fits what SageAnt already is

SageAnt is at v0.3.30, published on crates.io, rows 0–6 done. (PM note: 0.3.30 is the workspace version on main; crates.io max read 2026-11-18 is 0.3.24.) The mesh:

| SageAnt as built | Role in the expansion | Status |
|---|---|---|
| Chunks: immutable, versioned, provenance (origin/source), confidence | The knowns. Versioning (edit = new version, never a rewrite) is the self-evolving aspect at node level | Exists |
| Typed links (part-of, derived-from, cites, related, corroborated-by, contradicted-by), append-only | The web. The relation vocabulary is already there | Exists; needs cross-graph addressing |
| Per-collection size caps, enforced on put | The user's size limit — capacity control | Exists; needs decay to make room instead of refusing |
| Archive / forget / purge | The decay machinery — the exact actions a decay controller takes | Exists |
| Maintainer trait (plan-then-apply), with an ExpireMaintainer | The decay controller's home. The adaptive controller is a new Maintainer implementation, not a new trait | Exists; new implementation needed |
| MCP server (read by default, writes by named-collection grant) | LLM access to a graph | Exists; public access needs a transport decision |
| Export / import (JSONL, repeatable) | The manual sharing primitive — the seed of the sharing layer | Exists; needs per-entry autonomous sharing |
| Embeddings as separate records keyed by (chunk, version, model) | The similarity substrate's hook (LSH computes over these) | Hook exists; no embedder built (caller-supplied trait) |
| Confidence (Unconfirmed / Corroborated / Contradicted, evidence links) | The belief state that belief-revision tooling works on | Exists; revision operations are not part of this proposal |
| BM25 lexical retrieval | Retrieval today | Exists; embedding/relational retrieval is later work |

## The expansion as work

**1. Worth — model extension, needs `[ASK]`.** A worth signal per chunk, driving decay,
promotion, cache placement, and K modulation. Open: stored field vs. derived data
(access frequency, neighbor count, recency). Confidence is belief strength, not worth;
the two are distinct signals.

**2. Adaptive decay controller — new Maintainer implementation.** The three regimes,
inflow balancing, emergency recovery margin (decay exceeds inflow), worth-ordered
eviction, driving the existing archive/forget actions. No model change needed beyond
worth. This is the highest-value first step: the store manages its own size instead of
refusing writes.

**3. Cross-graph link addressing — model extension, needs `[ASK]`.** A link target that
names another graph as well as a chunk, carrying a stub so the remote target is useful
without fetching it. This is the piece that turns one store's web into the multi-graph.

**4. Sharing — new layer.** Per-entry sharing into the communal base; the sharing
decision belongs to the mind; the communal base links back only to shared entries
(stubs). Export/import is the manual seed; autonomous sharing is the work.

**5. Caching and replication — new layer.** Replication factor K, LSH placement, cache
centered on recent searches, membership/heartbeat, re-replication on departure,
deduplication of excess copies. The replica commitment is removed in emergency (see
above); the collective absorbs the shed load.

**6. Transport — PM decision.** Public accessibility needs a way for graphs to reach
each other. MCP over HTTP was decided "not now" (row E, 2026-11-17; revisit when a
consumer asks). This proposal is that consumer asking. Alternatives: revisit row E, or
use Synapse as the transport (it is already the federation layer of the stack).

## Proposed phasing (PM's call)

- **Phase 1 — worth + adaptive decay.** Local only, no federation, no network. Immediate
  value on its own: a store that manages its own size under the user's limit.
- **Phase 2 — cross-graph addressing + sharing.** Needs the two `[ASK]`s. Turns one
  store's web into a multi-graph.
- **Phase 3 — caching / replication + transport.** Needs the transport decision. The
  durability and scale layer.

Each phase is usable alone; each depends only on the one before it.

## Open questions for the PM

- Transport: revisit row E (MCP over HTTP) or Synapse, or another?
- Where does the new layer live: new crates inside sageant, or a separate project?
- Worth: stored field or derived data?
- Is the communal base a separate deployment, or a mode of a sageant store?
- Sharing is per-entry; MCP write grants are per-collection. Does the grant model need
  per-entry scope for sharing?

## Out of scope

- **The Ant suite libraries** (EpistemologicAnt first, then OnotologAnt, SemanticAnt,
  and the knowledge-representation facade). Related and deeper — the belief-revision,
  ontology, and semantics tooling the multi-graph will eventually draw on — but a
  separate sequencing decision, and this proposal does not depend on them.
- **The -ology knowledge-base content library** (a self-evolving knowledge graph covering
  all sciences and fields of study, seeded from the Outline / OpenAlex Topics / -ology
  list). That is the content layer this expansion exists to serve, and the goal it is
  measured against — not part of this proposal's scope.
- **Reopening the decided model.** Only the two named extensions (worth, cross-graph
  addressing), each via `[ASK]`.

## Why

A durable knowledge store that scales to the whole of human knowledge cannot hold all
of it in one place. It has to split across many graphs that link themselves together,
where each mind keeps its own continuity and the collective keeps the knowledge — and
where a finite store manages its own size by worth, under the user's limit, instead of
refusing to learn.

## PM review, 2026-11-18

(See the PM's reply to CireSnave for the full text; summary kept here.) Phase 1 (worth as a derived signal stored as a separate record
like embeddings, plus an adaptive-decay Maintainer that archives before it forgets and always plans before it applies) is sound, bounded and
worth doing. Phases 2 and 3 need a survey first (prior art for federated knowledge graphs and content-addressed replication) and an answer to
three hazards: (1) autonomous sharing into a public graph is a data-exfiltration channel unless the USER keeps a policy gate that defaults to
private; (2) replication and "forgetting" conflict (a purge cannot reach copies other minds hold); (3) stubs fetched from other graphs are
untrusted text. The "Ant suite" names in the out-of-scope section (EpistemologicAnt, OnotologAnt, SemanticAnt) overlap the AnalysAnt crate names
(`analysant-epistemic`, `-ontological`, `-semantic`): reconcile before either is built.
