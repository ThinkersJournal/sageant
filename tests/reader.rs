// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 6 of ROADMAP row 3: the `Reader` trait and `Filter` (CONTRACT D10, D11). Retrievers get a `&dyn Reader`
//! and never a writer. Written before the code.

use sageant::{
    ChunkId, Confidence, Filter, LinkKind, MemoryStore, NewChunk, Origin, Reader, Status, Timestamp,
};

fn put(
    s: &mut MemoryStore,
    coll: &str,
    kind: &str,
    text: &str,
    o: Origin,
    src: &str,
    at: u64,
) -> ChunkId {
    s.put(NewChunk::new(coll, kind, text, o, src), Timestamp(at))
        .unwrap()
        .id
}

fn fixture() -> (MemoryStore, [ChunkId; 4]) {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "notes", "note", "alpha", Origin::User, "cli", 10);
    let b = put(
        &mut s,
        "notes",
        "fact",
        "beta",
        Origin::Web,
        "https://x.example",
        20,
    );
    let c = put(&mut s, "docs", "note", "gamma", Origin::Model, "m", 30);
    let d = put(&mut s, "notes", "note", "delta", Origin::Tool, "t", 40);
    (s, [a, b, c, d])
}

fn sorted(mut v: Vec<&ChunkId>) -> Vec<&ChunkId> {
    v.sort();
    v
}

fn ids(v: &[sageant::ChunkRef]) -> Vec<&ChunkId> {
    v.iter().map(|r| &r.id).collect()
}

#[test]
fn the_reader_trait_is_object_safe_and_reads_like_the_store() {
    let (s, [a, ..]) = fixture();
    let r: &dyn Reader = &s;
    assert_eq!(r.get(&a).unwrap().content(), "alpha");
    assert_eq!(r.get_version(&a, 1).unwrap().version(), 1);
    assert_eq!(r.history(&a).unwrap().len(), 1);
    assert!(r.links_from(&a).unwrap().is_empty());
    assert!(r.links_to(&a).unwrap().is_empty());
    assert_eq!(r.confidence(&a).unwrap(), Confidence::Unconfirmed);
}

#[test]
fn an_empty_filter_lists_every_active_current_chunk_in_a_stated_order() {
    let (s, [a, b, c, d]) = fixture();
    let got = s.list(&Filter::new());
    assert_eq!(got.len(), 4);
    let mut want = vec![&a, &b, &c, &d];
    want.sort(); // order is (ChunkId asc, version desc)
    assert_eq!(ids(&got), want);
}

#[test]
fn filters_narrow_by_collection_kind_origin_and_time() {
    let (s, [a, b, c, d]) = fixture();
    assert_eq!(ids(&s.list(&Filter::new().collection("docs"))), vec![&c]);
    assert_eq!(
        ids(&s.list(&Filter::new().collection("notes").kind("note"))),
        sorted(vec![&a, &d])
    );
    assert_eq!(ids(&s.list(&Filter::new().origin(Origin::Web))), vec![&b]);
    assert_eq!(
        ids(&s.list(
            &Filter::new()
                .created_from(Timestamp(20))
                .created_until(Timestamp(30))
        )),
        sorted(vec![&b, &c])
    );
    assert!(s.list(&Filter::new().collection("nope")).is_empty());
}

#[test]
fn archived_chunks_are_hidden_unless_asked_for() {
    let (mut s, [a, ..]) = fixture();
    s.archive(&a, "alice", "stale", Timestamp(50)).unwrap();
    assert!(!ids(&s.list(&Filter::new())).contains(&&a));
    assert!(ids(&s.list(&Filter::new().include_archived())).contains(&&a));
    assert_eq!(
        ids(&s.list(&Filter::new().status(Status::Archived))),
        vec![&a]
    );
}

#[test]
fn superseded_versions_are_hidden_unless_asked_for_and_come_newest_first() {
    let (mut s, [a, ..]) = fixture();
    s.supersede(&a, "alpha two", Origin::User, "cli", Timestamp(60))
        .unwrap();
    let cur = s.list(&Filter::new().collection("notes").kind("note"));
    let mine: Vec<_> = cur.iter().filter(|r| r.id == a).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].version, 2);
    let all = s.list(&Filter::new().include_superseded());
    let versions: Vec<u32> = all
        .iter()
        .filter(|r| r.id == a)
        .map(|r| r.version)
        .collect();
    assert_eq!(versions, vec![2, 1]);
}

#[test]
fn confidence_filter_uses_the_derived_value() {
    let (mut s, [a, b, ..]) = fixture();
    // a is User/cli, b is Web/other source: independent evidence.
    s.link(&a, LinkKind::CorroboratedBy, &b, Timestamp(70))
        .unwrap();
    assert_eq!(
        ids(&s.list(&Filter::new().confidence(Confidence::Corroborated))),
        vec![&a]
    );
}

#[test]
fn limit_keeps_the_first_in_order() {
    let (s, _) = fixture();
    let all = s.list(&Filter::new());
    let two = s.list(&Filter::new().limit(2));
    assert_eq!(two, all[..2].to_vec());
    assert!(s.list(&Filter::new().limit(0)).is_empty());
}

#[test]
fn forgotten_chunks_are_never_listed() {
    let (mut s, [a, ..]) = fixture();
    s.forget(&a, "alice", "r", None, Timestamp(80)).unwrap();
    assert!(!ids(&s.list(&Filter::new().include_archived().include_superseded())).contains(&&a));
}

#[test]
fn reader_returns_owned_values_so_a_backend_need_not_lend_them() {
    // CONTRACT D11 (0.1.0): a kv-backed store cannot lend a reference into storage, so every read is owned.
    let (mut s, [a, b, ..]) = fixture();
    s.link(&a, LinkKind::Cites, &b, Timestamp(5)).unwrap();
    let r: &dyn Reader = &s;
    let chunk: sageant::Chunk = r.get(&a).unwrap();
    let version: sageant::Chunk = r.get_version(&a, 1).unwrap();
    let history: Vec<sageant::Chunk> = r.history(&a).unwrap();
    let from: Vec<sageant::Link> = r.links_from(&a).unwrap();
    let to: Vec<sageant::Link> = r.links_to(&b).unwrap();
    assert_eq!(chunk.content(), "alpha");
    assert_eq!(version.content(), chunk.content());
    assert_eq!(history.len(), 1);
    assert_eq!((from.len(), to.len()), (1, 1));
    // The owned values outlive the store.
    drop(s);
    assert_eq!(chunk.content(), "alpha");
}
