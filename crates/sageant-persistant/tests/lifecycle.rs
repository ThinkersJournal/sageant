// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant-persistant` slice 3: audit log, archive, forget, purge (CONTRACT D8-D10). Written before the code.
//! `MemoryStore` is the reference: both stores must agree on every result, errors included.

use persistant::registry::Endpoint;
use sageant::{
    Action, ChunkId, Error, Filter, LinkKind, LinkTarget, MemoryStore, NewChunk, Origin,
    PersonAuthority, Status, Timestamp, Writer,
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
        let make = || NewChunk::new("c", "k", &format!("secret-text {i}"), Origin::User, "cli");
        ids.push(p.put(make(), Timestamp(1)).unwrap().id);
        m.put(make(), Timestamp(1)).unwrap();
    }
    (p, m, ids)
}

type Entry = (u64, Action, Option<ChunkId>, String, String);

fn entries(events: &[sageant::AuditEvent]) -> Vec<Entry> {
    events
        .iter()
        .map(|e| {
            (
                e.at().0,
                e.action(),
                e.id().cloned(),
                e.actor().to_owned(),
                e.reason().to_owned(),
            )
        })
        .collect()
}

fn same_audit(p: &PersistStore, m: &MemoryStore) {
    assert_eq!(entries(&p.audit().unwrap()), entries(m.audit()));
}

fn listings_agree(p: &PersistStore, m: &MemoryStore) {
    let filters = [
        Filter::new(),
        Filter::new().include_archived(),
        Filter::new().status(Status::Archived),
        Filter::new().include_superseded().include_archived(),
    ];
    for f in &filters {
        assert_eq!(
            p.list(f).unwrap(),
            sageant::Reader::list(m, f).unwrap(),
            "{f:?}"
        );
    }
}

#[test]
fn writes_append_content_free_audit_entries_that_match_the_memory_store() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, ids) = pair(dir.path(), 2);
    p.supersede(&ids[0], "changed", Origin::Web, "w", Timestamp(2))
        .unwrap();
    m.supersede(&ids[0], "changed", Origin::Web, "w", Timestamp(2))
        .unwrap();
    p.link(&ids[0], LinkKind::Related, &ids[1], Timestamp(3))
        .unwrap();
    m.link(&ids[0], LinkKind::Related, &ids[1], Timestamp(3))
        .unwrap();
    same_audit(&p, &m);
    assert_eq!(p.audit().unwrap().len(), 4);
}

#[test]
fn a_refused_write_appends_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (p, _m, ids) = pair(dir.path(), 2);
    let before = p.audit().unwrap().len();
    assert_eq!(
        p.link(&ids[0], LinkKind::Related, &ids[0], Timestamp(3)),
        Err(Error::LinkCycle)
    );
    p.link(&ids[0], LinkKind::Related, &ids[1], Timestamp(3))
        .unwrap();
    assert_eq!(
        p.link(&ids[0], LinkKind::Related, &ids[1], Timestamp(4)),
        Err(Error::DuplicateLink)
    );
    assert_eq!(p.audit().unwrap().len(), before + 1);
}

#[test]
fn the_audit_log_survives_reopening_and_keeps_counting() {
    let dir = tempfile::tempdir().unwrap();
    let ids;
    {
        let (p, _m, i) = pair(dir.path(), 1);
        ids = i;
        assert_eq!(p.audit().unwrap().len(), 1);
    }
    let p = open(dir.path());
    p.archive(&ids[0], "me", "tidy", Timestamp(2)).unwrap();
    let log = p.audit().unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(log[1].action(), Action::Archive);
}

#[test]
fn archive_and_unarchive_agree_with_the_memory_store() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, ids) = pair(dir.path(), 3);
    p.supersede(&ids[0], "v2", Origin::User, "cli", Timestamp(2))
        .unwrap();
    m.supersede(&ids[0], "v2", Origin::User, "cli", Timestamp(2))
        .unwrap();
    assert_eq!(
        p.archive(&ids[0], "me", "old", Timestamp(3)),
        m.archive(&ids[0], "me", "old", Timestamp(3))
    );
    // Every version is archived, and a new version of an archived chunk stays archived.
    for v in 1..=2 {
        assert_eq!(
            p.get_version(&ids[0], v).unwrap().status(),
            Status::Archived
        );
    }
    p.supersede(&ids[0], "v3", Origin::User, "cli", Timestamp(4))
        .unwrap();
    m.supersede(&ids[0], "v3", Origin::User, "cli", Timestamp(4))
        .unwrap();
    assert_eq!(p.get(&ids[0]).unwrap().status(), Status::Archived);
    listings_agree(&p, &m);
    assert_eq!(
        p.archive(&ids[0], "me", "again", Timestamp(5)),
        Err(Error::BadState("already archived"))
    );
    assert_eq!(
        p.unarchive(&ids[1], "me", "x", Timestamp(5)),
        Err(Error::BadState("already active"))
    );
    assert_eq!(
        p.unarchive(&ids[0], "me", "back", Timestamp(6)),
        m.unarchive(&ids[0], "me", "back", Timestamp(6))
    );
    listings_agree(&p, &m);
    let unknown = ChunkId::parse(&"cd".repeat(32)).unwrap();
    assert_eq!(
        p.archive(&unknown, "me", "", Timestamp(7)),
        Err(Error::NotFound)
    );
    same_audit(&p, &m);
}

#[test]
fn note_records_an_audit_entry_and_checks_the_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, ids) = pair(dir.path(), 1);
    p.note(&ids[0], "role/agent", "why", Timestamp(2)).unwrap();
    Writer::note(
        &mut m,
        &ids[0],
        &sageant::Actor::new("role", "agent"),
        "why",
        Timestamp(2),
    )
    .unwrap();
    same_audit(&p, &m);
    let unknown = ChunkId::parse(&"cd".repeat(32)).unwrap();
    assert_eq!(
        p.note(&unknown, "a/b", "", Timestamp(3)),
        Err(Error::NotFound)
    );
    assert_eq!(p.audit().unwrap().len(), 2);
}

#[test]
fn forget_agrees_with_the_memory_store_and_removes_everything() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, ids) = pair(dir.path(), 3);
    let (a, b, c) = (&ids[0], &ids[1], &ids[2]);
    for s in [
        (a, LinkKind::Related, c),
        (b, LinkKind::DerivedFrom, a),
        (a, LinkKind::PartOf, c),
    ] {
        p.link(s.0, s.1, s.2, Timestamp(2)).unwrap();
        m.link(s.0, s.1, s.2, Timestamp(2)).unwrap();
    }
    p.supersede(a, "second secret-text", Origin::User, "cli", Timestamp(3))
        .unwrap();
    m.supersede(a, "second secret-text", Origin::User, "cli", Timestamp(3))
        .unwrap();
    assert_eq!(
        p.forget(a, "me", "asked", Some("kh"), Timestamp(4)),
        m.forget(a, "me", "asked", Some("kh"), Timestamp(4))
    );
    // Reads say forgotten, not not-found.
    assert_eq!(p.get(a).err(), Some(Error::Forgotten));
    assert_eq!(p.get_version(a, 1).err(), Some(Error::Forgotten));
    assert_eq!(p.history(a).err(), Some(Error::Forgotten));
    assert_eq!(p.links_from(a), Err(Error::Forgotten));
    assert_eq!(p.links_to(a), Err(Error::Forgotten));
    assert_eq!(p.confidence(a), Err(Error::Forgotten));
    assert_eq!(
        p.forget(a, "me", "again", None, Timestamp(5)),
        Err(Error::Forgotten)
    );
    assert_eq!(
        p.supersede(a, "x", Origin::User, "cli", Timestamp(5)),
        Err(Error::Forgotten)
    );
    assert_eq!(
        p.link(a, LinkKind::Related, b, Timestamp(5)),
        Err(Error::Forgotten)
    );
    // The tombstone, the review flag, link targets and confidence all agree.
    assert_eq!(p.tombstone(a).unwrap().as_ref(), m.tombstone(a));
    assert_eq!(p.tombstone(b).unwrap(), None);
    assert_eq!(p.needs_review(b).unwrap(), m.needs_review(b).unwrap());
    assert!(p.needs_review(b).unwrap());
    assert!(!p.needs_review(c).unwrap());
    assert_eq!(p.needs_review(a), Err(Error::Forgotten));
    let from_b = p.links_from(b).unwrap();
    assert_eq!(from_b.len(), 1);
    assert_eq!(p.link_target(&from_b[0]).unwrap(), LinkTarget::Removed);
    assert_eq!(
        p.link_target(&from_b[0]).unwrap(),
        m.link_target(&from_b[0])
    );
    assert_eq!(p.confidence(b).unwrap(), m.confidence(b).unwrap());
    assert_eq!(
        p.links_to(c).unwrap().len(),
        0,
        "its outgoing links went with it"
    );
    assert_eq!(
        p.links_to(c).unwrap(),
        m.links_to(c)
            .unwrap()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    );
    listings_agree(&p, &m);
    same_audit(&p, &m);
    // Nothing of the chunk is left in storage: no content, no source, no record of its versions.
    let all = p.dump_all_for_test().unwrap();
    for (k, v) in &all {
        let text = String::from_utf8_lossy(v);
        assert!(
            !text.contains("secret-text 0") && !text.contains("second secret-text"),
            "{k}"
        );
        assert!(!k.starts_with(&format!("c/{}/", a.as_str())) && k != &format!("h/{}", a.as_str()));
    }
}

#[test]
fn putting_the_same_text_again_clears_the_tombstone() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, ids) = pair(dir.path(), 1);
    p.forget(&ids[0], "me", "x", None, Timestamp(2)).unwrap();
    m.forget(&ids[0], "me", "x", None, Timestamp(2)).unwrap();
    let make = || NewChunk::new("c", "k", "secret-text 0", Origin::User, "cli");
    assert_eq!(p.put(make(), Timestamp(3)).unwrap().id, ids[0]);
    m.put(make(), Timestamp(3)).unwrap();
    assert_eq!(p.tombstone(&ids[0]).unwrap(), None);
    assert_eq!(p.tombstone(&ids[0]).unwrap().as_ref(), m.tombstone(&ids[0]));
    assert_eq!(p.get(&ids[0]).unwrap().version(), 1);
    same_audit(&p, &m);
}

#[test]
fn purge_removes_the_tombstone_too_and_needs_a_person() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m, ids) = pair(dir.path(), 3);
    p.link(&ids[1], LinkKind::DerivedFrom, &ids[0], Timestamp(2))
        .unwrap();
    m.link(&ids[1], LinkKind::DerivedFrom, &ids[0], Timestamp(2))
        .unwrap();
    let who = PersonAuthority::from_person_action("typed-confirmation");
    // Forget then purge: the tombstone goes.
    p.forget(&ids[2], "me", "x", None, Timestamp(3)).unwrap();
    m.forget(&ids[2], "me", "x", None, Timestamp(3)).unwrap();
    assert_eq!(
        p.purge(&ids[2], &who, Timestamp(4)),
        m.purge(&ids[2], &who, Timestamp(4))
    );
    assert_eq!(p.tombstone(&ids[2]).unwrap(), None);
    assert_eq!(p.get(&ids[2]).err(), Some(Error::NotFound));
    // Purge a present chunk: derived chunks are flagged for review.
    assert_eq!(
        p.purge(&ids[0], &who, Timestamp(5)),
        m.purge(&ids[0], &who, Timestamp(5))
    );
    assert_eq!(p.get(&ids[0]).err(), Some(Error::NotFound));
    assert_eq!(p.tombstone(&ids[0]).unwrap(), None);
    assert_eq!(
        p.needs_review(&ids[1]).unwrap(),
        m.needs_review(&ids[1]).unwrap()
    );
    assert!(p.needs_review(&ids[1]).unwrap());
    // Nothing to purge.
    assert_eq!(p.purge(&ids[0], &who, Timestamp(6)), Err(Error::NotFound));
    listings_agree(&p, &m);
    same_audit(&p, &m);
    assert!(
        p.audit()
            .unwrap()
            .iter()
            .any(|e| e.action() == Action::Purge)
    );
}

#[test]
fn concurrent_writers_never_lose_or_duplicate_an_audit_entry() {
    let dir = tempfile::tempdir().unwrap();
    let a = open(dir.path());
    let b = open(dir.path());
    std::thread::scope(|s| {
        for (store, tag) in [(&a, "a"), (&b, "b")] {
            s.spawn(move || {
                for i in 0..10 {
                    let n = NewChunk::new("c", "k", &format!("{tag} {i}"), Origin::User, "cli");
                    store.put(n, Timestamp(1)).unwrap();
                }
            });
        }
    });
    let log = a.audit().unwrap();
    assert_eq!(log.len(), 20);
    let mut ids: Vec<_> = log.iter().filter_map(|e| e.id().cloned()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        20,
        "one entry per chunk, none lost, none doubled"
    );
}
