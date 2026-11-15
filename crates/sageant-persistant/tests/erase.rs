// SPDX-License-Identifier: MIT OR Apache-2.0
//! Erase-on-forget, read back from the raw files (PersistAnt 0.18.12). The claim "erased from the store file" is
//! only made for a backend that has `storage.secure_delete`, and only because these tests read the database
//! files after the forget and do not find the content; the same steps on a backend without it DO find it (the
//! control that shows the test can see). Written before the code.

use persistant::registry::Endpoint;
use sageant::{ChunkRef, NewChunk, Origin, Timestamp, Writer};
use sageant_lexical::TextIndex;
use sageant_persistant::{KvTextIndex, PersistStore, backend_secure_delete};

const MARKER: &str = "zqxmarkeruniquecontent";

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

fn put_then_forget(backend: &str) -> (Vec<u8>, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = PersistStore::open(backend, &Endpoint::Path(dir.path().join("s.db"))).unwrap();
    let text = format!(
        "{MARKER} plus enough other words to fill a page {}",
        "padding ".repeat(80)
    );
    let r = Writer::put(
        &mut p,
        NewChunk::new("c", "k", &text, Origin::User, "t"),
        &sageant::Actor::new("a", "b"),
        Timestamp(1),
    )
    .unwrap();
    let before = raw(dir.path());
    Writer::forget(
        &mut p,
        &r.id,
        &sageant::Actor::new("a", "b"),
        "r",
        None,
        Timestamp(2),
    )
    .unwrap();
    drop(p);
    (before, raw(dir.path()))
}

#[test]
fn the_secure_backend_declares_secure_delete_and_the_plain_one_does_not() {
    assert!(backend_secure_delete("sqlite-secure").unwrap());
    assert!(!backend_secure_delete("sqlite").unwrap());
}

#[test]
fn a_forgotten_chunks_content_is_gone_from_the_files_on_the_secure_backend() {
    let (before, after) = put_then_forget("sqlite-secure");
    assert!(
        contains(&before, MARKER),
        "control: the content is in the file before the forget"
    );
    assert!(
        !contains(&after, MARKER),
        "the content is still in the store file after forget on sqlite-secure"
    );
}

#[test]
fn on_the_plain_backend_the_same_content_is_still_in_the_file_so_it_is_only_unretrievable() {
    let (before, after) = put_then_forget("sqlite");
    assert!(contains(&before, MARKER));
    assert!(
        contains(&after, MARKER),
        "control: without secure delete the freed page still holds the text"
    );
}

fn index_then_remove(backend: &str) -> (Vec<u8>, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let mut i = KvTextIndex::open(backend, &Endpoint::Path(dir.path().join("i.db"))).unwrap();
    let id = sageant::ChunkId::parse(&"ab".repeat(32)).unwrap();
    let chunk = ChunkRef {
        id: id.clone(),
        version: 1,
    };
    i.index(&chunk, &format!("{MARKER} and a few more terms to index"))
        .unwrap();
    let before = raw(dir.path());
    i.remove(&id).unwrap();
    drop(i);
    (before, raw(dir.path()))
}

#[test]
fn the_text_index_terms_of_a_forgotten_chunk_are_gone_too_on_the_secure_backend() {
    let (before, after) = index_then_remove("sqlite-secure");
    assert!(
        contains(&before, MARKER),
        "control: the term is in the index file"
    );
    assert!(
        !contains(&after, MARKER),
        "the forgotten chunk's term is still in the index file"
    );
    let (b, a) = index_then_remove("sqlite");
    assert!(
        contains(&b, MARKER) && contains(&a, MARKER),
        "control: plain sqlite keeps the term"
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
}
