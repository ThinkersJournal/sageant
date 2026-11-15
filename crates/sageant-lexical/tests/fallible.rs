// SPDX-License-Identifier: MIT OR Apache-2.0
//! A key-value-backed index can fail, so `TextIndex` is fallible and a failure reaches the caller of
//! `LexicalRetriever::retrieve` instead of looking like "no matches". Written before the code.

use sageant::{ChunkId, ChunkRef, Error, MemoryStore, Query, Retriever};
use sageant_lexical::{LexicalRetriever, Posting, Stats, TextIndex};

struct Broken;
impl TextIndex for Broken {
    fn index(&mut self, _: &ChunkRef, _: &str) -> Result<(), Error> {
        Err(Error::Backend("down".into()))
    }
    fn remove(&mut self, _: &ChunkId) -> Result<(), Error> {
        Err(Error::Backend("down".into()))
    }
    fn postings(&self, _: &str) -> Result<Vec<Posting>, Error> {
        Err(Error::Backend("down".into()))
    }
    fn doc_len(&self, _: &ChunkRef) -> Result<Option<u32>, Error> {
        Err(Error::Backend("down".into()))
    }
    fn stats(&self) -> Result<Stats, Error> {
        Err(Error::Backend("down".into()))
    }
}

#[test]
fn a_failing_index_is_an_error_not_an_empty_result() {
    let r = LexicalRetriever::new(Broken);
    let got = r.retrieve(&Query::new("anything"), &MemoryStore::new());
    assert!(matches!(got, Err(Error::Backend(_))));
}
