# Third-party licences

Dependencies of the published crate, by feature. Core has none. Recorded per the PM's request at 0.0.9; `deny.toml` enforces the allow list.

| Dependency | Feature | Licence | Recorded |
|---|---|---|---|
| `redactant` 0.1.2 | `redactant` | as published by the RedactAnt lane | 0.0.4 |
| `unicode-normalization` 0.1.26 | `nfc` | `(MIT OR Apache-2.0) AND Unicode-3.0` | 0.0.9 (PM) |

`Unicode-3.0` is in the `deny.toml` allow list. Never read or copy `eidetic-engine`, `cyberbrain-core` or `velesdb-memory` (see HANDOFF).
