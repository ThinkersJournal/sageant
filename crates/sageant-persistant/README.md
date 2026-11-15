# sageant-persistant

A durable [sageant](https://crates.io/crates/sageant) store on a PersistAnt key-value backend (SQLite).

- `PersistStore`: chunks and versions, links and derived confidence, archive, forget, purge, tombstones, an audit log,
  byte caps and a secret screen. It implements `sageant::Reader` and `sageant::Writer`. Every write is one atomic batch
  with a precondition on the state it expects, so two writers can never both win; every read verifies the stored record.
- `KvTextIndex`: a durable `sageant_lexical::TextIndex` for BM25 search, kept in the same kind of backend.

The backend must give atomic batches and ordered scans; PersistAnt refuses one that cannot, by name, when you open it.
"Forgotten" means unretrievable, not erased: a backend may keep old pages until it is compacted.

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
