# sageant-persistant

A durable [sageant](https://crates.io/crates/sageant) store on a PersistAnt key-value backend (SQLite).

- `PersistStore`: chunks and versions, links and derived confidence, archive, forget, purge, tombstones, an audit log,
  byte caps and a secret screen. It implements `sageant::Reader` and `sageant::Writer`. Every write is one atomic batch
  with a precondition on the state it expects, so two writers can never both win; every read verifies the stored record.
- `PersistTextIndex`: a durable `sageant_lexical::TextIndex` for BM25 search, kept by PersistAnt's text-index layer.

The backend must give atomic batches and ordered scans; PersistAnt refuses one that cannot, by name, when you open it.
What "forgotten" means depends on the backend: see the table in the main README (<https://github.com/ThinkersJournal/sageant#removing-things>), which is the single source. `Chosen::secure_delete()` and `backend_secure_delete(id)` say which you have; `storage_needs()` prefers `storage.secure_delete`.

```rust,no_run
use persistant::registry::Endpoint;
use sageant::{NewChunk, Origin, Timestamp};
use sageant_persistant::PersistStore;

let store = PersistStore::open("sqlite", &Endpoint::Path("sageant.db".into())).unwrap();
let chunk = store
    .put(NewChunk::new("notes", "note", "hello", Origin::User, "example"), Timestamp(1))
    .unwrap();
assert_eq!(store.get(&chunk.id).unwrap().content(), "hello");
```

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your
option.
