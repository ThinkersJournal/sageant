// SPDX-License-Identifier: MIT OR Apache-2.0
//! Retrieval types (CONTRACT D10): a retriever sees a [`Reader`] and a [`Query`] and returns scored, explained
//! [`Hit`]s. Combining several retrievers is the caller's job, through a [`Fuser`].

use crate::Error;
use crate::model::{ChunkId, ChunkRef};
use crate::reader::{Filter, Reader};

/// What to look for. `context` is opaque caller data the store never reads.
#[derive(Debug, Clone)]
pub struct Query {
    text: String,
    filter: Filter,
    limit: usize,
    context: Vec<u8>,
}

impl Query {
    /// A query for `text` with no filter and a limit of 1000 hits.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            filter: Filter::new(),
            limit: 1000,
            context: Vec::new(),
        }
    }
    /// Narrow the candidates.
    #[must_use]
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filter = filter;
        self
    }
    /// Return at most `n` hits.
    #[must_use]
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = n;
        self
    }
    /// Attach opaque caller data.
    #[must_use]
    pub fn context(mut self, bytes: Vec<u8>) -> Self {
        self.context = bytes;
        self
    }
    /// The query text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
    /// The candidate filter (read it as `q.filter()`; the builder method of the same name sets it).
    #[must_use]
    pub fn filter_ref(&self) -> &Filter {
        &self.filter
    }
    /// The hit limit.
    #[must_use]
    pub fn limit_value(&self) -> usize {
        self.limit
    }
    /// The opaque caller data.
    #[must_use]
    pub fn context_bytes(&self) -> &[u8] {
        &self.context
    }
}

/// Why a chunk was returned, so a harness can show it (CONTRACT D10). There is no tag reason: the model has no tags.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reason {
    /// Matched these query terms.
    Lexical {
        /// The matching terms.
        terms: Vec<String>,
    },
    /// Returned because it is recent.
    Recency,
    /// Linked to another hit.
    LinkNeighbour {
        /// The chunk it is linked to.
        via: ChunkId,
    },
    /// Anything else, in the retriever's words.
    Custom(String),
}

/// One retrieved chunk. `score` is an optional integer (basis points by convention): fusion wants ranks or validated
/// scores, and floats bring NaN and `-0.0` (CONTRACT D10). `None` means rank only; it sorts after every scored hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    chunk: ChunkRef,
    score: Option<i64>,
    reason: Reason,
}

impl Hit {
    /// A hit.
    #[must_use]
    pub fn new(chunk: ChunkRef, score: Option<i64>, reason: Reason) -> Self {
        Self {
            chunk,
            score,
            reason,
        }
    }
    /// Which chunk version.
    #[must_use]
    pub fn chunk(&self) -> &ChunkRef {
        &self.chunk
    }
    /// The score, if the retriever gave one.
    #[must_use]
    pub fn score(&self) -> Option<i64> {
        self.score
    }
    /// Why it was returned.
    #[must_use]
    pub fn reason(&self) -> &Reason {
        &self.reason
    }
}

/// Something that finds chunks. It receives a [`Reader`] and so cannot write.
pub trait Retriever {
    /// The best hits for `query`, already ordered (see [`order_hits`]) and at most `query.limit_value()` long.
    ///
    /// # Errors
    /// Whatever the retriever's backend reports.
    fn retrieve(&self, query: &Query, store: &dyn Reader) -> Result<Vec<Hit>, Error>;
}

/// Order hits by score descending (unscored last), then chunk id ascending, then version descending. This is the
/// stated determinism rule until SortAnt replaces it (CONTRACT D10).
pub fn order_hits(hits: &mut [Hit]) {
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.chunk.id.cmp(&b.chunk.id))
            .then_with(|| b.chunk.version.cmp(&a.chunk.version))
    });
}

/// Combines the results of several retrievers into one ordered list.
pub trait Fuser {
    /// Fuse `lists` (each in its retriever's order) into one list.
    fn fuse(&self, lists: &[Vec<Hit>]) -> Vec<Hit>;
}

/// The default fuser: score descending (unscored last), ties in input order; a chunk version that appears in several
/// lists keeps its best score and that hit's reason. SortAnt implements a better one later.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultFuser;

impl Fuser for DefaultFuser {
    fn fuse(&self, lists: &[Vec<Hit>]) -> Vec<Hit> {
        let mut out: Vec<Hit> = Vec::new();
        for hit in lists.iter().flatten() {
            match out.iter_mut().find(|h| h.chunk == hit.chunk) {
                Some(seen) => {
                    if hit.score > seen.score {
                        *seen = hit.clone();
                    }
                }
                None => out.push(hit.clone()),
            }
        }
        // `sort_by` is stable, so equal scores keep input order.
        out.sort_by_key(|h| core::cmp::Reverse(h.score));
        out
    }
}
