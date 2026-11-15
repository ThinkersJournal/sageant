//! Canonical stored forms of audit entries and tombstones (CONTRACT D17), written before the code.

use sageant::{Action, AuditEvent, ChunkId, Error, Timestamp, Tombstone};

fn id() -> ChunkId {
    ChunkId::parse(&"ab".repeat(32)).unwrap()
}

const ALL: [Action; 10] = [
    Action::Put,
    Action::Supersede,
    Action::Link,
    Action::Archive,
    Action::Unarchive,
    Action::Note,
    Action::Forget,
    Action::Purge,
    Action::Refused,
    Action::Overridden,
];

#[test]
fn action_names_round_trip_and_are_distinct() {
    let mut names = Vec::new();
    for a in ALL {
        assert_eq!(Action::parse(a.name()).unwrap(), a);
        names.push(a.name());
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ALL.len());
    assert!(Action::parse("nope").is_err());
}

#[test]
fn an_audit_entry_round_trips_with_and_without_an_id() {
    for id in [Some(id()), None] {
        let e = AuditEvent::new(
            Timestamp(7),
            Action::Note,
            id,
            "role/agent",
            "a reason\nwith lines",
        );
        let back = AuditEvent::decode(&e.encode()).unwrap();
        assert_eq!(back, e);
        assert_eq!(back.encode(), e.encode());
    }
}

#[test]
fn a_tombstone_round_trips_with_and_without_a_keyed_hash() {
    for h in [Some("k"), None] {
        let t = Tombstone::new(id(), "docs", 3, Timestamp(9), "role/agent", "why", h);
        let back = Tombstone::decode(&t.encode()).unwrap();
        assert_eq!(back, t);
        assert_eq!(back.versions_removed(), 3);
        assert_eq!(back.keyed_hash(), h);
    }
}

#[test]
fn damaged_audit_and_tombstone_records_are_corrupt() {
    let e = AuditEvent::new(Timestamp(7), Action::Put, Some(id()), "a", "").encode();
    let t = Tombstone::new(id(), "docs", 1, Timestamp(9), "a", "", None).encode();
    for good in [e, t] {
        for cut in [0, 5, good.len() - 1] {
            assert!(matches!(
                AuditEvent::decode(&good[..cut]),
                Err(Error::Corrupt(_))
            ));
            assert!(matches!(
                Tombstone::decode(&good[..cut]),
                Err(Error::Corrupt(_))
            ));
        }
        let mut extra = good.clone();
        extra.push(b'x');
        assert!(AuditEvent::decode(&extra).is_err() && Tombstone::decode(&extra).is_err());
    }
    // Each decoder refuses the other's record.
    let t = Tombstone::new(id(), "docs", 1, Timestamp(9), "a", "", None).encode();
    assert!(AuditEvent::decode(&t).is_err());
}
