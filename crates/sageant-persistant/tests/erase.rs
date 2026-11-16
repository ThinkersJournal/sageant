// SPDX-License-Identifier: MIT OR Apache-2.0
//! Erase-on-forget, read back from the raw files (PersistAnt 0.18.21). "Erased" is claimed only for `sqlite-secure`,
//! because these tests read every database file after forgetting up to thirty chunks (one of them archived and
//! superseded first, which rewrites its records) and do not find the text or its search terms. `sqlite` and
//! `sqlite-full` get a best-effort overwrite (`BatchOp::DeleteErased` in the same atomic batch) which helps for a few
//! chunks but measurably leaves stale fragments once several are forgotten (a test pins that), so they are
//! unretrievable, not erased. The same steps with the overwrite switched off DO leave the text in the files (the control
//! that shows the test can see).

use persistant::registry::Endpoint;
use sageant::{Actor, Error, NewChunk, Origin, Status, Timestamp, Writer};
use sageant_persistant::{Erasure, PersistStore, backend_erasure, backend_secure_delete};

const ONE: &str = "zqxmarkerversionone";
const TWO: &str = "zqxmarkerversiontwo";

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

#[test]
fn backends_say_how_they_get_rid_of_forgotten_bytes() {
    assert_eq!(backend_erasure("sqlite-secure").unwrap(), Erasure::Zeroed);
    assert_eq!(backend_erasure("sqlite").unwrap(), Erasure::Unretrievable);
    assert_eq!(
        backend_erasure("sqlite-full").unwrap(),
        Erasure::Unretrievable
    );
    assert!(Erasure::Zeroed.guarantees_erasure());
    assert!(!Erasure::Unretrievable.guarantees_erasure());
    assert!(backend_secure_delete("sqlite-secure").unwrap());
    assert!(!backend_secure_delete("sqlite").unwrap());
}

#[test]
fn a_forgotten_chunk_that_was_rewritten_first_is_gone_from_the_files_on_the_backend_that_guarantees_it()
 {
    let dir = tempfile::tempdir().unwrap();
    let mut p =
        PersistStore::open("sqlite-secure", &Endpoint::Path(dir.path().join("s.db"))).unwrap();
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
    let before = raw(dir.path());
    Writer::forget(&mut p, &r.id, &who(), "r", None, Timestamp(4)).unwrap();
    drop(p);
    let after = raw(dir.path());
    assert!(
        contains(&before, ONE) || contains(&before, TWO),
        "control: the content is in the files before the forget"
    );
    assert!(
        !contains(&after, ONE) && !contains(&after, TWO),
        "the text is still in a file after forget"
    );
}

/// Forget `n` chunks of about 700 bytes each; how many of them still have text in the files? `erase` is whether the
/// best-effort overwrite is on.
fn store_chunks_left(backend: &str, n: usize, erase: bool) -> usize {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::Path(dir.path().join("s.db"));
    let mut p = if erase {
        PersistStore::open(backend, &endpoint).unwrap()
    } else {
        PersistStore::open_without_erasing_for_test(backend, &endpoint).unwrap()
    };
    let mut ids = Vec::new();
    for i in 0..n {
        let text = format!("zqxscale{i:03} {}", filler());
        ids.push(
            Writer::put(
                &mut p,
                NewChunk::new("c", "k", &text, Origin::User, "t"),
                &who(),
                Timestamp(1),
            )
            .unwrap()
            .id,
        );
    }
    for id in &ids {
        Writer::forget(&mut p, id, &who(), "r", None, Timestamp(2)).unwrap();
    }
    drop(p);
    let files = raw(dir.path());
    (0..n)
        .filter(|i| contains(&files, &format!("zqxscale{i:03}")))
        .count()
}

#[test]
fn at_scale_the_secure_backend_leaves_no_copy_of_any_forgotten_chunk_or_term() {
    for n in [1, 3, 8, 30] {
        assert_eq!(store_chunks_left("sqlite-secure", n, true), 0, "{n} chunks");
    }
}

#[test]
fn plain_and_wal_sqlite_overwrite_a_few_but_keep_stale_fragments_once_several_are_forgotten() {
    // Measured with PersistAnt 0.18.21: the overwrite in the forget batch clears the text of one to three chunks, but
    // forgetting eight or thirty leaves copies of some of them in pages SQLite split or rebalanced earlier. If this ever
    // reports zero, a better erase exists: re-measure and only then change the README table.
    for backend in ["sqlite", "sqlite-full"] {
        for n in [1, 3] {
            assert_eq!(
                store_chunks_left(backend, n, true),
                0,
                "{backend}: {n} chunks are fully overwritten"
            );
        }
        for n in [8, 30] {
            assert!(
                store_chunks_left(backend, n, true) > 0,
                "{backend}: stale fragments remain at {n} chunks, so the word erased is not used"
            );
        }
    }
}

#[test]
fn with_the_overwrite_switched_off_a_plain_delete_leaves_the_text_so_the_test_can_see() {
    for backend in ["sqlite", "sqlite-full"] {
        assert!(
            store_chunks_left(backend, 3, false) > 0,
            "{backend}: control: a plain delete leaves the text"
        );
    }
}

#[test]
fn a_backend_that_refuses_erased_batches_still_forgets_and_remembers_not_to_ask_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = PersistStore::open("sqlite", &Endpoint::Path(dir.path().join("s.db"))).unwrap();
    let a = Writer::put(
        &mut p,
        NewChunk::new("c", "k", "first chunk text", Origin::User, "t"),
        &who(),
        Timestamp(1),
    )
    .unwrap();
    let b = Writer::put(
        &mut p,
        NewChunk::new("c", "k", "second chunk text", Origin::User, "t"),
        &who(),
        Timestamp(1),
    )
    .unwrap();
    assert!(
        p.overwrites_on_forget_for_test(),
        "a plain sqlite store starts out asking for overwriting deletes"
    );
    p.refuse_erased_batches_for_test(true);
    Writer::forget(&mut p, &a.id, &who(), "r", None, Timestamp(2)).unwrap();
    assert!(
        !p.overwrites_on_forget_for_test(),
        "after the refusal it uses plain deletes from now on"
    );
    assert_eq!(p.get(&a.id).err(), Some(Error::Forgotten));
    // It switched itself to plain deletes: the next forget never asks for an erased batch (so the refusal never fires).
    p.refuse_erased_batches_for_test(false);
    Writer::forget(&mut p, &b.id, &who(), "r", None, Timestamp(3)).unwrap();
    assert_eq!(p.get(&b.id).err(), Some(Error::Forgotten));
    assert!(
        p.list(&sageant::Filter::new().include_archived())
            .unwrap()
            .is_empty()
    );
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
