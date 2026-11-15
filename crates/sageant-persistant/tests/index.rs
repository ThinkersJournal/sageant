// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant-persistant` slice 4: a key-value text index (CONTRACT D17). Written before the code. `MemoryIndex` is
//! the reference: the same operations must leave both indexes answering identically.

use persistant::registry::Endpoint;
use sageant::{ChunkId, ChunkRef, MemoryStore, NewChunk, Origin, Query, Retriever, Timestamp};
use sageant_lexical::{LexicalRetriever, MemoryIndex, TextIndex, term_counts};
use sageant_persistant::KvTextIndex;

fn endpoint(dir: &std::path::Path) -> Endpoint {
    Endpoint::Path(dir.join("index.db"))
}

fn id(n: u8) -> ChunkId {
    ChunkId::parse(&format!("{n:02x}").repeat(32)).unwrap()
}

fn r(n: u8, version: u32) -> ChunkRef {
    ChunkRef { id: id(n), version }
}

const TERMS: [&str; 8] = [
    "safety",
    "model",
    "agent",
    "the",
    "rust",
    "other",
    "different",
    "missing",
];

fn same(kv: &KvTextIndex, mem: &MemoryIndex, docs: &[ChunkRef]) {
    for t in TERMS {
        assert_eq!(
            kv.postings(t).unwrap(),
            mem.postings(t).unwrap(),
            "postings of {t}"
        );
    }
    for d in docs {
        assert_eq!(
            kv.doc_len(d).unwrap(),
            mem.doc_len(d).unwrap(),
            "doc_len of {d:?}"
        );
    }
    assert_eq!(kv.stats().unwrap(), mem.stats().unwrap());
}

#[test]
fn the_kv_index_answers_like_the_memory_index_through_every_operation() {
    let dir = tempfile::tempdir().unwrap();
    let mut kv = KvTextIndex::open("sqlite", &endpoint(dir.path())).unwrap();
    let mut mem = MemoryIndex::default();
    let docs = [r(1, 1), r(1, 2), r(2, 1), r(3, 1), r(9, 9)];
    same(&kv, &mem, &docs);
    let steps: [(ChunkRef, &str); 5] = [
        (r(1, 1), "safety of the model, the agent"),
        (r(2, 1), "rust rust rust the other agent"),
        (r(3, 1), "model model different"),
        (r(1, 2), "safety safety safety"),
        // Re-indexing the same version replaces it: no stale postings, the length changes.
        (r(1, 1), "completely different words"),
    ];
    for (chunk, text) in steps {
        kv.index(&chunk, text).unwrap();
        mem.index(&chunk, text).unwrap();
        same(&kv, &mem, &docs);
    }
    // Dropping a lineage drops every version of it; an unknown lineage is a no-op.
    kv.remove(&id(1)).unwrap();
    mem.remove(&id(1)).unwrap();
    same(&kv, &mem, &docs);
    kv.remove(&id(77)).unwrap();
    same(&kv, &mem, &docs);
    assert_eq!(kv.stats().unwrap().docs, 2);
}

#[test]
fn stats_agree_with_term_counts() {
    let dir = tempfile::tempdir().unwrap();
    let mut kv = KvTextIndex::open("sqlite", &endpoint(dir.path())).unwrap();
    let text = "one two two three";
    kv.index(&r(1, 1), text).unwrap();
    let len: u32 = term_counts(text).values().sum();
    assert_eq!(kv.doc_len(&r(1, 1)).unwrap(), Some(len));
    assert_eq!(kv.stats().unwrap().total_len, u64::from(len));
}

#[test]
fn the_index_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut kv = KvTextIndex::open("sqlite", &endpoint(dir.path())).unwrap();
        kv.index(&r(1, 1), "safety model").unwrap();
    }
    let kv = KvTextIndex::open("sqlite", &endpoint(dir.path())).unwrap();
    assert_eq!(kv.postings("safety").unwrap().len(), 1);
    assert_eq!(kv.stats().unwrap().docs, 1);
}

#[test]
fn a_retriever_over_either_index_returns_the_same_hits() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::new();
    let mut kv = KvTextIndex::open("sqlite", &endpoint(dir.path())).unwrap();
    let mut mem = MemoryIndex::default();
    let texts = [
        "agent safety policy for the model",
        "the rust compiler and the borrow checker",
        "model safety, model behaviour, model limits",
        "unrelated gardening notes",
    ];
    for (i, text) in texts.iter().enumerate() {
        let c = store
            .put(
                NewChunk::new("docs", "note", text, Origin::User, "cli"),
                Timestamp(i as u64),
            )
            .unwrap();
        kv.index(&c, text).unwrap();
        mem.index(&c, text).unwrap();
    }
    let (a, b) = (LexicalRetriever::new(kv), LexicalRetriever::new(mem));
    for q in [
        "model safety",
        "rust",
        "the",
        "gardening agent",
        "nothing here",
    ] {
        let query = Query::new(q);
        assert_eq!(
            a.retrieve(&query, &store).unwrap(),
            b.retrieve(&query, &store).unwrap(),
            "{q}"
        );
    }
}

#[test]
fn concurrent_writers_keep_the_totals_equal_to_the_postings() {
    let dir = tempfile::tempdir().unwrap();
    let ep = endpoint(dir.path());
    std::thread::scope(|s| {
        for t in 0..2_u8 {
            let ep = &ep;
            s.spawn(move || {
                let mut kv = KvTextIndex::open("sqlite", ep).unwrap();
                for i in 0..10_u8 {
                    kv.index(&r(t * 10 + i + 1, 1), "alpha beta beta").unwrap();
                }
            });
        }
    });
    let kv = KvTextIndex::open("sqlite", &ep).unwrap();
    assert_eq!(kv.stats().unwrap().docs, 20);
    assert_eq!(kv.stats().unwrap().total_len, 60);
    assert_eq!(kv.postings("beta").unwrap().len(), 20);
}
