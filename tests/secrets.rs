// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 2 of ROADMAP row 3: secrets are refused on write (CONTRACT D9; PM ruling 2026-11-14: RedactAnt's
//! registered-secrets route behind a thin SageAnt-side trait). Written before the code.

use sageant::{Error, MemoryStore, NewChunk, Origin, SecretScreen, Timestamp};

/// A test screen that flags any text containing "HUNTER2".
struct Flags;
impl SecretScreen for Flags {
    fn screen(&self, text: &str) -> Option<&'static str> {
        text.contains("HUNTER2").then_some("test-class")
    }
}

fn new(content: &str, source: &str) -> NewChunk {
    NewChunk::new("notes", "note", content, Origin::User, source)
}

#[test]
fn a_store_without_a_screen_accepts_the_text() {
    // Positive control for the refusals below: the same text goes in when no screen is set.
    let mut s = MemoryStore::new();
    assert!(
        s.put(new("the pass is HUNTER2", "cli"), Timestamp(1))
            .is_ok()
    );
}

#[test]
fn put_refuses_flagged_content_with_the_class_and_stores_nothing() {
    let mut s = MemoryStore::new();
    s.set_screen(Box::new(Flags));
    let err = s
        .put(new("the pass is HUNTER2", "cli"), Timestamp(1))
        .unwrap_err();
    assert_eq!(
        err,
        Error::SecretRefused {
            class: "test-class"
        }
    );
    assert_eq!(s.len(), 0);
    // The error never carries the text, in Debug or Display.
    assert!(!format!("{err:?}").contains("HUNTER2"));
    assert!(!err.to_string().contains("HUNTER2"));
}

#[test]
fn put_refuses_a_flagged_source_too() {
    let mut s = MemoryStore::new();
    s.set_screen(Box::new(Flags));
    let err = s
        .put(new("fine text", "https://u:HUNTER2@host"), Timestamp(1))
        .unwrap_err();
    assert_eq!(
        err,
        Error::SecretRefused {
            class: "test-class"
        }
    );
    assert_eq!(s.len(), 0);
}

#[test]
fn supersede_refuses_flagged_content_and_keeps_the_history() {
    let mut s = MemoryStore::new();
    s.set_screen(Box::new(Flags));
    let r = s.put(new("clean", "cli"), Timestamp(1)).unwrap();
    let err = s
        .supersede(&r.id, "now HUNTER2", Origin::User, "cli", Timestamp(2))
        .unwrap_err();
    assert_eq!(
        err,
        Error::SecretRefused {
            class: "test-class"
        }
    );
    assert_eq!(s.history(&r.id).unwrap().len(), 1);
    assert_eq!(s.get(&r.id).unwrap().content(), "clean");
}

#[test]
fn clean_text_passes_a_set_screen() {
    let mut s = MemoryStore::new();
    s.set_screen(Box::new(Flags));
    assert!(s.put(new("nothing secret", "cli"), Timestamp(1)).is_ok());
}

#[cfg(feature = "redactant")]
mod with_redactant {
    use super::*;
    use sageant::RegisteredSecrets;

    #[test]
    fn a_registered_secret_is_refused_by_class_registered_secret() {
        let mut secrets = RegisteredSecrets::new();
        secrets
            .register(&redactant::Secret::new(String::from("s3cr3t-value")))
            .unwrap();
        let mut s = MemoryStore::new();
        s.set_screen(Box::new(secrets));
        let err = s
            .put(new("token is s3cr3t-value ok", "cli"), Timestamp(1))
            .unwrap_err();
        assert_eq!(
            err,
            Error::SecretRefused {
                class: "registered-secret"
            }
        );
        assert_eq!(s.len(), 0);
        assert!(s.put(new("nothing here", "cli"), Timestamp(1)).is_ok());
    }

    #[test]
    fn a_too_short_secret_is_refused_at_registration_not_silently_ignored() {
        let mut secrets = RegisteredSecrets::new();
        assert_eq!(
            secrets.register(&redactant::Secret::new(String::from("abc"))),
            Err(redactant::RegisterError::TooShort { len: 3 })
        );
    }
}
