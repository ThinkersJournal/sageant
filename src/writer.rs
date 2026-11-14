// SPDX-License-Identifier: MIT OR Apache-2.0
//! The write side of the store (CONTRACT D11). Retrievers get a `Reader`; only a harness holds a `Writer`.

use crate::Error;
use crate::lifecycle::{Actor, PersonAuthority, Removed};
use crate::model::{ChunkId, ChunkRef, LinkKind, NewChunk, Origin, Status, Timestamp};

/// Write access to a store. Every method takes the caller's time and an [`Actor`], returns what it wrote, and
/// appends an audit event naming `role/agent`. `purge` still needs a [`PersonAuthority`], so holding a `Writer`
/// is not enough to purge. The secrets override stays a separate, explicit call on the store.
pub trait Writer {
    /// Store a new chunk as version 1.
    ///
    /// # Errors
    /// As `MemoryStore::put`; `Invalid("actor")` for an empty role or agent.
    fn put(&mut self, new: NewChunk, actor: &Actor, at: Timestamp) -> Result<ChunkRef, Error>;
    /// Append a new version of a lineage.
    ///
    /// # Errors
    /// As `MemoryStore::supersede`; `Invalid("actor")`.
    fn supersede(
        &mut self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        actor: &Actor,
        at: Timestamp,
    ) -> Result<ChunkRef, Error>;
    /// Link `from` to `to`.
    ///
    /// # Errors
    /// As `MemoryStore::link`; `Invalid("actor")`.
    fn link(
        &mut self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        actor: &Actor,
        at: Timestamp,
    ) -> Result<(), Error>;
    /// Archive or reactivate a lineage.
    ///
    /// # Errors
    /// `NotFound`, `Forgotten`, `BadState` if it already has that status; `Invalid("actor")`.
    fn set_status(
        &mut self,
        id: &ChunkId,
        status: Status,
        actor: &Actor,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error>;
    /// Remove a lineage's content, leaving a tombstone.
    ///
    /// # Errors
    /// As `MemoryStore::forget`; `Invalid("actor")`.
    fn forget(
        &mut self,
        id: &ChunkId,
        actor: &Actor,
        reason: &str,
        keyed_hash: Option<&str>,
        at: Timestamp,
    ) -> Result<Removed, Error>;
    /// Remove everything about a lineage. The audit entry names the person's authority, not an actor.
    ///
    /// # Errors
    /// `NotFound`.
    fn purge(
        &mut self,
        id: &ChunkId,
        authority: &PersonAuthority,
        at: Timestamp,
    ) -> Result<(), Error>;
}
