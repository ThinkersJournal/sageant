// SPDX-License-Identifier: MIT OR Apache-2.0
//! X1 slice 2 (docs/DESIGN-WORTH-AND-TRUST.md section 2.1, 2.3): worth snapshots, stored per (chunk, version, model).
//! Written before the code. A snapshot is a recomputable cache: it holds numbers, never content, it is not an audited
//! write, it never changes a chunk, and it never outlives the chunk (forget and purge remove it).

use sageant::{
    ChunkId, Error, LinkKind, MemoryStore, NewChunk, Origin, PersonAuthority, Reader, Timestamp,
    WORTH_MODEL, Worth, WorthParams, WorthSnapshot, WorthStore, Writer, worth_of,
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

fn worth(s: &MemoryStore, id: &ChunkId, now: u64) -> Worth {
    worth_of(s, id, Timestamp(now), &params()).unwrap()
}

fn who() -> sageant::Actor {
    sageant::Actor::new("a", "b")
}

#[test]
fn a_recorded_snapshot_reads_back_exactly() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let b = put(&mut s, "beta", 10);
    s.link(&a, LinkKind::PartOf, &b, Timestamp(20)).unwrap();
    let w = worth(&s, &a, 130);
    let snap = s.record_worth(&a, &w, Timestamp(130)).unwrap();
    assert_eq!(snap.id(), &a);
    assert_eq!(snap.version(), 1);
    assert_eq!(snap.model_id(), WORTH_MODEL);
    assert_eq!(snap.computed_at(), Timestamp(130));
    assert_eq!(snap.worth(), &w);
    // 1 structural (1500) + recency 3000*100/(100+110) = 1428 + stability 0.
    assert_eq!(snap.worth().score_bp(), 1500 + 1428);
    assert_eq!(snap.worth().half_life(), 100);
    assert_eq!(s.worth_snapshots(&a).unwrap(), vec![snap]);
    assert_eq!(s.worth_snapshots(&b).unwrap(), vec![]);
}

#[test]
fn the_same_state_and_time_give_byte_identical_snapshots_and_a_new_time_changes_only_recency() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let one = s
        .record_worth(&a, &worth(&s, &a, 110), Timestamp(110))
        .unwrap();
    let again = s
        .record_worth(&a, &worth(&s, &a, 110), Timestamp(110))
        .unwrap();
    assert_eq!(one.encode(), again.encode());
    let later = s
        .record_worth(&a, &worth(&s, &a, 910), Timestamp(910))
        .unwrap();
    assert_ne!(one.encode(), later.encode());
    assert_eq!(later.computed_at(), Timestamp(910));
    let names: Vec<_> = later
        .worth()
        .components()
        .iter()
        .zip(one.worth().components())
        .filter(|(l, o)| l != o)
        .map(|(l, _)| l.name())
        .collect();
    assert_eq!(names, vec!["recency"]);
}

#[test]
fn recording_again_replaces_the_slot_and_a_new_version_gets_its_own() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    s.record_worth(&a, &worth(&s, &a, 110), Timestamp(110))
        .unwrap();
    s.record_worth(&a, &worth(&s, &a, 210), Timestamp(210))
        .unwrap();
    let only = s.worth_snapshots(&a).unwrap();
    assert_eq!(only.len(), 1, "same (chunk, version, model): one slot");
    assert_eq!(only[0].computed_at(), Timestamp(210));
    s.supersede(&a, "alpha two", Origin::User, "cli", Timestamp(300))
        .unwrap();
    s.record_worth(&a, &worth(&s, &a, 310), Timestamp(310))
        .unwrap();
    let both = s.worth_snapshots(&a).unwrap();
    let versions: Vec<u32> = both.iter().map(WorthSnapshot::version).collect();
    assert_eq!(versions, vec![1, 2], "ordered by version, old one kept");
    assert_eq!(both[0].computed_at(), Timestamp(210));
}

#[test]
fn a_snapshot_changes_nothing_about_the_chunk_and_is_not_an_audited_write() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let before = (Reader::get(&s, &a).unwrap().encode(), s.audit().len());
    s.record_worth(&a, &worth(&s, &a, 110), Timestamp(110))
        .unwrap();
    let after = (Reader::get(&s, &a).unwrap().encode(), s.audit().len());
    assert_eq!(before, after);
    assert_eq!(Reader::get(&s, &a).unwrap().id(), &a);
}

#[test]
fn unknown_forgotten_and_mismatched_chunks_are_refused() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let w = worth(&s, &a, 110);
    let ghost = ChunkId::parse(&sageant::sha256_hex(b"nothing")).unwrap();
    assert_eq!(
        s.record_worth(&ghost, &w, Timestamp(1)),
        Err(Error::NotFound)
    );
    assert_eq!(s.worth_snapshots(&ghost), Err(Error::NotFound));
    // A worth computed for version 3 cannot be filed against a chunk that is only at version 1.
    let c = put(&mut s, "gamma", 10);
    s.supersede(&c, "gamma 2", Origin::User, "cli", Timestamp(11))
        .unwrap();
    s.supersede(&c, "gamma 3", Origin::User, "cli", Timestamp(12))
        .unwrap();
    let three = worth(&s, &c, 20);
    assert_eq!(three.version(), 3);
    assert_eq!(
        s.record_worth(&a, &three, Timestamp(20)),
        Err(Error::Invalid("worth version"))
    );
    assert_eq!(s.worth_snapshots(&a).unwrap(), vec![]);
    Writer::forget(&mut s, &a, &who(), "r", None, Timestamp(30)).unwrap();
    assert_eq!(s.record_worth(&a, &w, Timestamp(31)), Err(Error::Forgotten));
    assert_eq!(s.worth_snapshots(&a), Err(Error::Forgotten));
}

#[test]
fn forget_and_purge_remove_the_snapshots_so_a_re_put_starts_clean() {
    for purge in [false, true] {
        let mut s = MemoryStore::new();
        let a = put(&mut s, "alpha", 10);
        s.record_worth(&a, &worth(&s, &a, 110), Timestamp(110))
            .unwrap();
        assert_eq!(
            s.worth_snapshots(&a).unwrap().len(),
            1,
            "control: present before"
        );
        if purge {
            let auth = PersonAuthority::from_person_action("tester");
            Writer::purge(&mut s, &a, &auth, Timestamp(120)).unwrap();
        } else {
            Writer::forget(&mut s, &a, &who(), "r", None, Timestamp(120)).unwrap();
        }
        let back = put(&mut s, "alpha", 200);
        assert_eq!(back, a, "same text, same id");
        assert_eq!(
            s.worth_snapshots(&a).unwrap(),
            vec![],
            "a stale snapshot must not come back with the chunk (purge={purge})"
        );
    }
}

#[test]
fn the_stored_form_round_trips_and_every_corruption_is_refused() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 10);
    let snap = s
        .record_worth(&a, &worth(&s, &a, 110), Timestamp(110))
        .unwrap();
    let bytes = snap.encode();
    assert_eq!(WorthSnapshot::decode(&bytes).unwrap(), snap);
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("sageant-worth/1\n"));
    assert!(!text.contains("alpha"), "numbers only, never content");
    // A score that is not the sum of its components.
    let bumped = text.replacen(
        &format!(
            "score_bp {}\n{}",
            snap.worth().score_bp().to_string().len(),
            snap.worth().score_bp()
        ),
        &format!(
            "score_bp {}\n{}",
            (snap.worth().score_bp() + 1).to_string().len(),
            snap.worth().score_bp() + 1
        ),
        1,
    );
    assert_ne!(bumped, text, "the mutation applied");
    assert_eq!(
        WorthSnapshot::decode(bumped.as_bytes()),
        Err(Error::Corrupt("score"))
    );
    // Truncated, trailing bytes, wrong header, empty.
    assert!(WorthSnapshot::decode(&bytes[..bytes.len() - 3]).is_err());
    let mut trailing = bytes.clone();
    trailing.push(b'x');
    assert_eq!(
        WorthSnapshot::decode(&trailing),
        Err(Error::Corrupt("trailing bytes"))
    );
    assert!(WorthSnapshot::decode(b"").is_err());
    assert!(WorthSnapshot::decode(text.replace("worth/1", "worth/9").as_bytes()).is_err());
    // A model id that would break the key (a slash) and a zero version.
    let slash = text.replacen("\nworth-v1\n", "\nworth/v1\n", 1);
    assert_ne!(slash, text);
    assert!(WorthSnapshot::decode(slash.as_bytes()).is_err());
    let zero = text.replacen("version 1\n1", "version 1\n0", 1);
    assert_ne!(zero, text);
    assert!(WorthSnapshot::decode(zero.as_bytes()).is_err());
}
