// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant-persistant` slice 2: links and derived confidence (CONTRACT D6, D7). Written before the code. The
//! in-memory store is the reference: both stores must agree on every result, errors included.

use persistant::registry::Endpoint;
use sageant::{
    ChunkId, Confidence, Error, Filter, LinkKind, MemoryStore, NewChunk, Origin, Timestamp,
};
use sageant_persistant::PersistStore;

fn open(dir: &std::path::Path) -> PersistStore {
    PersistStore::open("sqlite", &Endpoint::Path(dir.join("sageant.db"))).unwrap()
}

/// Two stores holding the same chunks: (persist, memory, ids).
fn pair(dir: &std::path::Path, n: usize) -> (PersistStore, MemoryStore, Vec<ChunkId>) {
    let p = open(dir);
    let mut m = MemoryStore::new();
    let mut ids = Vec::new();
    for i in 0..n {
        // Distinct origins and sources so corroboration can be exercised.
        let (origin, src) = [
            (Origin::User, "cli"),
            (Origin::Web, "w"),
            (Origin::Model, "m"),
        ][i % 3];
        let make = || NewChunk::new("c", "k", &format!("text {i}"), origin, src);
        ids.push(p.put(make(), Timestamp(1)).unwrap().id);
        m.put(make(), Timestamp(1)).unwrap();
    }
    (p, m, ids)
}

fn same_links(p: &PersistStore, m: &MemoryStore, id: &ChunkId) {
    let from_p = p.links_from(id).unwrap();
    let from_m: Vec<_> = m.links_from(id).unwrap().into_iter().cloned().collect();
    assert_eq!(from_p, from_m, "links_from");
    let to_p = p.links_to(id).unwrap();
    let to_m: Vec<_> = m.links_to(id).unwrap().into_iter().cloned().collect();
    assert_eq!(to_p, to_m, "links_to");
}

#[test]
fn links_and_confidence_agree_with_the_memory_store() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, n) = pair(dir.path(), 5);
    let script = [
        (0, LinkKind::Cites, 1),
        (0, LinkKind::CorroboratedBy, 1), // User/cli vs Web/w: independent
        (2, LinkKind::ContradictedBy, 3),
        (3, LinkKind::PartOf, 4),
        (4, LinkKind::Related, 3),
        (3, LinkKind::Related, 4), // mutual `related` is allowed
        (1, LinkKind::DerivedFrom, 0),
    ];
    for (i, (a, kind, b)) in script.into_iter().enumerate() {
        let at = Timestamp(10 + i as u64);
        p.link(&n[a], kind, &n[b], at).unwrap();
        m.link(&n[a], kind, &n[b], at).unwrap();
    }
    for id in &n {
        same_links(&p, &m, id);
        assert_eq!(
            p.confidence(id).unwrap(),
            m.confidence(id).unwrap(),
            "confidence of {id:?}"
        );
    }
    assert_eq!(p.confidence(&n[0]).unwrap(), Confidence::Corroborated);
    assert_eq!(p.confidence(&n[2]).unwrap(), Confidence::Contradicted);
    assert_eq!(p.confidence(&n[4]).unwrap(), Confidence::Unconfirmed);
    // The confidence filter works now and agrees too.
    for c in [
        Confidence::Unconfirmed,
        Confidence::Corroborated,
        Confidence::Contradicted,
    ] {
        assert_eq!(
            p.list(&Filter::new().confidence(c)).unwrap(),
            sageant::Reader::list(&m, &Filter::new().confidence(c)),
            "{c:?}"
        );
    }
    assert!(
        !p.list(&Filter::new().confidence(Confidence::Corroborated))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn every_refusal_matches_the_core_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, n) = pair(dir.path(), 4);
    let ghost = ChunkId::parse(&"b".repeat(64)).unwrap();
    // Set up a -> b -> c in part-of and derived-from.
    for k in [LinkKind::PartOf, LinkKind::DerivedFrom] {
        for (a, b) in [(0, 1), (1, 2)] {
            p.link(&n[a], k, &n[b], Timestamp(2)).unwrap();
            m.link(&n[a], k, &n[b], Timestamp(2)).unwrap();
        }
    }
    p.link(&n[0], LinkKind::Cites, &n[1], Timestamp(2)).unwrap();
    m.link(&n[0], LinkKind::Cites, &n[1], Timestamp(2)).unwrap();
    let cases = [
        (n[0].clone(), LinkKind::Cites, n[0].clone()), // self link
        (n[0].clone(), LinkKind::Cites, n[1].clone()), // duplicate
        (n[2].clone(), LinkKind::PartOf, n[0].clone()), // closes a part-of cycle (via b)
        (n[1].clone(), LinkKind::PartOf, n[0].clone()), // closes a part-of cycle directly
        (n[2].clone(), LinkKind::DerivedFrom, n[0].clone()), // closes a derived-from cycle
        (ghost.clone(), LinkKind::Cites, n[0].clone()), // unknown source
        (n[0].clone(), LinkKind::Cites, ghost.clone()), // unknown target
    ];
    for (a, kind, b) in cases {
        let from_p = p.link(&a, kind, &b, Timestamp(9)).unwrap_err();
        let from_m = m.link(&a, kind, &b, Timestamp(9)).unwrap_err();
        assert_eq!(from_p, from_m, "{a:?} {kind:?} {b:?}");
    }
    // A cycle in a kind that allows cycles is fine, and nothing above left a trace.
    p.link(&n[2], LinkKind::Cites, &n[0], Timestamp(9)).unwrap();
    m.link(&n[2], LinkKind::Cites, &n[0], Timestamp(9)).unwrap();
    for id in &n {
        same_links(&p, &m, id);
    }
}

#[test]
fn a_link_keeps_the_version_it_was_made_on_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let a_id;
    let b_id;
    {
        let p = open(dir.path());
        a_id = p
            .put(
                NewChunk::new("c", "k", "a", Origin::User, "s"),
                Timestamp(1),
            )
            .unwrap()
            .id;
        b_id = p
            .put(
                NewChunk::new("c", "k", "b", Origin::User, "s"),
                Timestamp(1),
            )
            .unwrap()
            .id;
        p.supersede(&a_id, "a2", Origin::User, "s", Timestamp(2))
            .unwrap();
        p.link(&a_id, LinkKind::Cites, &b_id, Timestamp(3)).unwrap(); // made on version 2
        p.supersede(&a_id, "a3", Origin::User, "s", Timestamp(4))
            .unwrap();
    }
    let p = open(dir.path());
    let links = p.links_from(&a_id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].from_version(), 2);
    assert_eq!(links[0].created_at(), Timestamp(3));
    assert_eq!(p.links_to(&b_id).unwrap(), links);
}

#[test]
fn two_writers_cannot_both_add_the_same_link() {
    let dir = tempfile::tempdir().unwrap();
    let a = open(dir.path());
    let n: Vec<ChunkId> = ["x", "y"]
        .iter()
        .map(|t| {
            a.put(NewChunk::new("c", "k", t, Origin::User, "s"), Timestamp(1))
                .unwrap()
                .id
        })
        .collect();
    let b = open(dir.path());
    a.link(&n[0], LinkKind::Cites, &n[1], Timestamp(2)).unwrap();
    assert_eq!(
        b.link(&n[0], LinkKind::Cites, &n[1], Timestamp(3))
            .unwrap_err(),
        Error::DuplicateLink
    );
    assert_eq!(b.links_from(&n[0]).unwrap().len(), 1);
}

#[test]
fn reading_links_of_an_unknown_chunk_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let ghost = ChunkId::parse(&"c".repeat(64)).unwrap();
    assert_eq!(p.links_from(&ghost).unwrap_err(), Error::NotFound);
    assert_eq!(p.links_to(&ghost).unwrap_err(), Error::NotFound);
    assert_eq!(p.confidence(&ghost).unwrap_err(), Error::NotFound);
}
