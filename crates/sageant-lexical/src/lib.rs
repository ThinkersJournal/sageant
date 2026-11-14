// SPDX-License-Identifier: MIT OR Apache-2.0

//! Lexical retrieval for `sageant` (CONTRACT D10, D17): a tokenizer, a [`TextIndex`] trait (so a key-value backend can
//! hold the postings), an in-memory index, and a [`Retriever`] that scores with BM25 using integer
//! arithmetic only, so a score is identical on every machine (CONTRACT D16).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::{BTreeMap, BTreeSet};

use sageant::{
    ChunkId, ChunkRef, Error, Filter, Hit, Query, Reader, Reason, Retriever, order_hits,
};

/// Split `text` into lowercase terms: maximal runs of alphanumeric characters, in order, repeats kept.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// How often each term occurs in `text`, in term order.
#[must_use]
pub fn term_counts(text: &str) -> BTreeMap<String, u32> {
    let mut counts = BTreeMap::new();
    for term in tokenize(text) {
        let n = counts.entry(term).or_insert(0u32);
        *n = n.saturating_add(1);
    }
    counts
}

/// One entry of a term's postings: which chunk version and how often the term occurs in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posting {
    chunk: ChunkRef,
    tf: u32,
}

impl Posting {
    /// A posting.
    #[must_use]
    pub fn new(chunk: ChunkRef, tf: u32) -> Self {
        Self { chunk, tf }
    }
    /// The chunk version.
    #[must_use]
    pub fn chunk(&self) -> &ChunkRef {
        &self.chunk
    }
    /// Term frequency in that version.
    #[must_use]
    pub fn tf(&self) -> u32 {
        self.tf
    }
}

/// Corpus totals the scorer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Number of indexed documents (chunk versions).
    pub docs: u64,
    /// Total number of tokens across them.
    pub total_len: u64,
}

/// Where postings live. The in-memory [`MemoryIndex`] is the reference; a key-value backend implements the same
/// trait (CONTRACT D17). Every method returns owned values so a backend need not lend references.
pub trait TextIndex {
    /// Index `text` as `chunk`, replacing anything indexed earlier for that exact chunk version.
    fn index(&mut self, chunk: &ChunkRef, text: &str);
    /// Drop every version of a lineage.
    fn remove(&mut self, id: &ChunkId);
    /// The postings of `term`, ordered by (chunk id, version).
    fn postings(&self, term: &str) -> Vec<Posting>;
    /// The token count of an indexed chunk version.
    fn doc_len(&self, chunk: &ChunkRef) -> Option<u32>;
    /// Corpus totals.
    fn stats(&self) -> Stats;
}

type DocKey = (ChunkId, u32);

/// An in-memory [`TextIndex`]. Ordered maps only, so no hash order reaches any result.
#[derive(Debug, Clone, Default)]
pub struct MemoryIndex {
    terms: BTreeMap<String, BTreeMap<DocKey, u32>>,
    docs: BTreeMap<DocKey, u32>,
}

impl MemoryIndex {
    /// Index the current version of every chunk the reader holds, archived ones included (visibility is the
    /// reader's call at query time).
    #[must_use]
    pub fn from_reader(reader: &dyn Reader) -> Self {
        let mut index = Self::default();
        for chunk_ref in reader.list(&Filter::new().include_archived()) {
            if let Ok(chunk) = reader.get_version(&chunk_ref.id, chunk_ref.version) {
                index.index(&chunk_ref, chunk.content());
            }
        }
        index
    }

    fn drop_doc(&mut self, key: &DocKey) {
        if self.docs.remove(key).is_some() {
            self.terms.retain(|_, docs| {
                docs.remove(key);
                !docs.is_empty()
            });
        }
    }
}

impl TextIndex for MemoryIndex {
    fn index(&mut self, chunk: &ChunkRef, text: &str) {
        let key: DocKey = (chunk.id.clone(), chunk.version);
        self.drop_doc(&key);
        let counts = term_counts(text);
        let len: u32 = counts.values().fold(0u32, |a, n| a.saturating_add(*n));
        for (term, tf) in counts {
            self.terms.entry(term).or_default().insert(key.clone(), tf);
        }
        self.docs.insert(key, len);
    }

    fn remove(&mut self, id: &ChunkId) {
        let keys: Vec<DocKey> = self.docs.keys().filter(|(i, _)| i == id).cloned().collect();
        for key in keys {
            self.drop_doc(&key);
        }
    }

    fn postings(&self, term: &str) -> Vec<Posting> {
        self.terms.get(term).map_or_else(Vec::new, |docs| {
            docs.iter()
                .map(|((id, version), tf)| {
                    Posting::new(
                        ChunkRef {
                            id: id.clone(),
                            version: *version,
                        },
                        *tf,
                    )
                })
                .collect()
        })
    }

    fn doc_len(&self, chunk: &ChunkRef) -> Option<u32> {
        self.docs.get(&(chunk.id.clone(), chunk.version)).copied()
    }

    fn stats(&self) -> Stats {
        Stats {
            docs: self.docs.len() as u64,
            total_len: self.docs.values().map(|n| u64::from(*n)).sum(),
        }
    }
}

const LN2_MICRO: i128 = 693_147;
const FRACTION_BITS: u32 = 24;

/// The natural logarithm of `num / den` in millionths, from integer arithmetic only (error under 3 millionths).
/// `den` must be non-zero and both must be below 2^63; out-of-range input returns 0.
#[must_use]
pub fn ln_ratio_micro(num: u64, den: u64) -> i64 {
    const LIMIT: u64 = 1 << 63;
    if num == 0 || den == 0 || num >= LIMIT || den >= LIMIT {
        return 0;
    }
    // value = num / den in Q64.
    let value: u128 = (u128::from(num) << 64) / u128::from(den);
    if value == 0 {
        return 0;
    }
    let msb = 127 - value.leading_zeros();
    let int_part = i128::from(msb) - 64;
    // Mantissa in [1, 2) as Q62.
    let mut m: u128 = if msb >= 62 {
        value >> (msb - 62)
    } else {
        value << (62 - msb)
    };
    // Fractional bits of log2 by repeated squaring.
    let mut frac: i128 = 0;
    for _ in 0..FRACTION_BITS {
        m = (m * m) >> 62;
        frac <<= 1;
        if m >= (1u128 << 63) {
            m >>= 1;
            frac |= 1;
        }
    }
    let log2_q = (int_part << FRACTION_BITS) + frac;
    i64::try_from((log2_q * LN2_MICRO) >> FRACTION_BITS).unwrap_or(0)
}

/// BM25 with `k1 = 1.2` and `b = 0.75`, in thousandths, from integers only.
///
/// `idf = ln((N + 1) / (n + 0.5))` and the term weight is `22 * tf * T / (10 * tf * T + 3 * T + 9 * dl * N)` where `T`
/// is the corpus token total, `N` the document count, `n` the documents holding the term and `dl` this document's
/// length (that is `tf * (k1 + 1) / (tf + k1 * (1 - b + b * dl / avgdl))` with the fractions cleared).
fn bm25_milli(tf: u32, dl: u32, docs_with_term: u64, stats: Stats) -> i64 {
    if stats.docs == 0 || stats.total_len == 0 || tf == 0 {
        return 0;
    }
    let idf_micro = i128::from(ln_ratio_micro(2 * (stats.docs + 1), 2 * docs_with_term + 1));
    let tf = i128::from(tf);
    let t = i128::from(stats.total_len);
    let n = i128::from(stats.docs);
    let dl = i128::from(dl);
    let weight_num = 22 * tf * t;
    let weight_den = 10 * tf * t + 3 * t + 9 * dl * n;
    // idf in millionths, weight a ratio; result in thousandths.
    i64::try_from(idf_micro * weight_num / weight_den / 1000).unwrap_or(i64::MAX)
}

/// A [`Retriever`] that scores chunks by BM25 over a [`TextIndex`].
///
/// The index says what matches; the [`Reader`] says what is visible (the query's filter, archived and superseded
/// versions, forgotten chunks), so a stale posting can never surface a chunk the store no longer serves.
#[derive(Debug, Clone)]
pub struct LexicalRetriever<I> {
    index: I,
}

impl<I: TextIndex> LexicalRetriever<I> {
    /// A retriever over `index`.
    #[must_use]
    pub fn new(index: I) -> Self {
        Self { index }
    }
    /// The index, for incremental updates.
    pub fn index_mut(&mut self) -> &mut I {
        &mut self.index
    }
}

impl<I: TextIndex> Retriever for LexicalRetriever<I> {
    fn retrieve(&self, query: &Query, store: &dyn Reader) -> Result<Vec<Hit>, Error> {
        let terms: BTreeSet<String> = tokenize(query.text()).into_iter().collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let stats = self.index.stats();
        let visible: BTreeSet<(ChunkId, u32)> = store
            .list(query.filter_ref())
            .into_iter()
            .map(|r| (r.id, r.version))
            .collect();
        let mut scored: BTreeMap<(ChunkId, u32), (i64, Vec<String>)> = BTreeMap::new();
        for term in &terms {
            let postings = self.index.postings(term);
            let n = postings.len() as u64;
            for p in postings {
                let key = (p.chunk().id.clone(), p.chunk().version);
                if !visible.contains(&key) {
                    continue;
                }
                let dl = self.index.doc_len(p.chunk()).unwrap_or(0);
                let entry = scored.entry(key).or_insert((0, Vec::new()));
                entry.0 = entry.0.saturating_add(bm25_milli(p.tf(), dl, n, stats));
                entry.1.push(term.clone());
            }
        }
        let mut hits: Vec<Hit> = scored
            .into_iter()
            .map(|((id, version), (score, matched))| {
                Hit::new(
                    ChunkRef { id, version },
                    Some(score),
                    Reason::Lexical { terms: matched },
                )
            })
            .collect();
        order_hits(&mut hits);
        hits.truncate(query.limit_value());
        Ok(hits)
    }
}
