// SPDX-License-Identifier: MIT OR Apache-2.0
//! A [`TextIndex`] on a key-value backend: the postings behind `sageant-lexical`'s BM25 retriever, kept durably.
//!
//! Keys: `x/<term>/<id>/<version, 10 digits>` holds the term frequency; `d/<id>/<version>` holds a document's token
//! count; `f/<id>/<version>/<term>` lists a document's terms so re-indexing and removal can find its postings;
//! `m/ix` holds the corpus totals as `<docs> <tokens>`. Every change is one atomic batch with a `Require` on the
//! totals, so two writers cannot both win and the totals always match the postings; a lost race is retried.

use persistant::blocking::registry::KvHandle;
use persistant::kv::BatchOp;
use persistant::registry::Endpoint;
use sageant::{ChunkId, ChunkRef, Error};
use sageant_lexical::{Posting, Stats, TextIndex, term_counts};

use crate::{Chosen, NO_TTL, SCAN_PAGE, choose_backend, kv_error, open_kv};

const TOTALS: &str = "m/ix";
const RETRIES: usize = 16;

fn posting_key(term: &str, id: &ChunkId, version: u32) -> String {
    format!("x/{term}/{}/{version:010}", id.as_str())
}

fn doc_key(id: &ChunkId, version: u32) -> String {
    format!("d/{}/{version:010}", id.as_str())
}

fn terms_prefix(id: &ChunkId, version: u32) -> String {
    format!("f/{}/{version:010}/", id.as_str())
}

fn number<T: core::str::FromStr>(bytes: &[u8]) -> Result<T, Error> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|t| t.parse().ok())
        .ok_or(Error::Corrupt("index number"))
}

/// What one index change does: the operations, and the change to the document count and token total.
type Change = (Vec<BatchOp>, i64, i64);

/// A text index kept in a PersistAnt key-value backend.
pub struct KvTextIndex {
    kv: KvHandle,
}

impl std::fmt::Debug for KvTextIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KvTextIndex")
            .field("backend", &self.kv.id())
            .finish()
    }
}

impl KvTextIndex {
    /// Let PersistAnt choose the backend for `endpoint` and open the index in it; see
    /// [`PersistStore::open_chosen`](crate::PersistStore::open_chosen).
    ///
    /// # Errors
    /// As `PersistStore::open_chosen`.
    pub fn open_chosen(endpoint: &Endpoint) -> Result<(Self, Chosen), Error> {
        let chosen = choose_backend(endpoint)?;
        let index = Self::open(chosen.backend(), endpoint)?;
        Ok((index, chosen))
    }

    /// Open the index in the PersistAnt backend named `backend_id`. It may share a backend with a
    /// [`PersistStore`](crate::PersistStore): their key prefixes do not overlap.
    ///
    /// # Errors
    /// `Backend` naming what is missing when the backend cannot give atomic batches and ordered scans, or cannot
    /// be opened.
    pub fn open(backend_id: &str, endpoint: &Endpoint) -> Result<Self, Error> {
        Ok(Self {
            kv: open_kv(backend_id, endpoint)?,
        })
    }

    /// Every `(key, value)` under `prefix`, ascending by key, across pages.
    fn scan(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self
                .kv
                .ordered_scan_values(prefix, after.as_deref(), SCAN_PAGE)
                .map_err(kv_error)?;
            out.extend(page.entries);
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(out),
            }
        }
    }

    fn totals(&self) -> Result<(Option<Vec<u8>>, Stats), Error> {
        let Some(bytes) = self.kv.get(TOTALS).map_err(kv_error)? else {
            return Ok((None, Stats::default()));
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::Corrupt("index totals"))?;
        let (docs, len) = text.split_once(' ').ok_or(Error::Corrupt("index totals"))?;
        let stats = Stats {
            docs: number(docs.as_bytes())?,
            total_len: number(len.as_bytes())?,
        };
        Ok((Some(bytes), stats))
    }

    /// The operations that drop one indexed document, and its token count (`None` if it was not indexed).
    fn drop_ops(&self, id: &ChunkId, version: u32) -> Result<(Vec<BatchOp>, Option<u32>), Error> {
        let Some(len) = self.kv.get(&doc_key(id, version)).map_err(kv_error)? else {
            return Ok((Vec::new(), None));
        };
        let prefix = terms_prefix(id, version);
        let mut ops = vec![BatchOp::Delete {
            key: doc_key(id, version),
        }];
        for (key, _) in self.scan(&prefix)? {
            let term = key
                .strip_prefix(&prefix)
                .ok_or(Error::Corrupt("index key"))?;
            ops.push(BatchOp::Delete {
                key: posting_key(term, id, version),
            });
            ops.push(BatchOp::Delete { key });
        }
        Ok((ops, Some(number(&len)?)))
    }

    /// Apply the change `build` describes together with the new totals, retrying a lost race for the totals.
    fn commit(&self, build: &dyn Fn(&Self) -> Result<Change, Error>) -> Result<(), Error> {
        for _ in 0..RETRIES {
            let (seen, stats) = self.totals()?;
            let (mut ops, docs, len) = build(self)?;
            let new_docs = u64::try_from(i64::try_from(stats.docs).unwrap_or(i64::MAX) + docs)
                .map_err(|_| Error::Corrupt("index totals"))?;
            let new_len = u64::try_from(i64::try_from(stats.total_len).unwrap_or(i64::MAX) + len)
                .map_err(|_| Error::Corrupt("index totals"))?;
            ops.insert(
                0,
                BatchOp::Require {
                    key: TOTALS.to_owned(),
                    expected: seen,
                },
            );
            ops.push(BatchOp::Put {
                key: TOTALS.to_owned(),
                value: format!("{new_docs} {new_len}").into_bytes(),
                ttl: NO_TTL,
            });
            if self.kv.batch(&ops).map_err(kv_error)? {
                return Ok(());
            }
        }
        Err(Error::Backend("index contention".to_owned()))
    }
}

impl TextIndex for KvTextIndex {
    fn index(&mut self, chunk: &ChunkRef, text: &str) -> Result<(), Error> {
        let counts = term_counts(text);
        let len: u32 = counts.values().fold(0u32, |a, n| a.saturating_add(*n));
        self.commit(&|s| {
            let (mut ops, old) = s.drop_ops(&chunk.id, chunk.version)?;
            let (mut docs, mut tokens) = (1_i64, i64::from(len));
            if let Some(old) = old {
                docs -= 1;
                tokens -= i64::from(old);
            }
            for (term, tf) in &counts {
                ops.push(BatchOp::Put {
                    key: posting_key(term, &chunk.id, chunk.version),
                    value: tf.to_string().into_bytes(),
                    ttl: NO_TTL,
                });
                ops.push(BatchOp::Put {
                    key: format!("{}{term}", terms_prefix(&chunk.id, chunk.version)),
                    value: Vec::new(),
                    ttl: NO_TTL,
                });
            }
            ops.push(BatchOp::Put {
                key: doc_key(&chunk.id, chunk.version),
                value: len.to_string().into_bytes(),
                ttl: NO_TTL,
            });
            Ok((ops, docs, tokens))
        })
    }

    fn remove(&mut self, id: &ChunkId) -> Result<(), Error> {
        self.commit(&|s| {
            let prefix = format!("d/{}/", id.as_str());
            let (mut ops, mut docs, mut tokens) = (Vec::new(), 0_i64, 0_i64);
            for (key, _) in s.scan(&prefix)? {
                let version: u32 = number(
                    key.strip_prefix(&prefix)
                        .ok_or(Error::Corrupt("index key"))?
                        .as_bytes(),
                )?;
                let (drop, old) = s.drop_ops(id, version)?;
                ops.extend(drop);
                if let Some(old) = old {
                    docs -= 1;
                    tokens -= i64::from(old);
                }
            }
            Ok((ops, docs, tokens))
        })
    }

    fn postings(&self, term: &str) -> Result<Vec<Posting>, Error> {
        let prefix = format!("x/{term}/");
        let mut out = Vec::new();
        for (key, value) in self.scan(&prefix)? {
            let rest = key
                .strip_prefix(&prefix)
                .ok_or(Error::Corrupt("index key"))?;
            let (id, version) = rest.split_once('/').ok_or(Error::Corrupt("index key"))?;
            let chunk = ChunkRef {
                id: ChunkId::parse(id).map_err(|_| Error::Corrupt("index key"))?,
                version: number(version.as_bytes())?,
            };
            out.push(Posting::new(chunk, number(&value)?));
        }
        Ok(out)
    }

    fn doc_len(&self, chunk: &ChunkRef) -> Result<Option<u32>, Error> {
        self.kv
            .get(&doc_key(&chunk.id, chunk.version))
            .map_err(kv_error)?
            .map(|bytes| number(&bytes))
            .transpose()
    }

    fn stats(&self) -> Result<Stats, Error> {
        Ok(self.totals()?.1)
    }
}
