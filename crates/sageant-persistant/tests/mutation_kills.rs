// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kill tests for the mutants the row 4 run (docs/MUTATION.md, run 3) missed in `sageant-persistant`. They were
//! missed, so there was no red state to write first; the recorded rerun is the evidence they are now caught.

use persistant::registry::Endpoint;
use sageant::{ChunkId, Error, LinkKind, NewChunk, Origin, Reader, Timestamp};
use sageant_persistant::{PersistStore, PersistTextIndex};

fn open(dir: &std::path::Path) -> PersistStore {
    PersistStore::open("sqlite", &Endpoint::Path(dir.join("s.db"))).unwrap()
}

fn new(text: &str) -> NewChunk {
    NewChunk::new("docs", "note", text, Origin::User, "cli")
}

#[test]
fn debug_output_names_the_store_and_never_shows_content() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    p.put(new("very private words"), Timestamp(1)).unwrap();
    let shown = format!("{p:?}");
    assert!(
        shown.contains("PersistStore") && shown.contains("backend"),
        "{shown}"
    );
    assert!(!shown.contains("private"));
    let kv = PersistTextIndex::open("sqlite", &Endpoint::Path(dir.path().join("i.db"))).unwrap();
    let shown = format!("{kv:?}");
    assert!(
        shown.contains("PersistTextIndex") && shown.contains("backend"),
        "{shown}"
    );
}

#[test]
fn a_write_exactly_at_the_cap_is_allowed_and_one_byte_over_is_not() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = open(dir.path());
    p.set_byte_cap("docs", 10);
    assert!(p.put(new("0123456789"), Timestamp(1)).is_ok());
    assert_eq!(
        p.put(new("0123456789a"), Timestamp(1)).err(),
        Some(Error::TooLarge {
            measured: 11,
            cap: 10
        })
    );
}

#[test]
fn a_record_stored_under_the_wrong_id_or_version_is_corrupt_not_served() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let a = p.put(new("aaa"), Timestamp(1)).unwrap();
    let b = p.put(new("bbb"), Timestamp(1)).unwrap();
    p.supersede(&a.id, "aaa two", Origin::User, "cli", Timestamp(2))
        .unwrap();
    let a2 = p.get_version(&a.id, 2).unwrap().encode();
    let b1 = p.get(&b.id).unwrap().encode();
    // Another chunk's record under a's key: the id differs, the version agrees.
    p.corrupt_for_test(&a.id, 1, b1);
    assert!(matches!(p.get_version(&a.id, 1), Err(Error::Corrupt(_))));
    assert!(matches!(p.history(&a.id), Err(Error::Corrupt(_))));
    // The same chunk's version 2 under version 1's key: the id agrees, the version differs.
    p.corrupt_for_test(&a.id, 1, a2);
    assert!(matches!(p.get_version(&a.id, 1), Err(Error::Corrupt(_))));
    assert!(matches!(p.history(&a.id), Err(Error::Corrupt(_))));
}

#[test]
fn a_dump_lists_exactly_what_a_put_stored() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let a = p.put(new("hello there"), Timestamp(1)).unwrap();
    let keys: Vec<String> = p
        .dump_all_for_test()
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let id = a.id.as_str();
    assert_eq!(
        keys,
        vec![
            "a/00000000000000000001".to_owned(),
            format!("c/{id}/0000000001"),
            format!("h/{id}"),
            "m/aseq".to_owned(),
        ]
    );
    assert!(
        p.dump_all_for_test()
            .unwrap()
            .iter()
            .all(|(_, v)| !v.is_empty())
    );
}

#[test]
fn the_reader_impl_serves_history_and_links_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let a = p.put(new("one"), Timestamp(1)).unwrap().id;
    let b = p.put(new("two"), Timestamp(1)).unwrap().id;
    p.supersede(&a, "one again", Origin::User, "cli", Timestamp(2))
        .unwrap();
    p.link(&a, LinkKind::Related, &b, Timestamp(3)).unwrap();
    let r: &dyn Reader = &p;
    assert_eq!(r.history(&a).unwrap().len(), 2);
    let from: Vec<ChunkId> = r
        .links_from(&a)
        .unwrap()
        .iter()
        .map(|l| l.to().clone())
        .collect();
    assert_eq!(from, vec![b.clone()]);
    let to: Vec<ChunkId> = r
        .links_to(&b)
        .unwrap()
        .iter()
        .map(|l| l.from().clone())
        .collect();
    assert_eq!(to, vec![a]);
}
