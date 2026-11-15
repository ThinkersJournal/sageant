# Patent sweep: sageant 0.3.4 @ 31bd649

Date: 2026-11-15; Run by: lane `sageant`, Claude Sonnet 5.5; Method: AI-assisted search and read; **NOT legal advice; NOT a
freedom-to-operate opinion.** Covers the version line 0.3.x (crates `sageant`, `sageant-lexical`, `sageant-persistant`,
`sageant-mcp`, `sageant-cli`) at main 31bd649. Process: `PATENT-SWEEP-PROCESS.md` (portfolio root). Nothing is published yet.

## Ideas swept

What the project does that might be new or unusual (mechanisms, not features):

1. A chunk is immutable text; an edit appends a new version to the chunk's lineage and the old version stays readable. A
   chunk's id is derived from a hash of its collection and its first version's content hash.
2. Provenance is split in two: an `origin` class chosen when the text is written (user, tool, web, agent, model) and a
   `confidence` that is never stored but derived on read from typed links to other chunks (unconfirmed, corroborated,
   contradicted).
3. A chunk counts as corroborated only when a corroborating chunk it links to differs in both origin class and source.
4. `forget` removes every version's content, source, hash and outgoing links in one atomic batch and leaves a minimal
   content-free tombstone; links pointing at it stay and read as removed; chunks derived from it are flagged for review.
5. `purge` removes everything including the tombstone and is available only to a person action (the command line, with a
   typed confirmation); no agent-facing tool can purge.
6. The audit log never holds content, is appended in the same atomic batch as the write it records, and a refused write
   records only a secret class.
7. A secret screen refuses writes containing registered secrets; an override must name the allowed class and a person
   and is itself audited. No notification is sent to anyone.
8. Retrieved content is served to an AI client in a fixed JSON envelope (id, version, collection, kind, origin,
   confidence, status, source, time) whose `content` field is the only place stored text appears; errors carry codes, never
   content.
9. An MCP memory server whose write tools work only on collections named at start-up; reads are on by default; a client
   writes with origin forced to "agent" and is attributed to a configured actor.
10. Responses are capped by an estimated token budget, and every cut is reported in the response.
11. Maintenance (promote, demote, expire) is plan-then-apply: a maintainer returns a plan of steps each with a reason, and
    applying is a separate action.
12. Retrieval is a trait; the default lexical retriever is BM25 in integer-only arithmetic so scores are identical on every
    machine, behind a pluggable text-index trait with an in-memory and a key-value implementation.
13. Cycle refusal in `part-of` and `derived-from` links; per-collection byte caps; NFC-normalised text required.
14. A store on a key-value backend where every write is one atomic batch with a precondition on the state it expects
    (optimistic concurrency), and reads verify a canonical stored record.

## Queries (source, query, date; all 2026-11-15)

Web search restricted to `patents.google.com` (and `patents.justia.com` where noted), then claims read with
`tools/patent-claims.py`.

| # | Idea | Query |
|---|---|---|
| Q1 | 1, 2 | "memory system for large language model agent storing memory entries with source provenance and confidence, superseding versions" (Google Patents) |
| Q2 | 1 | "patent content-addressed immutable versions new version supersedes previous version knowledge entries hash identifier append-only" (Google Patents, Justia) |
| Q3 | 2, 3 | "patent assertion trust score increased when corroborated by multiple independent sources of different type, decreased when contradicted, links between records" (Google Patents, Justia) |
| Q4 | 4, 5, 6 | "delete record leave tombstone without content, audit log, derived records flagged for review, purge requires user confirmation" (Google Patents) |
| Q5 (two queries) | 8 | "retrieved content wrapped with origin label to mitigate prompt injection language model retrieval augmented" (Google Patents); "patent untrusted retrieved document inserted into prompt of language model with provenance metadata field structured to prevent instruction injection" (Google Patents, Justia) |
| Q6 | 9 | "model context protocol server memory tools write permission per collection agent client" (Google Patents) |
| Q7 | 1, 2, 8 | "patent method persistent memory for conversational AI agent storing memories as records with source type and trust, retrieving memories into prompt" (Google Patents, Justia) |
| Q8 | 7, 6 | "screening content for secrets credentials before writing to AI agent memory store, refuse write, override recorded in audit log" (Google Patents, Justia) |
| Q9 | 11 | "memory maintenance for language model agent generating a plan of archive, expire, merge steps with reasons reviewed before applying to memory store" (Google Patents, Justia) |

Un-searched ideas: 10 (token-budget caps), 12 (integer BM25), 13, 14 had no dedicated query; they are ordinary engineering
and are covered only incidentally by Q2/Q9. See Limitations.

## Hits

Claims read with `tools/patent-claims.py` (Google Patents' machine-converted text). "Read" says what I read: the
independent claims in full unless noted, dependent claims only as far as the tool output shows them (the first
1,700 to 2,600 characters of each claim block).

| # | Patent | Title | Assignee | Status (Google Patents) | Claims read | Overlap | Our difference | Verdict |
|---|---|---|---|---|---|---|---|---|
| 1 | US20130226931A1 | Append-only storage and retrieval of information | Cloudtree / Deep Information Sciences | Expired - Fee Related | 1, 44 to 46 (independent) and dependents 2 to 15 | Claim 1: receive information; organise it "for optimal storage and retrieval based on the qualities of a storage medium"; store or present it. Append-only is a dependent claim (5) | We append versions, but we do not organise information based on storage-medium qualities ourselves: layout is a plain ordered key scheme on whatever PersistAnt's key-value backend gives (SQLite). Very broad claim language | clear (as read). Re-read if we write our own on-disk layout or tune it to a medium |
| 2 | US9495373B2 | File versions within content addressable storage | IBM | Active, est. 2034 | 1, 8, 16 | Versions of a stored object identified by content signatures | Claims are block-level hash trees with a reserved signature standing for the other version; we hash whole texts and have no block trees | clear |
| 3 | US6823362B2 | Updating content files among duplicate content servers | HP | Expired - Lifetime | 1, 10 | New file reference per updated version | Requires duplicate content servers and a file-name binding server; we have neither | clear |
| 4 | US8682913B1 | Corroborating facts extracted from multiple sources | Google | Active, est. 2033 | 1, 7, 8, 14, 15, 21 | Status of a fact updated once it meets corroboration requirements across source documents | Claims extract attribute-value pairs from source documents and apply count/score thresholds (and source importance); we extract nothing, count nothing, and derive confidence from typed links whose targets differ in origin class and source | clear. Re-read if we add automatic fact extraction or count thresholds |
| 5 | US9594790B2 | Evaluating claims to update a record from conflicting data sources | Salesforce | Active, est. 2034 | 1 (the only independent claim) | Trust of sources and acceptance of one claim over a record | Claim requires a per-source trust score from the accuracy of that source's previous claims and updating a record with the more trustworthy claim; we keep no source reputation and never overwrite (a new version is appended by the caller) | clear. Re-read if we add source reputation or automatic acceptance |
| 6 | US20200126116A1 | Proof and attestation services for claim verification | Adobe | Active, est. 2039 | 1, 12, 18 | A veracity score computed from agreement between results | Claims answer a request to confirm a statement by searching privileged-access repositories and scoring correlation between results; we have no verify-a-statement request, our confidence is derived from stored links | clear. Re-read if we add a verify tool over external repositories |
| 7 | US20060212931A1 | Trust evaluation systems and methods | MarkMonitor | Abandoned | 1, 21, 22, 24, 25 | Trust score from multiple data sources | Scores online entities (IP address, domain); we score no entity | clear |
| 8 | US5799321A | Replicating deletion information using sets of deleted record IDs | Microsoft | Expired - Lifetime | 1, 8 | Tombstone-like set of deleted record ids | Concerns replication across replicas; we have a single store and no replication | clear. Re-read if we add replication or sync |
| 9 | US20150220751A1 | Methods and systems for deleting requested information | Google | Active, est. 2034 | 1, 11 | Deleting records on request | Claims encrypt log records under a per-record key and delete the key; we do not encrypt, we remove the stored bytes | clear. Re-read if forget becomes key deletion |
| 10 | US20200336311A1 | Two-step data deletion having confirmation hold | EMC | Active, est. 2040 | 1, 10 | A hold then permanent delete after confirmation | Claims encrypt in place with a one-time key, decrypt, block access, verify, run a confirmation analysis, then delete; our purge is a single step gated on a person typing a confirmation, with no encryption or analysis | clear. Re-read if we add encrypt-then-hold deletion |
| 11 | US8943332B2 | Audit-log integrity using redactable signatures | HP | Active, est. 2033 | 1, 12, 17, 21 | Audit log that survives removing records | Claims use commitments, trees and certificates over the audit set; our audit log has no signatures | clear. Re-read if we sign or hash-chain the audit log |
| 12 | US12118471B2 | Mitigation for prompt injection in AI models | Preamble | Active | 1, 17 | Tagging trusted and untrusted input | Claims apply reinforcement learning to tag tokens with trusted/untrusted token vectors inside the model; our envelope is an application-level JSON structure and we train nothing | clear. Re-read if we add any model-side tagging |
| 13 | US20240289863A1 | Adaptive AI-driven conversational agents | Alai Vault / ASL | Pending | 1, 8, 15 | Knowledge base used by an agent | Claims brand-content embeds and indexes feeding a user profile that personalises responses; not what we do | clear |
| 14 | US20250348521A1 | Memory Assistant System | MIT | Pending | 1, 9 | Memory retrieved into a prompt | Claims speech capture with three LLM roles (encoder, query agent, retrieval agent); we have none | clear |
| 15 | US12602470B1 | Containerized execution of secure MCP servers | Airia | Active | 1, 19, 20 | An MCP server | Claims an SSE bridge that instantiates a sandbox per request; we run a stdio server in the user's process | clear. Re-read if we add an HTTP/SSE bridge with sandboxes |
| 16 | US12568091B1 | Policy administration of MCP servers | Airia | Active | 1, 19, 20 | Read/write permissions on an MCP server | Claims an administrator UI with restricted actions, triggers and remedial actions producing a management profile; our write grant is a command-line allow-list of collections | clear. Re-read if we build an admin UI with triggers |
| 17 | US12683991B1 | Insecure MCP server remediation | Airia | Active | 1 (first claim; later claims not read) | MCP | Claims an orchestrator that monitors an MCP server's resource profile and remediates on change; we are a server, not an orchestrator | clear |
| 18 | US12549574B1 | Insecure MCP server remediation | Airia | Active | 1 (first claim; later claims not read) | MCP | Same family; claims an SSE bridge comparing two resource profiles of a server and remediating | clear |
| 19 | US20230063214A1 | Secret detection on computing platform | Google | Active, est. 2042 | 1, 15, 20 | Detect secrets in a data stream by comparison with a repository of secrets | Our `RegisteredSecrets` screen likewise compares text against registered secrets, but claim 1's last step, sending an indication of the detected secrets to the entities associated with them, is absent: we send no notification to anyone | design-around (PM ruling carried forward: no owner notification on the put path). Re-read before adding any notification, alert or webhook on a detected secret |

Seen in search results but **not read** (titles only; not judged either way): US12676749B1 (pre-inference execution
compliance), US9672347B2 and US20120131057A1 (audit log integrity), US9391996B1 (auditable credential retrieval),
US11888981B2, US8370913B2, US20200389521A1 (Holochain), US20130226931A1's family members, and the published applications
and papers returned by the searches that were not patents. A hit I could not read is `unread`, never `clear`.

## Needs review (to the PM)

None. No claim read on a SageAnt mechanism. Hit 19 is the one design-around and it is the earlier PM ruling carried forward.

## Limitations

- AI-assisted. The searches were web searches that return a handful of results per query (US-only index); they are not a
  patent database search. No USPTO full-text, Espacenet or WIPO search was run. Patent families and continuation claims were
  not searched.
- The result lists came back mostly with papers and blog posts for the agent-memory ideas, and only a few patents per
  query. A short result list is not evidence that nothing exists; it is a search that found few patents.
- Ideas 10, 12, 13 and 14 had no dedicated query (see above). Idea 11 (plan-then-apply maintenance) returned papers only.
- I read claim text through `tools/patent-claims.py`; Google Patents text is machine-converted, and I read dependent claims
  only as far as the tool printed them. Status and expiry are Google Patents' estimates, not the official record.
- "clear" means: on the independent claims as read, at least one required element is absent from what we do, for the
  reason given. It is not a statement that the patent is invalid or that we infringe nothing; a lawyer's freedom-to-operate
  review is a separate, later purchase.
- Re-sweep when a new family of capability is added (replication or sync, encryption-based forget, a signed audit log,
  model-side tagging, an HTTP bridge, an admin UI, notification on detected secrets, source reputation) and at least yearly
  after release.
