// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kill tests for the mutants the row 4 run (docs/MUTATION.md, run 3) missed in the core. These mutants were
//! missed, so there was no red state to write first; the recorded rerun is the evidence they are now caught.

use sageant::{
    Action, AuditEvent, Chunk, ChunkId, Confidence, Error, Filter, Link, LinkKind, NewChunk,
    Origin, Timestamp, Tombstone,
};

fn chunk() -> Chunk {
    Chunk::first_version(
        NewChunk::new("docs", "note", "hello", Origin::User, "cli"),
        Timestamp(1),
    )
    .unwrap()
}

#[test]
fn the_filter_accessors_report_what_was_asked() {
    let f = Filter::new();
    assert_eq!(f.confidence_filter(), None);
    assert!(!f.includes_superseded());
    assert_eq!(f.limit_value(), None);
    let f = Filter::new()
        .confidence(Confidence::Corroborated)
        .include_superseded()
        .limit(3);
    assert_eq!(f.confidence_filter(), Some(Confidence::Corroborated));
    assert!(f.includes_superseded());
    assert_eq!(f.limit_value(), Some(3));
}

fn replace(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let at = bytes
        .windows(from.len())
        .position(|w| w == from)
        .expect("the pattern is in the record");
    let mut out = bytes[..at].to_vec();
    out.extend_from_slice(to);
    out.extend_from_slice(&bytes[at + from.len()..]);
    out
}

#[test]
fn a_chunk_record_with_a_zero_version_or_an_empty_name_is_refused_as_bad_fields() {
    let good = chunk().encode();
    assert!(Chunk::decode(&good).is_ok());
    let zero_version = replace(&good, b"version 1\n1\n", b"version 1\n0\n");
    let no_collection = replace(&good, b"collection 4\ndocs\n", b"collection 0\n\n");
    let no_kind = replace(&good, b"kind 4\nnote\n", b"kind 0\n\n");
    for bad in [zero_version, no_collection, no_kind] {
        assert_eq!(Chunk::decode(&bad).err(), Some(Error::Corrupt("fields")));
    }
}

#[test]
fn trailing_bytes_after_any_record_are_named_as_such() {
    let id = ChunkId::parse(&"ab".repeat(32)).unwrap();
    let mut chunk = chunk().encode();
    chunk.push(b'x');
    let mut link = Link::new(id.clone(), 1, LinkKind::Related, id.clone(), Timestamp(1)).encode();
    link.push(b'x');
    let mut audit = AuditEvent::new(Timestamp(1), Action::Put, None, "a", "").encode();
    audit.push(b'x');
    let mut tomb = Tombstone::new(id, "docs", 1, Timestamp(1), "a", "", None).encode();
    tomb.push(b'x');
    for (name, got) in [
        ("chunk", Chunk::decode(&chunk).err()),
        ("link", Link::decode(&link).err()),
        ("audit", AuditEvent::decode(&audit).err()),
        ("tombstone", Tombstone::decode(&tomb).err()),
    ] {
        assert_eq!(got, Some(Error::Corrupt("trailing bytes")), "{name}");
    }
}
