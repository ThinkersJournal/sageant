// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 8 of ROADMAP row 3: the `nfc` feature (CONTRACT D4). With it, the core accepts non-ASCII text that is
//! already NFC and refuses the rest with `NotNormalised`; a helper normalises. Written before the code.
#![cfg(feature = "nfc")]

use sageant::{Error, MemoryStore, NewChunk, Origin, Timestamp, is_nfc, normalise_nfc};

const COMPOSED: &str = "caf\u{e9}"; // e-acute, one char
const DECOMPOSED: &str = "cafe\u{301}"; // e + combining acute

fn new(text: &str) -> NewChunk {
    NewChunk::new("notes", "note", text, Origin::User, "cli")
}

#[test]
fn the_helper_composes_and_leaves_nfc_alone() {
    assert_eq!(normalise_nfc(DECOMPOSED), COMPOSED);
    assert_eq!(normalise_nfc(COMPOSED), COMPOSED);
    assert_eq!(normalise_nfc("plain ascii"), "plain ascii");
    assert!(is_nfc(COMPOSED) && is_nfc("ascii"));
    assert!(!is_nfc(DECOMPOSED)); // positive control: the check can say no
}

#[test]
fn nfc_non_ascii_text_is_accepted_and_stored_exactly() {
    let mut s = MemoryStore::new();
    let id = s.put(new(COMPOSED), Timestamp(1)).unwrap().id;
    assert_eq!(s.get(&id).unwrap().content(), COMPOSED);
}

#[test]
fn non_nfc_text_is_refused_with_the_first_differing_byte() {
    let mut s = MemoryStore::new();
    // "abc " is 4 bytes; the NFC form of the rest differs from byte 4 on ("e" becomes the 2-byte e-acute).
    let err = s.put(new("abc e\u{301}"), Timestamp(1)).unwrap_err();
    assert_eq!(err, Error::NotNormalised { at: 4 });
    assert!(s.is_empty());
}

#[test]
fn normalising_first_gives_the_same_id_for_both_spellings() {
    let mut s = MemoryStore::new();
    let id = s.put(new(COMPOSED), Timestamp(1)).unwrap().id;
    let mut t = MemoryStore::new();
    let id2 = t
        .put(new(&normalise_nfc(DECOMPOSED)), Timestamp(1))
        .unwrap()
        .id;
    assert_eq!(id, id2);
}

#[test]
fn supersede_applies_the_same_rule() {
    let mut s = MemoryStore::new();
    let id = s.put(new("plain"), Timestamp(1)).unwrap().id;
    assert_eq!(
        s.supersede(&id, DECOMPOSED, Origin::User, "cli", Timestamp(2))
            .unwrap_err(),
        Error::NotNormalised { at: 3 }
    );
    s.supersede(&id, COMPOSED, Origin::User, "cli", Timestamp(3))
        .unwrap();
}
