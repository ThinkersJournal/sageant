# Prior-art survey: eviction, decay, worth and tiering (ROADMAP X2, C1)

Date: 2026-11-21; Run by: lane `sageant`, Claude Sonnet 5.5. **Method: written from the published literature and
well-known system documentation as remembered; no web fetch, no third-party code or text read or copied, no check
of any citation against its source in this run.** Treat every name below as a pointer to look up, not a verified
quotation. **Not a patent search and not legal advice** (the patent position is `PATENT-SWEEP.md`). The three excluded
repositories in `HANDOFF.md` (`eidetic-engine`, `cyberbrain-core`, `velesdb-memory`) were not read.

Purpose: the owed survey before any X2 or C1 code (ROADMAP X2, C1; design note `DESIGN-PREWARM-AND-EVICT.md` s9). It
asks what is already known about the three decisions the design makes, so that we reuse ideas that are free to use and
do not reinvent a known failure.

## 1. Recency and frequency eviction (the "LRU" end)

| Family | Idea | Relevance to us |
|---|---|---|
| LRU, CLOCK | evict the least recently used; CLOCK approximates it with one bit and a sweep | the ruled base order for demotion (C1: LRU and worth). A single `u/<id>` last-use record is exactly what LRU needs |
| LFU | evict the least often used | needs a counter per chunk; the design rules out a read counter on read-only reads (design note 2.2), so plain LFU is out |
| 2Q, ARC, LIRS, SLRU | scan-resistant: a chunk seen once sits in a probationary segment and cannot flush the hot set | **the known failure of plain LRU is a one-off bulk read (an ingest, an export, a wide query) flushing everything useful.** C1 should say how a bulk `list`/`search --all` is kept from counting as use (design note s4 already says only a chunk returned in a query result is use; confirm a wide query result still counts) |
| TinyLFU / W-TinyLFU | admission filter: a newcomer enters only if it beats the victim | relevant to "a full store refuses writes" (ruled): refusal is the blunt form; admission by worth is the refined form, and is NOT ruled, so do not add it |
| Redis `maxmemory` policies | `allkeys-lru`, `volatile-lru`, `allkeys-lfu`, `noeviction` (refuse writes) | `noeviction` is the behaviour CireSnave ruled for the full last store; the "volatile" idea (only evict items that carry an expiry) is the same shape as "never touch pinned" |
| Memcached slab LRU, ZFS ARC | segmented LRU with hot/warm/cold; size-aware | confirms segmenting is normal; not needed for v1 |

Take: LRU by `u/<id>` is a sound, well-understood default. The one lesson to adopt is scan resistance; it costs nothing
if the use rule already excludes by-id and listing reads.

## 2. Value-aware ("worth") eviction

| Family | Idea | Relevance |
|---|---|---|
| GreedyDual-Size, GDSF | evict by (cost to refetch x frequency) / size, with an ageing term | **closest published relative of our worth formula**: cost-to-refetch is our refetchability, size is store pressure, ageing is recency. The paper-level result is that recency-only and size-blind policies lose on non-uniform costs |
| Cost-aware web caching, Landlord | same family; credit that decays and is topped up on use | a way to get ageing without a timer: a counter relative to a global "inflation" value |
| Hierarchical storage management (HSM), tiered object stores (S3 lifecycle, Ceph tiering) | demote by age and last access to cheaper media, promote on access | exactly the C1 ladder; typical rules are age plus last-access thresholds, and promotion on a hit, as ruled (Q13: one up, once per window) |
| Generational GC | young objects die young; promote survivors | supports the "promote one store up, once per window" damping; also warns about thrash at the boundary |
| Hysteresis / high and low watermarks | start evicting at a high mark, stop at a low one | the ruled 75/85/95 marks with a recovery margin are this; without hysteresis a store at the mark oscillates |

Take: our `worth-v1` and the C1 worth-v2 are already a GreedyDual-style score with fixed weights. The literature
supports fixed weights chosen by the owner of the workload; it does not support learning them without a trace, which is
why dropping C2 (recorder, replay) is consistent. If a future slice wants to tune weights it needs the replay harness
first; that sentence belongs in C2, not here.

## 3. Memory and forgetting models (the "decay" end)

| Source | Idea | Relevance |
|---|---|---|
| Ebbinghaus forgetting curve; power-law forgetting | retention falls roughly as a power of time since last use | a smooth recency term; the power law has a long tail, which suits "last store forgets only when full" |
| ACT-R base-level activation | activation = ln(sum of t_i^-d) over past uses | uses EVERY past use, so needs the access history the design dropped; last-use-only (Q11) is the cheap approximation, and ACT-R is the cite for what is lost |
| Spaced-repetition schedulers (SM-2 and descendants) | interval grows with each successful recall | a use should perhaps count more when the gap was long; not in v1 |
| LLM agent memory systems (tiered "main/archive" context, summarise-then-archive, importance-scored retrieval as in generative-agent memory work) | recency + importance + relevance score for retrieval | the three-term score is a published pattern; the importance term there is model-assigned, ours is derived (trust, origin, pins), which is the safer, deterministic choice |

Take: nothing here requires a learned model. The deterministic recency-plus-importance score is the mainstream
pattern; ours differs by being explainable per chunk and by deriving importance from provenance.

## 4. Where we differ, and what must be checked before code

1. **Forgetting is deletion of content with a tombstone, not eviction of a copy.** In a cache the origin still holds the
   data; here the last store is the only copy unless the chunk is refetchable. The cache literature assumes the origin
   exists, so its policies are cost-of-miss models, and ours is a cost-of-loss model. That is why refetchability sits
   inside worth (design note s5) and why a dry-run plan is mandatory (X2).
2. **Trust and provenance survive demotion byte-identical and vanish on delete.** No cache paper covers this; it is ours
   to specify and test (mutation run 10).
3. **A read recording a use under a read-only grant.** Solved by ruling (the store's own authority); the closest prior
   shape is an access-time (`atime`) update on a read-only mount, which filesystems handle with `relatime`, i.e. update
   at most once per window. **Adopt the same: at most one `u/<id>` write per chunk per window**, which also bounds write
   amplification on the persistent store.
4. **Eviction needing a size.** PersistAnt cannot report size (design note, PersistAnt answer), so marks are computed by
   scan or file size; the cost of the scan per window must be bounded and measured before it ships.

## 5. Proposed additions to the design (for the owner to accept or refuse; nothing is changed by this file)

| # | Proposal | From |
|---|---|---|
| S1 | A wide or bulk query result must not mark every chunk it returns as used past a cap per query, or one broad search flushes the hot set | scan resistance (2Q, ARC) |
| S2 | At most one `u/<id>` write per chunk per window | `relatime` |
| S3 | Keep hysteresis: demote down to a low mark, not to the trigger mark | watermarks |
| S4 | State in C2 that weight tuning needs the replay harness, so v1 weights stay fixed | GreedyDual, no-trace argument |

These are questions, not rulings: S1 and S2 touch the use rule CireSnave defined, so they go to him through the PM, not
into code.

## 6. Provenance and licence

Everything above is a description of published ideas and general system behaviour in my own words; no source file,
paper text, or code was copied, and no licence or credit obligation arises from this file. If a later slice
implements a named algorithm (GDSF, 2Q, ARC), look up the original paper and any patent on it first (ARC in
particular was patented by IBM at the time of publication; check expiry and status, do not assume), record the
result in `PATENT-SWEEP.md`, and cite the paper in the code.
