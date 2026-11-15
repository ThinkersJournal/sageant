// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant-persistant` slice 3b-2: caps, the secret screen with audited overrides, and the `Writer` impl
//! (CONTRACT D5, D11). Written before the code. `MemoryStore` is the reference.

use persistant::registry::Endpoint;
use sageant::{
    Action, Actor, ChunkId, Error, LinkKind, MemoryStore, NewChunk, Origin, PersonAuthority,
    SecretOverride, SecretScreen, Status, Timestamp, Writer,
};
use sageant_persistant::PersistStore;

/// Flags text containing `TOKEN` as class `token` and `KEY` as class `key`.
struct Flags;
impl SecretScreen for Flags {
    fn screen(&self, text: &str) -> Option<&'static str> {
        if text.contains("TOKEN") {
            Some("token")
        } else if text.contains("KEY") {
            Some("key")
        } else {
            None
        }
    }
}

fn pair(dir: &std::path::Path) -> (PersistStore, MemoryStore) {
    let mut p = PersistStore::open("sqlite", &Endpoint::Path(dir.join("s.db"))).unwrap();
    let mut m = MemoryStore::new();
    p.set_screen(Box::new(Flags));
    m.set_screen(Box::new(Flags));
    p.set_byte_cap("small", 10);
    m.set_byte_cap("small", 10);
    (p, m)
}

fn new(collection: &str, text: &str) -> NewChunk {
    NewChunk::new(collection, "k", text, Origin::User, "cli")
}

type Entry = (u64, Action, Option<ChunkId>, String, String);

fn same_audit(p: &PersistStore, m: &MemoryStore) {
    let f = |es: &[sageant::AuditEvent]| -> Vec<Entry> {
        es.iter()
            .map(|e| {
                (
                    e.at().0,
                    e.action(),
                    e.id().cloned(),
                    e.actor().to_owned(),
                    e.reason().to_owned(),
                )
            })
            .collect()
    };
    assert_eq!(f(&p.audit().unwrap()), f(m.audit()));
}

#[test]
fn a_byte_cap_refuses_a_big_write_and_leaves_other_collections_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m) = pair(dir.path());
    let big = "x".repeat(11);
    assert_eq!(
        p.put(new("small", &big), Timestamp(1)).err(),
        m.put(new("small", &big), Timestamp(1)).err()
    );
    assert_eq!(
        p.put(new("small", &big), Timestamp(1)).err(),
        Some(Error::TooLarge {
            measured: 11,
            cap: 10
        })
    );
    let ok = p.put(new("small", "short"), Timestamp(1)).unwrap();
    m.put(new("small", "short"), Timestamp(1)).unwrap();
    assert_eq!(
        p.supersede(&ok.id, &big, Origin::User, "cli", Timestamp(2))
            .err(),
        m.supersede(&ok.id, &big, Origin::User, "cli", Timestamp(2))
            .err()
    );
    assert!(p.put(new("other", &big), Timestamp(1)).is_ok());
    assert!(m.put(new("other", &big), Timestamp(1)).is_ok());
    same_audit(&p, &m);
}

#[test]
fn the_screen_refuses_secrets_in_content_and_source_and_audits_the_refusal_without_them() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m) = pair(dir.path());
    let in_content = new("docs", "my TOKEN is here");
    let in_source = NewChunk::new("docs", "k", "fine", Origin::Web, "https://x/KEY");
    for n in [in_content, in_source] {
        let a = p.put(n.clone(), Timestamp(1)).err();
        assert_eq!(a, m.put(n, Timestamp(1)).err());
        assert!(matches!(a, Some(Error::SecretRefused { .. })));
    }
    let good = p.put(new("docs", "clean"), Timestamp(2)).unwrap();
    m.put(new("docs", "clean"), Timestamp(2)).unwrap();
    assert_eq!(
        p.supersede(&good.id, "now TOKEN", Origin::User, "cli", Timestamp(3))
            .err(),
        m.supersede(&good.id, "now TOKEN", Origin::User, "cli", Timestamp(3))
            .err()
    );
    same_audit(&p, &m);
    assert_eq!(p.get(&good.id).unwrap().version(), 1);
    let log = format!("{:?}", p.audit().unwrap());
    assert!(
        !log.contains("TOKEN") && !log.contains("https://x"),
        "refusals name the class only"
    );
    let all = p.dump_all_for_test().unwrap();
    assert!(
        all.iter()
            .all(|(_, v)| !String::from_utf8_lossy(v).contains("my TOKEN"))
    );
}

#[test]
fn an_override_must_name_the_class_and_a_person_and_is_audited() {
    let dir = tempfile::tempdir().unwrap();
    let (p, mut m) = pair(dir.path());
    let n = || new("docs", "a TOKEN on purpose");
    let right = SecretOverride::allow_class("token", "alice");
    let wrong = SecretOverride::allow_class("key", "alice");
    let nobody = SecretOverride::allow_class("token", "");
    assert_eq!(
        p.put_overriding(n(), &wrong, Timestamp(1)).err(),
        m.put_overriding(n(), &wrong, Timestamp(1)).err()
    );
    assert_eq!(
        p.put_overriding(n(), &nobody, Timestamp(1)).err(),
        Some(Error::Invalid("override"))
    );
    let a = p.put_overriding(n(), &right, Timestamp(2)).unwrap();
    let b = m.put_overriding(n(), &right, Timestamp(2)).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        p.supersede_overriding(
            &a.id,
            "again TOKEN",
            Origin::User,
            "cli",
            &right,
            Timestamp(3)
        )
        .unwrap(),
        m.supersede_overriding(
            &a.id,
            "again TOKEN",
            Origin::User,
            "cli",
            &right,
            Timestamp(3)
        )
        .unwrap()
    );
    same_audit(&p, &m);
    assert!(
        p.audit()
            .unwrap()
            .iter()
            .any(|e| e.action() == Action::Overridden)
    );
}

/// Run the same writes through the `Writer` trait on any store.
fn drive(w: &mut dyn Writer) -> Vec<String> {
    let who = Actor::new("harness", "agent-1");
    let a = w.put(new("docs", "alpha"), &who, Timestamp(1)).unwrap();
    let b = w.put(new("docs", "beta"), &who, Timestamp(1)).unwrap();
    let mut log = vec![format!("{a:?}{b:?}")];
    log.push(format!(
        "{:?}",
        w.supersede(&a.id, "alpha 2", Origin::Tool, "t", &who, Timestamp(2))
    ));
    log.push(format!(
        "{:?}",
        w.link(&a.id, LinkKind::Related, &b.id, &who, Timestamp(3))
    ));
    log.push(format!(
        "{:?}",
        w.link(&a.id, LinkKind::Related, &b.id, &who, Timestamp(3))
    ));
    log.push(format!(
        "{:?}",
        w.set_status(&b.id, Status::Archived, &who, "old", Timestamp(4))
    ));
    log.push(format!(
        "{:?}",
        w.set_status(&b.id, Status::Archived, &who, "old", Timestamp(4))
    ));
    log.push(format!(
        "{:?}",
        w.set_status(&b.id, Status::Active, &who, "back", Timestamp(5))
    ));
    log.push(format!(
        "{:?}",
        w.note(&a.id, &who, "because", Timestamp(6))
    ));
    log.push(format!(
        "{:?}",
        w.put(new("docs", "gamma"), &Actor::new("", "x"), Timestamp(7))
    ));
    log.push(format!(
        "{:?}",
        w.forget(&b.id, &who, "asked", Some("kh"), Timestamp(8))
    ));
    let c = w.put(new("docs", "delta"), &who, Timestamp(9)).unwrap();
    let person = PersonAuthority::from_person_action("typed-confirmation");
    log.push(format!("{:?}", w.purge(&c.id, &person, Timestamp(10))));
    log
}

#[test]
fn the_writer_impl_behaves_exactly_like_the_memory_stores_and_attributes_the_actor() {
    let dir = tempfile::tempdir().unwrap();
    let (mut p, mut m) = pair(dir.path());
    assert_eq!(drive(&mut p), drive(&mut m));
    same_audit(&p, &m);
    let log = p.audit().unwrap();
    assert!(
        log.iter().all(|e| e.actor() == "harness/agent-1"
            || e.actor() == "person"
            || !e.actor().is_empty())
    );
    assert!(
        log.iter().any(|e| e.actor() == "harness/agent-1"),
        "writes are attributed to the actor"
    );
}
