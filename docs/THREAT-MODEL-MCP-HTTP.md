# Threat model: sageant MCP over HTTP (ROADMAP row E)

Status: **draft for a CireSnave decision. Nothing is built.** Written 2026-11-16 by the `sageant` lane (Claude Sonnet 5.5) at the
PM's request, for `sageant` 0.3.19. This is an AI-assisted analysis from the design and the code, not a penetration test or a review
of the MCP specification's current authorisation text: **before anything is built, read the current MCP spec's transport and
authorisation sections and re-check this document against them.**

## 1. What exists, and what is being asked

Today `sageant serve` speaks MCP on standard input and output. It is reachable only by the process that started it (the AI client's
parent process), runs as that user, and has no network listener. Reads are on; writes need `--allow-write <collection>`; there is no
purge tool; every write is attributed to a fixed actor (`mcp/stdio`) with origin `Agent`.

Row E asks for the same tools over **HTTP** (rmcp has a `transport-streamable-http-server` feature), so a client on another process,
machine or phone could use the store. That adds a listener, a way to say who is calling, and everything that follows from both.

## 2. Assets

| Asset | Why it matters |
|---|---|
| Chunk content and its provenance | Private notes; anything stored by mistake (the secret screen only catches secrets someone registered); what a client reads is data an agent may act on |
| Integrity of the store | A poisoned chunk is read later by other clients and people (stored prompt injection); confidence and origin labels are only as good as who may write |
| Deletion | `forget` is irreversible in practice (unretrievable, and only on `sqlite-secure` erased); a stolen right to forget is a right to destroy |
| Availability and disk | `put` grows the store; a loop of requests can fill the disk or starve the machine |
| The audit log | The only record of who did what; must stay content-free and trustworthy |
| The host | The process runs with the user's rights; a parsing or logic bug in a network-facing service is more serious than in a stdio child |

## 3. Who could attack, and from where

1. **A web page in the user's browser** (the classic local-service risk: DNS rebinding and cross-origin requests to `127.0.0.1`).
2. **Another user or process on the same machine** (reads a token from a file or process list; connects to the port).
3. **A peer on the same network** (if the listener is not loopback-only).
4. **A holder of a leaked or stolen token** (backup, shell history, a log, a screenshot, an agent that echoed it).
5. **The AI client itself**: semi-trusted, prompt-injectable; can be made to call tools it should not, or to leak a token.
6. **A chunk author**: content with origin Web, Tool or Agent that tries to instruct whoever reads it.
7. **Anyone on the internet**, only if the service is deliberately exposed.

## 4. Threats and what the current design already does

| # | Threat | Today (stdio) | Over HTTP, needs |
|---|---|---|---|
| T1 | Unauthenticated callers | Not possible: no listener | Every request authenticated; no anonymous mode; refuse and log (content-free) |
| T2 | Stolen or leaked token | n/a | **Reads are on by default** (`sageant_get/list/search/history/links` work on every collection), so a stolen token reads everything. It can write and forget in every granted collection. It cannot purge or change grants. See section 5 |
| T3 | DNS rebinding / cross-origin browser requests to a loopback listener | n/a | Validate `Host` and `Origin`; reject any browser-looking request that is not from an allowed origin; bind to loopback only unless told otherwise |
| T4 | Eavesdropping or tampering on the wire | n/a | Loopback needs none; anything else needs TLS (rustls) with a real certificate story, which is a project of its own |
| T5 | Token passthrough and confused deputy | n/a | sageant never forwards a client token anywhere; tokens are per client and per scope, never shared with other services |
| T6 | Resource exhaustion | Responses are budgeted by tokens; requests are not limited (one local client) | Maximum request size, concurrency limit, rate limit per token, a quota on `put` bytes per token per day, timeouts |
| T7 | Read-everything default | Acceptable: one local client | For HTTP, reads need an explicit grant per collection too (a **change from stdio's default**) |
| T8 | Weak attribution | Fixed actor `mcp/stdio` | Actor is the token's name (`mcp/http:<name>`); failed authentications are logged by token prefix or source address only, never content |
| T9 | Stored prompt injection | Envelope (content only in the `content` field, origin and confidence labelled), no link following | Unchanged, but with more writers the labels matter more; HTTP writes stay origin `Agent`, never `User` |
| T10 | Bigger attack surface and dependency tree | Small | An HTTP stack (hyper or axum, tower, possibly TLS) enters the build; `cargo deny`, MSRV and the install size must be re-checked; the server is a long-lived network service, not a child process |
| T11 | Remote irreversible deletion | A local client with a write grant can forget | The HTTP profile **omits `sageant_forget` unless a token is explicitly granted it**; `archive` is reversible from the command line, `forget` is not |
| T12 | Secrets written by a remote client | Secret screen applies only if configured | HTTP requires the secret screen to be on when writes are enabled (refuse to start otherwise) |
| T13 | Token storage | n/a | Tokens are random 256-bit values, shown once, stored hashed (not in the store's database), compared in constant time, revocable, with an optional expiry |
| T14 | Session fixation or hijack if sessions are stateful | n/a | Prefer a stateless mode; if sessions are used, ids are unguessable and bound to the token |

## 5. What a stolen token could do, by option

| Capability | Option B (loopback + token) token with read grant on `notes` | same, plus write grant on `notes` | same, plus forget grant |
|---|---|---|---|
| Read `notes` | yes | yes | yes |
| Read other collections | no (per-collection reads) | no | no |
| Add or supersede chunks in `notes` (origin Agent) | no | yes (bounded by quota) | yes |
| Archive chunks in `notes` | no | yes | yes |
| Forget chunks in `notes` (irreversible) | no | no | **yes** |
| Purge anything | never | never | never |
| Change grants, mint tokens, read other tokens | never (done by the person on the command line) | never | never |
| Fill the disk | no | up to the quota | up to the quota |

The residual risk of a stolen write token is stored prompt injection and disk use; of a forget token, data loss. Neither reaches the
host, because the service exposes only these tools.

## 6. Options

- **A. Do not build.** Keep stdio. A remote client reaches `sageant serve` through a stdio proxy the user already trusts (SSH, a
  container exec). Zero new attack surface. The cost is that a phone or a hosted agent cannot connect directly.
- **B. Loopback-only HTTP with per-client tokens** (the controls above: auth always on, `Host`/`Origin` checks, per-collection read and
  write grants, no forget by default, size and rate limits and quotas, hashed tokens, content-free audit, secret screen required).
  Remote access then goes through a tunnel the user chooses. Moderate work; reuses the existing `Tools` logic, which is already tested
  without a transport.
- **C. Network-facing HTTP with TLS and an authorisation server (OAuth).** Everything in B plus certificates, an identity provider,
  token lifetimes and refresh, and a public attack surface. A project, not a feature; it needs its own review.

## 7. Recommendation

**A now.** If a concrete need appears, **B**, with every control in section 4 treated as a release condition and the refutation tests
below. **C** only with a stated user, an owner for certificate and token operations, and a separate review.

## 8. Decisions needed from CireSnave

1. Is there a real client that cannot use stdio today, and which? (This decides whether B is wanted at all.)
2. If B: may remote clients ever hold a **forget** grant, or only read, write and archive?
3. Reads per collection explicit for HTTP (a change from stdio's read-everything default): yes?
4. Where do tokens live and who mints them: a `sageant token` command on the command line only?
5. Is a tunnel (SSH, a VPN, a Cloudflare tunnel) acceptable as the way to reach a loopback listener, or is a TLS listener wanted (C)?

## 9. If B is approved: how it would be tested (red first)

Each control gets a refutation test that fails against a build without it:

- a request with no token, a wrong token, an expired token and a revoked token is refused, with no content in the response or the log;
- a request with a foreign `Origin`, or a `Host` that is not the loopback name, is refused (the rebinding case);
- a read-only token cannot `put`, `supersede`, `link`, `archive` or `forget`; a token without a forget grant gets `not_authorised`; no
  token can purge; a token cannot read a collection it has no read grant for;
- a request over the size limit, over the rate limit, or over the `put` quota is refused with a stated code;
- the service refuses to start with writes enabled and the secret screen off, and with a non-loopback bind and no TLS;
- tokens are not in the store's database or the audit log, and the comparison is constant-time (a unit test on the helper);
- the existing D14 envelope test (hostile content stays in `content`) runs over HTTP too;
- `cargo deny`, the MSRV, the `package_metadata` test, the install-smoke job and a mutation run cover the new crate or feature.

## 10. Not covered here

The MCP specification's current authorisation requirements (to be read before building); the legal or privacy treatment of data a
remote agent may see; denial of service at the network layer; the host's own hardening; physical access. A real review by someone other
than the author would be needed before exposing any of this beyond loopback.
