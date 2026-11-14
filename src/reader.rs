// SPDX-License-Identifier: MIT OR Apache-2.0
//! The read side of the store (CONTRACT D10, D11): what a retriever is allowed to see.

use crate::Error;
use crate::model::{Chunk, ChunkId, ChunkRef, Confidence, Link, Origin, Status, Timestamp};

/// Which chunks `Reader::list` returns. Every field narrows; the default lists current, non-archived versions.
///
/// Order of results is `(ChunkId` ascending, version descending`)`, stated and tested (CONTRACT D10).
/// There are no tags in the chunk model yet, so there is no tag filter.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub(crate) collection: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) origin: Option<Origin>,
    pub(crate) status: Option<Status>,
    pub(crate) confidence: Option<Confidence>,
    pub(crate) from: Option<Timestamp>,
    pub(crate) until: Option<Timestamp>,
    pub(crate) include_archived: bool,
    pub(crate) include_superseded: bool,
    pub(crate) limit: Option<usize>,
}

impl Filter {
    /// A filter that matches every current, non-archived chunk.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Only this collection.
    #[must_use]
    pub fn collection(mut self, collection: &str) -> Self {
        self.collection = Some(collection.to_owned());
        self
    }
    /// Only this kind.
    #[must_use]
    pub fn kind(mut self, kind: &str) -> Self {
        self.kind = Some(kind.to_owned());
        self
    }
    /// Only this origin class.
    #[must_use]
    pub fn origin(mut self, origin: Origin) -> Self {
        self.origin = Some(origin);
        self
    }
    /// Only this status. Asking for `Archived` shows archived chunks without `include_archived`.
    #[must_use]
    pub fn status(mut self, status: Status) -> Self {
        self.status = Some(status);
        self
    }
    /// Only chunks whose derived confidence (from links) is exactly this.
    #[must_use]
    pub fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = Some(confidence);
        self
    }
    /// Only versions created at or after this time.
    #[must_use]
    pub fn created_from(mut self, at: Timestamp) -> Self {
        self.from = Some(at);
        self
    }
    /// Only versions created at or before this time.
    #[must_use]
    pub fn created_until(mut self, at: Timestamp) -> Self {
        self.until = Some(at);
        self
    }
    /// Also list archived chunks.
    #[must_use]
    pub fn include_archived(mut self) -> Self {
        self.include_archived = true;
        self
    }
    /// Also list superseded (older) versions, newest first within a lineage.
    #[must_use]
    pub fn include_superseded(mut self) -> Self {
        self.include_superseded = true;
        self
    }
    /// Keep at most the first `n` results in order.
    #[must_use]
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    pub(crate) fn matches(&self, c: &Chunk, confidence: Confidence) -> bool {
        self.collection.as_deref().is_none_or(|v| v == c.collection)
            && self.kind.as_deref().is_none_or(|v| v == c.kind)
            && self.origin.is_none_or(|v| v == c.origin)
            && self.confidence.is_none_or(|v| v == confidence)
            && self.from.is_none_or(|t| c.created_at >= t)
            && self.until.is_none_or(|t| c.created_at <= t)
            && match self.status {
                Some(s) => c.status == s,
                None => self.include_archived || c.status == Status::Active,
            }
    }
}

/// Read-only access to a store. A retriever receives a `&dyn Reader` and nothing that can write.
///
/// Every read returns an owned value (CONTRACT D11, 0.1.0): a backend that keeps its data in storage cannot lend
/// references into it, and a trait must not force the whole store into memory. `MemoryStore` also keeps its borrowing
/// inherent methods; on a concrete `MemoryStore` those are the ones called.
pub trait Reader {
    /// The current version. See `MemoryStore::get`.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    fn get(&self, id: &ChunkId) -> Result<Chunk, Error>;
    /// A specific version, from 1.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    fn get_version(&self, id: &ChunkId, version: u32) -> Result<Chunk, Error>;
    /// Every version, oldest first.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    fn history(&self, id: &ChunkId) -> Result<Vec<Chunk>, Error>;
    /// Links made from `id`.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    fn links_from(&self, id: &ChunkId) -> Result<Vec<Link>, Error>;
    /// Links pointing at `id`.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    fn links_to(&self, id: &ChunkId) -> Result<Vec<Link>, Error>;
    /// The derived confidence of `id`.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    fn confidence(&self, id: &ChunkId) -> Result<Confidence, Error>;
    /// The chunks matching `filter`, in `(ChunkId` ascending, version descending`)` order.
    fn list(&self, filter: &Filter) -> Vec<ChunkRef>;
}
