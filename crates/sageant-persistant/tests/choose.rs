// SPDX-License-Identifier: MIT OR Apache-2.0
//! Storage is chosen through PersistAnt, never hard-wired (the portfolio rule): the store declares what it needs and
//! PersistAnt's `providers` ranks the backends that have it. Written before the code.

use persistant::registry::Endpoint;
use sageant::{NewChunk, Origin, Timestamp};
use sageant_persistant::{KvTextIndex, PersistStore, storage_needs};

#[test]
fn the_declared_need_names_every_capability_the_store_relies_on() {
    let ids: Vec<String> = storage_needs()
        .ids()
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect();
    for want in [
        "kv.get",
        "kv.put",
        "kv.delete",
        "kv.ordered_scan_values",
        "kv.batch_atomic.rollback",
        "kv.batch_atomic.concurrent_read",
        "kv.batch_atomic.optimistic_sequence",
        "kv.batch_atomic.absent_race",
    ] {
        assert!(ids.iter().any(|i| i == want), "{want} missing from {ids:?}");
    }
}

#[test]
fn a_path_endpoint_opens_the_backend_persistant_ranks_first_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let (p, chosen) = PersistStore::open_chosen(&Endpoint::Path(dir.path().join("s.db"))).unwrap();
    assert!(!chosen.backend().is_empty());
    assert!(
        !chosen.why().is_empty(),
        "the choice comes with its reasons"
    );
    let c = p
        .put(
            NewChunk::new("c", "k", "hello", Origin::User, "t"),
            Timestamp(1),
        )
        .unwrap();
    drop(p);
    // The recorded id reopens the same data explicitly, which is what a program stores in its configuration.
    let again =
        PersistStore::open(chosen.backend(), &Endpoint::Path(dir.path().join("s.db"))).unwrap();
    assert_eq!(again.get(&c.id).unwrap().content(), "hello");
}

#[test]
fn the_text_index_is_chosen_the_same_way() {
    let dir = tempfile::tempdir().unwrap();
    let (_i, chosen) = KvTextIndex::open_chosen(&Endpoint::Path(dir.path().join("i.db"))).unwrap();
    assert!(!chosen.backend().is_empty());
}

#[test]
fn an_endpoint_no_eligible_backend_fits_is_refused_with_the_reason() {
    // "Nothing" is the endpoint of the in-memory backend, which has no recorded atomic batches.
    let e = PersistStore::open_chosen(&Endpoint::None).unwrap_err();
    let text = e.to_string();
    assert!(text.to_lowercase().contains("backend"), "{text}");
}
