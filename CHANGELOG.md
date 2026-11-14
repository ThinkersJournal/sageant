# Changelog

## 0.0.7

- core slice 6 (ROADMAP row 3, in progress): the `Reader` trait (get, get_version, history, links_from, links_to, confidence, `list`) implemented by `MemoryStore`, object-safe so a retriever can take `&dyn Reader`; `Filter` (collection, kind, origin, status, derived confidence, created from/until, `include_archived`, `include_superseded`, `limit`), ordered by `(ChunkId` ascending, version descending). No tag filter (the model has no tags); confidence is an exact match, not a minimum. Not yet: `Writer` and `Actor`, nfc feature, retrievers.

## 0.0.6

- core slice 5 (ROADMAP row 3, in progress): audited per-write secrets override. `SecretOverride::allow_class(class, by)` with `put_overriding` and `supersede_overriding`: covers one write and only the class the screen flagged; the audit log gets `Action::Overridden` (`secret-override:<class>`, actor = who allowed it, never the text); an override naming nobody is `Invalid`. Plain `put` and `supersede` are unchanged. Not yet: Reader/Writer traits, nfc feature, retrievers.

## 0.0.5

- core slice 4 (ROADMAP row 3, in progress): `archive`, `forget` (content, source, hash and outgoing links removed; minimal `Tombstone`; returns `Removed::Unretrievable`, there is no "erased" until PersistAnt row 38 passes), `purge` (needs `PersonAuthority`; also removes the tombstone), `Error::Forgotten`, links to removed targets read `LinkTarget::Removed`, derived chunks flagged `needs_review`, removed evidence ignored by `confidence`, and a content-free audit log (`audit()`; secret refusals are logged as `Refused` with the class only). Not yet: per-put secrets override, retrievers, Reader/Writer traits, nfc feature.

## 0.0.4

- core slice 1 (ROADMAP row 3, in progress): `MemoryStore` with `put`, `supersede`, `get`, `get_version`, `history`; `Chunk`, `Origin`, `Confidence`, `Error`; own SHA-256 (checked against `sha256sum`); caps; NFC is verified for ASCII only and anything else is refused with `NotNormalised` until the `nfc` helper exists (CONTRACT D4). Slice 2: `SecretScreen` trait run on content and source of every write (`Error::SecretRefused{class}`, never the text); feature `redactant` adds `RegisteredSecrets` over `redactant::Scrubber` (registered secrets only, no detector; no per-put override or audit log yet). Slice 3: typed append-only links (`MemoryStore::link`, `links_from`, `links_to`; `LinkKind`; cycles refused in `part-of` and `derived-from`; `Error::LinkCycle`, `DuplicateLink`) and `MemoryStore::confidence` derived from corroborating or contradicting links. No deletion, retrievers or audit log yet. Still `publish = false`.

## 0.0.3

- docs: `docs/CONTRACT.md` (ROADMAP row 2): contract note, options first (D1-D18), PersistAnt facts as reported at `persistant@b6a37fb`. Draft for the PM; no code.

## 0.0.2

- docs: `docs/INVENTORY.md` (ROADMAP row 1): read-only inventory of `weavatrix-memory` 0.3.3 and `areev-store` 1.13.2, recommendation: fresh. Read at weavatrix-memory `905c933`, areev-store `fddbeb2`.

## 0.0.1

- docs: `ROADMAP.md` (rows 0-5) and the lane charter in `HANDOFF.md`. No code; the crate is still a stub (`publish = false`).
