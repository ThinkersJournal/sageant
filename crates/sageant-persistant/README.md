# sageant-persistant

A durable [sageant](https://crates.io/crates/sageant) store on a PersistAnt key-value backend (SQLite).

- `PersistStore`: chunks and versions, links and derived confidence, archive, forget, purge, tombstones, an audit log,
  byte caps and a secret screen. It implements `sageant::Reader` and `sageant::Writer`. Every write is one atomic batch
  with a precondition on the state it expects, so two writers can never both win; every read verifies the stored record.
- `KvTextIndex`: a durable `sageant_lexical::TextIndex` for BM25 search, kept in the same kind of backend.

The backend must give atomic batches and ordered scans; PersistAnt refuses one that cannot, by name, when you open it.
"Forgotten" means unretrievable, and on a backend with `storage.secure_delete` (PersistAnt's `sqlite-secure`, which `storage_needs()` prefers) it also means erased from the database file: freed bytes are zeroed, and the tests read the raw files back to check. On any other backend a freed page may keep the old bytes until the file is compacted. `Chosen::secure_delete()` and `backend_secure_delete(id)` say which you have. Copies kept by a file system, snapshot or backup are never covered.

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
