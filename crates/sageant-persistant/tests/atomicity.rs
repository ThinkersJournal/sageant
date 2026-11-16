// SPDX-License-Identifier: MIT OR Apache-2.0
//! The two documented atomicity limits of 0.3.x, closed. Each race is made deterministic with a hook that runs
//! another write after a writer has read what it will act on and before it commits. Written before the fix.

use persistant::registry::Endpoint;
use sageant::{ChunkId, Error, LinkKind, NewChunk, Origin, Timestamp};
use sageant_persistant::PersistStore;

fn store() -> (tempfile::TempDir, PersistStore) {
    let dir = tempfile::tempdir().unwrap();
    let p = PersistStore::open("sqlite", &Endpoint::Path(dir.path().join("s.db"))).unwrap();
    (dir, p)
}

fn put(p: &PersistStore, text: &str) -> ChunkId {
    p.put(
        NewChunk::new("c", "k", text, Origin::User, "cli"),
        Timestamp(1),
    )
    .unwrap()
    .id
}

#[test]
fn two_opposite_part_of_links_cannot_both_win() {
    let (_d, p) = store();
    let (a, b) = (put(&p, "a"), put(&p, "b"));
    // While a->b has checked for a cycle but not yet committed, b->a commits.
    let outer = p.link_with_hook_for_test(&a, LinkKind::PartOf, &b, Timestamp(2), &|| {
        p.link(&b, LinkKind::PartOf, &a, Timestamp(2)).unwrap();
    });
    assert_eq!(
        outer,
        Err(Error::LinkCycle),
        "the second link would close a cycle and must be refused"
    );
    assert_eq!(p.links_from(&b).unwrap().len(), 1);
    assert!(p.links_from(&a).unwrap().is_empty());
    let links = p
        .audit()
        .unwrap()
        .iter()
        .filter(|e| e.action() == sageant::Action::Link)
        .count();
    assert_eq!(links, 1, "a refused link appends nothing");
}

#[test]
fn a_longer_cycle_is_caught_when_the_closing_edge_races_too() {
    let (_d, p) = store();
    let (a, b, c) = (put(&p, "a"), put(&p, "b"), put(&p, "c"));
    p.link(&a, LinkKind::DerivedFrom, &b, Timestamp(2)).unwrap();
    // b->c is being made; meanwhile c->a commits. Together a->b->c->a would be a cycle.
    let outer = p.link_with_hook_for_test(&b, LinkKind::DerivedFrom, &c, Timestamp(3), &|| {
        p.link(&c, LinkKind::DerivedFrom, &a, Timestamp(3)).unwrap();
    });
    assert_eq!(outer, Err(Error::LinkCycle));
}

#[test]
fn a_link_made_while_its_source_is_forgotten_leaves_nothing_behind() {
    let (_d, p) = store();
    let (a, b) = (put(&p, "alpha text"), put(&p, "beta text"));
    // forget(a) has read a's links (none) and is about to commit; a link from a lands first.
    let r = p.forget_with_hook_for_test(&a, "me", "r", None, Timestamp(5), &|| {
        p.link(&a, LinkKind::Related, &b, Timestamp(4)).unwrap();
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(
        p.links_to(&b).unwrap().is_empty(),
        "no link from a forgotten chunk survives"
    );
    let dump = p.dump_all_for_test().unwrap();
    assert!(
        dump.iter()
            .all(|(k, _)| !(k.starts_with(&format!("l/{}/", a.as_str()))
                || k.contains(&format!("/{}", a.as_str())) && k.starts_with("r/"))),
        "{:?}",
        dump.iter().map(|(k, _)| k).collect::<Vec<_>>()
    );
}

#[test]
fn a_derived_chunk_linked_while_its_source_is_forgotten_is_still_flagged_for_review() {
    let (_d, p) = store();
    let (a, y) = (put(&p, "source text"), put(&p, "summary text"));
    let r = p.forget_with_hook_for_test(&a, "me", "r", None, Timestamp(5), &|| {
        p.link(&y, LinkKind::DerivedFrom, &a, Timestamp(4)).unwrap();
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(p.needs_review(&y).unwrap());
}

#[test]
fn a_link_to_a_chunk_forgotten_mid_link_is_refused_not_left_dangling() {
    let (_d, p) = store();
    let (a, b) = (put(&p, "from text"), put(&p, "to text"));
    let outer = p.link_with_hook_for_test(&a, LinkKind::DerivedFrom, &b, Timestamp(3), &|| {
        p.forget(&b, "me", "r", None, Timestamp(4)).unwrap();
    });
    assert_eq!(outer, Err(Error::Forgotten));
    assert!(p.links_from(&a).unwrap().is_empty());
    assert!(
        !p.needs_review(&a).unwrap(),
        "no link, so nothing to review"
    );
}

#[test]
fn purge_also_survives_a_link_landing_mid_purge() {
    let (_d, p) = store();
    let (a, b) = (put(&p, "purge me"), put(&p, "other"));
    let who = sageant::PersonAuthority::from_person_action("typed");
    let r = p.purge_with_hook_for_test(&a, &who, Timestamp(5), &|| {
        p.link(&a, LinkKind::Related, &b, Timestamp(4)).unwrap();
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(p.links_to(&b).unwrap().is_empty());
}

#[test]
fn racing_threads_never_leave_a_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.db");
    let a_store = PersistStore::open("sqlite", &Endpoint::Path(path.clone())).unwrap();
    let b_store = PersistStore::open("sqlite", &Endpoint::Path(path)).unwrap();
    for round in 0..20 {
        let a = put(&a_store, &format!("left {round}"));
        let b = put(&a_store, &format!("right {round}"));
        let barrier = std::sync::Barrier::new(2);
        let (r1, r2) = std::thread::scope(|s| {
            let h1 = s.spawn(|| {
                barrier.wait();
                a_store.link(&a, LinkKind::PartOf, &b, Timestamp(2))
            });
            let h2 = s.spawn(|| {
                barrier.wait();
                b_store.link(&b, LinkKind::PartOf, &a, Timestamp(2))
            });
            (h1.join().unwrap(), h2.join().unwrap())
        });
        let oks = usize::from(r1.is_ok()) + usize::from(r2.is_ok());
        assert_eq!(oks, 1, "round {round}: {r1:?} / {r2:?}");
        let loser = if r1.is_ok() { r2 } else { r1 };
        assert_eq!(loser, Err(Error::LinkCycle));
    }
}

// ---- kill tests for the row 4 / D mutation run (docs/MUTATION.md, run 4); written after the misses ----

#[test]
fn every_link_counts_up_the_generation_of_both_chunks_it_touches() {
    let (_d, p) = store();
    let (a, b, c, d) = (put(&p, "a"), put(&p, "b"), put(&p, "c"), put(&p, "d"));
    p.link(&a, LinkKind::Related, &b, Timestamp(2)).unwrap();
    p.link(&a, LinkKind::Related, &c, Timestamp(2)).unwrap();
    p.link(&a, LinkKind::Related, &d, Timestamp(2)).unwrap();
    let generation = |id: &ChunkId| {
        p.dump_all_for_test()
            .unwrap()
            .into_iter()
            .find(|(k, _)| k == &format!("n/{}", id.as_str()))
            .map(|(_, v)| String::from_utf8(v).unwrap())
    };
    assert_eq!(
        generation(&a).as_deref(),
        Some("3"),
        "a was linked from three times"
    );
    assert_eq!(generation(&b).as_deref(), Some("1"));
    assert_eq!(generation(&c).as_deref(), Some("1"));
}

#[test]
fn a_link_landing_mid_forget_is_caught_even_when_the_chunk_already_had_links() {
    let (_d, p) = store();
    let (a, b, c) = (
        put(&p, "alpha text"),
        put(&p, "beta text"),
        put(&p, "gamma text"),
    );
    p.link(&a, LinkKind::Related, &c, Timestamp(2)).unwrap(); // a's generation is now 1, not absent
    let r = p.forget_with_hook_for_test(&a, "me", "r", None, Timestamp(5), &|| {
        p.link(&a, LinkKind::Related, &b, Timestamp(4)).unwrap();
    });
    assert!(r.is_ok(), "{r:?}");
    assert!(
        p.links_to(&b).unwrap().is_empty(),
        "the late link was removed with the rest"
    );
    assert!(p.links_to(&c).unwrap().is_empty());
}

#[test]
fn a_link_that_would_close_a_cycle_through_a_chunk_in_the_middle_of_the_path_is_caught() {
    let (_d, p) = store();
    let (a, b, m, n) = (put(&p, "a"), put(&p, "b"), put(&p, "m"), put(&p, "n"));
    p.link(&b, LinkKind::PartOf, &m, Timestamp(2)).unwrap();
    p.link(&n, LinkKind::PartOf, &a, Timestamp(2)).unwrap();
    // a->b is being made (so far a path from b leads b->m and stops). Meanwhile m->n lands: it touches neither a
    // nor b, only the middle chunk m, yet it completes a->b->m->n->a.
    let outer = p.link_with_hook_for_test(&a, LinkKind::PartOf, &b, Timestamp(3), &|| {
        p.link(&m, LinkKind::PartOf, &n, Timestamp(3)).unwrap();
    });
    assert_eq!(outer, Err(Error::LinkCycle));
    assert!(p.links_from(&a).unwrap().is_empty());
}

#[test]
fn the_test_hooks_run_exactly_once_and_a_purge_or_forget_really_happens_around_them() {
    use std::cell::Cell;
    let (_d, p) = store();
    let (a, b) = (put(&p, "purge me"), put(&p, "forget me"));
    let ran = Cell::new(0);
    let who = sageant::PersonAuthority::from_person_action("typed");
    p.purge_with_hook_for_test(&a, &who, Timestamp(5), &|| ran.set(ran.get() + 1))
        .unwrap();
    assert_eq!(ran.get(), 1);
    assert_eq!(p.get(&a).err(), Some(Error::NotFound));
    p.forget_with_hook_for_test(&b, "me", "r", None, Timestamp(5), &|| {
        ran.set(ran.get() + 1)
    })
    .unwrap();
    assert_eq!(ran.get(), 2);
    assert_eq!(p.get(&b).err(), Some(Error::Forgotten));
}

#[test]
fn a_purge_or_forget_whose_chunk_gets_a_new_version_meanwhile_reports_a_conflict() {
    let (_d, p) = store();
    let (a, b) = (put(&p, "first purge text"), put(&p, "first forget text"));
    let who = sageant::PersonAuthority::from_person_action("typed");
    let purge = p.purge_with_hook_for_test(&a, &who, Timestamp(5), &|| {
        p.supersede(&a, "changed meanwhile", Origin::User, "cli", Timestamp(4))
            .unwrap();
    });
    assert_eq!(
        purge,
        Err(Error::Conflict {
            expected: 1,
            found: 2
        })
    );
    assert!(p.get(&a).is_ok(), "nothing was removed");
    let forget = p.forget_with_hook_for_test(&b, "me", "r", None, Timestamp(5), &|| {
        p.supersede(
            &b,
            "changed meanwhile too",
            Origin::User,
            "cli",
            Timestamp(4),
        )
        .unwrap();
    });
    assert_eq!(
        forget,
        Err(Error::Conflict {
            expected: 1,
            found: 2
        })
    );
    assert!(p.get(&b).is_ok());
}
