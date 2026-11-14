// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 9 of ROADMAP row 3: retriever types and the default `Fuser` (CONTRACT D10). Written before the code.
//! A retriever gets a `&dyn Reader` and a `Query`; combining retrievers is the caller's job through a `Fuser`.

use sageant::{
    ChunkId, ChunkRef, DefaultFuser, Error, Filter, Fuser, Hit, MemoryStore, NewChunk, Origin,
    Query, Reader, Reason, Retriever, Timestamp, order_hits,
};

fn r(id: &ChunkId, version: u32) -> ChunkRef {
    ChunkRef {
        id: id.clone(),
        version,
    }
}

fn ids3() -> (MemoryStore, [ChunkId; 3]) {
    let mut s = MemoryStore::new();
    let mut put = |t: &str| {
        s.put(
            NewChunk::new("n", "note", t, Origin::User, "cli"),
            Timestamp(1),
        )
        .unwrap()
        .id
    };
    let ids = [put("aaa"), put("bbb"), put("ccc")];
    (s, ids)
}

/// A retriever that returns every chunk the filter lists, newest first, scored by `created_at`.
struct Everything;
impl Retriever for Everything {
    fn retrieve(&self, q: &Query, store: &dyn Reader) -> Result<Vec<Hit>, Error> {
        let mut hits: Vec<Hit> = store
            .list(q.filter_ref())
            .into_iter()
            .map(|chunk| Hit::new(chunk, Some(100), Reason::Recency))
            .collect();
        order_hits(&mut hits);
        hits.truncate(q.limit_value());
        Ok(hits)
    }
}

#[test]
fn a_retriever_runs_over_a_dyn_reader_and_respects_the_query() {
    let (s, _) = ids3();
    let q = Query::new("anything")
        .filter(Filter::new().collection("n"))
        .limit(2);
    assert_eq!(q.text(), "anything");
    let hits = Everything.retrieve(&q, &s).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.reason() == &Reason::Recency));
    // The default limit is generous, not zero.
    assert_eq!(Everything.retrieve(&Query::new("x"), &s).unwrap().len(), 3);
}

#[test]
fn query_carries_opaque_context_untouched() {
    let q = Query::new("x").context(b"caller data".to_vec());
    assert_eq!(q.context_bytes(), b"caller data");
    assert!(Query::new("x").context_bytes().is_empty());
}

#[test]
fn order_hits_is_score_desc_then_id_asc_then_version_desc_with_unscored_last() {
    let (_, [a, b, _]) = ids3();
    let (lo, hi) = if a < b { (&a, &b) } else { (&b, &a) };
    let mut hits = vec![
        Hit::new(r(hi, 1), Some(5), Reason::Recency),
        Hit::new(r(lo, 1), Some(5), Reason::Recency),
        Hit::new(r(lo, 2), Some(5), Reason::Recency),
        Hit::new(r(hi, 1), None, Reason::Recency),
        Hit::new(r(lo, 1), Some(9), Reason::Recency),
    ];
    order_hits(&mut hits);
    let got: Vec<(i64, u32, bool)> = hits
        .iter()
        .map(|h| {
            (
                h.score().unwrap_or(-1),
                h.chunk().version,
                &h.chunk().id == lo,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (9, 1, true),
            (5, 2, true),
            (5, 1, true),
            (5, 1, false),
            (-1, 1, false)
        ]
    );
}

#[test]
fn the_default_fuser_orders_by_score_then_input_order_and_keeps_the_best_duplicate() {
    let (_, [a, b, c]) = ids3();
    let one = vec![
        Hit::new(
            r(&a, 1),
            Some(10),
            Reason::Lexical {
                terms: vec!["x".into()],
            },
        ),
        Hit::new(r(&b, 1), Some(30), Reason::Recency),
    ];
    let two = vec![
        Hit::new(r(&c, 1), Some(30), Reason::Custom("two".into())),
        Hit::new(r(&a, 1), Some(20), Reason::Custom("better a".into())),
    ];
    let out = DefaultFuser.fuse(&[one, two]);
    let got: Vec<_> = out
        .iter()
        .map(|h| (h.chunk().id.clone(), h.score()))
        .collect();
    // b (30, list 0 first) before c (30, list 1); a keeps its best score 20.
    assert_eq!(got, vec![(b, Some(30)), (c, Some(30)), (a, Some(20))]);
    assert_eq!(out[2].reason(), &Reason::Custom("better a".into()));
}

#[test]
fn the_default_fuser_puts_unscored_hits_after_scored_ones_in_input_order() {
    let (_, [a, b, c]) = ids3();
    let out = DefaultFuser.fuse(&[
        vec![Hit::new(r(&a, 1), None, Reason::Recency)],
        vec![
            Hit::new(r(&b, 1), None, Reason::Recency),
            Hit::new(r(&c, 1), Some(1), Reason::Recency),
        ],
    ]);
    let order: Vec<_> = out.iter().map(|h| h.chunk().id.clone()).collect();
    assert_eq!(order, vec![c, a, b]);
    assert!(DefaultFuser.fuse(&[]).is_empty());
}

#[test]
fn different_versions_of_one_chunk_are_different_hits() {
    let (_, [a, ..]) = ids3();
    let out = DefaultFuser.fuse(&[vec![
        Hit::new(r(&a, 1), Some(1), Reason::Recency),
        Hit::new(r(&a, 2), Some(1), Reason::Recency),
    ]]);
    assert_eq!(out.len(), 2);
}
