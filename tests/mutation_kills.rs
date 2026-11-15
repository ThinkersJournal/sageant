// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests added from the recorded mutation run (docs/MUTATION.md): each one kills a mutant the earlier suite let
//! survive. The mutants were missed (the suite stayed green with them applied), so there was no red state to write
//! first; docs/MUTATION.md records the re-run that shows them caught.

use sageant::{
    Actor, ChunkId, ChunkRef, DefaultFuser, Error, Filter, Fuser, Hit, MemoryStore, NewChunk,
    Origin, Query, Reader, Reason, Retriever, Timestamp,
};

#[test]
fn every_error_says_what_it_is() {
    let cases: [(Error, &str); 10] = [
        (
            Error::NotNormalised { at: 3 },
            "text not verifiably NFC (first non-ASCII byte at 3)",
        ),
        (
            Error::TooLarge {
                measured: 5,
                cap: 4,
            },
            "content is 5 bytes, cap is 4",
        ),
        (Error::NotFound, "not found"),
        (
            Error::Conflict {
                expected: 1,
                found: 2,
            },
            "conflict: expected version 1, found 2",
        ),
        (
            Error::SecretRefused { class: "k" },
            "refused: text matched a secret screen (k)",
        ),
        (Error::LinkCycle, "link would form a cycle"),
        (Error::DuplicateLink, "link already exists"),
        (Error::Forgotten, "forgotten"),
        (Error::BadState("why"), "bad state: why"),
        (Error::Invalid("field"), "empty field"),
    ];
    for (e, text) in cases {
        assert_eq!(e.to_string(), text);
    }
}

#[test]
fn an_actor_reads_back_its_role_and_agent() {
    let a = Actor::new("curator", "agent-7");
    assert_eq!(a.role(), "curator");
    assert_eq!(a.agent(), "agent-7");
}

#[test]
fn a_chunk_id_is_64_lowercase_hex_characters() {
    let mut s = MemoryStore::new();
    let id: ChunkId = s
        .put(
            NewChunk::new("c", "k", "text", Origin::User, "s"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    assert_eq!(id.as_str().len(), 64);
    assert!(
        id.as_str()
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    );
}

#[test]
fn a_query_hands_its_own_filter_to_the_retriever() {
    struct Listing;
    impl Retriever for Listing {
        fn retrieve(&self, q: &Query, store: &dyn Reader) -> Result<Vec<Hit>, Error> {
            Ok(store
                .list(q.filter_ref())?
                .into_iter()
                .map(|c| Hit::new(c, None, Reason::Recency))
                .collect())
        }
    }
    let mut s = MemoryStore::new();
    s.put(
        NewChunk::new("one", "k", "a", Origin::User, "s"),
        Timestamp(1),
    )
    .unwrap();
    s.put(
        NewChunk::new("two", "k", "b", Origin::User, "s"),
        Timestamp(1),
    )
    .unwrap();
    let q = Query::new("x").filter(Filter::new().collection("two"));
    let hits = Listing.retrieve(&q, &s).unwrap();
    assert_eq!(hits.len(), 1); // the default (empty) filter would return both
}

#[test]
fn on_an_equal_score_the_first_duplicate_wins() {
    let mut s = MemoryStore::new();
    let id = s
        .put(
            NewChunk::new("c", "k", "t", Origin::User, "s"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    let r = ChunkRef { id, version: 1 };
    let out = DefaultFuser.fuse(&[
        vec![Hit::new(r.clone(), Some(5), Reason::Custom("first".into()))],
        vec![Hit::new(r, Some(5), Reason::Custom("second".into()))],
    ]);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].reason(), &Reason::Custom("first".into()));
}

#[test]
fn len_and_is_empty_follow_what_is_stored() {
    let mut s = MemoryStore::new();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    s.put(
        NewChunk::new("c", "k", "t", Origin::User, "s"),
        Timestamp(1),
    )
    .unwrap();
    assert!(!s.is_empty());
    assert_eq!(s.len(), 1);
}

#[test]
fn the_store_debug_names_itself_and_counts_versions() {
    let mut s = MemoryStore::new();
    s.put(
        NewChunk::new("c", "k", "SECRET-BODY", Origin::User, "s"),
        Timestamp(1),
    )
    .unwrap();
    let text = format!("{s:?}");
    assert!(text.contains("MemoryStore") && text.contains("versions: 1"));
    assert!(!text.contains("SECRET-BODY")); // never the content
}

#[test]
fn reader_links_see_real_links_both_ways() {
    let mut s = MemoryStore::new();
    let a = s
        .put(
            NewChunk::new("c", "k", "aa", Origin::User, "s"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    let b = s
        .put(
            NewChunk::new("c", "k", "bb", Origin::User, "s"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    s.link(&a, sageant::LinkKind::Cites, &b, Timestamp(2))
        .unwrap();
    let r: &dyn Reader = &s;
    assert_eq!(r.links_from(&a).unwrap().len(), 1);
    assert_eq!(r.links_to(&b).unwrap().len(), 1);
}

#[test]
fn only_derived_from_links_to_the_forgotten_chunk_flag_review() {
    use sageant::LinkKind;
    let mut s = MemoryStore::new();
    let mut put = |t: &str| {
        s.put(NewChunk::new("c", "k", t, Origin::User, "s"), Timestamp(1))
            .unwrap()
            .id
    };
    let (gone, other, cites, derived_other, derived) = (
        put("gone"),
        put("other"),
        put("cites"),
        put("dother"),
        put("derived"),
    );
    s.link(&cites, LinkKind::Cites, &gone, Timestamp(2))
        .unwrap(); // right target, wrong kind
    s.link(&derived_other, LinkKind::DerivedFrom, &other, Timestamp(2))
        .unwrap(); // right kind, wrong target
    s.link(&derived, LinkKind::DerivedFrom, &gone, Timestamp(2))
        .unwrap();
    s.forget(&gone, "a", "r", None, Timestamp(3)).unwrap();
    assert!(s.needs_review(&derived).unwrap());
    assert!(!s.needs_review(&cites).unwrap());
    assert!(!s.needs_review(&derived_other).unwrap());
}

#[test]
fn text_that_is_all_non_ascii_is_still_checked() {
    // Two decomposed Hangul jamo: no ASCII byte at all, and not NFC (they compose to one syllable).
    let jamo = "\u{1100}\u{1161}";
    let mut s = MemoryStore::new();
    assert!(
        s.put(
            NewChunk::new("c", "k", jamo, Origin::User, "s"),
            Timestamp(1)
        )
        .is_err()
    );
    // A lone precomposed letter: accepted only when the `nfc` feature can verify it.
    let e = s.put(
        NewChunk::new("c", "k", "\u{e9}", Origin::User, "s"),
        Timestamp(1),
    );
    assert_eq!(e.is_ok(), cfg!(feature = "nfc"));
}
