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

/// Per-chunk link generation: `n/<id>` counts the links ever added from or to the chunk. A writer that read links
/// (a cycle check, a forget) requires the counters it read to be unchanged when it commits, so a link that lands in
/// between makes it retry on fresh state instead of acting on stale reads.
fn gen_key(id: &ChunkId) -> String {
    format!("n/{}", id.as_str())
}

/// The counter after one more link; an absent counter is 0.
fn bumped(seen: Option<&Vec<u8>>) -> Result<Vec<u8>, Error> {
    let n: u64 = match seen {
        None => 0,
        Some(bytes) => std::str::from_utf8(bytes)
            .ok()
            .and_then(|t| t.parse().ok())
            .ok_or(Error::Corrupt("link generation"))?,
    };
    Ok((n + 1).to_string().into_bytes())
}

/// Tries at a write whose only failed precondition was another writer's change to a link generation.
const GEN_RETRIES: usize = 16;

/// The next audit sequence number is held here; the entries themselves are `a/<seq>`.
const AUDIT_COUNTER: &str = "m/aseq";
/// Tries at a write whose only failed precondition was another writer's audit entry.
const AUDIT_RETRIES: usize = 16;

fn reverse_key(from: &ChunkId, kind: LinkKind, to: &ChunkId) -> String {
    format!("r/{}/{}/{}", to.as_str(), kind.name(), from.as_str())
}

/// Open the PersistAnt backend with every capability the stores here rely on.
pub(crate) fn open_kv(backend_id: &str, endpoint: &Endpoint) -> Result<KvHandle, Error> {
    open_named(backend_id, endpoint, &storage_needs()).map_err(|e| backend(&e))
}

/// What these stores need from a PersistAnt backend: key-value get, put, delete and ordered value scans, the four
/// atomic-batch leaves (rollback, concurrent read, optimistic sequence, absent race) each with a recorded passing test,
/// and durability that survives the process exiting. Erase-on-forget is not asked for: PersistAnt 0.18 records no
/// secure-delete capability, so "forgotten" stays "unretrievable"; when it does, it is declared here as a preference.
#[must_use]
pub fn storage_needs() -> ProviderNeeds {
    let ids: Vec<CapabilityId> = [
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
    .filter_map(|id| CapabilityId::new(id).ok())
    .collect();
    // Preferred, never required: a backend that zeroes freed content (PersistAnt 0.18.12 `sqlite-secure`) ranks first, so
    // a forgotten chunk's bytes are gone from the file, not only unretrievable. Others still qualify.
    let prefer: Vec<CapabilityId> = ["storage.secure_delete", "kv.delete_erased"]
        .into_iter()
        .filter_map(|id| CapabilityId::new(id).ok())
        .collect();
    ProviderNeeds::new(ids, Durability::Process).prefer(prefer)
}

/// The backend PersistAnt chose for a [`storage_needs`], and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    backend: &'static str,
    why: Vec<String>,
    secure_delete: bool,
    erasure: Erasure,
}

impl Chosen {
    /// The stable backend id: store it in your configuration and pass it to `open` next time, so a later change in
    /// PersistAnt's ranking can never make an existing store be read by a different engine.
    #[must_use]
    pub fn backend(&self) -> &'static str {
        self.backend
    }

    /// Whether the backend has `storage.secure_delete`: content a delete or an overwrite frees is zeroed in the
    /// database file. Only then may a forget be described as erased from the store file (and even then not from
    /// copies a file system, snapshot or backup keeps); otherwise it is unretrievable.
    #[must_use]
    pub fn secure_delete(&self) -> bool {
        self.secure_delete
    }

    /// How this backend gets rid of a forgotten chunk's bytes.
    #[must_use]
    pub fn erasure(&self) -> Erasure {
        self.erasure
    }

    /// Why this backend, as lines for a person: the capabilities it meets, its durability and its caveats.
    #[must_use]
    pub fn why(&self) -> &[String] {
        &self.why
    }
}

fn chosen_from(listing: &persistant::registry::Listing) -> Chosen {
    Chosen {
        backend: listing.id,
        why: listing.why(),
        secure_delete: has_secure_delete(listing),
        erasure: erasure_of(listing),
    }
}

/// How a backend gets rid of the bytes of a forgotten chunk. Never covers copies a file system, snapshot or backup keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Erasure {
    /// The backend has `storage.secure_delete`: content a delete or overwrite frees is zeroed as it is freed.
    Zeroed,
    /// The backend has `kv.delete_erased`: forgetting erases each stored copy with it while the key is still live (two
    /// steps; an interrupted forget is finished the next time the store is opened).
    EraseOnDelete,
    /// Neither: a forgotten chunk is unretrievable but its freed bytes may stay in the file.
    Unretrievable,
}

fn has_capability(listing: &persistant::registry::Listing, capability: &str) -> bool {
    listing
        .preferred
        .iter()
        .any(|(id, on)| id.as_str() == capability && *on)
}

fn has_secure_delete(listing: &persistant::registry::Listing) -> bool {
    has_capability(listing, "storage.secure_delete")
}

fn erasure_of(listing: &persistant::registry::Listing) -> Erasure {
    if has_secure_delete(listing) {
        Erasure::Zeroed
    } else if has_capability(listing, "kv.delete_erased") {
        Erasure::EraseOnDelete
    } else {
        Erasure::Unretrievable
    }
}

/// How `backend_id` gets rid of a forgotten chunk's bytes in this build of PersistAnt.
///
/// # Errors
/// `Backend` if the backend does not satisfy [`storage_needs`] at all.
pub fn backend_erasure(backend_id: &str) -> Result<Erasure, Error> {
    let listings = persistant::registry::providers(&storage_needs()).map_err(|e| backend(&e))?;
    listings
        .iter()
        .find(|l| l.id == backend_id)
        .map(erasure_of)
        .ok_or_else(|| {
            Error::Backend(format!(
                "backend {backend_id:?} does not satisfy the storage need in this build"
            ))
        })
}

/// Whether `backend_id` has `storage.secure_delete` in this build of PersistAnt (a forgotten chunk's bytes are zeroed
/// in the database file as they are freed).
///
/// # Errors
/// `Backend` if the backend does not satisfy [`storage_needs`] at all.
pub fn backend_secure_delete(backend_id: &str) -> Result<bool, Error> {
    let listings = persistant::registry::providers(&storage_needs()).map_err(|e| backend(&e))?;
    listings
        .iter()
        .find(|l| l.id == backend_id)
        .map(has_secure_delete)
        .ok_or_else(|| {
            Error::Backend(format!(
                "backend {backend_id:?} does not satisfy the storage need in this build"
            ))
        })
}

/// Why PersistAnt lists `backend_id` for a [`storage_needs`], as lines for a person.
///
/// # Errors
/// `Backend` if the backend does not have every needed capability in this build of PersistAnt (the message says what
/// each backend lacks), or is not offered at all.
pub fn explain_backend(backend_id: &str) -> Result<Vec<String>, Error> {
    let listings = persistant::registry::providers(&storage_needs()).map_err(|e| backend(&e))?;
    listings
        .iter()
        .find(|l| l.id == backend_id)
        .map(persistant::registry::Listing::why)
        .ok_or_else(|| {
            Error::Backend(format!(
                "backend {backend_id:?} does not satisfy the storage need in this build (offered: {})",
                listings.iter().map(|l| l.id).collect::<Vec<_>>().join(", ")
            ))
        })
}

/// Ask PersistAnt which backend to use for a file or directory `endpoint`: the first it ranks, among those with every
/// needed capability, that opens from a path. No silent fallback: if none qualifies the error says what each lacks.
/// A server (URL) endpoint is not chosen for you, because which backend a URL needs depends on its scheme: name the
/// backend and use `open`.
pub(crate) fn choose_backend(endpoint: &Endpoint) -> Result<Chosen, Error> {
    use persistant::capability::provider::EndpointSpec;
    let wanted = match endpoint {
        Endpoint::Path(_) => EndpointSpec::Path,
        Endpoint::None => EndpointSpec::None,
        _ => {
            return Err(Error::Backend(
                "a server endpoint is not chosen for you: name the backend and use `open`"
                    .to_owned(),
            ));
        }
    };
    let listings = persistant::registry::providers(&storage_needs()).map_err(|e| backend(&e))?;
    if let Some(listing) = listings.iter().find(|l| l.endpoint == wanted) {
        return Ok(chosen_from(listing));
    }
    Err(Error::Backend(format!(
        "no backend PersistAnt ranks for this need accepts that kind of endpoint (candidates: {})",
        listings.iter().map(|l| l.id).collect::<Vec<_>>().join(", ")
    )))
}

/// A sageant store kept in a PersistAnt key-value backend.
pub struct PersistStore {
    kv: KvHandle,
    screen: Option<Box<dyn SecretScreen + Send + Sync>>,
    caps: BTreeMap<String, usize>,
    /// Set while a `Writer` call runs: names the actor on every audit entry that call appends.
    acting: Option<String>,
    erasure: Erasure,
    /// Test only: stop a forget after its atomic step, as a crash would, leaving the erasing to the next open.
    crash_before_erasing: bool,
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
    /// Let PersistAnt choose the backend for `endpoint` (see [`storage_needs`]) and open it. Returns the store and the
    /// [`Chosen`] backend with its reasons; record [`Chosen::backend`] and use [`open`](Self::open) with it afterwards.
    ///
    /// # Errors
    /// `Backend` when no backend with the needed capabilities accepts that kind of endpoint (the message says what
    /// each lacks), or when the chosen one cannot be opened.
    pub fn open_chosen(endpoint: &Endpoint) -> Result<(Self, Chosen), Error> {
        let chosen = choose_backend(endpoint)?;
        let store = Self::open(chosen.backend(), endpoint)?;
        Ok((store, chosen))
    }

    /// Open the PersistAnt backend named `backend_id` (for example `"sqlite"` with `Endpoint::Path(file)`).
    ///
    /// The store needs `kv.get`, `kv.put`, `kv.delete`, `kv.ordered_scan_values` and the four `kv.batch_atomic.*` leaves (rollback, concurrent read, optimistic
    /// sequence, absent race), each with a recorded passing test in PersistAnt's matrix, and data
    /// that survives the process exiting.
    ///
    /// # Errors
    /// `Backend` naming what is missing when the backend cannot give those, or cannot be opened.
    pub fn open(backend_id: &str, endpoint: &Endpoint) -> Result<Self, Error> {
        let erasure = backend_erasure(backend_id).unwrap_or(Erasure::Unretrievable);
        let store = Self::open_with(backend_id, endpoint, erasure)?;
        // A forget interrupted between its atomic step and the erasing is finished here.
        store.finish_erasing()?;
        Ok(store)
    }

    /// Open with erasing switched off, so a test can show that the same steps leave the text in the files. Not for
    /// production use.
    ///
    /// # Errors
    /// As `open`.
    #[doc(hidden)]
    pub fn open_without_erasing_for_test(
        backend_id: &str,
        endpoint: &Endpoint,
    ) -> Result<Self, Error> {
        Self::open_with(backend_id, endpoint, Erasure::Unretrievable)
    }

    /// Test only: make the next forgets stop after their atomic step, as a crash would.
    #[doc(hidden)]
    pub fn set_crash_before_erasing_for_test(&mut self, on: bool) {
        self.crash_before_erasing = on;
    }

    fn open_with(backend_id: &str, endpoint: &Endpoint, erasure: Erasure) -> Result<Self, Error> {
        let kv = open_kv(backend_id, endpoint)?;
        Ok(Self {
            kv,
            screen: None,
            caps: BTreeMap::new(),
            acting: None,
            erasure,
            crash_before_erasing: false,
        })
    }

    /// Every key and value under `prefix`, ascending, across pages.
    fn scan_entries(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>, Error> {
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

    /// Erase each key with `kv.delete_erased` (the stored bytes are overwritten before the key is removed).
    fn erase_keys(&self, keys: &[String]) -> Result<(), Error> {
        for key in keys {
            self.kv.delete_erased(key).map_err(|e| {
                Error::Backend(format!(
                    "forgotten, but erasing a stored copy failed ({e}); it is finished the next time the store is opened"
                ))
            })?;
        }
        Ok(())
    }

    /// Finish the erasing of any forgotten chunk whose content is still stored (a forget that stopped after its atomic
    /// step). Returns how many stored copies were erased. Does nothing unless the backend erases on delete.
    ///
    /// # Errors
    /// `Corrupt` or `Backend`.
    pub fn finish_erasing(&self) -> Result<usize, Error> {
        if self.erasure != Erasure::EraseOnDelete {
            return Ok(0);
        }
        let mut erased = 0;
        for bytes in self.scan("t/")? {
            let tombstone = Tombstone::decode(&bytes)?;
            let left: Vec<String> = self
                .scan_entries(&format!("c/{}/", tombstone.id().as_str()))?
                .into_iter()
                .map(|(key, _)| key)
                .collect();
            self.erase_keys(&left)?;
            erased += left.len();
        }
        Ok(erased)
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
        // Content may still be stored for a chunk that was forgotten but not yet erased: no head, no history.
        if self.head(id)?.is_none() {
            return Err(self.missing(id));
        }
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
        let live: std::collections::BTreeSet<String> = self
            .scan_entries("h/")?
            .into_iter()
            .map(|(key, _)| key.trim_start_matches("h/").to_owned())
            .collect();
        for bytes in self.scan("c/")? {
            let chunk = Chunk::decode(&bytes)?;
            // A forgotten chunk whose content is not yet erased has no head and is not listed.
            if live.contains(chunk.id().as_str()) {
                all.push(chunk);
            }
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
    /// changes nothing. The cycle check is atomic with the write: every chunk it reads has a link generation that must be
    /// unchanged at commit, so two writers adding opposite `part-of` links at the same moment cannot both win (the
    /// loser reads again and is refused).
    pub fn link(
        &self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        at: Timestamp,
    ) -> Result<(), Error> {
        self.link_inner(from, kind, to, at, None)
    }

    /// [`link`](Self::link), running `hook` once after the checks and before the commit, to make a race
    /// deterministic in a test. Not for production use.
    #[doc(hidden)]
    pub fn link_with_hook_for_test(
        &self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        at: Timestamp,
        hook: &dyn Fn(),
    ) -> Result<(), Error> {
        self.link_inner(from, kind, to, at, Some(hook))
    }

    fn link_inner(
        &self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        at: Timestamp,
        hook: Option<&dyn Fn()>,
    ) -> Result<(), Error> {
        for attempt in 0..GEN_RETRIES {
            let from_version = self.get(from)?.version();
            let to_version = self.get(to)?.version();
            if from == to {
                return Err(Error::LinkCycle);
            }
            let forward = link_key(from, kind, to);
            if self.read(&forward)?.is_some() {
                return Err(Error::DuplicateLink);
            }
            // Every generation read below is required unchanged at commit.
            let mut seen: BTreeMap<ChunkId, Option<Vec<u8>>> = BTreeMap::new();
            for id in [from, to] {
                seen.insert(id.clone(), self.read(&gen_key(id))?);
            }
            if matches!(kind, LinkKind::PartOf | LinkKind::DerivedFrom) {
                let mut next = |node: &ChunkId| -> Result<Vec<ChunkId>, Error> {
                    if !seen.contains_key(node) {
                        seen.insert(node.clone(), self.read(&gen_key(node))?);
                    }
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
            if attempt == 0
                && let Some(hook) = hook
            {
                hook();
            }
            let link = Link::new(from.clone(), from_version, kind, to.clone(), at);
            let event = AuditEvent::new(
                at,
                Action::Link,
                Some(from.clone()),
                &self.who(""),
                &format!("{kind:?}"),
            );
            let mut ops = vec![
                BatchOp::Require {
                    key: forward.clone(),
                    expected: None,
                },
                BatchOp::Require {
                    key: head_key(from),
                    expected: Some(from_version.to_string().into_bytes()),
                },
                BatchOp::Require {
                    key: head_key(to),
                    expected: Some(to_version.to_string().into_bytes()),
                },
            ];
            for (id, generation) in &seen {
                ops.push(BatchOp::Require {
                    key: gen_key(id),
                    expected: generation.clone(),
                });
            }
            ops.push(BatchOp::Put {
                key: forward,
                value: link.encode(),
                ttl: NO_TTL,
            });
            ops.push(BatchOp::Put {
                key: reverse_key(from, kind, to),
                value: link.encode(),
                ttl: NO_TTL,
            });
            for id in [from, to] {
                let current = seen.get(id).and_then(Option::as_ref);
                ops.push(BatchOp::Put {
                    key: gen_key(id),
                    value: bumped(current)?,
                    ttl: NO_TTL,
                });
            }
            if self.apply(&ops, &[event])? {
                return Ok(());
            }
            if self.read(&link_key(from, kind, to))?.is_some() {
                return Err(Error::DuplicateLink);
            }
            let found = self.head(from)?.unwrap_or(0);
            if found != from_version {
                return Err(Error::Conflict {
                    expected: from_version,
                    found,
                });
            }
            // Otherwise a link landed on a chunk this one read, or the target changed: read again.
        }
        Err(Error::Backend("link contention".to_owned()))
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
    fn removal_ops(&self, id: &ChunkId) -> Result<(Vec<BatchOp>, Vec<String>), Error> {
        let mut ops = Vec::new();
        let mut erase_later = Vec::new();
        for chunk in self.history(id)? {
            let key = chunk_key(id, chunk.version());
            if self.erasure == Erasure::EraseOnDelete {
                // Erased while the key is live, after the atomic step that makes the chunk unreadable.
                erase_later.push(key);
            } else {
                ops.push(BatchOp::Delete { key });
            }
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
        Ok((ops, erase_later))
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
        self.forget_inner(id, actor, reason, keyed_hash, at, None)
    }

    /// [`forget`](Self::forget), running `hook` once after the reads and before the commit. Test only.
    #[doc(hidden)]
    pub fn forget_with_hook_for_test(
        &self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        keyed_hash: Option<&str>,
        at: Timestamp,
        hook: &dyn Fn(),
    ) -> Result<Removed, Error> {
        self.forget_inner(id, actor, reason, keyed_hash, at, Some(hook))
    }

    fn forget_inner(
        &self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        keyed_hash: Option<&str>,
        at: Timestamp,
        hook: Option<&dyn Fn()>,
    ) -> Result<Removed, Error> {
        for attempt in 0..GEN_RETRIES {
            let head = self.head(id)?.ok_or_else(|| self.missing(id))?;
            // Read the link generation before the links, so a link that lands later makes the commit retry.
            let generation = self.read(&gen_key(id))?;
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
            let mut ops = vec![
                BatchOp::Require {
                    key: head_key(id),
                    expected: Some(head.to_string().into_bytes()),
                },
                BatchOp::Require {
                    key: gen_key(id),
                    expected: generation,
                },
            ];
            let (removal, erase_later) = self.removal_ops(id)?;
            ops.extend(removal);
            ops.push(BatchOp::Put {
                key: tombstone_key(id),
                value: tombstone.encode(),
                ttl: NO_TTL,
            });
            if attempt == 0
                && let Some(hook) = hook
            {
                hook();
            }
            let event = AuditEvent::new(
                at,
                Action::Forget,
                Some(id.clone()),
                &self.who(actor),
                reason,
            );
            if self.apply(&ops, &[event])? {
                if !self.crash_before_erasing {
                    self.erase_keys(&erase_later)?;
                }
                return Ok(Removed::Unretrievable);
            }
            let found = self.head(id)?.unwrap_or(0);
            if found != head {
                return Err(Error::Conflict {
                    expected: head,
                    found,
                });
            }
        }
        Err(Error::Backend("forget contention".to_owned()))
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
        self.purge_inner(id, authority, at, None)
    }

    /// [`purge`](Self::purge), running `hook` once after the reads and before the commit. Test only.
    #[doc(hidden)]
    pub fn purge_with_hook_for_test(
        &self,
        id: &ChunkId,
        authority: &PersonAuthority,
        at: Timestamp,
        hook: &dyn Fn(),
    ) -> Result<(), Error> {
        self.purge_inner(id, authority, at, Some(hook))
    }

    fn purge_inner(
        &self,
        id: &ChunkId,
        authority: &PersonAuthority,
        at: Timestamp,
        hook: Option<&dyn Fn()>,
    ) -> Result<(), Error> {
        for attempt in 0..GEN_RETRIES {
            let head = self.head(id)?;
            let tombstone = self.read(&tombstone_key(id))?;
            if head.is_none() && tombstone.is_none() {
                return Err(Error::NotFound);
            }
            let generation = self.read(&gen_key(id))?;
            let mut ops = vec![
                BatchOp::Require {
                    key: head_key(id),
                    expected: head.map(|h| h.to_string().into_bytes()),
                },
                BatchOp::Require {
                    key: tombstone_key(id),
                    expected: tombstone,
                },
                BatchOp::Require {
                    key: gen_key(id),
                    expected: generation,
                },
                BatchOp::Delete {
                    key: tombstone_key(id),
                },
                BatchOp::Delete { key: gen_key(id) },
            ];
            let mut erase_later = Vec::new();
            if head.is_some() {
                let (removal, later) = self.removal_ops(id)?;
                ops.extend(removal);
                erase_later = later;
            }
            if attempt == 0
                && let Some(hook) = hook
            {
                hook();
            }
            let event = AuditEvent::new(at, Action::Purge, Some(id.clone()), authority.how(), "");
            if self.apply(&ops, &[event])? {
                self.erase_keys(&erase_later)?;
                // Content a forget left behind (it stopped after its atomic step) goes too.
                self.finish_erasing()?;
                return Ok(());
            }
            let found = self.head(id)?.unwrap_or(0);
            if found != head.unwrap_or(0) {
                return Err(Error::Conflict {
                    expected: head.unwrap_or(0),
                    found,
                });
            }
        }
        Err(Error::Backend("purge contention".to_owned()))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(id: &str) -> persistant::registry::Listing {
        persistant::registry::providers(&storage_needs())
            .unwrap()
            .into_iter()
            .find(|l| l.id == id)
            .unwrap_or_else(|| panic!("{id} is offered"))
    }

    #[test]
    fn a_chosen_backend_carries_its_id_its_reasons_and_whether_it_zeroes_freed_bytes() {
        let secure = chosen_from(&listing("sqlite-secure"));
        assert_eq!(secure.backend(), "sqlite-secure");
        assert!(secure.secure_delete());
        assert!(
            secure.why().iter().any(|l| l.contains("durability")),
            "{:?}",
            secure.why()
        );
        let plain = chosen_from(&listing("sqlite"));
        assert_eq!(plain.backend(), "sqlite");
        assert!(
            !plain.secure_delete(),
            "plain sqlite does not zero freed bytes"
        );
        assert_ne!(plain.why(), secure.why());
    }

    #[test]
    fn explain_backend_gives_persistants_reasons_for_a_known_id_and_an_error_for_an_unknown_one() {
        let lines = explain_backend("sqlite-secure").unwrap();
        assert!(lines.iter().any(|l| l.contains("durability")), "{lines:?}");
        assert!(lines.iter().all(|l| !l.is_empty()));
        assert_eq!(lines, listing("sqlite-secure").why());
        let e = explain_backend("no-such-backend").unwrap_err().to_string();
        assert!(
            e.contains("no-such-backend") && e.contains("offered"),
            "{e}"
        );
    }

    #[test]
    fn only_a_path_or_nothing_is_chosen_for_and_each_other_kind_is_told_why_not() {
        let none = choose_backend(&Endpoint::None).unwrap_err().to_string();
        assert!(
            none.contains("accepts that kind of endpoint"),
            "nothing: {none}"
        );
        let url = choose_backend(&Endpoint::url_env("NO_SUCH_VARIABLE"))
            .unwrap_err()
            .to_string();
        assert!(url.contains("server endpoint"), "url: {url}");
        assert!(choose_backend(&Endpoint::Path("x.db".into())).is_ok());
    }

    #[test]
    fn a_generation_counts_up_by_one_and_an_unreadable_one_is_corrupt() {
        assert_eq!(bumped(None).unwrap(), b"1");
        assert_eq!(bumped(Some(&b"1".to_vec())).unwrap(), b"2");
        assert_eq!(bumped(Some(&b"41".to_vec())).unwrap(), b"42");
        assert!(matches!(
            bumped(Some(&b"x".to_vec())),
            Err(Error::Corrupt(_))
        ));
        assert!(matches!(bumped(Some(&vec![0xff])), Err(Error::Corrupt(_))));
    }
}
