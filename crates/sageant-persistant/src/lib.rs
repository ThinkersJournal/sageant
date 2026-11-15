// SPDX-License-Identifier: MIT OR Apache-2.0

//! A durable `sageant` store on a PersistAnt key-value backend (CONTRACT D17).
//!
//! Every write is one atomic `batch` carrying a `Require` on the state it expects, so two writers can never both
//! win; a lost race is `Error::Conflict` with what was found. Listing uses `ordered_scan_values` (ascending, one
//! consistent snapshot), never `prefix_list`, so results are deterministic (CONTRACT D16). A backend that cannot give
//! atomic batches is refused when the store is opened, by PersistAnt, not worked around here.
//!
//! Stored so far: chunks and their versions, typed links and the confidence derived from them. Deletion, the audit
//! log and the `Reader`/`Writer` trait impls follow.
//!
//! Keys: `c/<id>/<version, 10 digits>` holds one version's record (the core's canonical encoding, verified on every
//! read); `h/<id>` holds the current version number; `l/<from>/<kind>/<to>` holds a link, and `r/<to>/<kind>/<from>`
//! the same link for reverse lookup (both written in the same batch).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::time::Duration;

use persistant::blocking::registry::{KvHandle, open_named};
use persistant::capability::CapabilityId;
use persistant::capability::choose::Durability;
use persistant::capability::provider::ProviderNeeds;
use persistant::kv::{BatchOp, KvError};
use persistant::registry::Endpoint;
use sageant::{
    Chunk, ChunkId, ChunkRef, Confidence, Error, Filter, Link, LinkKind, NewChunk, Origin,
    Timestamp, closes_cycle, derive_confidence,
};

const SCAN_PAGE: usize = 256;
const NO_TTL: Option<Duration> = None;

fn backend(e: &impl std::fmt::Display) -> Error {
    Error::Backend(e.to_string())
}

fn kv_error(e: KvError) -> Error {
    backend(&e)
}

fn chunk_key(id: &ChunkId, version: u32) -> String {
    format!("c/{}/{version:010}", id.as_str())
}

fn head_key(id: &ChunkId) -> String {
    format!("h/{}", id.as_str())
}

fn link_key(from: &ChunkId, kind: LinkKind, to: &ChunkId) -> String {
    format!("l/{}/{}/{}", from.as_str(), kind.name(), to.as_str())
}

fn reverse_key(from: &ChunkId, kind: LinkKind, to: &ChunkId) -> String {
    format!("r/{}/{}/{}", to.as_str(), kind.name(), from.as_str())
}

/// A sageant store kept in a PersistAnt key-value backend.
pub struct PersistStore {
    kv: KvHandle,
}

impl std::fmt::Debug for PersistStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistStore")
            .field("backend", &self.kv.id())
            .finish()
    }
}

impl PersistStore {
    /// Open the PersistAnt backend named `backend_id` (for example `"sqlite"` with `Endpoint::Path(file)`).
    ///
    /// The store needs `kv.get`, `kv.put`, `kv.delete`, `kv.ordered_scan_values` and the four `kv.batch_atomic.*` leaves (rollback, concurrent read, optimistic
    /// sequence, absent race), each with a recorded passing test in PersistAnt's matrix, and data
    /// that survives the process exiting.
    ///
    /// # Errors
    /// `Backend` naming what is missing when the backend cannot give those, or cannot be opened.
    pub fn open(backend_id: &str, endpoint: &Endpoint) -> Result<Self, Error> {
        let ids: Result<Vec<CapabilityId>, _> = [
            "kv.get",
            "kv.put",
            "kv.delete",
            "kv.ordered_scan_values",
            "kv.batch_atomic.rollback",
            "kv.batch_atomic.concurrent_read",
            "kv.batch_atomic.optimistic_sequence",
            "kv.batch_atomic.absent_race",
        ]
        .into_iter()
        .map(CapabilityId::new)
        .collect();
        let ids = ids.map_err(|e| backend(&e))?;
        let needs = ProviderNeeds::new(ids, Durability::Process);
        let kv = open_named(backend_id, endpoint, &needs).map_err(|e| backend(&e))?;
        Ok(Self { kv })
    }

    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        self.kv.get(key).map_err(kv_error)
    }

    /// Apply `ops` atomically; `Ok(false)` means a `Require` no longer held and nothing was written.
    fn batch(&self, ops: &[BatchOp]) -> Result<bool, Error> {
        self.kv.batch(ops).map_err(kv_error)
    }

    fn head(&self, id: &ChunkId) -> Result<Option<u32>, Error> {
        match self.read(&head_key(id))? {
            None => Ok(None),
            Some(bytes) => std::str::from_utf8(&bytes)
                .ok()
                .and_then(|t| t.parse().ok())
                .map(Some)
                .ok_or(Error::Corrupt("head")),
        }
    }

    fn load(&self, id: &ChunkId, version: u32) -> Result<Chunk, Error> {
        let bytes = self.read(&chunk_key(id, version))?.ok_or(Error::NotFound)?;
        let chunk = Chunk::decode(&bytes)?;
        if chunk.id() != id || chunk.version() != version {
            return Err(Error::Corrupt("key does not match record"));
        }
        Ok(chunk)
    }

    /// Store a new chunk as version 1 of a new lineage.
    ///
    /// # Errors
    /// `Invalid`, `NotNormalised`, `Conflict` when the same text already exists in the collection, or `Backend`.
    pub fn put(&self, new: NewChunk, at: Timestamp) -> Result<ChunkRef, Error> {
        let chunk = Chunk::first_version(new, at)?;
        let id = chunk.id().clone();
        let applied = self.batch(&[
            BatchOp::Require {
                key: head_key(&id),
                expected: None,
            },
            BatchOp::Put {
                key: chunk_key(&id, 1),
                value: chunk.encode(),
                ttl: NO_TTL,
            },
            BatchOp::Put {
                key: head_key(&id),
                value: b"1".to_vec(),
                ttl: NO_TTL,
            },
        ])?;
        if applied {
            Ok(ChunkRef { id, version: 1 })
        } else {
            Err(Error::Conflict {
                expected: 0,
                found: self.head(&id)?.unwrap_or(0),
            })
        }
    }

    /// Append a new version of an existing lineage; the old version stays readable.
    ///
    /// # Errors
    /// `NotFound`, `NotNormalised`, `Invalid`, `Conflict` when the text is unchanged or another writer got there
    /// first, or `Backend`.
    pub fn supersede(
        &self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        let current = self.head(id)?.ok_or(Error::NotFound)?;
        self.supersede_expecting(id, current, content, origin, source, at)
    }

    /// As [`supersede`](Self::supersede), but only if `expected` is still the current version: optimistic
    /// concurrency for a caller that read the chunk earlier.
    ///
    /// # Errors
    /// As `supersede`; `Conflict { expected, found }` when the current version is not `expected`.
    pub fn supersede_expecting(
        &self,
        id: &ChunkId,
        expected: u32,
        content: &str,
        origin: Origin,
        source: &str,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        let found = self.head(id)?.ok_or(Error::NotFound)?;
        if found != expected {
            return Err(Error::Conflict { expected, found });
        }
        let next = self
            .load(id, expected)?
            .next_version(content, origin, source, at)?;
        let version = next.version();
        let applied = self.batch(&[
            BatchOp::Require {
                key: head_key(id),
                expected: Some(expected.to_string().into_bytes()),
            },
            BatchOp::Put {
                key: chunk_key(id, version),
                value: next.encode(),
                ttl: NO_TTL,
            },
            BatchOp::Put {
                key: head_key(id),
                value: version.to_string().into_bytes(),
                ttl: NO_TTL,
            },
        ])?;
        if applied {
            Ok(ChunkRef {
                id: id.clone(),
                version,
            })
        } else {
            Err(Error::Conflict {
                expected,
                found: self.head(id)?.unwrap_or(0),
            })
        }
    }

    /// The current (latest) version.
    ///
    /// # Errors
    /// `NotFound`, `Corrupt` or `Backend`.
    pub fn get(&self, id: &ChunkId) -> Result<Chunk, Error> {
        let head = self.head(id)?.ok_or(Error::NotFound)?;
        self.load(id, head)
    }

    /// A specific version, from 1.
    ///
    /// # Errors
    /// `NotFound` for an unknown id or version, `Corrupt` or `Backend`.
    pub fn get_version(&self, id: &ChunkId, version: u32) -> Result<Chunk, Error> {
        if version == 0 {
            return Err(Error::NotFound);
        }
        self.load(id, version)
    }

    /// Every version, oldest first.
    ///
    /// # Errors
    /// `NotFound`, `Corrupt` or `Backend`.
    pub fn history(&self, id: &ChunkId) -> Result<Vec<Chunk>, Error> {
        let versions = self.scan(&format!("c/{}/", id.as_str()))?;
        if versions.is_empty() {
            return Err(Error::NotFound);
        }
        versions
            .into_iter()
            .enumerate()
            .map(|(i, bytes)| {
                let chunk = Chunk::decode(&bytes)?;
                let expected_version = u32::try_from(i + 1).unwrap_or(u32::MAX);
                if chunk.id() != id || chunk.version() != expected_version {
                    return Err(Error::Corrupt("key does not match record"));
                }
                Ok(chunk)
            })
            .collect()
    }

    /// Every value under `prefix`, ascending by key, across pages.
    fn scan(&self, prefix: &str) -> Result<Vec<Vec<u8>>, Error> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self
                .kv
                .ordered_scan_values(prefix, after.as_deref(), SCAN_PAGE)
                .map_err(kv_error)?;
            out.extend(page.entries.into_iter().map(|(_, value)| value));
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(out),
            }
        }
    }

    /// The chunks matching `filter`, ordered by chunk id ascending then version descending, exactly as
    /// `sageant::Reader::list` orders them.
    ///
    /// # Errors
    /// `Corrupt` or `Backend`.
    pub fn list(&self, filter: &Filter) -> Result<Vec<ChunkRef>, Error> {
        let wants_confidence = filter.confidence_filter().is_some();
        let mut all: Vec<Chunk> = Vec::new();
        for bytes in self.scan("c/")? {
            all.push(Chunk::decode(&bytes)?);
        }
        // Keys ascend by (id, version); regroup so each lineage is newest first.
        let mut out = Vec::new();
        let mut i = 0;
        while i < all.len() {
            let mut j = i;
            while j < all.len() && all[j].id() == all[i].id() {
                j += 1;
            }
            for (n, chunk) in all[i..j].iter().rev().enumerate() {
                if n > 0 && !filter.includes_superseded() {
                    break;
                }
                let confidence = if wants_confidence {
                    self.confidence(chunk.id())?
                } else {
                    Confidence::Unconfirmed
                };
                if filter.matches(chunk, confidence) {
                    out.push(ChunkRef {
                        id: chunk.id().clone(),
                        version: chunk.version(),
                    });
                }
            }
            i = j;
        }
        if let Some(n) = filter.limit_value() {
            out.truncate(n);
        }
        Ok(out)
    }

    /// Link `from` to `to`, attached to `from`'s current version.
    ///
    /// # Errors
    /// `NotFound` if either lineage is unknown; `LinkCycle` for a self-link or a cycle in `part-of` or
    /// `derived-from`; `DuplicateLink` for the same (source, kind, target), including when another writer added it
    /// first; `Conflict` if `from` got a new version while the link was being made; `Backend`. A refused link
    /// changes nothing. The cycle check reads the existing links and is not atomic with the write: two writers
    /// adding opposite `part-of` links at the same moment could both pass it.
    pub fn link(
        &self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        at: Timestamp,
    ) -> Result<(), Error> {
        let from_version = self.get(from)?.version();
        self.get(to)?;
        if from == to {
            return Err(Error::LinkCycle);
        }
        let forward = link_key(from, kind, to);
        if self.read(&forward)?.is_some() {
            return Err(Error::DuplicateLink);
        }
        if matches!(kind, LinkKind::PartOf | LinkKind::DerivedFrom) {
            let mut next = |node: &ChunkId| -> Result<Vec<ChunkId>, Error> {
                Ok(self
                    .scan_links(&format!("l/{}/{}/", node.as_str(), kind.name()))?
                    .into_iter()
                    .map(|l| l.to().clone())
                    .collect())
            };
            if closes_cycle(to, from, &mut next)? {
                return Err(Error::LinkCycle);
            }
        }
        let link = Link::new(from.clone(), from_version, kind, to.clone(), at);
        let applied = self.batch(&[
            BatchOp::Require {
                key: forward.clone(),
                expected: None,
            },
            BatchOp::Require {
                key: head_key(from),
                expected: Some(from_version.to_string().into_bytes()),
            },
            BatchOp::Put {
                key: forward,
                value: link.encode(),
                ttl: NO_TTL,
            },
            BatchOp::Put {
                key: reverse_key(from, kind, to),
                value: link.encode(),
                ttl: NO_TTL,
            },
        ])?;
        if applied {
            return Ok(());
        }
        if self.read(&link_key(from, kind, to))?.is_some() {
            return Err(Error::DuplicateLink);
        }
        Err(Error::Conflict {
            expected: from_version,
            found: self.head(from)?.unwrap_or(0),
        })
    }

    fn scan_links(&self, prefix: &str) -> Result<Vec<Link>, Error> {
        self.scan(prefix)?.iter().map(|b| Link::decode(b)).collect()
    }

    /// Links made from `id`, ordered by (kind, target id).
    ///
    /// # Errors
    /// `NotFound`, `Corrupt` or `Backend`.
    pub fn links_from(&self, id: &ChunkId) -> Result<Vec<Link>, Error> {
        self.get(id)?;
        let mut links = self.scan_links(&format!("l/{}/", id.as_str()))?;
        if links.iter().any(|l| l.from() != id) {
            return Err(Error::Corrupt("key does not match record"));
        }
        links.sort_by(|a, b| (a.kind(), a.to()).cmp(&(b.kind(), b.to())));
        Ok(links)
    }

    /// Links pointing at `id`, ordered by (kind, source id).
    ///
    /// # Errors
    /// `NotFound`, `Corrupt` or `Backend`.
    pub fn links_to(&self, id: &ChunkId) -> Result<Vec<Link>, Error> {
        self.get(id)?;
        let mut links = self.scan_links(&format!("r/{}/", id.as_str()))?;
        if links.iter().any(|l| l.to() != id) {
            return Err(Error::Corrupt("key does not match record"));
        }
        links.sort_by(|a, b| (a.kind(), a.from()).cmp(&(b.kind(), b.from())));
        Ok(links)
    }

    /// The current confidence of `id`, derived from its links by the core's rule (CONTRACT D6).
    ///
    /// # Errors
    /// `NotFound`, `Corrupt` or `Backend`.
    pub fn confidence(&self, id: &ChunkId) -> Result<Confidence, Error> {
        let me = self.get(id)?;
        let mut targets: Vec<(LinkKind, Option<Chunk>)> = Vec::new();
        for link in self.links_from(id)? {
            let target = match self.get(link.to()) {
                Ok(chunk) => Some(chunk),
                Err(Error::NotFound) => None, // removed evidence counts for nothing
                Err(e) => return Err(e),
            };
            targets.push((link.kind(), target));
        }
        let evidence: Vec<(LinkKind, Option<&Chunk>)> =
            targets.iter().map(|(k, c)| (*k, c.as_ref())).collect();
        Ok(derive_confidence(&me, &evidence))
    }

    /// Overwrite a stored version with raw bytes, to test that corrupt records are refused. Not for production use.
    #[doc(hidden)]
    pub fn corrupt_for_test(&self, id: &ChunkId, version: u32, bytes: Vec<u8>) {
        let _ = self.kv.put(&chunk_key(id, version), bytes, None);
    }
}

impl sageant::Reader for PersistStore {
    fn get(&self, id: &ChunkId) -> Result<Chunk, Error> {
        Self::get(self, id)
    }
    fn get_version(&self, id: &ChunkId, version: u32) -> Result<Chunk, Error> {
        Self::get_version(self, id, version)
    }
    fn history(&self, id: &ChunkId) -> Result<Vec<Chunk>, Error> {
        Self::history(self, id)
    }
    fn links_from(&self, id: &ChunkId) -> Result<Vec<Link>, Error> {
        Self::links_from(self, id)
    }
    fn links_to(&self, id: &ChunkId) -> Result<Vec<Link>, Error> {
        Self::links_to(self, id)
    }
    fn confidence(&self, id: &ChunkId) -> Result<Confidence, Error> {
        Self::confidence(self, id)
    }
    fn list(&self, filter: &Filter) -> Result<Vec<ChunkRef>, Error> {
        Self::list(self, filter)
    }
}
