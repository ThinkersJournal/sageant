// SPDX-License-Identifier: MIT OR Apache-2.0
//! Row 4, first crate: lexical retrieval (CONTRACT D10, D17). Integer arithmetic only, so scores are identical on
//! every machine (D16). Written before the code.

use sageant::{Filter, MemoryStore, NewChunk, Origin, Query, Reader, Reason, Retriever, Timestamp};
use sageant_lexical::{
    LexicalRetriever, MemoryIndex, TextIndex, ln_ratio_micro, term_counts, tokenize,
};

fn put(s: &mut MemoryStore, coll: &str, text: &str) -> sageant::ChunkId {
    s.put(
        NewChunk::new(coll, "note", text, Origin::User, "cli"),
        Timestamp(1),
    )
    .unwrap()
    .id
}

#[test]
fn the_tokenizer_lowercases_and_splits_on_anything_not_alphanumeric() {
    assert_eq!(
        tokenize("Rust's Memory-Safety, 2026!  CAFÉ"),
        vec!["rust", "s", "memory", "safety", "2026", "café"]
    );
    assert!(tokenize("  ...  ").is_empty());
    assert!(tokenize("").is_empty());
}

#[test]
fn term_counts_count_repeats_in_sorted_order() {
    let c: Vec<(String, u32)> = term_counts("b a b c b a").into_iter().collect();
    assert_eq!(
        c,
        vec![
            ("a".to_owned(), 2),
            ("b".to_owned(), 3),
            ("c".to_owned(), 1)
        ]
    );
}

#[test]
fn the_integer_log_matches_known_values_to_a_micro_unit() {
    assert_eq!(ln_ratio_micro(1, 1), 0);
    for (num, den, want) in [
        (2u64, 1u64, 693_147i64),
        (10, 1, 2_302_585),
        (1, 2, -693_147),
        (3, 2, 405_465),
        (1000, 7, 4_961_845),
    ] {
        let got = ln_ratio_micro(num, den);
        assert!(
            (got - want).abs() <= 2,
            "ln({num}/{den}) = {got}, want about {want}"
        );
    }
    // Monotone.
    assert!(ln_ratio_micro(5, 3) < ln_ratio_micro(6, 3));
}

fn corpus() -> (MemoryStore, [sageant::ChunkId; 4]) {
    let mut s = MemoryStore::new();
    let ids = [
        put(&mut s, "n", "rust is a systems language with memory safety"),
        put(&mut s, "n", "memory allocators and memory pools"),
        put(&mut s, "n", "gardening notes about tomatoes"),
        put(&mut s, "m", "rust rust rust"),
    ];
    (s, ids)
}

#[test]
fn a_query_ranks_the_chunk_with_more_of_its_terms_first_and_says_why() {
    let (s, [a, b, ..]) = corpus();
    let r = LexicalRetriever::new(MemoryIndex::from_reader(&s).unwrap());
    // Within collection "n": `a` holds both terms, `b` holds only "memory" (twice).
    let q = Query::new("rust memory").filter(Filter::new().collection("n"));
    let hits = r.retrieve(&q, &s).unwrap();
    assert_eq!(hits[0].chunk().id, a);
    assert_eq!(
        hits[0].reason(),
        &Reason::Lexical {
            terms: vec!["memory".to_owned(), "rust".to_owned()]
        }
    );
    assert!(hits.iter().any(|h| h.chunk().id == b));
    assert!(hits.iter().all(|h| h.score().is_some_and(|v| v > 0)));
    let scores: Vec<i64> = hits.iter().filter_map(|h| h.score()).collect();
    assert!(
        scores.windows(2).all(|w| w[0] >= w[1]),
        "scores descend: {scores:?}"
    );
}

#[test]
fn the_integer_scores_match_a_float_bm25_to_a_thousandth() {
    // Float reference computed independently (python, k1 = 1.2, b = 0.75, idf = ln(1 + (N - n + 0.5) / (n + 0.5))):
    // a = 1.1131, b = 0.9531, d = 1.1913. Integer scores are in thousandths, so allow 2 for truncation.
    let (s, [a, b, _, d]) = corpus();
    let r = LexicalRetriever::new(MemoryIndex::from_reader(&s).unwrap());
    let hits = r.retrieve(&Query::new("rust memory"), &s).unwrap();
    let score = |id: &sageant::ChunkId| {
        hits.iter()
            .find(|h| &h.chunk().id == id)
            .and_then(sageant::Hit::score)
            .unwrap()
    };
    for (id, want) in [(&a, 1113i64), (&b, 953), (&d, 1191)] {
        assert!(
            (score(id) - want).abs() <= 2,
            "got {}, want about {want}",
            score(id)
        );
    }
    assert_eq!(hits[0].chunk().id, d); // shortest document with the most "rust" wins overall
}

#[test]
fn a_rarer_term_is_worth_more() {
    let (s, [_, _, gardening, _]) = corpus();
    let r = LexicalRetriever::new(MemoryIndex::from_reader(&s).unwrap());
    let rare = r.retrieve(&Query::new("tomatoes"), &s).unwrap();
    let common = r.retrieve(&Query::new("memory"), &s).unwrap();
    assert_eq!(rare[0].chunk().id, gardening);
    assert!(rare[0].score() > common[0].score());
}

#[test]
fn no_matching_term_or_an_empty_query_returns_nothing() {
    let (s, _) = corpus();
    let r = LexicalRetriever::new(MemoryIndex::from_reader(&s).unwrap());
    assert!(r.retrieve(&Query::new("zeppelin"), &s).unwrap().is_empty());
    assert!(r.retrieve(&Query::new("  !! "), &s).unwrap().is_empty());
    let empty = LexicalRetriever::new(MemoryIndex::default());
    assert!(empty.retrieve(&Query::new("rust"), &s).unwrap().is_empty());
}

#[test]
fn the_filter_and_limit_apply_and_archived_or_forgotten_chunks_are_not_returned() {
    let (mut s, [a, b, _, d]) = corpus();
    let r = LexicalRetriever::new(MemoryIndex::from_reader(&s).unwrap());
    let only_m = r
        .retrieve(
            &Query::new("rust").filter(Filter::new().collection("m")),
            &s,
        )
        .unwrap();
    assert_eq!(only_m.len(), 1);
    assert_eq!(only_m[0].chunk().id, d);
    assert_eq!(
        r.retrieve(&Query::new("rust memory").limit(1), &s)
            .unwrap()
            .len(),
        1
    );
    // The index is not updated, but the reader decides what is visible.
    s.archive(&a, "x", "r", Timestamp(2)).unwrap();
    s.forget(&b, "x", "r", None, Timestamp(3)).unwrap();
    let ids: Vec<_> = r
        .retrieve(&Query::new("rust memory"), &s)
        .unwrap()
        .into_iter()
        .map(|h| h.chunk().id.clone())
        .collect();
    assert!(!ids.contains(&a) && !ids.contains(&b));
}

#[test]
fn indexing_order_does_not_change_the_result() {
    let (s, ids) = corpus();
    let mut forward = MemoryIndex::default();
    let mut backward = MemoryIndex::default();
    for id in &ids {
        let c = Reader::get(&s, id).unwrap();
        forward.index(
            &sageant::ChunkRef {
                id: id.clone(),
                version: 1,
            },
            c.content(),
        );
    }
    for id in ids.iter().rev() {
        let c = Reader::get(&s, id).unwrap();
        backward.index(
            &sageant::ChunkRef {
                id: id.clone(),
                version: 1,
            },
            c.content(),
        );
    }
    let q = Query::new("rust memory pools");
    let a = LexicalRetriever::new(forward).retrieve(&q, &s).unwrap();
    let b = LexicalRetriever::new(backward).retrieve(&q, &s).unwrap();
    assert_eq!(a, b);
}

#[test]
fn reindexing_a_chunk_replaces_its_postings_and_remove_drops_a_lineage() {
    let (s, [a, ..]) = corpus();
    let mut idx = MemoryIndex::from_reader(&s).unwrap();
    let r1 = sageant::ChunkRef {
        id: a.clone(),
        version: 1,
    };
    assert!(idx.postings("safety").iter().any(|p| p.chunk() == &r1));
    idx.index(&r1, "completely different words");
    assert!(idx.postings("safety").is_empty());
    assert!(!idx.postings("different").is_empty());
    assert_eq!(idx.doc_len(&r1), Some(3));
    idx.remove(&a);
    assert!(idx.postings("different").is_empty());
    assert_eq!(idx.doc_len(&r1), None);
    assert_eq!(idx.stats().docs, 3);
}

#[test]
fn stats_count_documents_and_tokens() {
    let mut idx = MemoryIndex::default();
    let (_, [a, b, ..]) = corpus();
    idx.index(&sageant::ChunkRef { id: a, version: 1 }, "one two three");
    idx.index(&sageant::ChunkRef { id: b, version: 1 }, "four five");
    let st = idx.stats();
    assert_eq!((st.docs, st.total_len), (2, 5));
}
