// SPDX-License-Identifier: MIT OR Apache-2.0
//! Row 4: building chunk versions without a store (CONTRACT D17). `sageant-persistant` must not reimplement the
//! rules for a valid chunk, so the core exposes them: `Chunk::first_version` and `Chunk::next_version` validate,
//! hash and derive ids exactly as `MemoryStore` does. Written before the code.

use sageant::{Chunk, Error, MemoryStore, NewChunk, Origin, Timestamp};

fn new(text: &str) -> NewChunk {
    NewChunk::new("notes", "note", text, Origin::User, "cli")
}

#[test]
fn a_first_version_matches_what_the_memory_store_stores() {
    let c = Chunk::first_version(new("hello"), Timestamp(5)).unwrap();
    let mut s = MemoryStore::new();
    let r = s.put(new("hello"), Timestamp(5)).unwrap();
    assert_eq!(c.id(), &r.id);
    assert_eq!(c.version(), 1);
    assert_eq!(c.encode(), s.get(&r.id).unwrap().encode());
}

#[test]
fn a_first_version_is_validated_like_put() {
    assert_eq!(
        Chunk::first_version(NewChunk::new("", "k", "x", Origin::User, "s"), Timestamp(1))
            .unwrap_err(),
        Error::Invalid("collection")
    );
    assert_eq!(
        Chunk::first_version(NewChunk::new("c", "", "x", Origin::User, "s"), Timestamp(1))
            .unwrap_err(),
        Error::Invalid("kind")
    );
    assert_eq!(
        Chunk::first_version(new(""), Timestamp(1)).unwrap_err(),
        Error::Invalid("content")
    );
    assert!(matches!(
        Chunk::first_version(new("cafe\u{301}"), Timestamp(1)),
        Err(Error::NotNormalised { .. })
    ));
}

#[test]
fn a_next_version_keeps_the_lineage_and_matches_supersede() {
    let first = Chunk::first_version(new("one"), Timestamp(1)).unwrap();
    let second = first
        .next_version("two", Origin::Model, "m", Timestamp(2))
        .unwrap();
    let mut s = MemoryStore::new();
    let id = s.put(new("one"), Timestamp(1)).unwrap().id;
    s.supersede(&id, "two", Origin::Model, "m", Timestamp(2))
        .unwrap();
    assert_eq!((second.id(), second.version()), (first.id(), 2));
    assert_eq!(second.encode(), s.get_version(&id, 2).unwrap().encode());
}

#[test]
fn a_next_version_refuses_unchanged_or_invalid_text_and_keeps_status() {
    let first = Chunk::first_version(new("one"), Timestamp(1)).unwrap();
    assert_eq!(
        first
            .next_version("one", Origin::User, "cli", Timestamp(2))
            .unwrap_err(),
        Error::Conflict {
            expected: 1,
            found: 1
        }
    );
    assert_eq!(
        first
            .next_version("", Origin::User, "cli", Timestamp(2))
            .unwrap_err(),
        Error::Invalid("content")
    );
}

#[test]
fn the_backend_error_says_what_failed_without_content() {
    let e = Error::Backend("connection refused".to_owned());
    assert_eq!(e.to_string(), "storage backend: connection refused");
}

#[test]
fn with_status_changes_only_the_status() {
    use sageant::Status;
    let c = Chunk::first_version(new("hello"), Timestamp(5)).unwrap();
    let a = c.with_status(Status::Archived);
    assert_eq!(a.status(), Status::Archived);
    assert_eq!(a.content(), c.content());
    assert_eq!(a.id(), c.id());
    assert_eq!(a.version(), c.version());
    assert_ne!(a.encode(), c.encode());
    assert_eq!(a.with_status(Status::Active).encode(), c.encode());
}
