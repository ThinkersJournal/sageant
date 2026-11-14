# Changelog

## 0.0.4

- core slice 1 (ROADMAP row 3, in progress): `MemoryStore` with `put`, `supersede`, `get`, `get_version`, `history`; `Chunk`, `Origin`, `Confidence`, `Error`; own SHA-256 (checked against `sha256sum`); caps; NFC is verified for ASCII only and anything else is refused with `NotNormalised` until the `nfc` helper exists (CONTRACT D4). Slice 2: `SecretScreen` trait run on content and source of every write (`Error::SecretRefused{class}`, never the text); feature `redactant` adds `RegisteredSecrets` over `redactant::Scrubber` (registered secrets only, no detector; no per-put override or audit log yet). Slice 3: typed append-only links (`MemoryStore::link`, `links_from`, `links_to`; `LinkKind`; cycles refused in `part-of` and `derived-from`; `Error::LinkCycle`, `DuplicateLink`) and `MemoryStore::confidence` derived from corroborating or contradicting links. No deletion, retrievers or audit log yet. Still `publish = false`.

## 0.0.3

- docs: `docs/CONTRACT.md` (ROADMAP row 2): contract note, options first (D1-D18), PersistAnt facts as reported at `persistant@b6a37fb`. Draft for the PM; no code.

## 0.0.2

- docs: `docs/INVENTORY.md` (ROADMAP row 1): read-only inventory of `weavatrix-memory` 0.3.3 and `areev-store` 1.13.2, recommendation: fresh. Read at weavatrix-memory `905c933`, areev-store `fddbeb2`.

## 0.0.1

- docs: `ROADMAP.md` (rows 0-5) and the lane charter in `HANDOFF.md`. No code; the crate is still a stub (`publish = false`).
