// SPDX-License-Identifier: MIT OR Apache-2.0
//! Row 4, `sageant-persistant` slice 1: chunks on a durable PersistAnt key-value backend (CONTRACT D17). Written
//! before the code. The in-memory store is the reference: the two must agree on every observable result.

use persistant::registry::Endpoint;
use sageant::{ChunkId, Error, Filter, MemoryStore, NewChunk, Origin, Status, Timestamp};
use sageant_persistant::PersistStore;

fn open(dir: &std::path::Path) -> PersistStore {
    PersistStore::open("sqlite", &Endpoint::Path(dir.join("sageant.db"))).unwrap()
}

fn new(coll: &str, text: &str) -> NewChunk {
    NewChunk::new(coll, "note", text, Origin::User, "cli")
}

#[test]
fn a_put_is_durable_and_reads_back_exactly_what_the_memory_store_holds() {
    let dir = tempfile::tempdir().unwrap();
    let id = {
        let s = open(dir.path());
        s.put(new("notes", "hello"), Timestamp(5)).unwrap().id
    }; // store dropped: closed
    let s = open(dir.path()); // reopened from disk
    let mut m = MemoryStore::new();
    m.put(new("notes", "hello"), Timestamp(5)).unwrap();
    assert_eq!(s.get(&id).unwrap().encode(), m.get(&id).unwrap().encode());
}

#[test]
fn the_same_text_in_the_same_collection_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    s.put(new("notes", "x"), Timestamp(1)).unwrap();
    assert_eq!(
        s.put(new("notes", "x"), Timestamp(2)).unwrap_err(),
        Error::Conflict {
            expected: 0,
            found: 1
        }
    );
    assert!(s.put(new("other", "x"), Timestamp(2)).is_ok()); // another collection is another lineage
}

#[test]
fn validation_errors_match_the_core() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    assert_eq!(
        s.put(new("", "x"), Timestamp(1)).unwrap_err(),
        Error::Invalid("collection")
    );
    assert_eq!(
        s.put(new("c", ""), Timestamp(1)).unwrap_err(),
        Error::Invalid("content")
    );
    assert!(matches!(
        s.put(new("c", "cafe\u{301}"), Timestamp(1)),
        Err(Error::NotNormalised { .. })
    ));
}

#[test]
fn supersede_appends_versions_and_history_is_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let id = s.put(new("n", "one"), Timestamp(1)).unwrap().id;
    let v2 = s
        .supersede(&id, "two", Origin::Model, "m", Timestamp(2))
        .unwrap();
    s.supersede(&id, "three", Origin::User, "cli", Timestamp(3))
        .unwrap();
    assert_eq!(v2.version, 2);
    assert_eq!(s.get(&id).unwrap().content(), "three");
    assert_eq!(s.get_version(&id, 1).unwrap().content(), "one");
    let contents: Vec<String> = s
        .history(&id)
        .unwrap()
        .iter()
        .map(|c| c.content().to_owned())
        .collect();
    assert_eq!(contents, ["one", "two", "three"]);
    assert_eq!(s.get_version(&id, 9).unwrap_err(), Error::NotFound);
    assert_eq!(s.get_version(&id, 0).unwrap_err(), Error::NotFound);
}

#[test]
fn supersede_refuses_unchanged_text_and_unknown_ids() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let id = s.put(new("n", "one"), Timestamp(1)).unwrap().id;
    assert_eq!(
        s.supersede(&id, "one", Origin::User, "cli", Timestamp(2))
            .unwrap_err(),
        Error::Conflict {
            expected: 1,
            found: 1
        }
    );
    let ghost = ChunkId::parse(&"a".repeat(64)).unwrap();
    assert_eq!(s.get(&ghost).unwrap_err(), Error::NotFound);
    assert_eq!(
        s.supersede(&ghost, "x", Origin::User, "cli", Timestamp(2))
            .unwrap_err(),
        Error::NotFound
    );
}

#[test]
fn an_expected_version_guards_against_a_concurrent_writer() {
    let dir = tempfile::tempdir().unwrap();
    let a = open(dir.path());
    let id = a.put(new("n", "one"), Timestamp(1)).unwrap().id;
    // A second writer (another handle on the same database) supersedes first.
    let b = open(dir.path());
    b.supersede_expecting(&id, 1, "two", Origin::User, "cli", Timestamp(2))
        .unwrap();
    // The first writer still thinks version 1 is current: refused, and nothing was written.
    assert_eq!(
        a.supersede_expecting(&id, 1, "stale", Origin::User, "cli", Timestamp(3))
            .unwrap_err(),
        Error::Conflict {
            expected: 1,
            found: 2
        }
    );
    assert_eq!(a.history(&id).unwrap().len(), 2);
    assert_eq!(a.get(&id).unwrap().content(), "two");
}

#[test]
fn listing_agrees_with_the_memory_store_for_every_filter() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let mut m = MemoryStore::new();
    let mut ids = Vec::new();
    for (coll, kind, text, origin, at) in [
        ("notes", "note", "alpha", Origin::User, 10),
        ("notes", "fact", "beta", Origin::Web, 20),
        ("docs", "note", "gamma", Origin::Model, 30),
        ("notes", "note", "delta", Origin::Tool, 40),
    ] {
        let n = || NewChunk::new(coll, kind, text, origin, "src");
        ids.push(p.put(n(), Timestamp(at)).unwrap().id);
        m.put(n(), Timestamp(at)).unwrap();
    }
    p.supersede(&ids[0], "alpha two", Origin::User, "cli", Timestamp(50))
        .unwrap();
    m.supersede(&ids[0], "alpha two", Origin::User, "cli", Timestamp(50))
        .unwrap();
    let filters = [
        Filter::new(),
        Filter::new().collection("notes"),
        Filter::new().collection("notes").kind("note"),
        Filter::new().origin(Origin::Web),
        Filter::new()
            .created_from(Timestamp(20))
            .created_until(Timestamp(40)),
        Filter::new().include_superseded(),
        Filter::new().include_superseded().limit(3),
        Filter::new().limit(0),
        Filter::new().status(Status::Archived),
        Filter::new().include_archived(),
    ];
    for f in filters {
        let from_persist = p.list(&f).unwrap();
        let from_memory = sageant::Reader::list(&m, &f);
        assert_eq!(from_persist, from_memory, "filter {f:?}");
    }
    // Positive control: the listings above are not all empty.
    assert!(!p.list(&Filter::new()).unwrap().is_empty());
}

#[test]
fn a_backend_without_atomic_batches_is_refused_at_open() {
    // The `memory` backend cannot give kv.batch_atomic in persistant 0.18: refused by name, not worked around.
    let err = PersistStore::open("memory", &Endpoint::None).unwrap_err();
    assert!(matches!(err, Error::Backend(_)), "{err:?}");
    assert!(PersistStore::open("no-such-backend", &Endpoint::None).is_err());
}

#[test]
fn a_corrupt_record_on_disk_is_refused_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let s = open(dir.path());
    let id = s.put(new("n", "one"), Timestamp(1)).unwrap().id;
    s.corrupt_for_test(&id, 1, b"not a record".to_vec());
    assert!(matches!(s.get(&id), Err(Error::Corrupt(_))));
    assert!(matches!(s.history(&id), Err(Error::Corrupt(_))));
}
