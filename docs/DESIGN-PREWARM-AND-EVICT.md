# Design note: pre-warming a working-set cache and ranking its eviction candidates (docs only)

Status: **proposal, 2026-11-20, nothing built; option 1 agreed by the PM, CireSnave has not ruled (board 472).** Task from pm-ants (relaying CireSnave's idea of 2026-11-20 via the PM);
scope: options and a recommendation, no code. Anything for CireSnave goes through the PM board.

CireSnave's idea, verbatim (via the PM): *"If we allow a tiny LLM to be a silent observer in the chat stream of our large models to
predict where the conversation is likely headed and what information will likely be needed so we can generate queries to pre-warm the
cache, much of the information our larger agent needs should already be sitting in its cache before it needs it. If we have the LRU
output from the cache output a somewhat large list of removal candidates, that same tiny LLM could rank those removal candidates by
likelihood to be needed again soon."*

## 1. What exists (read at `origin/main` e147018, and my branch `work/phase1-design` for what is about to)

Checked with `git grep -n -i -E "\blru\b|cache|evict|prewarm|pre-warm|probation" origin/main -- '*.rs' '*.md' '*.toml'`. Positive control: the
same query finds the word "cache" in `docs/PROPOSAL-expansion-2026-11-18.md` (the replication proposal), so the query works.

- **There is no cache and no LRU in sageant's code.** Zero hits in any `.rs` file. The only "cache" and "eviction" text is in the
  expansion proposal and ROADMAP X2, which are plans. If the PM means a cache in another project (the harness, or PersistAnt), I have not read
  it; this note would then apply to whatever interface that cache offers.
- **What sageant does have that this idea would plug into** (`src/maintain.rs`, `src/retrieve.rs`, CONTRACT D10/D12):
  - `Retriever`, `Query` (with an opaque `context` byte string), `Hit` with a score and a `Reason`, and `Fuser`: retrieval is already
    pluggable and explained.
  - `Promoter::promote(ctx, reader) -> Vec<Hit>`: "a retriever with a working-memory target and no extra power: it reads, it cannot
    write." This is the existing seam for "what should be in the agent's working set". The harness decides when to call it.
  - `Demoter` and the plan-then-apply `Maintainer` with `Change::Archive`: storage-level, reversible demotion. The header says
    *"No model calls happen here: a harness that wants summaries supplies its own `Demoter`."* That rule decides where a tiny model may live (section 4).
- **What sageant does not have: any read or hit signal.** The worth design (`docs/DESIGN-WORTH-AND-TRUST.md` 2.2) refused a counter updated
  by every read, including on the read-only MCP server, because it makes a read-only server write and lets a reader influence what is kept.
  The PM's baseline "LRU plus cheap signals (hits, age, source still in discussion)" needs a hit signal, so this note's recommended
  sequencing depends on the planned owner/grant-only access log (X1 option b).
- **Two different "evictions".** X2's `DecayMaintainer` archives lowest-worth chunks when the **store** is over its byte budget: durable,
  reversible, audited. A cache's eviction drops a **reference** from a bounded hot set; the chunk stays stored and nothing is lost.
  They must not be conflated: the ranker below only ever touches the second.

## 2. What "the cache" is, since sageant has none (decision needed)

- **(A) A harness-side working set (recommended).** The large agent's harness keeps a bounded set of chunk references (id, version) it
  has loaded or pre-loaded into the agent's context or fast local store. Sageant stays as it is and supplies reads, the `Promoter`
  seam, and signals. The cache, its LRU and the model live in the harness (or a separate generic cache-policy project).
- **(B) A hot tier inside sageant.** A derived, bounded table of `(chunk id, version, tier, entered_at, last_hit)`. It is recomputable
  like worth snapshots, holds no content, and `forget`/`purge` remove its rows in the same batch. Only worth building if several
  harnesses need the same cache.
- **Not recommended: copying content into a cache store.** It would put forgotten content in a second place (CONTRACT D3, D8).

Either way an entry is a **reference to a stored chunk version**, so a pre-warmed entry carries the same origin, confidence and trust
labels as any read (the PM's thought 2), and removing the chunk removes it.

## 3. The two jobs, with baselines that need no model

**3.1 Eviction ranker (first).** The cache's LRU emits N candidates (the PM suggests about 50); a ranker reorders them by likelihood of
being needed again soon; the cache evicts from the low end.
- *Baseline 0:* plain LRU. *Baseline 1:* LRU plus cheap signals: recency, hit count, whether the source chunk's collection or
  lineage appeared in the last few turns, structural neighbours of what is currently cached (the worth components `structural_links`,
  `loose_links` are reusable here), a pin. *Candidate:* baseline 1 with a model reordering the same candidate list.
- **Safety properties (non-negotiable, each a test):** the ranker may only **reorder** the candidates it was given (a returned id not in
  the list is dropped, a missing id is appended in LRU order); pinned chunks are never candidates; a timeout, a parse failure or any error
  returns plain LRU order; the ranker is off the request path (eviction runs asynchronously, so a slow model costs nothing a reader sees).

**3.2 Pre-warm (second).** An observer predicts what the big agent will need and produces queries; the results are loaded as
*speculative* entries.
- *Baseline:* run retrieval on the latest user message plus the agent's last plan step, with no model (a `Promoter` that fuses two
  queries). *Candidate:* the same plus queries a tiny model proposes from the stream.
- **Probationary tier:** speculative entries sit in a small separate budget (a fixed fraction of the cache) and are **promoted only on a real
  hit** by the big agent; unpromoted entries age out first. A wrong guess therefore costs one small tier, never a good entry.
- **Same gates as any read:** every pre-warm query is a read, so per-collection grants and the secret screen apply exactly as for the
  big agent's own reads. A pre-warm may never read a collection the agent could not. The observer does not write to the store.

## 4. Where a tiny model may live, and the risks that come with the stream

1. **Not in sageant's core.** The contract says the store makes no model calls. The model sits behind a trait in the harness or a
   separate crate: `Ranker { rank(candidates, context) -> order }` and `Predictor { propose(stream_window) -> Vec<Query> }`, each with a
   no-model default (the baselines above). Swappable, so the choice of model is a replay result, not a design commitment.
2. **Privacy.** The stream holds the owner's private conversation. The observer is **local only** (no network call, no hosted model), as the PM
   says; the owner's secret store (`with-secret`) is the boundary, and any recorded trace passes the existing `SecretScreen` before it is
   written. A query a model generated is data, never an instruction.
3. **Injection.** The stream includes untrusted text (web pages, tool output). An attacker can write text meant to make a model keep or
   fetch something. Bounds that do not rely on the model behaving: effect limited to reordering a given list (3.1) and to a small
   probationary tier that needs a real hit to promote (3.2); no tool access for the model; output parsed against a strict schema; every
   decision reproducible from the logged inputs. This is the same worry as the refused read counter ("an agent could pin a chunk by reading it
   in a loop"): the cache must not become a way to influence what is *kept*; it only influences what is *near*.
4. **Cost.** The portfolio rule is to route bulk work off Claude; a small local model fits, a hosted one does not.

## 5. How to test it: replay first, and know the ceiling

No observer or ranker is built before the replay exists (the PM's thought 4). The replay is the work.
- **Trace:** per conversation turn, the chunk references the big agent actually requested (and the query text that produced them), the
  cache's contents at that moment, and timestamps. Replaying a trace against a policy gives its hit rate with no model in the loop for
  the baselines.
- **Metrics:** hit rate on requests the agent really made; **eviction regret** (evicted, then requested within T); pre-warm precision
  (promoted over loaded) and wasted loads; added latency of the ranker on and off the request path (p50 and p95); and the **offline optimum
  (Belady's rule) on the same trace**, which bounds what any policy can gain. If plain LRU is already close to the optimum on our traces, stop:
  there is nothing for a model to win. If LRU plus signals closes most of the gap, the model has to beat *that*, not LRU.
- **Adoption rule, fixed before measuring** (my proposal, the owner may change the numbers): a model is adopted only if it beats LRU plus
  signals on hit rate by at least 5 percentage points on a held-out half of the traces, with the ranker's p95 off-path latency under
  the interval between evictions, and no safety test failing. Compare at least two sizes so cost is visible.
- **Trace source, in order of preference:** (1) a harness-side recorder, **opt-in and local**, switched on for our own lanes' sessions; the
  X1 access log (owner/grant only) gives the chunk-level half of it. (2) Existing session transcripts, replayed offline on this machine,
  screened and never copied off it. (3) Synthetic traces from a generator, for the safety tests and to check the harness itself, not
  for choosing a model. A trace is only as good as the work it came from; ours is mostly engineering sessions, and a result may not
  carry to other uses.
- **Which tiny model:** I do not name one. Candidates must be local, small and permissively licensed; the choice is made on replay results
  behind the `Ranker`/`Predictor` traits. Picking by reputation before a replay would be a guess.

## 6. Options and recommendation

| Option | What it is | Cost | Risk |
|---|---|---|---|
| 0 | Do nothing | none | none; no gain |
| 1 | **Recorder and replay harness only**, with LRU, LRU plus signals, and the Belady ceiling | smallest | none to the store; gives the numbers to decide |
| 2 | 1, plus the eviction ranker behind a trait, no-model default | small | low (reorder-only, LRU fallback) |
| 3 | 2, plus the pre-warm predictor and probationary tier | larger | medium (privacy and injection surface of an observer) |

**Recommendation: option 1 now, then 2 only if the ceiling and the signals baseline leave room, then 3 only after 2 pays.** Ranker before
pre-warm because its failure mode is cheaper and its worst case is "plain LRU". Cache is (A), harness side, unless two or more harnesses ask for the
same one. The generic policy and replay code belongs in its own generic project rather than shaped to sageant (CireSnave's rule of
2026-11-18), with sageant supplying signals and the `Promoter` baseline; creating that project is a decision for the PM and CireSnave.

**What sageant's own rows would be**, in this order, none started: (a) the X1 access log (already queued; it is the only new signal the
baselines need), (b) a `Promoter` that fuses "latest message" and "last plan step" queries (no model; fits the existing trait), (c) if (B) is chosen, a derived hot-tier table
removed by `forget` and `purge`, with a read-every-file test. ROADMAP row **C1** records this as an idea awaiting priority.

## 7. Open questions: board 472 (open, awaiting CireSnave)

The PM agreed option 1 (recorder and replay harness first) on 2026-11-20, via pm-ants. Docs only until CireSnave rules; the recorder stays unbuilt. The PM's recommendations on the three questions, as relayed: (1) a harness-side working set; (2) yes, an opt-in, local, screened recorder; (3) a new small generic replay-harness project, name later. These are recommendations, not decisions.

1. Which cache does the idea mean: a harness working set (A), a sageant hot tier (B), or something in another project I have not read?
2. Is the recorder acceptable (opt-in, local, screened)? It is the first thing that writes a trace of conversations.
3. Who owns the replay harness: a new generic project, the harness lane, or me as a docs-first stub?
