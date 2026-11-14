// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stored records (CONTRACT D17): a canonical, versioned, zero-dependency byte format for chunks and links, owned by
//! the core so a backend never builds a `Chunk` by hand. Decoding verifies everything it can (content hash, NFC, the
//! id of a first version), so a corrupt or forged record is refused, never trusted. Written before the code.

use sageant::{Chunk, ChunkId, Error, Link, LinkKind, MemoryStore, NewChunk, Origin, Timestamp};

fn store_with(text: &str) -> (MemoryStore, ChunkId) {
    let mut s = MemoryStore::new();
    let id = s
        .put(
            NewChunk::new("c", "k", text, Origin::User, "s"),
            Timestamp(7),
        )
        .unwrap()
        .id;
    (s, id)
}

#[test]
fn the_chunk_format_is_stable_and_canonical() {
    let (s, id) = store_with("hi");
    let c = s.get(&id).unwrap();
    let hash = c.content_hash();
    let want = format!(
        "sageant-chunk/1\nid 64\n{id}\nversion 1\n1\ncollection 1\nc\nkind 1\nk\ncontent 2\nhi\ncontent_hash 64\n{hash}\norigin 4\nUser\nsource 1\ns\nstatus 6\nActive\ncreated_at 1\n7\n",
        id = id.as_str()
    );
    assert_eq!(c.encode(), want.as_bytes());
}

#[test]
fn a_chunk_round_trips_including_newlines_and_every_origin_and_status() {
    for (text, origin) in [
        (
            "line one\nline two\n\nsageant-chunk/1\nstatus 6",
            Origin::User,
        ),
        ("a", Origin::Tool),
        ("b", Origin::Web),
        ("c", Origin::Agent),
        ("d", Origin::Model),
    ] {
        let mut s = MemoryStore::new();
        let id = s
            .put(
                NewChunk::new("c", "k", text, origin, "src\nwith newline"),
                Timestamp(9),
            )
            .unwrap()
            .id;
        s.archive(&id, "x", "r", Timestamp(10)).unwrap();
        let c = s.get(&id).unwrap();
        let back = Chunk::decode(&c.encode()).unwrap();
        assert_eq!(back.id(), c.id());
        assert_eq!(back.content(), text);
        assert_eq!(back.source(), "src\nwith newline");
        assert_eq!(back.origin(), origin);
        assert_eq!(back.status(), c.status());
        assert_eq!(back.created_at(), Timestamp(9));
        assert_eq!(back.encode(), c.encode()); // canonical: decode then encode is the identity
    }
}

#[test]
fn a_later_version_round_trips_and_keeps_the_lineage_id() {
    let (mut s, id) = store_with("one");
    s.supersede(&id, "two", Origin::Model, "m", Timestamp(8))
        .unwrap();
    let v2 = s.get_version(&id, 2).unwrap();
    let back = Chunk::decode(&v2.encode()).unwrap();
    assert_eq!((back.version(), back.id()), (2, &id));
}

#[test]
fn every_truncation_of_a_record_is_refused() {
    let (s, id) = store_with("some text");
    let bytes = s.get(&id).unwrap().encode();
    for cut in 0..bytes.len() {
        assert!(
            Chunk::decode(&bytes[..cut]).is_err(),
            "prefix of {cut} bytes was accepted"
        );
    }
    assert!(Chunk::decode(&bytes).is_ok()); // positive control: the whole record is fine
}

#[test]
fn tampering_is_refused() {
    let (s, id) = store_with("some text");
    let good = String::from_utf8(s.get(&id).unwrap().encode()).unwrap();
    let corrupt = |bad: String| Chunk::decode(bad.as_bytes()).unwrap_err();
    // Content changed, hash not: the hash check fires.
    assert!(matches!(
        corrupt(good.replace("some text", "some TEXT")),
        Error::Corrupt(_)
    ));
    // A first version whose id is not derived from its collection and content.
    let other = good.replacen(id.as_str(), &"0".repeat(64), 1);
    assert!(matches!(corrupt(other), Error::Corrupt(_)));
    // Header, trailing bytes, duplicate or reordered fields, a version of 0, a bad origin.
    assert!(matches!(
        corrupt(good.replacen("chunk/1", "chunk/2", 1)),
        Error::Corrupt(_)
    ));
    assert!(matches!(corrupt(format!("{good}x")), Error::Corrupt(_)));
    assert!(matches!(
        corrupt(good.replacen("version 1\n1", "version 1\n0", 1)),
        Error::Corrupt(_)
    ));
    assert!(matches!(
        corrupt(good.replacen("User", "Nope", 1)),
        Error::Corrupt(_)
    ));
    assert!(matches!(
        corrupt(good.replacen("kind 1\nk\n", "", 1)),
        Error::Corrupt(_)
    ));
    // Length prefix lying about the content.
    assert!(matches!(
        corrupt(good.replacen("content 9", "content 3", 1)),
        Error::Corrupt(_)
    ));
    assert!(Chunk::decode(&[0xff, 0xfe, 0x00]).is_err()); // not even text
}

#[test]
fn links_round_trip_and_are_verified() {
    let (mut s, a) = store_with("aaa");
    let b = s
        .put(
            NewChunk::new("c", "k", "bbb", Origin::Web, "w"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    s.link(&a, LinkKind::Cites, &b, Timestamp(11)).unwrap();
    let link: Link = s.links_from(&a).unwrap()[0].clone();
    let bytes = link.encode();
    assert_eq!(
        bytes,
        format!(
            "sageant-link/1\nfrom 64\n{a}\nfrom_version 1\n1\nkind 5\nCites\nto 64\n{b}\ncreated_at 2\n11\n",
            a = a.as_str(),
            b = b.as_str()
        )
        .as_bytes()
    );
    assert_eq!(Link::decode(&bytes).unwrap(), link);
    for cut in 0..bytes.len() {
        assert!(Link::decode(&bytes[..cut]).is_err());
    }
    let bad = String::from_utf8(bytes)
        .unwrap()
        .replacen("Cites", "Cited", 1);
    assert!(matches!(
        Link::decode(bad.as_bytes()),
        Err(Error::Corrupt(_))
    ));
}

#[test]
fn chunk_ids_parse_only_from_64_lowercase_hex() {
    let (_, id) = store_with("x");
    assert_eq!(ChunkId::parse(id.as_str()).unwrap(), id);
    for bad in [
        "",
        "abc",
        &"A".repeat(64),
        &"g".repeat(64),
        &format!("{}0", id.as_str()),
    ] {
        assert_eq!(
            ChunkId::parse(bad).unwrap_err(),
            Error::Invalid("id"),
            "{bad}"
        );
    }
}

#[cfg(feature = "nfc")]
#[test]
fn non_ascii_nfc_text_round_trips_and_a_non_nfc_record_is_refused() {
    let (s, id) = store_with("caf\u{e9}");
    let c = s.get(&id).unwrap();
    assert_eq!(Chunk::decode(&c.encode()).unwrap().content(), "caf\u{e9}");
    let (s2, id2) = store_with("cafe");
    let good = String::from_utf8(s2.get(&id2).unwrap().encode()).unwrap();
    // Swap in decomposed text and fix the length prefix; the hash no longer matches, so it is refused either way.
    let bad = good.replacen("content 4\ncafe", "content 6\ncafe\u{301}", 1);
    assert!(matches!(
        Chunk::decode(bad.as_bytes()),
        Err(Error::Corrupt(_))
    ));
}
