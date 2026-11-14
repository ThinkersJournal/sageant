// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 1 of ROADMAP row 3: chunk model, content hash, versioned put/supersede/get/history.
//! Written before the code (red first); every assertion is on an exact value.

use sageant::{Confidence, Error, MemoryStore, NewChunk, Origin, Status, Timestamp, sha256_hex};

fn new(collection: &str, content: &str) -> NewChunk {
    NewChunk::new(collection, "note", content, Origin::User, "cli")
}

#[test]
fn sha256_matches_nist_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // Two-block message (56 bytes): exercises the padding boundary.
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
}

#[test]
fn sha256_padding_boundaries_match_sha256sum() {
    // Expected values taken from coreutils `sha256sum` over n copies of b'a' (2026-11-14).
    let cases = [
        (
            55,
            "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
        ),
        (
            56,
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
        ),
        (
            63,
            "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
        ),
        (
            64,
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
        ),
        (
            65,
            "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
        ),
        (
            1000,
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3",
        ),
    ];
    for (n, want) in cases {
        assert_eq!(sha256_hex(&vec![b'a'; n]), want, "length {n}");
    }
}

#[test]
fn put_then_get_returns_exactly_what_was_stored() {
    let mut s = MemoryStore::new();
    let r = s
        .put(new("notes", "the sky is blue"), Timestamp(1_000))
        .unwrap();
    assert_eq!(r.version, 1);
    let c = s.get(&r.id).unwrap();
    assert_eq!(c.content(), "the sky is blue");
    assert_eq!(c.collection(), "notes");
    assert_eq!(c.kind(), "note");
    assert_eq!(c.version(), 1);
    assert_eq!(c.created_at(), Timestamp(1_000));
    assert_eq!(c.origin(), Origin::User);
    assert_eq!(c.source(), "cli");
    assert_eq!(c.status(), Status::Active);
    assert_eq!(c.confidence(), Confidence::Unconfirmed); // new = one strike
    assert_eq!(c.content_hash(), sha256_hex(b"the sky is blue"));
}

#[test]
fn ids_are_deterministic_and_collection_scoped() {
    let mut a = MemoryStore::new();
    let mut b = MemoryStore::new();
    let ra = a.put(new("notes", "x"), Timestamp(1)).unwrap();
    let rb = b.put(new("notes", "x"), Timestamp(999)).unwrap(); // time does not enter the id
    assert_eq!(ra.id, rb.id);
    let rc = a.put(new("other", "x"), Timestamp(1)).unwrap(); // same text, other collection
    assert_ne!(ra.id, rc.id);
}

#[test]
fn identical_put_is_a_conflict_not_a_silent_duplicate() {
    let mut s = MemoryStore::new();
    let r = s.put(new("notes", "x"), Timestamp(1)).unwrap();
    let err = s.put(new("notes", "x"), Timestamp(2)).unwrap_err();
    assert_eq!(
        err,
        Error::Conflict {
            expected: 0,
            found: 1
        }
    );
    assert_eq!(s.history(&r.id).unwrap().len(), 1);
}

#[test]
fn supersede_appends_a_version_and_keeps_the_old_one_readable() {
    let mut s = MemoryStore::new();
    let r1 = s.put(new("notes", "v one"), Timestamp(1)).unwrap();
    let r2 = s
        .supersede(&r1.id, "v two", Origin::Model, "summariser", Timestamp(2))
        .unwrap();
    assert_eq!((r2.id.clone(), r2.version), (r1.id.clone(), 2));
    assert_eq!(s.get(&r1.id).unwrap().content(), "v two");
    assert_eq!(s.get(&r1.id).unwrap().origin(), Origin::Model);
    let h: Vec<_> = s
        .history(&r1.id)
        .unwrap()
        .iter()
        .map(|c| (c.version(), c.content().to_owned()))
        .collect();
    assert_eq!(h, vec![(1, "v one".to_owned()), (2, "v two".to_owned())]);
    assert_eq!(s.get_version(&r1.id, 1).unwrap().content(), "v one");
    // The superseded version is out of the default read but present on request.
    assert_eq!(s.get_version(&r1.id, 3).unwrap_err(), Error::NotFound);
}

#[test]
fn supersede_with_unchanged_text_is_refused() {
    let mut s = MemoryStore::new();
    let r = s.put(new("notes", "same"), Timestamp(1)).unwrap();
    let err = s
        .supersede(&r.id, "same", Origin::User, "cli", Timestamp(2))
        .unwrap_err();
    assert_eq!(
        err,
        Error::Conflict {
            expected: 1,
            found: 1
        }
    );
    assert_eq!(s.history(&r.id).unwrap().len(), 1);
}

#[test]
fn get_of_unknown_id_is_not_found() {
    let mut s = MemoryStore::new();
    let r = s.put(new("notes", "x"), Timestamp(1)).unwrap();
    let other = MemoryStore::new();
    assert_eq!(other.get(&r.id).unwrap_err(), Error::NotFound);
    assert_eq!(other.history(&r.id).unwrap_err(), Error::NotFound);
}

#[test]
fn non_ascii_text_is_refused_until_the_nfc_helper_exists() {
    // The core can verify only ASCII as normalised (CONTRACT D4); anything else is refused, not trusted.
    let mut s = MemoryStore::new();
    let err = s.put(new("notes", "caf\u{e9}"), Timestamp(1)).unwrap_err();
    assert_eq!(err, Error::NotNormalised { at: 3 }); // byte offset of the first non-ASCII byte
}

#[test]
fn collection_cap_is_enforced_with_the_measured_size() {
    let mut s = MemoryStore::new();
    s.set_byte_cap("small", 4);
    let err = s.put(new("small", "12345"), Timestamp(1)).unwrap_err();
    assert_eq!(
        err,
        Error::TooLarge {
            measured: 5,
            cap: 4
        }
    );
    assert!(s.put(new("small", "1234"), Timestamp(1)).is_ok()); // exactly at the cap passes
    assert!(s.put(new("big", "12345"), Timestamp(1)).is_ok()); // other collections are not capped
}

#[test]
fn empty_names_and_empty_content_are_refused() {
    let mut s = MemoryStore::new();
    assert_eq!(
        s.put(new("", "x"), Timestamp(1)).unwrap_err(),
        Error::Invalid("collection")
    );
    assert_eq!(
        s.put(new("notes", ""), Timestamp(1)).unwrap_err(),
        Error::Invalid("content")
    );
}

#[test]
fn refused_write_changes_nothing() {
    let mut s = MemoryStore::new();
    let _ = s.put(new("notes", "caf\u{e9}"), Timestamp(1));
    assert_eq!(s.len(), 0);
}
