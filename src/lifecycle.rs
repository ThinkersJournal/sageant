// SPDX-License-Identifier: MIT OR Apache-2.0
//! Deletion and audit types (CONTRACT D8, D18).
//!
//! The promise is "unretrievable", never "erased": PersistAnt cannot yet prove removed bytes are gone
//! (persistant ROADMAP row 38). There is deliberately no `Erased` variant.

use crate::Error;
use crate::model::{ChunkId, Timestamp};

/// What `forget` guarantees. Only `Unretrievable` exists until a backend can prove erasure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Removed {
    /// No API of the store returns the content any more. Storage underneath may retain bytes.
    Unretrievable,
}

/// The minimal record `forget` leaves: no content, no source, no content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tombstone {
    pub(crate) id: ChunkId,
    pub(crate) collection: String,
    pub(crate) versions_removed: u32,
    pub(crate) at: Timestamp,
    pub(crate) actor: String,
    pub(crate) reason: String,
    pub(crate) keyed_hash: Option<String>,
}

impl Tombstone {
    /// The forgotten lineage.
    #[must_use]
    pub fn id(&self) -> &ChunkId {
        &self.id
    }
    /// Its collection.
    #[must_use]
    pub fn collection(&self) -> &str {
        &self.collection
    }
    /// How many versions were removed.
    #[must_use]
    pub fn versions_removed(&self) -> u32 {
        self.versions_removed
    }
    /// When (caller's time).
    #[must_use]
    pub fn at(&self) -> Timestamp {
        self.at
    }
    /// Who asked.
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }
    /// Why, in the caller's words.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
    /// A caller-supplied keyed hash, so an ingester can be told not to re-add the text.
    #[must_use]
    pub fn keyed_hash(&self) -> Option<&str> {
        self.keyed_hash.as_deref()
    }
}

/// Whether a link's target still exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTarget {
    /// The target is stored.
    Present,
    /// The target was forgotten or purged; the link says so instead of dangling.
    Removed,
}

/// Proof that a person took the action. Needed to `purge`. A CLI constructs it after a typed confirmation or a host
/// prompt; the MCP server never does (CONTRACT D13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonAuthority(String);

impl PersonAuthority {
    /// Record that a person authorised this, described in `how` (for the audit log; never content).
    #[must_use]
    pub fn from_person_action(how: &str) -> Self {
        Self(how.to_owned())
    }
    /// What the person did to authorise this, as it will appear in the audit log.
    #[must_use]
    pub fn how(&self) -> &str {
        &self.0
    }
}

/// What an audit entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// A chunk was stored.
    Put,
    /// A new version was stored.
    Supersede,
    /// A link was made.
    Link,
    /// A chunk was archived.
    Archive,
    /// An archived chunk was made active again.
    Unarchive,
    /// A reason recorded for an earlier write (for example why a maintenance step ran); the reason is the text.
    Note,
    /// A chunk was forgotten.
    Forget,
    /// A chunk was purged.
    Purge,
    /// A write was refused by the secret screen; the reason is `secret:<class>`.
    Refused,
    /// A write the secret screen flagged was allowed by a caller-held [`SecretOverride`]; the reason is
    /// `secret-override:<class>` and the actor is who allowed it. Never holds the text.
    Overridden,
}

/// A caller-held, per-write permission to store text the secret screen flagged as one class. It covers exactly one
/// write and only that class; the store audits every use and never logs the text (CONTRACT D9). The MCP server
/// never builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretOverride {
    class: String,
    by: String,
}

impl SecretOverride {
    /// Allow one write whose text the screen flags as `class`. `by` says who allowed it, for the audit log.
    #[must_use]
    pub fn allow_class(class: &str, by: &str) -> Self {
        Self {
            class: class.to_owned(),
            by: by.to_owned(),
        }
    }
    /// The class of secret this override allows.
    #[must_use]
    pub fn class(&self) -> &str {
        &self.class
    }
    /// Who allowed it, as it will appear in the audit log.
    #[must_use]
    pub fn by(&self) -> &str {
        &self.by
    }
}

/// One audit entry. Never holds content (the text, the source text or the content hash).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub(crate) at: Timestamp,
    pub(crate) action: Action,
    pub(crate) id: Option<ChunkId>,
    pub(crate) actor: String,
    pub(crate) reason: String,
}

impl AuditEvent {
    /// When (caller's time).
    #[must_use]
    pub fn at(&self) -> Timestamp {
        self.at
    }
    /// What happened.
    #[must_use]
    pub fn action(&self) -> Action {
        self.action
    }
    /// The lineage concerned, if any.
    #[must_use]
    pub fn id(&self) -> Option<&ChunkId> {
        self.id.as_ref()
    }
    /// Who did it (for a purge, how the person authorised it).
    #[must_use]
    pub fn actor(&self) -> &str {
        &self.actor
    }
    /// Why, or the link kind, or the refusal class.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// Who is writing, in the caller's terms: a role and an agent. One type for every write through
/// [`Writer`](crate::Writer) (CONTRACT D11); the audit log names it as `role/agent`. Both parts must be non-empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    role: String,
    agent: String,
}

impl Actor {
    /// An actor with a role (for example `curator`) and an agent name.
    #[must_use]
    pub fn new(role: &str, agent: &str) -> Self {
        Self {
            role: role.to_owned(),
            agent: agent.to_owned(),
        }
    }
    /// The role.
    #[must_use]
    pub fn role(&self) -> &str {
        &self.role
    }
    /// The agent.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }
    /// Whether both parts are named.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.role.is_empty() && !self.agent.is_empty()
    }
    /// The `role/agent` label written to the audit log.
    #[must_use]
    pub fn label(&self) -> String {
        format!("{}/{}", self.role, self.agent)
    }
}

impl Action {
    /// The stable name written into stored audit entries.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Put => "put",
            Self::Supersede => "supersede",
            Self::Link => "link",
            Self::Archive => "archive",
            Self::Unarchive => "unarchive",
            Self::Note => "note",
            Self::Forget => "forget",
            Self::Purge => "purge",
            Self::Refused => "refused",
            Self::Overridden => "overridden",
        }
    }

    /// The action a stored name stands for.
    ///
    /// # Errors
    /// `Invalid("action")` for a name `name` never writes.
    pub fn parse(name: &str) -> Result<Self, Error> {
        Ok(match name {
            "put" => Self::Put,
            "supersede" => Self::Supersede,
            "link" => Self::Link,
            "archive" => Self::Archive,
            "unarchive" => Self::Unarchive,
            "note" => Self::Note,
            "forget" => Self::Forget,
            "purge" => Self::Purge,
            "refused" => Self::Refused,
            "overridden" => Self::Overridden,
            _ => return Err(Error::Invalid("action")),
        })
    }
}

impl AuditEvent {
    /// An audit entry. Stores build these; this is for a store that keeps its own log.
    #[must_use]
    pub fn new(
        at: Timestamp,
        action: Action,
        id: Option<ChunkId>,
        actor: &str,
        reason: &str,
    ) -> Self {
        Self {
            at,
            action,
            id,
            actor: actor.to_owned(),
            reason: reason.to_owned(),
        }
    }
}

impl Tombstone {
    /// A tombstone. Stores build these; this is for a store that keeps its own.
    #[must_use]
    pub fn new(
        id: ChunkId,
        collection: &str,
        versions_removed: u32,
        at: Timestamp,
        actor: &str,
        reason: &str,
        keyed_hash: Option<&str>,
    ) -> Self {
        Self {
            id,
            collection: collection.to_owned(),
            versions_removed,
            at,
            actor: actor.to_owned(),
            reason: reason.to_owned(),
            keyed_hash: keyed_hash.map(str::to_owned),
        }
    }
}
