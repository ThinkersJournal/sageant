// SPDX-License-Identifier: MIT OR Apache-2.0
//! X1 slice 1 (docs/DESIGN-WORTH-AND-TRUST.md section 2): worth as a pure derived signal, integer basis points.
//! Written before the code. Every expected number below is worked out by hand from the stated weights, so a changed
//! weight, cap, signal or rounding rule fails a test.
//!
//! Formula `worth-v1` (fixed weights, CireSnave-approved defaults; only the time unit is the caller's):
//! structural links (part-of, derived-from, in or out) 1500 bp each, capped at 4000; loose links (cites, related, in or
//! out) 500 bp each, capped at 2000; recency `3000 * H / (H + age)` where `age` is `now` minus the newest of the
//! current version's creation and the newest structural or loose link touching it, and `H` is the half-life; stability
//! `250 * (min(versions, 5) - 1)`, so at most 1000. Evidence links (corroborated-by, contradicted-by) are trust's
//! business and count for nothing here, neither as neighbours nor as a touch. The maximum is 10 000.

use sageant::{
    ChunkId, Error, LinkKind, MemoryStore, NewChunk, Origin, Timestamp, WORTH_MODEL, WorthParams,
    worth_of,
};

fn put(s: &mut MemoryStore, text: &str, at: u64) -> ChunkId {
    s.put(
        NewChunk::new("n", "note", text, Origin::User, "cli"),
        Timestamp(at),
    )
    .unwrap()
    .id
}

fn params() -> WorthParams {
    WorthParams::new(100)
}

fn bp(s: &MemoryStore, id: &ChunkId, now: u64) -> u32 {
    worth_of(s, id, Timestamp(now), &params())
        .unwrap()
        .score_bp()
}

/// A, B, C created at 10. A part-of B at 20 (structural for both). A cites C at 30 (loose for both). An evidence link
/// A corroborated-by C at 40, which must not count.
fn fixture() -> (MemoryStore, ChunkId, ChunkId, ChunkId) {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let b = put(&mut s, "beta", 10);
    let c = put(&mut s, "gamma", 10);
    s.link(&a, LinkKind::PartOf, &b, Timestamp(20)).unwrap();
    s.link(&a, LinkKind::Cites, &c, Timestamp(30)).unwrap();
    s.link(&a, LinkKind::CorroboratedBy, &c, Timestamp(40))
        .unwrap();
    (s, a, b, c)
}

#[test]
fn the_model_has_a_stable_id() {
    assert_eq!(WORTH_MODEL, "worth-v1");
}

#[test]
fn worth_is_exact_for_a_hand_built_graph() {
    let (s, a, b, c) = fixture();
    // A: 1 structural (1500) + 1 loose (500); newest touch 30 (the evidence link at 40 is ignored), age 100 at now
    // 130, recency 3000*100/200 = 1500; one version, stability 0.
    assert_eq!(bp(&s, &a, 130), 1500 + 500 + 1500);
    // B: 1 structural in (1500); newest touch 20, age 110, recency 300000/210 = 1428 (floor); stability 0.
    assert_eq!(bp(&s, &b, 130), 1500 + 1428);
    // C: 1 loose in (500); the corroborated-by link into C is ignored; newest touch 30, age 100, recency 1500.
    assert_eq!(bp(&s, &c, 130), 500 + 1500);
}

#[test]
fn the_components_name_their_raw_values_and_add_up_to_the_score() {
    let (s, a, _, _) = fixture();
    let w = worth_of(&s, &a, Timestamp(130), &params()).unwrap();
    let parts: Vec<(&str, u64, u32)> = w
        .components()
        .iter()
        .map(|c| (c.name(), c.raw(), c.bp()))
        .collect();
    assert_eq!(
        parts,
        vec![
            ("structural_links", 1, 1500),
            ("loose_links", 1, 500),
            ("recency", 100, 1500),
            ("stability", 1, 0),
        ]
    );
    assert_eq!(
        w.components().iter().map(|c| c.bp()).sum::<u32>(),
        w.score_bp()
    );
}

#[test]
fn evidence_links_alone_change_nothing_about_worth() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let b = put(&mut s, "beta", 10);
    let before = bp(&s, &a, 110);
    s.link(&a, LinkKind::CorroboratedBy, &b, Timestamp(100))
        .unwrap();
    s.link(&b, LinkKind::ContradictedBy, &a, Timestamp(105))
        .unwrap();
    // Age is still measured from creation (10), not from the evidence links: 3000*100/200 = 1500.
    assert_eq!(before, 1500);
    assert_eq!(bp(&s, &a, 110), 1500);
    assert_eq!(bp(&s, &b, 110), 1500);
}

#[test]
fn structural_and_loose_links_are_capped() {
    let mut s = MemoryStore::new();
    let hub = put(&mut s, "hub", 10);
    for i in 0..5u64 {
        let child = put(&mut s, &format!("child {i}"), 10);
        s.link(&child, LinkKind::PartOf, &hub, Timestamp(10))
            .unwrap();
    }
    for i in 0..5u64 {
        let other = put(&mut s, &format!("other {i}"), 10);
        s.link(&other, LinkKind::Related, &hub, Timestamp(10))
            .unwrap();
    }
    // 5 structural would be 7500 but is capped at 4000; 5 loose would be 2500 but is capped at 2000; age 0 at now 10
    // gives the full 3000 recency; one version.
    assert_eq!(bp(&s, &hub, 10), 4000 + 2000 + 3000);
    // Below the caps nothing is cut: 2 structural (3000) and 3 loose (1500) are counted in full.
    let mut t = MemoryStore::new();
    let h = put(&mut t, "hub", 10);
    for i in 0..2u64 {
        let c = put(&mut t, &format!("c{i}"), 10);
        t.link(&c, LinkKind::DerivedFrom, &h, Timestamp(10))
            .unwrap();
    }
    for i in 0..3u64 {
        let o = put(&mut t, &format!("o{i}"), 10);
        t.link(&o, LinkKind::Cites, &h, Timestamp(10)).unwrap();
    }
    assert_eq!(bp(&t, &h, 10), 3000 + 1500 + 3000);
}

#[test]
fn stability_grows_with_versions_and_stops_at_five() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "v1", 10);
    // Each supersede is a new current version created at 10, so age stays 0 at now 10 and recency is the full 3000.
    assert_eq!(bp(&s, &a, 10), 3000);
    s.supersede(&a, "v2", Origin::User, "cli", Timestamp(10))
        .unwrap();
    assert_eq!(bp(&s, &a, 10), 3000 + 250);
    s.supersede(&a, "v3", Origin::User, "cli", Timestamp(10))
        .unwrap();
    assert_eq!(bp(&s, &a, 10), 3000 + 500);
    s.supersede(&a, "v4", Origin::User, "cli", Timestamp(10))
        .unwrap();
    s.supersede(&a, "v5", Origin::User, "cli", Timestamp(10))
        .unwrap();
    assert_eq!(bp(&s, &a, 10), 3000 + 1000);
    s.supersede(&a, "v6", Origin::User, "cli", Timestamp(10))
        .unwrap();
    assert_eq!(bp(&s, &a, 10), 3000 + 1000); // capped at five versions
}

#[test]
fn recency_halves_at_the_half_life_and_never_goes_negative_for_a_future_touch() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "a", 1000);
    // now == creation: age 0, the full 3000. One half-life later: 1500. Two: 1000. Far later: small but positive.
    assert_eq!(bp(&s, &a, 1000), 3000);
    assert_eq!(bp(&s, &a, 1100), 1500);
    assert_eq!(bp(&s, &a, 1200), 1000);
    assert_eq!(bp(&s, &a, 1000 + 99_900), 3);
    // A `now` before the creation time (clock skew) is age 0, never a panic or an underflow.
    assert_eq!(bp(&s, &a, 5), 3000);
}

#[test]
fn a_zero_half_life_is_treated_as_one_and_never_divides_by_zero() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "a", 10);
    let w = |now| worth_of(&s, &a, Timestamp(now), &WorthParams::new(0)).unwrap();
    assert_eq!(w(10).score_bp(), 3000); // age 0: 3000 * 1 / 1
    assert_eq!(w(11).score_bp(), 1500); // age 1, H 1: 3000 * 1 / 2
}

#[test]
fn the_score_is_the_same_every_time_and_depends_only_on_the_store_and_now() {
    let (s, a, _, _) = fixture();
    let first = worth_of(&s, &a, Timestamp(130), &params()).unwrap();
    let second = worth_of(&s, &a, Timestamp(130), &params()).unwrap();
    assert_eq!(first, second);
    // A later `now` changes only the recency term.
    let later = worth_of(&s, &a, Timestamp(230), &params()).unwrap();
    assert_eq!(first.components()[0], later.components()[0]);
    assert_eq!(first.components()[1], later.components()[1]);
    assert_eq!(first.components()[3], later.components()[3]);
    assert_ne!(first.components()[2], later.components()[2]);
}

#[test]
fn worth_never_exceeds_ten_thousand_basis_points() {
    let mut s = MemoryStore::new();
    let hub = put(&mut s, "hub", 10);
    for i in 0..10u64 {
        let c = put(&mut s, &format!("c{i}"), 10);
        s.link(&c, LinkKind::PartOf, &hub, Timestamp(10)).unwrap();
        s.link(&c, LinkKind::Cites, &hub, Timestamp(10)).unwrap();
    }
    for v in 2..=8 {
        s.supersede(&hub, &format!("v{v}"), Origin::User, "cli", Timestamp(10))
            .unwrap();
    }
    assert_eq!(bp(&s, &hub, 10), 10_000);
}

#[test]
fn an_unknown_chunk_has_no_worth() {
    let s = MemoryStore::new();
    let ghost = MemoryStore::new()
        .put(
            NewChunk::new("x", "note", "ghost", Origin::User, "cli"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    assert!(matches!(
        worth_of(&s, &ghost, Timestamp(10), &params()),
        Err(Error::NotFound)
    ));
}

#[test]
fn computing_worth_writes_nothing_and_leaves_the_chunk_unchanged() {
    let (s, a, _, _) = fixture();
    let before = sageant::Reader::get(&s, &a).unwrap();
    let hash_before = before.content_hash().to_owned();
    let _ = worth_of(&s, &a, Timestamp(130), &params()).unwrap();
    let after = sageant::Reader::get(&s, &a).unwrap();
    assert_eq!(after.content_hash(), hash_before);
    assert_eq!(after.version(), before.version());
}
