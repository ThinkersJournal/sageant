// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 5 of ROADMAP row 3: an audited, caller-held, per-put override of the secret screen (CONTRACT D9).
//! Written before the code. The audit entry names the class and who allowed it, never the text.

use sageant::{
    Action, Error, MemoryStore, NewChunk, Origin, SecretOverride, SecretScreen, Timestamp,
};

struct Flags;
impl SecretScreen for Flags {
    fn screen(&self, text: &str) -> Option<&'static str> {
        text.contains("HUNTER2").then_some("test-class")
    }
}

fn new(content: &str) -> NewChunk {
    NewChunk::new("notes", "note", content, Origin::User, "cli")
}

fn store() -> MemoryStore {
    let mut s = MemoryStore::new();
    s.set_screen(Box::new(Flags));
    s
}

#[test]
fn without_an_override_the_text_is_still_refused() {
    // Positive control for the tests below.
    let mut s = store();
    assert!(matches!(
        s.put(new("pw HUNTER2"), Timestamp(1)),
        Err(Error::SecretRefused { .. })
    ));
}

#[test]
fn an_override_for_the_flagged_class_lets_the_put_through_and_is_audited() {
    let mut s = store();
    let allow = SecretOverride::allow_class("test-class", "alice, typed at the CLI");
    let r = s
        .put_overriding(new("pw HUNTER2"), &allow, Timestamp(1))
        .unwrap();
    assert_eq!(s.get(&r.id).unwrap().content(), "pw HUNTER2");
    let e = s.audit().last().unwrap();
    assert_eq!(e.action(), Action::Overridden);
    assert_eq!(e.reason(), "secret-override:test-class");
    assert_eq!(e.actor(), "alice, typed at the CLI");
    assert_eq!(e.id(), Some(&r.id));
    // The put itself is also in the log, and neither entry holds the text.
    assert!(s.audit().iter().any(|e| e.action() == Action::Put));
    assert!(!format!("{:?}", s.audit()).contains("HUNTER2"));
}

#[test]
fn an_override_for_another_class_does_not_apply() {
    let mut s = store();
    let allow = SecretOverride::allow_class("other-class", "alice");
    assert_eq!(
        s.put_overriding(new("pw HUNTER2"), &allow, Timestamp(1))
            .unwrap_err(),
        Error::SecretRefused {
            class: "test-class"
        }
    );
    assert_eq!(s.len(), 0);
    assert_eq!(s.audit().last().unwrap().action(), Action::Refused);
}

#[test]
fn an_override_on_clean_text_is_not_recorded() {
    let mut s = store();
    let allow = SecretOverride::allow_class("test-class", "alice");
    s.put_overriding(new("harmless"), &allow, Timestamp(1))
        .unwrap();
    assert!(s.audit().iter().all(|e| e.action() != Action::Overridden));
}

#[test]
fn an_override_covers_one_put_only() {
    let mut s = store();
    let allow = SecretOverride::allow_class("test-class", "alice");
    s.put_overriding(new("pw HUNTER2"), &allow, Timestamp(1))
        .unwrap();
    assert!(matches!(
        s.put(new("another HUNTER2"), Timestamp(2)),
        Err(Error::SecretRefused { .. })
    ));
}

#[test]
fn an_override_with_no_stated_person_is_invalid() {
    let mut s = store();
    let allow = SecretOverride::allow_class("test-class", "");
    assert_eq!(
        s.put_overriding(new("pw HUNTER2"), &allow, Timestamp(1))
            .unwrap_err(),
        Error::Invalid("override")
    );
}

#[test]
fn a_supersede_can_be_overridden_the_same_way() {
    let mut s = store();
    let id = s.put(new("plain"), Timestamp(1)).unwrap().id;
    assert!(matches!(
        s.supersede(&id, "now HUNTER2", Origin::User, "cli", Timestamp(2)),
        Err(Error::SecretRefused { .. })
    ));
    let allow = SecretOverride::allow_class("test-class", "alice");
    s.supersede_overriding(
        &id,
        "now HUNTER2",
        Origin::User,
        "cli",
        &allow,
        Timestamp(3),
    )
    .unwrap();
    assert_eq!(s.get(&id).unwrap().version(), 2);
    assert_eq!(s.audit().last().unwrap().action(), Action::Overridden);
    assert!(s.audit().iter().any(|e| e.action() == Action::Supersede));
}
