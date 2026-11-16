// SPDX-License-Identifier: MIT OR Apache-2.0
//! [`PersistTextIndex`]: `sageant-lexical`'s [`TextIndex`] over PersistAnt's own text index layer (`persistant` 0.18.22,
//! feature `text-index`), the replacement for the hand-written [`KvTextIndex`](crate::KvTextIndex).
//!
//! A document is one chunk version, named `<chunk id>/<version, 10 digits>` inside the index `sageant`, so PersistAnt's
//! ascending-by-id order is exactly our (chunk id, version) order. We keep our own tokeniser and BM25 (the layer takes
//! terms with frequencies and hands back integers). A removal drops every version of a chunk with
//! `remove_prefix_erased`, one atomic erased batch per round of documents; a backend that cannot overwrite refuses it, and
//! we then remove with plain deletes from then on (the same rule as the store).

use persistant::blocking::registry::KvHandle;
use persistant::kv::KvError;
use persistant::registry::Endpoint;
use sageant::{ChunkId, ChunkRef, Error};
use sageant_lexical::{Posting, Stats, TextIndex, term_counts};

use crate::{Chosen, Erasure, SCAN_PAGE, backend_erasure, choose_backend, kv_error, open_kv};

const INDEX: &str = "sageant";
/// Documents removed per round on the plain-delete path.
const ROUND: usize = 100;

fn doc_id(chunk: &ChunkRef) -> String {
    format!("{}/{:010}", chunk.id.as_str(), chunk.version)
}

fn parse_doc_id(doc: &str) -> Result<ChunkRef, Error> {
    let (id, version) = doc.split_once('/').ok_or(Error::Corrupt("index key"))?;
    Ok(ChunkRef {
        id: ChunkId::parse(id).map_err(|_| Error::Corrupt("index key"))?,
        version: version.parse().map_err(|_| Error::Corrupt("index key"))?,
    })
}

/// A text index kept by PersistAnt's text layer.
pub struct PersistTextIndex {
    kv: KvHandle,
    /// Whether a removal asks for the erased batches. Switched off for good the first time the backend refuses them.
    overwrite: std::sync::atomic::AtomicBool,
    /// Test only: make erased removals be refused as unsupported.
    refuse_erased_for_test: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for PersistTextIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistTextIndex")
            .field("backend", &self.kv.id())
            .finish()
    }
}

impl PersistTextIndex {
    /// Let PersistAnt choose the backend for `endpoint` and open the index in it.
    ///
    /// # Errors
    /// As [`PersistStore::open_chosen`](crate::PersistStore::open_chosen).
    pub fn open_chosen(endpoint: &Endpoint) -> Result<(Self, Chosen), Error> {
        let chosen = choose_backend(endpoint)?;
        let index = Self::open(chosen.backend(), endpoint)?;
        Ok((index, chosen))
    }

    /// Open the index in the PersistAnt backend named `backend_id`.
    ///
    /// # Errors
    /// `Backend` naming what is missing when the backend has no text layer (atomic batches and ordered scans), or
    /// cannot be opened.
    pub fn open(backend_id: &str, endpoint: &Endpoint) -> Result<Self, Error> {
        let erasure = backend_erasure(backend_id).unwrap_or(Erasure::Unretrievable);
        Self::open_with(backend_id, endpoint, erasure != Erasure::Zeroed)
    }

    /// Open with erased removals switched off, so a test can show what plain deletes leave in the files. Not for
    /// production use.
    ///
    /// # Errors
    /// As `open`.
    #[doc(hidden)]
    pub fn open_without_erasing_for_test(
        backend_id: &str,
        endpoint: &Endpoint,
    ) -> Result<Self, Error> {
        Self::open_with(backend_id, endpoint, false)
    }

    /// Test only: make erased removals be refused as unsupported, as a backend that cannot overwrite would.
    #[doc(hidden)]
    pub fn refuse_erased_batches_for_test(&self, on: bool) {
        self.refuse_erased_for_test
            .store(on, std::sync::atomic::Ordering::SeqCst);
    }

    /// Test only: whether removals still ask for erased batches.
    #[doc(hidden)]
    pub fn overwrites_on_remove_for_test(&self) -> bool {
        self.overwrite.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn open_with(backend_id: &str, endpoint: &Endpoint, overwrite: bool) -> Result<Self, Error> {
        let kv = open_kv(backend_id, endpoint)?;
        // Fail at open, not at the first search, when the backend has no text layer.
        kv.text().stats(INDEX).map_err(kv_error)?;
        Ok(Self {
            kv,
            overwrite: std::sync::atomic::AtomicBool::new(overwrite),
            refuse_erased_for_test: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Every document id under `prefix`, in rounds.
    fn plain_remove(&self, prefix: &str) -> Result<(), Error> {
        loop {
            let page = self
                .kv
                .text()
                .docs_with_prefix(INDEX, prefix, None, ROUND)
                .map_err(kv_error)?;
            if page.ids.is_empty() {
                return Ok(());
            }
            let ids: Vec<&str> = page.ids.iter().map(String::as_str).collect();
            self.kv.text().remove_many(INDEX, &ids).map_err(kv_error)?;
        }
    }
}

impl TextIndex for PersistTextIndex {
    fn index(&mut self, chunk: &ChunkRef, text: &str) -> Result<(), Error> {
        let terms: Vec<(String, u32)> = term_counts(text).into_iter().collect();
        self.kv
            .text()
            .put(INDEX, &doc_id(chunk), &terms)
            .map_err(kv_error)
    }

    fn remove(&mut self, id: &ChunkId) -> Result<(), Error> {
        use std::sync::atomic::Ordering;
        let prefix = format!("{}/", id.as_str());
        if self.overwrite.load(Ordering::SeqCst) {
            let first = if self.refuse_erased_for_test.load(Ordering::SeqCst) {
                Err(KvError::Unsupported("simulated".to_owned()))
            } else {
                self.kv.text().remove_prefix_erased(INDEX, &prefix)
            };
            match first {
                Ok(_) => return Ok(()),
                Err(KvError::Unsupported(_)) => self.overwrite.store(false, Ordering::SeqCst),
                Err(e) => return Err(kv_error(e)),
            }
        }
        self.plain_remove(&prefix)
    }

    fn postings(&self, term: &str) -> Result<Vec<Posting>, Error> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self
                .kv
                .text()
                .postings(INDEX, term, after.as_deref(), SCAN_PAGE)
                .map_err(kv_error)?;
            for (doc, tf) in &page.entries {
                out.push(Posting::new(parse_doc_id(doc)?, *tf));
            }
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(out),
            }
        }
    }

    fn doc_len(&self, chunk: &ChunkRef) -> Result<Option<u32>, Error> {
        self.kv
            .text()
            .doc_len(INDEX, &doc_id(chunk))
            .map_err(kv_error)
    }

    fn stats(&self) -> Result<Stats, Error> {
        let s = self.kv.text().stats(INDEX).map_err(kv_error)?;
        Ok(Stats {
            docs: s.docs,
            total_len: s.total_len,
        })
    }
}
