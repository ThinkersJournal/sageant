// SPDX-License-Identifier: MIT OR Apache-2.0
//! Erase-on-forget, read back from the raw files (PersistAnt 0.18.15). "Erased" is claimed for a backend only because
//! these tests read every database file after a forget and do not find the chunk's text or its search terms, on every
//! backend that claims it, even for a chunk that had been archived and superseded first (which rewrites its records).
//! The same steps with erasing switched off DO leave the text in the files (the control that shows the test can see).

use persistant::registry::Endpoint;
use sageant::{Actor, ChunkRef, NewChunk, Origin, Status, Timestamp, Writer};
use sageant_lexical::TextIndex;
use sageant_persistant::{
    Erasure, KvTextIndex, PersistStore, backend_erasure, backend_secure_delete,
};

const ONE: &str = "zqxmarkerversionone";
const TWO: &str = "zqxmarkerversiontwo";
const BACKENDS: [&str; 3] = ["sqlite-secure", "sqlite", "sqlite-full"];

/// Every byte of every file in `dir` (the database, a journal, a WAL), concatenated.
fn raw(dir: &std::path::Path) -> Vec<u8> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        all.extend(std::fs::read(entry.unwrap().path()).unwrap_or_default());
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

fn who() -> Actor {
    Actor::new("a", "b")
}

fn filler() -> String {
    "padding ".repeat(80)
}

/// A chunk with two versions, archived (which rewrites both records), then forgotten. Returns the raw files before
/// and after.
fn forget_a_rewritten_chunk(mut p: PersistStore, dir: &std::path::Path) -> (Vec<u8>, Vec<u8>) {
    let r = Writer::put(
        &mut p,
        NewChunk::new("c", "k", &format!("{ONE} {}", filler()), Origin::User, "t"),
        &who(),
        Timestamp(1),
    )
    .unwrap();
    Writer::supersede(
        &mut p,
        &r.id,
        &format!("{TWO} {}", filler()),
        Origin::User,
        "t",
        &who(),
        Timestamp(2),
    )
    .unwrap();
    Writer::set_status(&mut p, &r.id, Status::Archived, &who(), "old", Timestamp(3)).unwrap();
    let before = raw(dir);
    Writer::forget(&mut p, &r.id, &who(), "r", None, Timestamp(4)).unwrap();
    drop(p);
    (before, raw(dir))
}

#[test]
fn backends_say_how_they_get_rid_of_forgotten_bytes() {
    assert_eq!(backend_erasure("sqlite-secure").unwrap(), Erasure::Zeroed);
    assert_eq!(backend_erasure("sqlite").unwrap(), Erasure::EraseOnDelete);
    assert_eq!(
        backend_erasure("sqlite-full").unwrap(),
        Erasure::EraseOnDelete
    );
    assert!(backend_secure_delete("sqlite-secure").unwrap());
    assert!(!backend_secure_delete("sqlite").unwrap());
}

#[test]
fn a_forgotten_chunk_that_was_rewritten_first_is_gone_from_the_files_on_every_sqlite_backend() {
    for backend in BACKENDS {
        let dir = tempfile::tempdir().unwrap();
        let p = PersistStore::open(backend, &Endpoint::Path(dir.path().join("s.db"))).unwrap();
        let (before, after) = forget_a_rewritten_chunk(p, dir.path());
        assert!(
            contains(&before, ONE) || contains(&before, TWO),
            "{backend}: control: the content is in the files before the forget"
        );
        assert!(
            !contains(&after, ONE),
            "{backend}: version one is still in a file after forget"
        );
        assert!(
            !contains(&after, TWO),
            "{backend}: version two is still in a file after forget"
        );
    }
}

#[test]
fn with_erasing_switched_off_the_same_steps_leave_the_text_in_the_files() {
    for backend in ["sqlite", "sqlite-full"] {
        let dir = tempfile::tempdir().unwrap();
        let p = PersistStore::open_without_erasing_for_test(
            backend,
            &Endpoint::Path(dir.path().join("s.db")),
        )
        .unwrap();
        let (_, after) = forget_a_rewritten_chunk(p, dir.path());
        assert!(
            contains(&after, ONE) || contains(&after, TWO),
            "{backend}: control: a plain delete leaves the text, so the test can see it"
        );
    }
}

fn index_then_remove(backend: &str, erase: bool) -> (Vec<u8>, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::Path(dir.path().join("i.db"));
    let mut i = if erase {
        KvTextIndex::open(backend, &endpoint).unwrap()
    } else {
        KvTextIndex::open_without_erasing_for_test(backend, &endpoint).unwrap()
    };
    let id = sageant::ChunkId::parse(&"ab".repeat(32)).unwrap();
    for version in 1..=2 {
        let chunk = ChunkRef {
            id: id.clone(),
            version,
        };
        i.index(
            &chunk,
            &format!("{ONE} and a few more terms to index {version}"),
        )
        .unwrap();
    }
    let before = raw(dir.path());
    i.remove(&id).unwrap();
    drop(i);
    (before, raw(dir.path()))
}

#[test]
fn the_search_terms_of_a_forgotten_chunk_are_gone_from_the_index_files_on_every_sqlite_backend() {
    for backend in BACKENDS {
        let (before, after) = index_then_remove(backend, true);
        assert!(
            contains(&before, ONE),
            "{backend}: control: the term is in the index file"
        );
        assert!(
            !contains(&after, ONE),
            "{backend}: the forgotten chunk's term is still in the index file"
        );
    }
    for backend in ["sqlite", "sqlite-full"] {
        let (_, after) = index_then_remove(backend, false);
        assert!(
            contains(&after, ONE),
            "{backend}: control: without erasing the term stays"
        );
    }
}

#[test]
fn a_crash_between_the_forget_and_the_erasing_leaves_nothing_visible_and_the_next_open_finishes_the_job()
 {
    for backend in ["sqlite", "sqlite-full"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.db");
        let mut p = PersistStore::open(backend, &Endpoint::Path(path.clone())).unwrap();
        p.set_crash_before_erasing_for_test(true);
        let r = Writer::put(
            &mut p,
            NewChunk::new("c", "k", &format!("{ONE} {}", filler()), Origin::User, "t"),
            &who(),
            Timestamp(1),
        )
        .unwrap();
        let keep = Writer::put(
            &mut p,
            NewChunk::new("c", "k", "an unrelated chunk", Origin::User, "t"),
            &who(),
            Timestamp(1),
        )
        .unwrap();
        let _ = Writer::forget(&mut p, &r.id, &who(), "r", None, Timestamp(2));
        // The chunk is forgotten as far as every read can tell, though its bytes have not been erased yet.
        assert_eq!(p.get(&r.id).err(), Some(sageant::Error::Forgotten));
        assert_eq!(
            p.history(&r.id).err(),
            Some(sageant::Error::Forgotten),
            "{backend}"
        );
        let listed: Vec<_> = p
            .list(
                &sageant::Filter::new()
                    .include_archived()
                    .include_superseded(),
            )
            .unwrap();
        assert_eq!(
            listed.len(),
            1,
            "{backend}: only the unrelated chunk is listed"
        );
        assert_eq!(listed[0].id, keep.id);
        assert!(
            contains(&raw(dir.path()), ONE),
            "{backend}: control: the bytes are still there before the repair"
        );
        drop(p);
        // The next open sees a tombstone with content left behind and erases it.
        let p = PersistStore::open(backend, &Endpoint::Path(path)).unwrap();
        drop(p);
        assert!(
            !contains(&raw(dir.path()), ONE),
            "{backend}: opening again finished the erasing"
        );
    }
}

#[test]
fn a_new_store_is_chosen_with_secure_delete_when_persistant_offers_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_p, chosen) = PersistStore::open_chosen(&Endpoint::Path(dir.path().join("s.db"))).unwrap();
    assert_eq!(
        chosen.backend(),
        "sqlite-secure",
        "storage.secure_delete is a preference, so it ranks first"
    );
    assert!(chosen.secure_delete());
    assert_eq!(chosen.erasure(), Erasure::Zeroed);
}
