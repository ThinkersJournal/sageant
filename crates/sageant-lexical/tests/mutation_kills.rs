// SPDX-License-Identifier: MIT OR Apache-2.0
//! Kill tests for the mutants the row 4 run (docs/MUTATION.md, run 3) missed in `sageant-lexical`. They were
//! missed, so there was no red state to write first; the recorded rerun is the evidence they are now caught.

use sageant::{
    ChunkId, ChunkRef, Error, MemoryStore, NewChunk, Origin, Query, Retriever, Timestamp,
};
use sageant_lexical::{LexicalRetriever, MemoryIndex, Posting, Stats, TextIndex, ln_ratio_micro};

fn id(n: u8) -> ChunkId {
    ChunkId::parse(&format!("{n:02x}").repeat(32)).unwrap()
}

fn r(n: u8, version: u32) -> ChunkRef {
    ChunkRef { id: id(n), version }
}

#[test]
fn dropping_one_document_leaves_every_other_posting_alone() {
    let mut idx = MemoryIndex::default();
    idx.index(&r(1, 1), "shared alpha").unwrap();
    idx.index(&r(2, 1), "shared beta").unwrap();
    idx.index(&r(1, 1), "shared gamma").unwrap(); // re-index: drops the old version of 1 first
    let shared: Vec<_> = idx
        .postings("shared")
        .unwrap()
        .iter()
        .map(|p| p.chunk().clone())
        .collect();
    assert_eq!(shared, vec![r(1, 1), r(2, 1)]);
    assert_eq!(idx.postings("beta").unwrap().len(), 1);
    idx.remove(&id(1)).unwrap();
    assert_eq!(idx.postings("shared").unwrap().len(), 1);
    assert_eq!(idx.postings("beta").unwrap().len(), 1);
}

#[test]
fn the_integer_log_returns_zero_outside_its_range_and_is_right_for_small_ratios() {
    const LIMIT: u64 = 1 << 63;
    assert_eq!(ln_ratio_micro(0, 5), 0);
    assert_eq!(ln_ratio_micro(5, 0), 0);
    assert_eq!(ln_ratio_micro(LIMIT, 5), 0);
    assert_eq!(ln_ratio_micro(5, LIMIT), 0);
    assert_ne!(
        ln_ratio_micro(LIMIT - 1, 1),
        0,
        "just inside the range is computed"
    );
    assert_ne!(ln_ratio_micro(1, LIMIT - 1), 0);
    // Ratios below 1/4 take the other normalisation branch.
    for (num, den, want) in [
        (1u64, 4u64, -1_386_294i64),
        (1, 10, -2_302_585),
        (1, 1000, -6_907_755),
        (3, 100, -3_506_558),
    ] {
        let got = ln_ratio_micro(num, den);
        assert!(
            (got - want).abs() <= 3,
            "ln({num}/{den}): got {got}, want {want}"
        );
    }
}

/// An index that answers with fixed numbers, to reach the scorer's guards.
struct Fixed {
    stats: Stats,
    tf: u32,
    len: u32,
    chunk: ChunkRef,
}

impl TextIndex for Fixed {
    fn index(&mut self, _: &ChunkRef, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn remove(&mut self, _: &ChunkId) -> Result<(), Error> {
        Ok(())
    }
    fn postings(&self, _: &str) -> Result<Vec<Posting>, Error> {
        Ok(vec![Posting::new(self.chunk.clone(), self.tf)])
    }
    fn doc_len(&self, _: &ChunkRef) -> Result<Option<u32>, Error> {
        Ok(Some(self.len))
    }
    fn stats(&self) -> Result<Stats, Error> {
        Ok(self.stats)
    }
}

fn score_with(stats: Stats, tf: u32, len: u32) -> Option<i64> {
    let mut store = MemoryStore::new();
    let chunk = store
        .put(
            NewChunk::new("c", "k", "x", Origin::User, "cli"),
            Timestamp(1),
        )
        .unwrap();
    let r = LexicalRetriever::new(Fixed {
        stats,
        tf,
        len,
        chunk,
    });
    let hits = r.retrieve(&Query::new("x"), &store).unwrap();
    assert_eq!(hits.len(), 1);
    hits[0].score()
}

#[test]
fn a_corpus_with_no_documents_no_tokens_or_a_zero_frequency_scores_nothing() {
    assert_eq!(
        score_with(
            Stats {
                docs: 0,
                total_len: 10
            },
            1,
            3
        ),
        Some(0)
    );
    assert_eq!(
        score_with(
            Stats {
                docs: 2,
                total_len: 0
            },
            1,
            0
        ),
        Some(0)
    );
    assert_eq!(
        score_with(
            Stats {
                docs: 2,
                total_len: 10
            },
            0,
            3
        ),
        Some(0)
    );
    assert!(
        score_with(
            Stats {
                docs: 2,
                total_len: 10
            },
            1,
            3
        )
        .unwrap()
            > 0,
        "control: a normal corpus scores"
    );
}

#[test]
fn the_score_matches_a_float_bm25_when_several_documents_hold_the_term() {
    let mut store = MemoryStore::new();
    let mut idx = MemoryIndex::default();
    let texts = [
        "alpha one two",
        "alpha alpha one two three four five",
        "alpha one",
        "beta one two three",
        "beta four",
        "gamma five six seven eight",
    ];
    let mut lens = Vec::new();
    for (i, t) in texts.iter().enumerate() {
        let c = store
            .put(
                NewChunk::new("c", "k", t, Origin::User, "cli"),
                Timestamp(i as u64 + 1),
            )
            .unwrap();
        idx.index(&c, t).unwrap();
        lens.push(t.split_whitespace().count() as f64);
    }
    let total: f64 = lens.iter().sum();
    let avgdl = total / texts.len() as f64;
    let n_docs = texts.len() as f64;
    let with_term = 3.0; // "alpha" is in three of the six
    let idf = ((n_docs + 1.0) / (with_term + 0.5)).ln();
    let hits = LexicalRetriever::new(idx)
        .retrieve(&Query::new("alpha"), &store)
        .unwrap();
    assert_eq!(hits.len(), 3);
    for h in &hits {
        let i = texts
            .iter()
            .position(|t| store.get(&h.chunk().id).unwrap().content() == *t)
            .unwrap();
        let tf = texts[i].matches("alpha").count() as f64;
        let want = idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * lens[i] / avgdl)) * 1000.0;
        let got = h.score().unwrap() as f64;
        assert!(
            (got - want).abs() <= 2.0,
            "doc {i}: got {got}, want about {want}"
        );
    }
}
