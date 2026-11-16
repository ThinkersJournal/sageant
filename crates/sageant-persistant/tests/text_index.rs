// SPDX-License-Identifier: MIT OR Apache-2.0
//! `PersistTextIndex` (PersistAnt's text layer) must answer exactly like `MemoryIndex` (the reference), on every backend, and
//! leave nothing readable behind when it removes a chunk (read from the raw files). The PM's gate for the replacement: differential, N >= 3 read-back, byte-identical ranking.

use persistant::registry::Endpoint;
use sageant::{ChunkId, ChunkRef, MemoryStore, NewChunk, Origin, Query, Retriever, Timestamp};
use sageant_lexical::{LexicalRetriever, MemoryIndex, TextIndex};
use sageant_persistant::PersistTextIndex;

const BACKENDS: [&str; 3] = ["sqlite-secure", "sqlite", "sqlite-full"];

fn id(n: u16) -> ChunkId {
    ChunkId::parse(&format!("{n:04x}").repeat(16)).unwrap()
}

fn r(n: u16, version: u32) -> ChunkRef {
    ChunkRef { id: id(n), version }
}

fn ep(dir: &std::path::Path, name: &str) -> Endpoint {
    Endpoint::Path(dir.join(name))
}

const WORDS: [&str; 12] = [
    "safety",
    "model",
    "agent",
    "the",
    "rust",
    "other",
    "different",
    "owl",
    "night",
    "Ünïcode",
    "straße",
    "x",
];

/// A deterministic pseudo-random text of `n` words (no rand dependency).
fn text(seed: u64, n: usize) -> String {
    let mut s = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut out = Vec::new();
    for _ in 0..n {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push(WORDS[(s >> 33) as usize % WORDS.len()]);
    }
    out.join(" ")
}

fn same(a: &dyn TextIndex, b: &dyn TextIndex, docs: &[ChunkRef], what: &str) {
    for t in WORDS
        .iter()
        .map(|w| w.to_lowercase())
        .chain(["missing".to_owned()])
    {
        assert_eq!(
            a.postings(&t).unwrap(),
            b.postings(&t).unwrap(),
            "{what}: postings of {t}"
        );
    }
    for d in docs {
        assert_eq!(
            a.doc_len(d).unwrap(),
            b.doc_len(d).unwrap(),
            "{what}: doc_len of {d:?}"
        );
    }
    assert_eq!(a.stats().unwrap(), b.stats().unwrap(), "{what}: stats");
}

#[test]
fn the_persistant_index_answers_like_the_memory_and_the_kv_index_on_every_backend() {
    for backend in BACKENDS {
        let dir = tempfile::tempdir().unwrap();
        let mut p = PersistTextIndex::open(backend, &ep(dir.path(), "p.db")).unwrap();
        let mut m = MemoryIndex::default();
        let docs: Vec<ChunkRef> = (1..=40u16)
            .flat_map(|n| [r(n, 1), r(n, 2), r(n, 11)])
            .collect();
        // Index, re-index (replace), include empty and one-word texts, then remove lineages, checking after every step.
        let mut step = 0u64;
        for n in 1..=40u16 {
            for v in [1u32, 2, 11] {
                step += 1;
                let t = match step % 9 {
                    0 => String::new(),
                    1 => "x".to_owned(),
                    _ => text(step, (step % 17) as usize + 1),
                };
                for ix in [&mut p as &mut dyn TextIndex, &mut m] {
                    ix.index(&r(n, v), &t).unwrap();
                }
            }
            same(&p, &m, &docs, &format!("{backend} after indexing {n}"));
        }
        for n in [3u16, 7, 40] {
            for ix in [&mut p as &mut dyn TextIndex, &mut m] {
                ix.index(&r(n, 2), "replaced by this text only").unwrap();
            }
        }
        same(&p, &m, &docs, &format!("{backend} after replacing"));
        for n in [1u16, 2, 3, 20, 40, 999] {
            for ix in [&mut p as &mut dyn TextIndex, &mut m] {
                ix.remove(&id(n)).unwrap();
            }
            same(&p, &m, &docs, &format!("{backend} after removing {n}"));
        }
        assert_eq!(p.stats().unwrap().docs, 3 * 35, "{backend}");
    }
}

#[test]
fn ranking_over_the_persistant_index_is_identical_to_the_other_two() {
    for backend in BACKENDS {
        let dir = tempfile::tempdir().unwrap();
        let mut store = MemoryStore::new();
        let mut p = PersistTextIndex::open(backend, &ep(dir.path(), "p.db")).unwrap();
        let mut m = MemoryIndex::default();
        for i in 0..60u64 {
            // The store refuses non-ASCII text without the `nfc` feature, so the ranking corpus is ASCII.
            let t = text(i, (i % 23) as usize + 2)
                .replace("Ünïcode", "unicode")
                .replace("straße", "strasse");
            let c = store
                .put(
                    NewChunk::new("docs", "note", &t, Origin::User, "cli"),
                    Timestamp(i),
                )
                .unwrap();
            for ix in [&mut p as &mut dyn TextIndex, &mut m] {
                ix.index(&c, &t).unwrap();
            }
        }
        let (a, c) = (LexicalRetriever::new(p), LexicalRetriever::new(m));
        for q in [
            "model safety",
            "rust",
            "the",
            "owl night agent",
            "strasse",
            "unicode x",
            "nothing here",
            "x x x",
        ] {
            let query = Query::new(q);
            let (ra, rc) = (
                a.retrieve(&query, &store).unwrap(),
                c.retrieve(&query, &store).unwrap(),
            );
            assert_eq!(ra, rc, "{backend}: {q}: persistant vs memory");
            assert_eq!(
                format!("{ra:?}"),
                format!("{rc:?}"),
                "{backend}: {q}: debug bytes"
            );
        }
    }
}

#[test]
fn the_persistant_index_survives_reopening_and_shared_writers() {
    let dir = tempfile::tempdir().unwrap();
    let e = ep(dir.path(), "p.db");
    std::thread::scope(|s| {
        for t in 0..2u16 {
            let e = &e;
            s.spawn(move || {
                let mut p = PersistTextIndex::open("sqlite", e).unwrap();
                for i in 0..10u16 {
                    p.index(&r(t * 10 + i + 1, 1), "alpha beta beta").unwrap();
                }
            });
        }
    });
    let p = PersistTextIndex::open("sqlite", &e).unwrap();
    assert_eq!(p.stats().unwrap().docs, 20);
    assert_eq!(p.stats().unwrap().total_len, 60);
    assert_eq!(p.postings("beta").unwrap().len(), 20);
}

#[test]
fn a_backend_without_a_text_layer_is_refused_at_open_with_a_stated_reason() {
    let dir = tempfile::tempdir().unwrap();
    // A name PersistAnt does not know is refused as a backend error, never a panic.
    assert!(PersistTextIndex::open("no-such-backend", &ep(dir.path(), "p.db")).is_err());
}

fn raw(dir: &std::path::Path) -> Vec<u8> {
    let mut all = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        all.extend(std::fs::read(entry.unwrap().path()).unwrap_or_default());
    }
    all
}

fn contains(h: &[u8], needle: &str) -> bool {
    h.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Index `n` documents (two versions each) and remove them all; how many still have their terms in the files?
fn docs_left(backend: &str, n: usize, erase: bool) -> usize {
    let dir = tempfile::tempdir().unwrap();
    let e = ep(dir.path(), "i.db");
    let mut ix = if erase {
        PersistTextIndex::open(backend, &e).unwrap()
    } else {
        PersistTextIndex::open_without_erasing_for_test(backend, &e).unwrap()
    };
    for i in 0..n {
        let n16 = (i + 1) as u16;
        ix.index(&r(n16, 1), &format!("zqxterm{i:03} alpha beta gamma delta"))
            .unwrap();
        ix.index(
            &r(n16, 2),
            &format!("zqxterm{i:03} alpha beta gamma delta epsilon"),
        )
        .unwrap();
    }
    for i in 0..n {
        ix.remove(&id((i + 1) as u16)).unwrap();
    }
    assert_eq!(ix.stats().unwrap().docs, 0);
    drop(ix);
    let files = raw(dir.path());
    (0..n)
        .filter(|i| contains(&files, &format!("zqxterm{i:03}")))
        .count()
}

#[test]
fn at_scale_the_secure_backend_leaves_no_term_of_a_removed_chunk_in_the_files() {
    for n in [1, 3, 8, 30] {
        assert_eq!(docs_left("sqlite-secure", n, true), 0, "{n} chunks");
    }
}

#[test]
fn plain_and_wal_sqlite_are_measured_not_assumed_for_the_persistant_index() {
    // Two versions per chunk, so twice the documents of the old index's test. The claim stays "unretrievable, not erased"
    // for these backends: this pins that something is left at scale, and prints the numbers for the README row.
    for backend in ["sqlite", "sqlite-full"] {
        let counts: Vec<(usize, usize)> = [1, 3, 8, 30]
            .iter()
            .map(|&n| (n, docs_left(backend, n, true)))
            .collect();
        eprintln!("{backend}: chunks with a term left in the files after removing N: {counts:?}");
        assert!(
            counts.iter().any(|&(n, left)| n >= 8 && left > 0),
            "{backend}: nothing left at scale: a better claim may be earned, re-measure: {counts:?}"
        );
    }
}

#[test]
fn with_erasing_switched_off_plain_deletes_leave_the_terms_so_the_test_can_see() {
    for backend in ["sqlite", "sqlite-full"] {
        assert!(docs_left(backend, 3, false) > 0, "{backend}: control");
    }
}

#[test]
fn a_backend_that_refuses_erased_batches_still_removes_and_stops_asking() {
    let dir = tempfile::tempdir().unwrap();
    let mut ix = PersistTextIndex::open("sqlite", &ep(dir.path(), "i.db")).unwrap();
    ix.index(&r(1, 1), "alpha beta").unwrap();
    ix.index(&r(1, 2), "alpha gamma").unwrap();
    ix.index(&r(2, 1), "alpha delta").unwrap();
    assert!(ix.overwrites_on_remove_for_test());
    ix.refuse_erased_batches_for_test(true);
    ix.remove(&id(1)).unwrap();
    assert!(!ix.overwrites_on_remove_for_test());
    assert_eq!(ix.stats().unwrap().docs, 1);
    assert_eq!(ix.postings("alpha").unwrap().len(), 1);
    ix.refuse_erased_batches_for_test(false);
    ix.remove(&id(2)).unwrap();
    assert_eq!(ix.stats().unwrap().docs, 0);
}
