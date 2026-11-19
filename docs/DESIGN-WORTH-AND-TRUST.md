# Design note: worth (X1), adaptive decay (X2) and the trust tool sageant needs (X3)

Status: **design only; no code, no row started.** Written by the `sageant` lane on 2026-11-19 for ROADMAP "Expansion". CireSnave
approved X1 to X3 on 2026-11-18 ("The SageAnt lane can proceed with that work whenever. It has my approval."), which answers the
`[ASK]` for those three decided-model changes. This note is the "design note first" step; the PM reads it before any code.
Facts about today's code are from `origin/main` e147018 (0.3.32); a claim about code names the file.

## 0. Rules that bind everything below

- Derived data is never a field on a chunk. A chunk version is immutable (CONTRACT D2); what we compute about it is a separate
  record that can be recomputed and deleted.
- `forget` deletes every derived record of the chunk (CONTRACT D3, D8); the audit log and the records hold no content.
- No floats in storage (CONTRACT D6): integer basis points, caller-supplied time, deterministic output for the same inputs.
- Local only, no network. Secrets are never stored. No new privacy guard (CireSnave 2026-11-18).
- "Unretrievable", never "erased", except on `sqlite-secure` (README table).
- Plans before they apply, and apply nothing irreversible: `archive` yes, `forget` and `purge` never from a maintainer.

## 1. What exists today (read from the code)

| Fact | Where |
|---|---|
| `Origin` is a closed enum (User, Tool, Web, Agent, Model); `source` is a free string | `src/model.rs`, CONTRACT D5 |
| `Confidence` has three states; `derive_confidence` is a pure function: any contradicting evidence still stored gives Contradicted; else Corroborated if some corroborating chunk has a different origin **and** a different source | `src/graph.rs` |
| `Maintainer` has `plan(reader)` (read only) and `apply(writer, plan, actor, at)`; changes are `Archive`, `Link`, `Supersede`; the one reference maintainer, `ExpireMaintainer`, archives chunks created before a time | `src/maintain.rs` |
| Keys in the PersistAnt kv store use prefixes `c/` (chunk version), `h/`, `l/` (link), `r/` (reverse link), `t/`, `v/`, `n/`, `a/` (audit) | `crates/sageant-persistant/src/lib.rs` |
| The size cap is per collection and per chunk (bytes of one chunk), not a store budget | `src/store.rs` `set_byte_cap` |
| **There is no `pinned` or `retention` field in code** (CONTRACT D3 lists `retention`; it was not built) | grep of `src` and `crates/*/src` |
| **Embeddings do not exist in code** (CONTRACT D3 describes them as records keyed `(chunk, version, model_id)`) | same grep |
| **Nothing records reads**: a `get` or `search` writes nothing, and the MCP server is read-only unless granted | `src/reader.rs`, `crates/sageant-mcp` |

Three things the approved rows lean on are therefore not there yet: a pin, a store-level budget, and a read signal. Section 2 and 3
say how each is added, and which of them needs your word.

## 2. X1: worth, a derived signal

**What it is.** A non-negative integer score (basis points) for one chunk version meaning "how much is this worth keeping", derived
from signals, stored as a snapshot, used by X2 to order eviction and by a retriever as a tie-break. It is distinct from
`confidence` (belief state) and from trust (X3).

**Signals (v1).**

| Signal | Source | Needs new storage? |
|---|---|---|
| Neighbour count | links in and out of the lineage (`l/`, `r/` already stored), counted by kind with `part-of` and `derived-from` weighted above `related` | no |
| Recency | `created_at` of the current version and of the newest link touching it | no |
| Stability | number of versions and time since the last supersede | no |
| Access frequency | a count of reads | **yes**, see 2.2 |

**2.1 Snapshot record.** Keyed like the planned embeddings: `(chunk id, version, worth_model_id)`, in the kv store under a new
prefix (proposed `w/<chunk id>/<version>/<model id>`; the prefix is free in the table above). Value: `{score_bp, components[]
(name, raw, weight_bp), computed_at, model_id, model_version}`. `worth_model_id` names the formula and its parameters, so a change
of formula never rewrites a record and two models can coexist. Records are recomputable: deleting all of them loses nothing but
time. `forget` of a chunk deletes its `w/` records in the same atomic batch that removes its content and index rows (the existing
forget batch; a test reads the files back, as for the text index).

**2.2 Access frequency, and why it is an open question.** A read count needs a write on a read. Today the MCP server is read-only
unless a collection is granted, and the CLI `get` writes nothing. Options:
(a) **no read signal in v1**: worth uses only the first three signals; nothing changes about reads. 
(b) an **access log** appended only by adapters that already hold a write grant (the CLI as the owner, MCP only on a granted
collection), as audit-like records `x/<seq>` holding `(chunk, version, at)` and never content, folded into the snapshot.
(c) a counter updated by every read, including read-only MCP.
**Recommendation: (a) for X1's first slice, (b) as a later slice with its own tests**; (c) is refused, because it makes a read-only
server write and lets a reader influence what is kept (an agent could pin a chunk by reading it in a loop). OPEN, PM: confirm (a)
then (b).

**2.3 Computation.** A pure function `worth(chunk, links, versions, now, params) -> Worth` in the core crate (no I/O), a
`WorthStore` trait in the core for reading and writing snapshots, an in-memory implementation, and the PersistAnt-backed one in
`sageant-persistant`. Recompute is an explicit step (a maintainer pass or `sageant reindex`-like command), not on every read, so the
score is stable between passes and a plan is deterministic.

**2.4 Tests (red first).** Value-asserting, with sabotage:
- Exact scores for a hand-built graph (a three-chunk fixture with known links), asserted to the basis point.
- Sabotage: drop each signal in turn and the corresponding test fails; swap `part-of` and `related` weights and it fails.
- `forget` removes the `w/` records: read every database file back (positive control: the same scan finds the records before).
- Determinism: the same store state and `now` give byte-identical snapshots twice; a different `now` changes only the recency term.
- A snapshot never changes a chunk's content hash or id.

## 3. X2: adaptive-decay maintainer

A new `Maintainer` implementation, `DecayMaintainer`, in the core crate behind the existing trait (the trait is not changed).

**3.1 Two things it needs that do not exist.**
- **A store budget.** The existing cap is per chunk. Decay needs a target such as "this collection may hold N bytes of active
  content" (and optionally a count). Proposed: `Budget { collection, bytes }` set by the owner, stored as a record, never inferred.
  A collection with no budget is never touched by the decay maintainer.
- **A pin.** "Never touches pinned chunks" needs a way to pin: proposed as a tag-like marker record (`p/<chunk id>`) set by the
  owner (CLI `sageant pin`), readable through `Reader`, and **not settable by an MCP client** (an agent must not protect or expose
  its own writes). OPEN, PM: confirm a pin is added now rather than reusing the unbuilt `retention` field.

**3.2 Regimes.** From fill = active bytes / budget, in integer percent: **comfortable** (below a low mark: plan nothing),
**pressured** (between marks: archive lowest-worth unpinned chunks until fill is at the target), **emergency** (above a high mark:
the same, to a lower target, still never touching pinned chunks). Marks and targets are parameters with documented defaults
(proposed 70 / 85 / 95 and targets 70 / 60), part of the plan so a reviewer can see them. Inflow balancing: the plan also reports
bytes written in the last window (from `created_at`), and a pressured regime sizes its target to leave room for one window of
inflow. Recovery margin: after a pass, fill must be at or below target minus a margin before the regime returns to comfortable,
so the maintainer does not flap.

**3.3 Eviction order.** Lowest worth first; ties by older `created_at`, then by lineage id (total, deterministic). It archives
(reversible) and never forgets or purges. It skips any chunk that other chunks depend on through `part-of` (a parent with live
children) and any chunk whose archive would orphan a `derived-from` source still active, and records why in the step's `Why`. It
does **not** purge or forget on the rule "reproducible from its decomposition": that cannot be verified without a model and conflicts
with the lossy-step rule (ROADMAP X2).

**3.4 Plan before apply.** `plan` is read-only and returns the regime, the numbers it used, and each step with its reason, so
`sageant maintain --dry-run` prints exactly what `apply` would do; the plan for the same store state and `now` is identical every
time. `apply` goes through the existing `Writer` and audit.

**3.5 Acceptance (from the ROADMAP row), as tests.** A store filled to its budget under a synthetic inflow sequence stays at or
under budget after each pass; nothing is lost that cannot be recovered (every archived chunk is still readable with
`include_archived`, and `unarchive` restores the search hit); plans are deterministic; a pinned chunk is never in a plan, at any
fill. Sabotage: remove the pin check and the pinned-chunk test fails; invert the worth order and the eviction-order test fails;
remove the recovery margin and a flapping test fails.

## 4. X3: the trust tool, stated as requirements for any consumer

CireSnave's standing rules (2026-11-18, via the PM): "If something needs a tool that falls in one of those fields, we should build
that tool in those projects and use it as a dependency for the project that needs it", and "tools should be built to be generic over
any use of that tool type and not specific to the individual consumer." So the derivation of trust is built in **EpistemologicAnt**
(crate `epistemologicant`), and this section is written as what such a tool should offer **any** body of claims and **any** kind of
source. SageAnt is one example and the first test case, not the shape of the API. Nothing here creates that project; its lane writes
its own spec from this and may disagree.

**4.1 Capabilities (generic).**

1. **Claims and sources as abstract things.** A *claim* is an opaque identified item (the tool does not read its text). A *source*
   is an identity with a *class* (person, tool and version, web domain, agent, model, anything a consumer names). The tool takes
   them by id; it knows nothing about chunks, collections or origins as sageant spells them.
2. **Evidence as typed relations between claims.** Supports and contradicts (and optionally cites), each with the source that
   asserted it and a time. The tool derives from these relations; it does not fetch them.
3. **Independence test.** Whether two supporting items count as independent, as a pure, documented, replaceable rule over source
   identity and class (today sageant's rule: different class and different source). Supporting items that share a source, or that only
   cite each other in a cycle, count once. A source can never corroborate its own claim.
4. **Source track record.** For each source, and per topic with a fallback to overall (a topic is an opaque label the consumer
   supplies): how many of its claims were later supported or contradicted by independent sources, recency-weighted so a source can
   recover.
5. **Shrinkage.** A source's rating is pulled toward an "unknown source" baseline while its evidence is thin, with the strength of the
   pull a parameter, so one lucky or unlucky claim does not move it far.
6. **Priors, capped.** A claim starts from its source's rating (known-good above unknown above known-bad). A prior is capped below the
   level reached by independent corroboration: it is a starting point, never a substitute. A consumer may give an initial prior for a
   named source (sageant's use: the owner starts slightly above unknown), and the tool records that it is a configured prior, not
   earned.
7. **Outputs: ordered levels with reasons.** For a claim: a level from a small ordered set, plus the reasons (which relations,
   which sources, which prior) in a machine-readable form a consumer can show. For a source: the same, per topic. No float is
   required of the consumer.
8. **Thin evidence says so.** Where there is too little evidence to rate, the output is "unrated", never a guessed level.
9. **Remote ratings are claims.** A rating imported from another graph is input evidence with its own source, never a stored trust
   level; the tool has no operation that sets a level directly.
10. **Pure and deterministic.** Same inputs and same `now` give the same outputs; no network, no clock, no model call; the consumer
    owns storage. Time comes from the caller.
11. **Explanations and what-ifs.** A consumer can ask "what would change this level" (for example, one more independent supporter),
    for display and for tests.

**4.2 What sageant keeps.**
- **Storage** of the derived result as snapshot records `(chunk, version, trust_model_id)` and per-source records, under their own
  prefixes, recomputable, deleted by `forget` (for chunks), carrying no content.
- **Mapping** its own data onto the tool: chunk -> claim, `origin` + `source` -> source identity and class, `corroborated-by` and
  `contradicted-by` links -> evidence relations, `created_at` -> time.
- **Display**: the level and its reasons on every retrieval result (CLI and the MCP envelope: new named fields, not content).
- **The default rating policy** the owner chose (a few ordered levels shown with reasons; the owner gets a small day-one prior then
  earns it; shown on every result; CireSnave's defaults, changeable).
- What stays decided: `confidence` (three states) remains the stored belief state; trust is a separate, derived rating beside it.

**4.3 Migration for existing chunks.** All existing chunks have `origin`, `source` and links, so their trust can be derived on first
run of the new tool without rewriting any chunk. Order: a store from before the tool has no `w/` or trust records, so the first derivation pass creates them;
`sageant export` carries chunks and links but not derived records (they are recomputed after `import`); a store from before the
tool has no records and shows "unrated" until a pass runs. A home written by an older sageant opens unchanged (no format change to
`c/`, `l/`, `h/`).

**4.4 Test plan.** Two layers. In `epistemologicant` (its lane): property tests for independence (cycles of mutual citation count
once, self-corroboration is refused), shrinkage (a long record beats a short one at the same rate), the prior cap, and determinism.
In sageant: a mapping test with a fixed fixture (a user note, a tool output and a web page with known links) asserting the exact
level and reason for each; a sabotage test that makes the mapper treat two chunks from the same source as independent and fails; a
display test that the MCP envelope carries the level and reasons but never changes `content`; a forget test that no trust record of
a forgotten chunk survives in any file.

## 5. Order, survey and decisions

- **Survey before build** (portfolio rule): X1 and X2 need a short prior-art read (decay and eviction policies in caches and in
  memory systems; worth or salience signals) before code; this note names the signals from the approved rows, not from a survey. The
  slice that begins X1 starts with that survey, not with code.
- **Order:** X1 snapshot and signals (no read log) -> X2 budget, pin, `DecayMaintainer` -> X3 after `epistemologicant` exists with
  a spec derived from section 4 -> X1 access log (2.2 option b).
- **OPEN, PM:** (1) read signal: confirm (a) then (b), refuse (c); (2) add a pin and a budget as owner-only records; (3) whether the
  worth formula weights are fixed defaults or owner-tunable in v1 (recommendation: fixed defaults, recorded in `worth_model_id`).
- No decided-model change beyond the approved rows; no network; nothing here starts X4 to X7.
