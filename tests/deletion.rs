// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 4 of ROADMAP row 3: archive / forget / purge and the content-free audit log (CONTRACT D8, D15, D18).
//! Written before the code. The promise is "unretrievable", never "erased": there is no `Erased` anywhere.

use sageant::{
    Action, ChunkId, Error, LinkKind, LinkTarget, MemoryStore, NewChunk, Origin, PersonAuthority,
    Removed, SecretScreen, Status, Timestamp,
};

const TEXT: &str = "MARKER-TEXT-9f3a unique body";
const SRC: &str = "MARKER-SRC-77c1";

fn put(s: &mut MemoryStore, text: &str, source: &str) -> ChunkId {
    s.put(
        NewChunk::new("notes", "note", text, Origin::User, source),
        Timestamp(1),
    )
    .unwrap()
    .id
}

#[test]
fn archive_keeps_the_chunk_readable_and_marks_its_status() {
    let mut s = MemoryStore::new();
    let id = put(&mut s, TEXT, SRC);
    s.archive(&id, "alice", "stale", Timestamp(5)).unwrap();
    let c = s.get(&id).unwrap();
    assert_eq!(c.status(), Status::Archived);
    assert_eq!(c.content(), TEXT); // kept
    assert_eq!(
        s.archive(&id, "alice", "again", Timestamp(6)).unwrap_err(),
        Error::BadState("already archived")
    );
}

#[test]
fn a_new_version_of_an_archived_chunk_stays_archived() {
    let mut s = MemoryStore::new();
    let id = put(&mut s, "one", "cli");
    s.archive(&id, "alice", "stale", Timestamp(2)).unwrap();
    s.supersede(&id, "two", Origin::User, "cli", Timestamp(3))
        .unwrap();
    assert_eq!(s.get(&id).unwrap().status(), Status::Archived);
    assert_eq!(s.get_version(&id, 1).unwrap().status(), Status::Archived);
}

#[test]
fn forget_makes_the_chunk_unretrievable_and_leaves_a_minimal_tombstone() {
    let mut s = MemoryStore::new();
    let id = put(&mut s, "one", SRC);
    s.supersede(&id, TEXT, Origin::Model, SRC, Timestamp(2))
        .unwrap();
    let removed = s
        .forget(&id, "alice", "wrong fact", Some("kh-abc"), Timestamp(9))
        .unwrap();
    assert_eq!(removed, Removed::Unretrievable);
    assert_eq!(s.get(&id).unwrap_err(), Error::Forgotten);
    assert_eq!(s.get_version(&id, 1).unwrap_err(), Error::Forgotten);
    assert_eq!(s.history(&id).unwrap_err(), Error::Forgotten);
    let t = s.tombstone(&id).unwrap();
    assert_eq!(t.id(), &id);
    assert_eq!(t.collection(), "notes");
    assert_eq!(t.versions_removed(), 2);
    assert_eq!(t.at(), Timestamp(9));
    assert_eq!(t.actor(), "alice");
    assert_eq!(t.reason(), "wrong fact");
    assert_eq!(t.keyed_hash(), Some("kh-abc"));
    assert_eq!(s.len(), 0); // no chunk versions remain
}

#[test]
fn nothing_the_store_holds_contains_the_content_after_forget() {
    let mut s = MemoryStore::new();
    let id = put(&mut s, TEXT, SRC);
    let hash = s.get(&id).unwrap().content_hash().to_owned();
    // Positive control: the same scan finds the text, the source and the hash before forgetting.
    let before = s.dump_all_text();
    assert!(
        before.contains("MARKER-TEXT-9f3a")
            && before.contains("MARKER-SRC-77c1")
            && before.contains(&hash)
    );
    s.forget(&id, "alice", "unwanted", None, Timestamp(9))
        .unwrap();
    let after = s.dump_all_text();
    for needle in ["MARKER-TEXT-9f3a", "MARKER-SRC-77c1", hash.as_str()] {
        assert!(!after.contains(needle), "{needle} survived forget");
    }
    assert!(after.contains("unwanted")); // the tombstone's reason is kept: the scan can see what is kept
}

#[test]
fn the_audit_log_never_holds_content() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, TEXT, SRC);
    let b = put(&mut s, "other", "cli");
    s.supersede(
        &a,
        "MARKER-TEXT-2 second",
        Origin::User,
        "cli",
        Timestamp(2),
    )
    .unwrap();
    s.link(&a, LinkKind::Cites, &b, Timestamp(3)).unwrap();
    s.archive(&a, "alice", "r1", Timestamp(4)).unwrap();
    s.forget(&a, "alice", "r2", None, Timestamp(5)).unwrap();
    let log = format!("{:?}", s.audit());
    assert!(!log.contains("MARKER-TEXT") && !log.contains("MARKER-SRC"));
    let actions: Vec<_> = s.audit().iter().map(|e| e.action()).collect();
    assert_eq!(
        actions,
        vec![
            Action::Put,
            Action::Put,
            Action::Supersede,
            Action::Link,
            Action::Archive,
            Action::Forget
        ]
    );
    assert_eq!(s.audit()[4].reason(), "r1");
    assert_eq!(s.audit()[5].id(), Some(&a));
    assert_eq!(s.audit()[5].at(), Timestamp(5));
}

#[test]
fn a_refused_write_appends_only_a_content_free_refused_entry() {
    struct Flags;
    impl SecretScreen for Flags {
        fn screen(&self, t: &str) -> Option<&'static str> {
            t.contains("HUNTER2").then_some("test-class")
        }
    }
    let mut s = MemoryStore::new();
    s.set_screen(Box::new(Flags));
    let _ = s.put(
        NewChunk::new("notes", "note", "pw HUNTER2", Origin::User, "cli"),
        Timestamp(1),
    );
    assert_eq!(s.audit().len(), 1);
    assert_eq!(s.audit()[0].action(), Action::Refused);
    assert_eq!(s.audit()[0].reason(), "secret:test-class");
    assert_eq!(s.audit()[0].id(), None);
    assert!(!format!("{:?}", s.audit()).contains("HUNTER2"));
    assert_eq!(s.len(), 0);
}

#[test]
fn links_to_a_forgotten_target_read_as_removed_and_new_links_to_it_are_refused() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "a text", "cli");
    let b = put(&mut s, "b text", "cli");
    s.link(&a, LinkKind::Cites, &b, Timestamp(2)).unwrap();
    let live = s.links_from(&a).unwrap();
    assert_eq!(s.link_target(live[0]), LinkTarget::Present);
    s.forget(&b, "alice", "gone", None, Timestamp(3)).unwrap();
    let after = s.links_from(&a).unwrap();
    assert_eq!(after.len(), 1); // the link is kept, never dangling: it says so
    assert_eq!(s.link_target(after[0]), LinkTarget::Removed);
    let c = put(&mut s, "c text", "cli");
    assert_eq!(
        s.link(&c, LinkKind::Cites, &b, Timestamp(4)).unwrap_err(),
        Error::Forgotten
    );
}

#[test]
fn forgetting_a_source_flags_its_derived_chunks_for_review_without_deleting_them() {
    let mut s = MemoryStore::new();
    let source = put(&mut s, "source text", "cli");
    let summary = put(&mut s, "summary text", "cli");
    let unrelated = put(&mut s, "unrelated", "cli");
    s.link(&summary, LinkKind::DerivedFrom, &source, Timestamp(2))
        .unwrap();
    assert!(!s.needs_review(&summary).unwrap());
    s.forget(&source, "alice", "wrong", None, Timestamp(3))
        .unwrap();
    assert!(s.needs_review(&summary).unwrap());
    assert!(!s.needs_review(&unrelated).unwrap());
    assert_eq!(s.get(&summary).unwrap().content(), "summary text"); // not auto-deleted
}

#[test]
fn removed_evidence_counts_for_nothing_in_confidence() {
    let mut s = MemoryStore::new();
    let claim = s
        .put(
            NewChunk::new("notes", "note", "claim", Origin::Model, "m"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    let ev = s
        .put(
            NewChunk::new("notes", "note", "evidence", Origin::User, "cli"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    s.link(&claim, LinkKind::CorroboratedBy, &ev, Timestamp(2))
        .unwrap();
    assert_eq!(
        s.confidence(&claim).unwrap(),
        sageant::Confidence::Corroborated
    );
    s.forget(&ev, "alice", "gone", None, Timestamp(3)).unwrap();
    assert_eq!(
        s.confidence(&claim).unwrap(),
        sageant::Confidence::Unconfirmed
    );
}

#[test]
fn the_same_text_can_be_learned_again_after_forget() {
    let mut s = MemoryStore::new();
    let id = put(&mut s, "relearn me", "cli");
    s.forget(&id, "alice", "oops", None, Timestamp(2)).unwrap();
    let again = put(&mut s, "relearn me", "web");
    assert_eq!(again, id); // same text, same collection: same id
    assert_eq!(s.get(&id).unwrap().version(), 1);
    assert!(s.tombstone(&id).is_none()); // live again; the earlier forget stays in the audit log
    assert_eq!(
        s.audit()
            .iter()
            .filter(|e| e.action() == Action::Forget)
            .count(),
        1
    );
}

#[test]
fn forgetting_twice_or_an_unknown_id_is_refused() {
    let mut s = MemoryStore::new();
    let id = put(&mut s, "x", "cli");
    s.forget(&id, "alice", "r", None, Timestamp(2)).unwrap();
    assert_eq!(
        s.forget(&id, "alice", "r", None, Timestamp(3)).unwrap_err(),
        Error::Forgotten
    );
    let ghost = MemoryStore::new()
        .put(
            NewChunk::new("q", "note", "ghost", Origin::User, "cli"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    assert_eq!(
        s.forget(&ghost, "alice", "r", None, Timestamp(3))
            .unwrap_err(),
        Error::NotFound
    );
}

#[test]
fn purge_removes_everything_including_the_tombstone_and_needs_a_person() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, TEXT, SRC);
    let b = put(&mut s, "b text", "cli");
    s.link(&b, LinkKind::Cites, &a, Timestamp(2)).unwrap();
    let person = PersonAuthority::from_person_action("typed confirmation at the CLI");
    s.purge(&a, &person, Timestamp(7)).unwrap();
    assert_eq!(s.get(&a).unwrap_err(), Error::NotFound);
    assert!(s.tombstone(&a).is_none());
    let links = s.links_from(&b).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(s.link_target(links[0]), LinkTarget::Removed); // "target removed", not a dangling id
    let dump = s.dump_all_text();
    assert!(!dump.contains("MARKER-TEXT-9f3a") && !dump.contains("MARKER-SRC-77c1"));
    let last = s.audit().last().unwrap();
    assert_eq!(last.action(), Action::Purge);
    assert_eq!(last.actor(), "typed confirmation at the CLI");
    // A purge of something already gone is NotFound.
    assert_eq!(
        s.purge(&a, &person, Timestamp(8)).unwrap_err(),
        Error::NotFound
    );
}

#[test]
fn purge_also_removes_a_tombstone() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "x", "cli");
    s.forget(&a, "alice", "r", Some("kh"), Timestamp(2))
        .unwrap();
    assert!(s.tombstone(&a).is_some());
    s.purge(
        &a,
        &PersonAuthority::from_person_action("cli"),
        Timestamp(3),
    )
    .unwrap();
    assert!(s.tombstone(&a).is_none());
    assert!(!s.dump_all_text().contains("kh"));
}
