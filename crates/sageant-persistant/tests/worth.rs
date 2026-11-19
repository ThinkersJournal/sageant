// SPDX-License-Identifier: MIT OR Apache-2.0
//! X1 slice 2 on PersistAnt: worth snapshots under `w/<chunk id>/<version, 10 digits>/<model id>`. Written before the
//! code. `MemoryStore` is the reference. A snapshot is derived data, so `forget` and `purge` must take it in the same
//! atomic batch as the content, and a write racing a forget must never leave one behind.

use persistant::registry::Endpoint;
use sageant::{
    Actor, ChunkId, Error, LinkKind, MemoryStore, NewChunk, Origin, PersonAuthority, Reader,
    Timestamp, WorthParams, WorthSnapshot, WorthStore, Writer, worth_of,
};
use sageant_persistant::PersistStore;

fn open(dir: &std::path::Path) -> PersistStore {
    PersistStore::open("sqlite", &Endpoint::Path(dir.join("sageant.db"))).unwrap()
}

fn who() -> Actor {
    Actor::new("a", "b")
}

fn make(text: &str) -> NewChunk {
    NewChunk::new("c", "k", text, Origin::User, "cli")
}

fn params() -> WorthParams {
    WorthParams::new(100)
}

fn snap(p: &PersistStore, id: &ChunkId, now: u64) -> WorthSnapshot {
    let w = worth_of(p, id, Timestamp(now), &params()).unwrap();
    p.record_worth(id, &w, Timestamp(now)).unwrap()
}

#[test]
fn persist_and_memory_agree_on_snapshots_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = open(dir.path());
    let mut m = MemoryStore::new();
    let a = Writer::put(&mut p, make("alpha"), &who(), Timestamp(10))
        .unwrap()
        .id;
    let b = Writer::put(&mut p, make("beta"), &who(), Timestamp(10))
        .unwrap()
        .id;
    Writer::put(&mut m, make("alpha"), &who(), Timestamp(10)).unwrap();
    Writer::put(&mut m, make("beta"), &who(), Timestamp(10)).unwrap();
    Writer::link(&mut p, &a, LinkKind::PartOf, &b, &who(), Timestamp(20)).unwrap();
    Writer::link(&mut m, &a, LinkKind::PartOf, &b, &who(), Timestamp(20)).unwrap();
    for (now, text) in [(130, "alpha two"), (400, "alpha three")] {
        let wp = worth_of(&p, &a, Timestamp(now), &params()).unwrap();
        let wm = worth_of(&m, &a, Timestamp(now), &params()).unwrap();
        assert_eq!(wp, wm);
        let sp = WorthStore::record_worth(&mut p, &a, &wp, Timestamp(now)).unwrap();
        let sm = WorthStore::record_worth(&mut m, &a, &wm, Timestamp(now)).unwrap();
        assert_eq!(sp.encode(), sm.encode());
        Writer::supersede(
            &mut p,
            &a,
            text,
            Origin::User,
            "cli",
            &who(),
            Timestamp(now + 1),
        )
        .unwrap();
        Writer::supersede(
            &mut m,
            &a,
            text,
            Origin::User,
            "cli",
            &who(),
            Timestamp(now + 1),
        )
        .unwrap();
    }
    let sp: Vec<_> = WorthStore::worth_snapshots(&p, &a)
        .unwrap()
        .iter()
        .map(WorthSnapshot::encode)
        .collect();
    let sm: Vec<_> = WorthStore::worth_snapshots(&m, &a)
        .unwrap()
        .iter()
        .map(WorthSnapshot::encode)
        .collect();
    assert_eq!(sp.len(), 2, "one per version, ordered");
    assert_eq!(sp, sm);
    assert_eq!(WorthStore::worth_snapshots(&p, &b).unwrap(), vec![]);
    // Errors agree: unknown, a worth from a later version, forgotten.
    let ghost = ChunkId::parse(&sageant::sha256_hex(b"nothing")).unwrap();
    let w = worth_of(&p, &a, Timestamp(500), &params()).unwrap();
    assert_eq!(w.version(), 3);
    assert_eq!(
        p.record_worth(&ghost, &w, Timestamp(1)),
        m.record_worth(&ghost, &w, Timestamp(1))
    );
    assert_eq!(
        p.record_worth(&b, &w, Timestamp(1)),
        Err(Error::Invalid("worth version"))
    );
    assert_eq!(
        m.record_worth(&b, &w, Timestamp(1)),
        Err(Error::Invalid("worth version"))
    );
    Writer::forget(&mut p, &b, &who(), "r", None, Timestamp(600)).unwrap();
    assert_eq!(p.worth_snapshots(&b), Err(Error::Forgotten));
    assert_eq!(
        p.record_worth(&b, &w, Timestamp(601)),
        Err(Error::Forgotten)
    );
    assert_eq!(p.worth_snapshots(&ghost), Err(Error::NotFound));
}

#[test]
fn snapshots_survive_a_reopen_and_change_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let a;
    let before;
    {
        let p = open(dir.path());
        a = p.put(make("alpha"), Timestamp(10)).unwrap().id;
        let audit = p.audit().unwrap().len();
        before = (Reader::get(&p, &a).unwrap().encode(), audit);
        snap(&p, &a, 110);
        assert_eq!(p.audit().unwrap().len(), audit, "not an audited write");
    }
    let p = open(dir.path());
    let got = p.worth_snapshots(&a).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].computed_at(), Timestamp(110));
    assert_eq!(Reader::get(&p, &a).unwrap().encode(), before.0);
    assert_eq!(p.audit().unwrap().len(), before.1);
}

#[test]
fn forget_and_purge_take_the_snapshots_with_the_chunk() {
    for purge in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let p = open(dir.path());
        let a = p.put(make("alpha"), Timestamp(10)).unwrap().id;
        snap(&p, &a, 110);
        p.supersede(&a, "alpha two", Origin::User, "cli", Timestamp(120))
            .unwrap();
        snap(&p, &a, 130);
        assert_eq!(
            p.worth_snapshots(&a).unwrap().len(),
            2,
            "control: present before"
        );
        if purge {
            p.purge(
                &a,
                &PersonAuthority::from_person_action("tester"),
                Timestamp(140),
            )
            .unwrap();
        } else {
            p.forget(&a, "x", "r", None, Timestamp(140)).unwrap();
        }
        let back = p.put(make("alpha"), Timestamp(200)).unwrap().id;
        assert_eq!(back, a);
        assert_eq!(
            p.worth_snapshots(&a).unwrap(),
            vec![],
            "stale snapshots came back (purge={purge})"
        );
    }
}

fn raw(dir: &std::path::Path) -> Vec<u8> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        all.extend(std::fs::read(entry.unwrap().path()).unwrap_or_default());
    }
    all
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn a_forgotten_chunks_snapshot_keys_are_gone_from_the_files_on_the_backend_that_erases() {
    let dir = tempfile::tempdir().unwrap();
    let p = PersistStore::open("sqlite-secure", &Endpoint::Path(dir.path().join("s.db"))).unwrap();
    let a = p.put(make("alpha"), Timestamp(10)).unwrap().id;
    snap(&p, &a, 110);
    let key = format!("w/{}/", a.as_str());
    assert!(
        contains(&raw(dir.path()), &key),
        "control: the scan sees the key before"
    );
    p.forget(&a, "x", "r", None, Timestamp(120)).unwrap();
    drop(p);
    assert!(
        !contains(&raw(dir.path()), &key),
        "snapshot key still in the files"
    );
}

#[test]
fn a_snapshot_written_while_a_forget_is_committing_is_removed_by_it() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let a = p.put(make("alpha"), Timestamp(10)).unwrap().id;
    let w = worth_of(&p, &a, Timestamp(50), &params()).unwrap();
    // The hook runs after forget has read what to remove and before it commits.
    p.forget_with_hook_for_test(&a, "x", "r", None, Timestamp(60), &|| {
        p.record_worth(&a, &w, Timestamp(55)).unwrap();
    })
    .unwrap();
    p.put(make("alpha"), Timestamp(70)).unwrap();
    assert_eq!(
        p.worth_snapshots(&a).unwrap(),
        vec![],
        "orphan left by the race"
    );
}

#[test]
fn a_forget_that_lands_while_a_snapshot_is_being_written_wins() {
    let dir = tempfile::tempdir().unwrap();
    let p = open(dir.path());
    let a = p.put(make("alpha"), Timestamp(10)).unwrap().id;
    let w = worth_of(&p, &a, Timestamp(50), &params()).unwrap();
    let out = p.record_worth_with_hook_for_test(&a, &w, Timestamp(55), &|| {
        p.forget(&a, "x", "r", None, Timestamp(60)).unwrap();
    });
    assert_eq!(out, Err(Error::Forgotten));
    p.put(make("alpha"), Timestamp(70)).unwrap();
    assert_eq!(
        p.worth_snapshots(&a).unwrap(),
        vec![],
        "orphan left by the race"
    );
}

#[test]
fn a_stored_snapshot_filed_under_the_wrong_chunk_is_corrupt_not_trusted() {
    use persistant::blocking::registry::open_named;
    use persistant::kv::BatchOp;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sageant.db");
    let (a, b, bytes);
    {
        let p = open(dir.path());
        a = p.put(make("alpha"), Timestamp(10)).unwrap().id;
        b = p.put(make("beta"), Timestamp(10)).unwrap().id;
        bytes = snap(&p, &b, 110).encode();
    }
    let kv = open_named(
        "sqlite",
        &Endpoint::Path(path),
        &sageant_persistant::storage_needs(),
    )
    .unwrap();
    kv.batch(&[BatchOp::Put {
        key: format!("w/{}/{:010}/worth-v1", a.as_str(), 1),
        value: bytes,
        ttl: None,
    }])
    .unwrap();
    drop(kv);
    let p = open(dir.path());
    assert_eq!(
        p.worth_snapshots(&a),
        Err(Error::Corrupt("key does not match record"))
    );
    assert_eq!(p.worth_snapshots(&b).unwrap().len(), 1);
}
