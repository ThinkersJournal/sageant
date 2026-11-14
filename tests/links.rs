// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 3 of ROADMAP row 3: typed links (CONTRACT D7) and derived confidence (D6). Written before the code.

use sageant::{ChunkId, Confidence, Error, LinkKind, MemoryStore, NewChunk, Origin, Timestamp};

fn put(s: &mut MemoryStore, text: &str, origin: Origin, source: &str) -> ChunkId {
    s.put(
        NewChunk::new("notes", "note", text, origin, source),
        Timestamp(1),
    )
    .unwrap()
    .id
}

fn three() -> (MemoryStore, ChunkId, ChunkId, ChunkId) {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "a", Origin::User, "cli");
    let b = put(&mut s, "b", Origin::User, "cli");
    let c = put(&mut s, "c", Origin::User, "cli");
    (s, a, b, c)
}

#[test]
fn link_is_recorded_on_the_source_version_and_read_both_ways() {
    let (mut s, a, b, _) = three();
    s.supersede(&a, "a2", Origin::User, "cli", Timestamp(2))
        .unwrap(); // a is now version 2
    s.link(&a, LinkKind::Cites, &b, Timestamp(5)).unwrap();
    let out = s.links_from(&a).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind(), LinkKind::Cites);
    assert_eq!(out[0].from(), &a);
    assert_eq!(out[0].from_version(), 2); // belongs to the version current at link time
    assert_eq!(out[0].to(), &b);
    assert_eq!(out[0].created_at(), Timestamp(5));
    let back = s.links_to(&b).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].from(), &a);
    assert!(s.links_from(&b).unwrap().is_empty());
}

#[test]
fn links_survive_a_later_supersede_of_the_source() {
    let (mut s, a, b, _) = three();
    s.link(&a, LinkKind::Related, &b, Timestamp(2)).unwrap();
    s.supersede(&a, "a2", Origin::User, "cli", Timestamp(3))
        .unwrap();
    let out = s.links_from(&a).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].from_version(), 1); // append-only: still attached to the version it was made on
}

#[test]
fn unknown_endpoint_is_not_found_and_adds_nothing() {
    let (mut s, a, _, _) = three();
    let ghost = MemoryStore::new()
        .put(
            NewChunk::new("x", "note", "ghost", Origin::User, "cli"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    assert_eq!(
        s.link(&a, LinkKind::Cites, &ghost, Timestamp(2))
            .unwrap_err(),
        Error::NotFound
    );
    assert_eq!(
        s.link(&ghost, LinkKind::Cites, &a, Timestamp(2))
            .unwrap_err(),
        Error::NotFound
    );
    assert!(s.links_from(&a).unwrap().is_empty());
}

#[test]
fn self_link_and_duplicate_link_are_refused() {
    let (mut s, a, b, _) = three();
    assert_eq!(
        s.link(&a, LinkKind::Related, &a, Timestamp(2)).unwrap_err(),
        Error::LinkCycle
    );
    s.link(&a, LinkKind::Cites, &b, Timestamp(2)).unwrap();
    assert_eq!(
        s.link(&a, LinkKind::Cites, &b, Timestamp(3)).unwrap_err(),
        Error::DuplicateLink
    );
    // A different kind between the same pair is a different link.
    assert!(s.link(&a, LinkKind::Related, &b, Timestamp(3)).is_ok());
    assert_eq!(s.links_from(&a).unwrap().len(), 2);
}

#[test]
fn part_of_and_derived_from_refuse_cycles_but_related_allows_them() {
    let (mut s, a, b, c) = three();
    s.link(&a, LinkKind::PartOf, &b, Timestamp(2)).unwrap();
    s.link(&b, LinkKind::PartOf, &c, Timestamp(2)).unwrap();
    assert_eq!(
        s.link(&c, LinkKind::PartOf, &a, Timestamp(2)).unwrap_err(),
        Error::LinkCycle
    ); // 3-cycle
    assert_eq!(
        s.link(&b, LinkKind::PartOf, &a, Timestamp(2)).unwrap_err(),
        Error::LinkCycle
    ); // 2-cycle
    s.link(&a, LinkKind::DerivedFrom, &b, Timestamp(2)).unwrap();
    assert_eq!(
        s.link(&b, LinkKind::DerivedFrom, &a, Timestamp(2))
            .unwrap_err(),
        Error::LinkCycle
    );
    // Kinds are checked separately: a part-of edge does not make a derived-from edge a cycle.
    assert!(s.link(&c, LinkKind::DerivedFrom, &a, Timestamp(2)).is_ok());
    // `related` may be mutual.
    s.link(&a, LinkKind::Related, &b, Timestamp(2)).unwrap();
    assert!(s.link(&b, LinkKind::Related, &a, Timestamp(2)).is_ok());
    // The refused links were not added.
    assert_eq!(s.links_from(&c).unwrap().len(), 1);
}

#[test]
fn links_are_listed_in_a_fixed_order_independent_of_insertion_order() {
    let (mut s1, a, b, c) = three();
    s1.link(&a, LinkKind::Related, &c, Timestamp(2)).unwrap();
    s1.link(&a, LinkKind::Cites, &b, Timestamp(3)).unwrap();
    s1.link(&a, LinkKind::Cites, &c, Timestamp(4)).unwrap();
    let (mut s2, a2, b2, c2) = three();
    s2.link(&a2, LinkKind::Cites, &c2, Timestamp(4)).unwrap();
    s2.link(&a2, LinkKind::Cites, &b2, Timestamp(3)).unwrap();
    s2.link(&a2, LinkKind::Related, &c2, Timestamp(2)).unwrap();
    let key = |s: &MemoryStore, id: &ChunkId| {
        s.links_from(id)
            .unwrap()
            .iter()
            .map(|l| (l.kind(), l.to().clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(key(&s1, &a), key(&s2, &a2));
    // Order is by kind first, then target id.
    let kinds: Vec<_> = key(&s1, &a).iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        vec![LinkKind::Cites, LinkKind::Cites, LinkKind::Related]
    );
}

#[test]
fn corroboration_needs_an_independent_origin_and_source() {
    let mut s = MemoryStore::new();
    let claim = put(&mut s, "claim", Origin::Model, "gpt-x");
    assert_eq!(s.confidence(&claim).unwrap(), Confidence::Unconfirmed); // new = one strike
    let same_origin = put(&mut s, "echo", Origin::Model, "other-model");
    s.link(&claim, LinkKind::CorroboratedBy, &same_origin, Timestamp(2))
        .unwrap();
    assert_eq!(s.confidence(&claim).unwrap(), Confidence::Unconfirmed); // same origin class: not independent
    let independent = put(&mut s, "again", Origin::Web, "https://z.example");
    s.link(&claim, LinkKind::CorroboratedBy, &independent, Timestamp(3))
        .unwrap();
    assert_eq!(s.confidence(&claim).unwrap(), Confidence::Corroborated); // different origin class and source
}

#[test]
fn same_source_string_is_not_independent_even_with_another_origin_class() {
    let mut s = MemoryStore::new();
    let claim = put(&mut s, "claim", Origin::Web, "https://x.example");
    let echo = put(&mut s, "echo", Origin::Tool, "https://x.example");
    s.link(&claim, LinkKind::CorroboratedBy, &echo, Timestamp(2))
        .unwrap();
    assert_eq!(s.confidence(&claim).unwrap(), Confidence::Unconfirmed);
}

#[test]
fn contradiction_wins_over_corroboration() {
    let mut s = MemoryStore::new();
    let claim = put(&mut s, "claim", Origin::Model, "m");
    let yes = put(&mut s, "yes", Origin::User, "cli");
    let no = put(&mut s, "no", Origin::Web, "https://y.example");
    s.link(&claim, LinkKind::CorroboratedBy, &yes, Timestamp(2))
        .unwrap();
    assert_eq!(s.confidence(&claim).unwrap(), Confidence::Corroborated);
    s.link(&claim, LinkKind::ContradictedBy, &no, Timestamp(3))
        .unwrap();
    assert_eq!(s.confidence(&claim).unwrap(), Confidence::Contradicted);
}

#[test]
fn confidence_of_unknown_id_is_not_found() {
    let s = MemoryStore::new();
    let other = {
        let mut o = MemoryStore::new();
        put(&mut o, "x", Origin::User, "cli")
    };
    assert_eq!(s.confidence(&other).unwrap_err(), Error::NotFound);
}
