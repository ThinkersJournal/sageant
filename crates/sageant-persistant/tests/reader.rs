// SPDX-License-Identifier: MIT OR Apache-2.0
//! `PersistStore` is a `sageant::Reader` (slice 3), so every `Retriever`, `Promoter` and `Maintainer` works on
//! it unchanged. Written before the impl.

use persistant::registry::Endpoint;
use sageant::{ExpireMaintainer, Filter, Maintainer, NewChunk, Origin, Reader, Timestamp};
use sageant_persistant::PersistStore;

#[test]
fn a_persist_store_is_a_reader_and_a_maintainer_can_plan_over_it() {
    let dir = tempfile::tempdir().unwrap();
    let p = PersistStore::open("sqlite", &Endpoint::Path(dir.path().join("s.db"))).unwrap();
    let old = p
        .put(
            NewChunk::new("c", "k", "old", Origin::User, "cli"),
            Timestamp(10),
        )
        .unwrap();
    p.put(
        NewChunk::new("c", "k", "new", Origin::User, "cli"),
        Timestamp(90),
    )
    .unwrap();
    let r: &dyn Reader = &p;
    assert_eq!(r.get(&old.id).unwrap().content(), "old");
    assert_eq!(r.list(&Filter::new()).unwrap().len(), 2);
    let plan = ExpireMaintainer::new(Timestamp(50)).plan(r).unwrap();
    assert_eq!(plan.steps().len(), 1);
}
