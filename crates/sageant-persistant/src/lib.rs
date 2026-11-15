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

mod index;

pub use index::KvTextIndex;

use std::collections::BTreeMap;
use std::time::Duration;

use persistant::blocking::registry::{KvHandle, open_named};
use persistant::capability::CapabilityId;
use persistant::capability::choose::Durability;
use persistant::capability::provider::ProviderNeeds;
use persistant::kv::{BatchOp, KvError};
use persistant::registry::Endpoint;
use sageant::{
    Action, Actor, AuditEvent, Chunk, ChunkId, ChunkRef, Confidence, Error, Filter, Link, LinkKind,
    LinkTarget, NewChunk, Origin, PersonAuthority, Removed, SecretOverride, SecretScreen, Status,
    Timestamp, Tombstone, Writer, closes_cycle, derive_confidence,
};

pub(crate) const SCAN_PAGE: usize = 256;
pub(crate) const NO_TTL: Option<Duration> = None;

pub(crate) fn backend(e: &impl std::fmt::Display) -> Error {
    Error::Backend(e.to_string())
}

pub(crate) fn kv_error(e: KvError) -> Error {
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

fn tombstone_key(id: &ChunkId) -> String {
    format!("t/{}", id.as_str())
}

fn review_key(id: &ChunkId) -> String {
    format!("v/{}", id.as_str())
}

fn audit_key(seq: u64) -> String {
    format!("a/{seq:020}")
}

/// The next audit sequence number is held here; the entries themselves are `a/<seq>`.
const AUDIT_COUNTER: &str = "m/aseq";
/// Tries at a write whose only failed precondition was another writer's audit entry.
const AUDIT_RETRIES: usize = 16;

fn reverse_key(from: &ChunkId, kind: LinkKind, to: &ChunkId) -> String {
    format!("r/{}/{}/{}", to.as_str(), kind.name(), from.as_str())
}

/// Open the PersistAnt backend with every capability the stores here rely on.
pub(crate) fn open_kv(backend_id: &str, endpoint: &Endpoint) -> Result<KvHandle, Error> {
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
    Ok(kv)
}

/// A sageant store kept in a PersistAnt key-value backend.
pub struct PersistStore {
    kv: KvHandle,
    screen: Option<Box<dyn SecretScreen + Send + Sync>>,
    caps: BTreeMap<String, usize>,
    /// Set while a `Writer` call runs: names the actor on every audit entry that call appends.
    acting: Option<String>,
}

impl std::fmt::Debug for PersistStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PersistStore")
            .field("backend", &self.kv.id())
            .field("screened", &self.screen.is_some())
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
        let kv = open_kv(backend_id, endpoint)?;
        Ok(Self {
            kv,
            screen: None,
            caps: BTreeMap::new(),
            acting: None,
        })
    }

    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        self.kv.get(key).map_err(kv_error)
    }

    /// Apply `ops` atomically; `Ok(false)` means a `Require` no longer held and nothing was written.
    fn batch(&self, ops: &[BatchOp]) -> Result<bool, Error> {
        self.kv.batch(ops).map_err(kv_error)
    }

    fn audit_counter(&self) -> Result<u64, Error> {
        match self.read(AUDIT_COUNTER)? {
            None => Ok(0),
            Some(bytes) => std::str::from_utf8(&bytes)
                .ok()
                .and_then(|t| t.parse().ok())
                .ok_or(Error::Corrupt("audit counter")),
        }
    }

    /// Apply `ops` and append `events` to the audit log in one atomic batch. `Ok(false)` means one of `ops`'s own
    /// `Require`s no longer held; a lost race for the audit sequence number is retried, because nothing else
    /// changed.
    fn apply(&self, ops: &[BatchOp], events: &[AuditEvent]) -> Result<bool, Error> {
        for _ in 0..AUDIT_RETRIES {
            let n = self.audit_counter()?;
            let mut all = ops.to_vec();
            all.push(BatchOp::Require {
                key: AUDIT_COUNTER.to_owned(),
                expected: (n > 0).then(|| n.to_string().into_bytes()),
            });
            let mut next = n;
            for event in events {
                next += 1;
                all.push(BatchOp::Put {
                    key: audit_key(next),
                    value: event.encode(),
                    ttl: NO_TTL,
                });
            }
            all.push(BatchOp::Put {
                key: AUDIT_COUNTER.to_owned(),
                value: next.to_string().into_bytes(),
                ttl: NO_TTL,
            });
            if self.batch(&all)? {
                return Ok(true);
            }
            if self.audit_counter()? == n {
                return Ok(false);
            }
        }
        Err(Error::Backend("audit log contention".to_owned()))
    }

    /// Why `id` has no head: forgotten (a tombstone) or never stored.
    fn missing(&self, id: &ChunkId) -> Error {
        match self.read(&tombstone_key(id)) {
            Ok(Some(_)) => Error::Forgotten,
            Ok(None) => Error::NotFound,
            Err(e) => e,
        }
    }

    /// The actor an audit entry names: the `Writer` caller if one is acting, else `default`.
    fn who(&self, default: &str) -> String {
        self.acting.clone().unwrap_or_else(|| default.to_owned())
    }

    /// Set the secret screen run on the content and source of every write. Applies to later writes.
    pub fn set_screen(&mut self, screen: Box<dyn SecretScreen + Send + Sync>) {
        self.screen = Some(screen);
    }

    /// Cap the content size of `collection` in bytes. Applies to later writes.
    pub fn set_byte_cap(&mut self, collection: &str, bytes: usize) {
        self.caps.insert(collection.to_owned(), bytes);
    }

    fn check_cap(&self, collection: &str, content: &str) -> Result<(), Error> {
        match self.caps.get(collection) {
            Some(&cap) if content.len() > cap => Err(Error::TooLarge {
                measured: content.len(),
                cap,
            }),
            _ => Ok(()),
        }
    }

    /// Screen `texts`. `Ok(Some(class))` means the screen flagged `class` and `allow` covers it: the caller records
    /// the override once the write has happened. A refusal is audited (the class only) before it is returned.
    fn run_screen(
        &self,
        texts: &[&str],
        allow: Option<&SecretOverride>,
        at: Timestamp,
    ) -> Result<Option<&'static str>, Error> {
        if allow.is_some_and(|a| a.by().is_empty()) {
            return Err(Error::Invalid("override"));
        }
        let hit = self
            .screen
            .as_ref()
            .and_then(|screen| texts.iter().find_map(|t| screen.screen(t)));
        match hit {
            None => Ok(None),
            Some(class) if allow.is_some_and(|a| a.class() == class) => Ok(Some(class)),
            Some(class) => {
                let event = AuditEvent::new(
                    at,
                    Action::Refused,
                    None,
                    &self.who(""),
                    &format!("secret:{class}"),
                );
                self.apply(&[], &[event])?;
                Err(Error::SecretRefused { class })
            }
        }
    }

    /// The audit entry for an allowed secret, if the screen flagged one.
    fn override_event(
        &self,
        class: Option<&'static str>,
        allow: Option<&SecretOverride>,
        id: &ChunkId,
        at: Timestamp,
    ) -> Option<AuditEvent> {
        let (class, allow) = class.zip(allow)?;
        Some(AuditEvent::new(
            at,
            Action::Overridden,
            Some(id.clone()),
            &self.who(allow.by()),
            &format!("secret-override:{class}"),
        ))
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
        self.put_inner(new, None, at)
    }

    /// [`put`](Self::put), allowing text the screen flags as the class `allow` names. Audited.
    ///
    /// # Errors
    /// As `put`; `Invalid("override")` if `allow` names nobody; `SecretRefused` if the flagged class differs.
    pub fn put_overriding(
        &self,
        new: NewChunk,
        allow: &SecretOverride,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        self.put_inner(new, Some(allow), at)
    }

    fn put_inner(
        &self,
        new: NewChunk,
        allow: Option<&SecretOverride>,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        let chunk = Chunk::first_version(new, at)?;
        self.check_cap(chunk.collection(), chunk.content())?;
        let overridden = self.run_screen(&[chunk.content(), chunk.source()], allow, at)?;
        let id = chunk.id().clone();
        // The audit log names the origin class, never the free-text source (which may hold a URL or a name).
        let event = AuditEvent::new(
            at,
            Action::Put,
            Some(id.clone()),
            &self.who(&format!("{:?}", chunk.origin())),
            "",
        );
        let events: Vec<AuditEvent> = std::iter::once(event)
            .chain(self.override_event(overridden, allow, &id, at))
            .collect();
        let applied = self.apply(
            &[
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
                // Storing the text again after a forget ends the tombstone.
                BatchOp::Delete {
                    key: tombstone_key(&id),
                },
            ],
            &events,
        )?;
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
        let current = self.head(id)?.ok_or_else(|| self.missing(id))?;
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
        self.supersede_inner(id, expected, content, origin, source, None, at)
    }

    /// [`supersede`](Self::supersede), allowing text the screen flags as the class `allow` names. Audited.
    ///
    /// # Errors
    /// As `supersede`; `Invalid("override")` if `allow` names nobody; `SecretRefused` if the class differs.
    pub fn supersede_overriding(
        &self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        allow: &SecretOverride,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        let current = self.head(id)?.ok_or_else(|| self.missing(id))?;
        self.supersede_inner(id, current, content, origin, source, Some(allow), at)
    }

    #[allow(clippy::too_many_arguments)]
    fn supersede_inner(
        &self,
        id: &ChunkId,
        expected: u32,
        content: &str,
        origin: Origin,
        source: &str,
        allow: Option<&SecretOverride>,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        let found = self.head(id)?.ok_or_else(|| self.missing(id))?;
        if found != expected {
            return Err(Error::Conflict { expected, found });
        }
        let current = self.load(id, expected)?;
        self.check_cap(current.collection(), content)?;
        let overridden = self.run_screen(&[content, source], allow, at)?;
        let next = current.next_version(content, origin, source, at)?;
        let version = next.version();
        let event = AuditEvent::new(
            at,
            Action::Supersede,
            Some(id.clone()),
            &self.who(&format!("{origin:?}")),
            "",
        );
        let events: Vec<AuditEvent> = std::iter::once(event)
            .chain(self.override_event(overridden, allow, id, at))
            .collect();
        let applied = self.apply(
            &[
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
            ],
            &events,
        )?;
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
        let head = self.head(id)?.ok_or_else(|| self.missing(id))?;
        self.load(id, head)
    }

    /// A specific version, from 1.
    ///
    /// # Errors
    /// `NotFound` for an unknown id or version, `Corrupt` or `Backend`.
    pub fn get_version(&self, id: &ChunkId, version: u32) -> Result<Chunk, Error> {
        if self.head(id)?.is_none() {
            return Err(self.missing(id));
        }
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
            return Err(self.missing(id));
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
        let event = AuditEvent::new(
            at,
            Action::Link,
            Some(from.clone()),
            &self.who(""),
            &format!("{kind:?}"),
        );
        let applied = self.apply(
            &[
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
            ],
            &[event],
        )?;
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
                Err(Error::NotFound | Error::Forgotten) => None, // removed evidence counts for nothing
                Err(e) => return Err(e),
            };
            targets.push((link.kind(), target));
        }
        let evidence: Vec<(LinkKind, Option<&Chunk>)> =
            targets.iter().map(|(k, c)| (*k, c.as_ref())).collect();
        Ok(derive_confidence(&me, &evidence))
    }

    /// Make an archived chunk active again (every version).
    ///
    /// # Errors
    /// `NotFound`, `Forgotten`, `BadState` if already active, `Conflict` if another writer changed the chunk
    /// meanwhile, `Corrupt` or `Backend`.
    pub fn unarchive(
        &self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.set_status(id, false, actor, reason, at)
    }

    /// Mark a chunk archived: kept, out of retrieval. New versions stay archived.
    ///
    /// # Errors
    /// `NotFound`, `Forgotten`, `BadState` if already archived, `Conflict` if another writer changed the chunk
    /// meanwhile, `Corrupt` or `Backend`.
    pub fn archive(
        &self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.set_status(id, true, actor, reason, at)
    }

    fn set_status(
        &self,
        id: &ChunkId,
        archive: bool,
        actor: &str,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        let (status, action) = if archive {
            (Status::Archived, Action::Archive)
        } else {
            (Status::Active, Action::Unarchive)
        };
        let head = self.head(id)?.ok_or_else(|| self.missing(id))?;
        let history = self.history(id)?;
        if history.last().is_some_and(|c| c.status() == status) {
            return Err(Error::BadState(if archive {
                "already archived"
            } else {
                "already active"
            }));
        }
        let mut ops = vec![BatchOp::Require {
            key: head_key(id),
            expected: Some(head.to_string().into_bytes()),
        }];
        for chunk in &history {
            ops.push(BatchOp::Put {
                key: chunk_key(id, chunk.version()),
                value: chunk.with_status(status).encode(),
                ttl: NO_TTL,
            });
        }
        let event = AuditEvent::new(at, action, Some(id.clone()), &self.who(actor), reason);
        if self.apply(&ops, &[event])? {
            Ok(())
        } else {
            Err(Error::Conflict {
                expected: head,
                found: self.head(id)?.unwrap_or(0),
            })
        }
    }

    /// Record a note against a chunk in the audit log (a reason for a link or a supersede, say).
    ///
    /// # Errors
    /// `NotFound`, `Forgotten`, `Corrupt` or `Backend`.
    pub fn note(
        &self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.head(id)?.ok_or_else(|| self.missing(id))?;
        let event = AuditEvent::new(at, Action::Note, Some(id.clone()), &self.who(actor), reason);
        self.apply(&[], &[event])?;
        Ok(())
    }

    /// The operations that remove a lineage: its versions and head, its outgoing links (and their reverse
    /// entries), and a review flag on every chunk derived from it. Links pointing at it stay and read as removed.
    fn removal_ops(&self, id: &ChunkId) -> Result<Vec<BatchOp>, Error> {
        let mut ops = Vec::new();
        for chunk in self.history(id)? {
            ops.push(BatchOp::Delete {
                key: chunk_key(id, chunk.version()),
            });
        }
        ops.push(BatchOp::Delete { key: head_key(id) });
        for link in self.scan_links(&format!("l/{}/", id.as_str()))? {
            ops.push(BatchOp::Delete {
                key: link_key(link.from(), link.kind(), link.to()),
            });
            ops.push(BatchOp::Delete {
                key: reverse_key(link.from(), link.kind(), link.to()),
            });
        }
        for link in self.scan_links(&format!("r/{}/", id.as_str()))? {
            if link.kind() == LinkKind::DerivedFrom {
                ops.push(BatchOp::Put {
                    key: review_key(link.from()),
                    value: b"1".to_vec(),
                    ttl: NO_TTL,
                });
            }
        }
        Ok(ops)
    }

    /// Remove a chunk's content (every version), its source, its hash and its outgoing links, leaving a minimal
    /// tombstone. Links pointing at it stay and read as removed; chunks derived from it are flagged for review.
    /// "Unretrievable", not "erased": a backend may keep old pages until it is compacted.
    ///
    /// # Errors
    /// `NotFound`, `Forgotten` if it is already forgotten, `Conflict` if another writer changed the chunk
    /// meanwhile, `Corrupt` or `Backend`.
    pub fn forget(
        &self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        keyed_hash: Option<&str>,
        at: Timestamp,
    ) -> Result<Removed, Error> {
        let head = self.head(id)?.ok_or_else(|| self.missing(id))?;
        let history = self.history(id)?;
        let tombstone = Tombstone::new(
            id.clone(),
            history.first().map_or("", Chunk::collection),
            u32::try_from(history.len()).unwrap_or(u32::MAX),
            at,
            actor,
            reason,
            keyed_hash,
        );
        let mut ops = vec![BatchOp::Require {
            key: head_key(id),
            expected: Some(head.to_string().into_bytes()),
        }];
        ops.extend(self.removal_ops(id)?);
        ops.push(BatchOp::Put {
            key: tombstone_key(id),
            value: tombstone.encode(),
            ttl: NO_TTL,
        });
        let event = AuditEvent::new(
            at,
            Action::Forget,
            Some(id.clone()),
            &self.who(actor),
            reason,
        );
        if self.apply(&ops, &[event])? {
            Ok(Removed::Unretrievable)
        } else {
            Err(Error::Conflict {
                expected: head,
                found: self.head(id)?.unwrap_or(0),
            })
        }
    }

    /// Remove everything about a chunk, including its tombstone. Needs a [`PersonAuthority`].
    ///
    /// # Errors
    /// `NotFound` if there is neither a chunk nor a tombstone, `Conflict` if another writer changed it
    /// meanwhile, `Corrupt` or `Backend`.
    pub fn purge(
        &self,
        id: &ChunkId,
        authority: &PersonAuthority,
        at: Timestamp,
    ) -> Result<(), Error> {
        let head = self.head(id)?;
        let tombstone = self.read(&tombstone_key(id))?;
        if head.is_none() && tombstone.is_none() {
            return Err(Error::NotFound);
        }
        let mut ops = vec![
            BatchOp::Require {
                key: head_key(id),
                expected: head.map(|h| h.to_string().into_bytes()),
            },
            BatchOp::Require {
                key: tombstone_key(id),
                expected: tombstone,
            },
            BatchOp::Delete {
                key: tombstone_key(id),
            },
        ];
        if head.is_some() {
            ops.extend(self.removal_ops(id)?);
        }
        let event = AuditEvent::new(at, Action::Purge, Some(id.clone()), authority.how(), "");
        if self.apply(&ops, &[event])? {
            Ok(())
        } else {
            Err(Error::Conflict {
                expected: head.unwrap_or(0),
                found: self.head(id)?.unwrap_or(0),
            })
        }
    }

    /// The tombstone of a forgotten lineage, if it is currently forgotten.
    ///
    /// # Errors
    /// `Corrupt` or `Backend`.
    pub fn tombstone(&self, id: &ChunkId) -> Result<Option<Tombstone>, Error> {
        let Some(bytes) = self.read(&tombstone_key(id))? else {
            return Ok(None);
        };
        let tombstone = Tombstone::decode(&bytes)?;
        if tombstone.id() != id {
            return Err(Error::Corrupt("key does not match record"));
        }
        Ok(Some(tombstone))
    }

    /// Whether a link's target is still stored.
    ///
    /// # Errors
    /// `Corrupt` or `Backend`.
    pub fn link_target(&self, link: &Link) -> Result<LinkTarget, Error> {
        Ok(if self.head(link.to())?.is_some() {
            LinkTarget::Present
        } else {
            LinkTarget::Removed
        })
    }

    /// Whether a chunk derived from something later forgotten or purged is flagged for review.
    ///
    /// # Errors
    /// `NotFound`, `Forgotten`, `Corrupt` or `Backend`.
    pub fn needs_review(&self, id: &ChunkId) -> Result<bool, Error> {
        self.head(id)?.ok_or_else(|| self.missing(id))?;
        Ok(self.read(&review_key(id))?.is_some())
    }

    /// The audit log, oldest first. It never holds content.
    ///
    /// # Errors
    /// `Corrupt` or `Backend`.
    pub fn audit(&self) -> Result<Vec<AuditEvent>, Error> {
        self.scan("a/")?
            .iter()
            .map(|b| AuditEvent::decode(b))
            .collect()
    }

    /// Every stored key and value, for tests that prove content is gone. Not for production use.
    ///
    /// # Errors
    /// `Backend`.
    #[doc(hidden)]
    pub fn dump_all_for_test(&self) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let page = self
                .kv
                .ordered_scan_values("", after.as_deref(), SCAN_PAGE)
                .map_err(kv_error)?;
            out.extend(page.entries);
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(out),
            }
        }
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

impl PersistStore {
    /// Run `f` with every audit entry it appends attributed to `actor`.
    fn acting_as<T>(
        &mut self,
        actor: &Actor,
        f: impl FnOnce(&Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if !actor.is_valid() {
            return Err(Error::Invalid("actor"));
        }
        self.acting = Some(actor.label());
        let out = f(self);
        self.acting = None;
        out
    }
}

impl Writer for PersistStore {
    fn put(&mut self, new: NewChunk, actor: &Actor, at: Timestamp) -> Result<ChunkRef, Error> {
        self.acting_as(actor, |s| Self::put(s, new, at))
    }
    fn supersede(
        &mut self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        actor: &Actor,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        self.acting_as(actor, |s| {
            Self::supersede(s, id, content, origin, source, at)
        })
    }
    fn link(
        &mut self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        actor: &Actor,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.acting_as(actor, |s| Self::link(s, from, kind, to, at))
    }
    fn set_status(
        &mut self,
        id: &ChunkId,
        status: Status,
        actor: &Actor,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.acting_as(actor, |s| match status {
            Status::Archived => Self::archive(s, id, "", reason, at),
            _ => Self::unarchive(s, id, "", reason, at),
        })
    }
    fn note(
        &mut self,
        id: &ChunkId,
        actor: &Actor,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.acting_as(actor, |s| Self::note(s, id, "", reason, at))
    }
    fn forget(
        &mut self,
        id: &ChunkId,
        actor: &Actor,
        reason: &str,
        keyed_hash: Option<&str>,
        at: Timestamp,
    ) -> Result<Removed, Error> {
        self.acting_as(actor, |s| Self::forget(s, id, "", reason, keyed_hash, at))
    }
    fn purge(
        &mut self,
        id: &ChunkId,
        authority: &PersonAuthority,
        at: Timestamp,
    ) -> Result<(), Error> {
        Self::purge(self, id, authority, at)
    }
}
