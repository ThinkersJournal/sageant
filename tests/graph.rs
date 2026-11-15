// SPDX-License-Identifier: MIT OR Apache-2.0
//! Row 4: the link rules as pure functions (CONTRACT D6, D7), so a second store applies exactly the rules
//! `MemoryStore` does instead of copying them. Written before the code.

use std::collections::BTreeMap;

use sageant::{
    Chunk, ChunkId, Confidence, Error, LinkKind, MemoryStore, NewChunk, Origin, Timestamp,
    closes_cycle, derive_confidence,
};

fn chunk(text: &str, origin: Origin, source: &str) -> Chunk {
    Chunk::first_version(NewChunk::new("c", "k", text, origin, source), Timestamp(1)).unwrap()
}

#[test]
fn confidence_needs_independent_corroboration_and_contradiction_wins() {
    let me = chunk("claim", Origin::Model, "m");
    let other_origin_and_source = chunk("e1", Origin::User, "cli");
    let same_origin = chunk("e2", Origin::Model, "x");
    let same_source = chunk("e3", Origin::User, "m");
    let no_links: [(LinkKind, Option<&Chunk>); 0] = [];
    assert_eq!(derive_confidence(&me, &no_links), Confidence::Unconfirmed);
    assert_eq!(
        derive_confidence(
            &me,
            &[(LinkKind::CorroboratedBy, Some(&other_origin_and_source))]
        ),
        Confidence::Corroborated
    );
    assert_eq!(
        derive_confidence(&me, &[(LinkKind::CorroboratedBy, Some(&same_origin))]),
        Confidence::Unconfirmed
    );
    assert_eq!(
        derive_confidence(&me, &[(LinkKind::CorroboratedBy, Some(&same_source))]),
        Confidence::Unconfirmed
    );
    // Removed evidence (None) counts for nothing, in either direction.
    assert_eq!(
        derive_confidence(
            &me,
            &[
                (LinkKind::CorroboratedBy, None),
                (LinkKind::ContradictedBy, None)
            ]
        ),
        Confidence::Unconfirmed
    );
    // Contradiction wins over corroboration, in either order.
    for links in [
        [
            (LinkKind::CorroboratedBy, Some(&other_origin_and_source)),
            (LinkKind::ContradictedBy, Some(&same_origin)),
        ],
        [
            (LinkKind::ContradictedBy, Some(&same_origin)),
            (LinkKind::CorroboratedBy, Some(&other_origin_and_source)),
        ],
    ] {
        assert_eq!(derive_confidence(&me, &links), Confidence::Contradicted);
    }
    // Other kinds say nothing about confidence.
    assert_eq!(
        derive_confidence(&me, &[(LinkKind::Cites, Some(&other_origin_and_source))]),
        Confidence::Unconfirmed
    );
}

fn ids(n: usize) -> Vec<ChunkId> {
    let mut s = MemoryStore::new();
    (0..n)
        .map(|i| {
            s.put(
                NewChunk::new("c", "k", &format!("t{i}"), Origin::User, "s"),
                Timestamp(1),
            )
            .unwrap()
            .id
        })
        .collect()
}

#[test]
fn closes_cycle_follows_edges_and_terminates_on_cyclic_data() {
    let n = ids(4);
    // a -> b -> c, and d -> d (a cycle in the data itself).
    let edges: BTreeMap<ChunkId, Vec<ChunkId>> = BTreeMap::from([
        (n[0].clone(), vec![n[1].clone()]),
        (n[1].clone(), vec![n[2].clone()]),
        (n[3].clone(), vec![n[3].clone()]),
    ]);
    let mut next = |id: &ChunkId| -> Result<Vec<ChunkId>, Error> {
        Ok(edges.get(id).cloned().unwrap_or_default())
    };
    assert!(closes_cycle(&n[0], &n[2], &mut next).unwrap()); // a reaches c
    assert!(closes_cycle(&n[0], &n[0], &mut next).unwrap()); // a node reaches itself
    assert!(!closes_cycle(&n[2], &n[0], &mut next).unwrap()); // c does not reach a
    assert!(!closes_cycle(&n[3], &n[0], &mut next).unwrap()); // terminates on d -> d
}

#[test]
fn closes_cycle_passes_a_lookup_error_through() {
    let n = ids(2);
    let mut next =
        |_: &ChunkId| -> Result<Vec<ChunkId>, Error> { Err(Error::Backend("down".into())) };
    assert_eq!(
        closes_cycle(&n[0], &n[1], &mut next).unwrap_err(),
        Error::Backend("down".into())
    );
}

#[test]
fn link_kinds_have_stable_names_that_parse_back() {
    for (kind, name) in [
        (LinkKind::PartOf, "PartOf"),
        (LinkKind::DerivedFrom, "DerivedFrom"),
        (LinkKind::Cites, "Cites"),
        (LinkKind::Related, "Related"),
        (LinkKind::CorroboratedBy, "CorroboratedBy"),
        (LinkKind::ContradictedBy, "ContradictedBy"),
    ] {
        assert_eq!(kind.name(), name);
        assert_eq!(LinkKind::parse(name).unwrap(), kind);
    }
    assert_eq!(
        LinkKind::parse("cites").unwrap_err(),
        Error::Invalid("link kind")
    );
    assert_eq!(
        LinkKind::parse("").unwrap_err(),
        Error::Invalid("link kind")
    );
}
