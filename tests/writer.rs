// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 7 of ROADMAP row 3: the `Writer` trait and one `Actor` type across every write (CONTRACT D11).
//! Written before the code. Every write through `Writer` is audited under the caller's `Actor`.

use sageant::{
    Action, Actor, Error, LinkKind, MemoryStore, NewChunk, Origin, PersonAuthority, Status,
    Timestamp, Writer,
};

fn w(s: &mut MemoryStore) -> &mut dyn Writer {
    s
}

fn actor() -> Actor {
    Actor::new("curator", "agent-7")
}

fn new(text: &str) -> NewChunk {
    NewChunk::new("notes", "note", text, Origin::Model, "MARKER-SRC")
}

#[test]
fn the_writer_trait_is_object_safe_and_every_write_is_audited_under_the_actor() {
    let mut s = MemoryStore::new();
    let w: &mut dyn Writer = &mut s;
    let a = w.put(new("one"), &actor(), Timestamp(1)).unwrap();
    let b = w.put(new("two"), &actor(), Timestamp(2)).unwrap();
    assert_eq!(a.version, 1);
    let v2 = w
        .supersede(
            &a.id,
            "one again",
            Origin::Model,
            "x",
            &actor(),
            Timestamp(3),
        )
        .unwrap();
    assert_eq!(v2.version, 2);
    w.link(&a.id, LinkKind::Cites, &b.id, &actor(), Timestamp(4))
        .unwrap();
    w.set_status(&a.id, Status::Archived, &actor(), "stale", Timestamp(5))
        .unwrap();
    w.set_status(&a.id, Status::Active, &actor(), "revived", Timestamp(6))
        .unwrap();
    w.forget(&b.id, &actor(), "wrong", None, Timestamp(7))
        .unwrap();
    let log = s.audit();
    let actions: Vec<_> = log.iter().map(|e| e.action()).collect();
    assert_eq!(
        actions,
        vec![
            Action::Put,
            Action::Put,
            Action::Supersede,
            Action::Link,
            Action::Archive,
            Action::Unarchive,
            Action::Forget
        ]
    );
    assert!(log.iter().all(|e| e.actor() == "curator/agent-7"));
    assert!(!format!("{log:?}").contains("MARKER-SRC"));
}

#[test]
fn set_status_round_trips_and_refuses_a_no_op() {
    let mut s = MemoryStore::new();
    let id = w(&mut s).put(new("x"), &actor(), Timestamp(1)).unwrap().id;
    assert_eq!(
        w(&mut s)
            .set_status(&id, Status::Active, &actor(), "r", Timestamp(2))
            .unwrap_err(),
        Error::BadState("already active")
    );
    w(&mut s)
        .set_status(&id, Status::Archived, &actor(), "r", Timestamp(3))
        .unwrap();
    assert_eq!(s.get(&id).unwrap().status(), Status::Archived);
    w(&mut s)
        .set_status(&id, Status::Active, &actor(), "r", Timestamp(4))
        .unwrap();
    assert_eq!(s.get(&id).unwrap().status(), Status::Active);
    assert_eq!(s.get_version(&id, 1).unwrap().status(), Status::Active);
}

#[test]
fn purge_through_the_writer_still_needs_a_person() {
    let mut s = MemoryStore::new();
    let id = w(&mut s).put(new("x"), &actor(), Timestamp(1)).unwrap().id;
    let person = PersonAuthority::from_person_action("typed confirmation");
    w(&mut s).purge(&id, &person, Timestamp(2)).unwrap();
    assert_eq!(s.get(&id).unwrap_err(), Error::NotFound);
    assert_eq!(s.audit().last().unwrap().actor(), "typed confirmation");
}

#[test]
fn writer_errors_match_the_store_and_a_refused_write_is_not_attributed_content() {
    let mut s = MemoryStore::new();
    w(&mut s).put(new("x"), &actor(), Timestamp(1)).unwrap();
    assert!(matches!(
        w(&mut s).put(new("x"), &actor(), Timestamp(2)),
        Err(Error::Conflict { .. })
    ));
    assert_eq!(
        Writer::put(&mut s, new(""), &actor(), Timestamp(3)).unwrap_err(),
        Error::Invalid("content")
    );
}

#[test]
fn an_actor_with_an_empty_role_or_agent_is_invalid() {
    let mut s = MemoryStore::new();
    for bad in [Actor::new("", "a"), Actor::new("r", "")] {
        assert_eq!(
            w(&mut s).put(new("y"), &bad, Timestamp(1)).unwrap_err(),
            Error::Invalid("actor")
        );
    }
    assert!(s.is_empty());
}
