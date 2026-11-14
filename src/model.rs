// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::sha256::sha256_hex;

/// Caller-supplied time (the core has no clock). Unit is the caller's; the store only orders by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub u64);

/// Where a chunk came from (CONTRACT D5). Policy matches on this; the open part is `source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Origin {
    /// A person.
    User,
    /// A tool's output.
    Tool,
    /// Fetched from the web.
    Web,
    /// Another agent.
    Agent,
    /// A model's own output.
    Model,
}

/// Evidence-based state (CONTRACT D6). A new chunk starts `Unconfirmed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Confidence {
    /// No independent corroboration yet.
    Unconfirmed,
    /// Corroborated by an independent origin.
    Corroborated,
    /// Contradicted by evidence.
    Contradicted,
}

/// Lifecycle state (CONTRACT D8). Only `Active` exists in this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    /// Retrievable.
    Active,
}

/// Stable identity of a chunk lineage: 64 lowercase hex characters derived from collection and first content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkId(pub(crate) String);

impl ChunkId {
    pub(crate) fn derive(collection: &str, content_hash: &str) -> Self {
        let mut material = Vec::with_capacity(collection.len() + content_hash.len() + 1);
        material.extend_from_slice(collection.as_bytes());
        material.push(0);
        material.extend_from_slice(content_hash.as_bytes());
        Self(sha256_hex(&material))
    }

    /// The id as hex text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The result of a write: which lineage and which version was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkRef {
    /// Lineage id.
    pub id: ChunkId,
    /// Version number, from 1.
    pub version: u32,
}

/// A chunk to be stored.
#[derive(Debug, Clone)]
pub struct NewChunk {
    pub(crate) collection: String,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) origin: Origin,
    pub(crate) source: String,
}

impl NewChunk {
    /// A chunk of `content` in `collection`, of `kind`, from `origin` with free-text `source`.
    #[must_use]
    pub fn new(collection: &str, kind: &str, content: &str, origin: Origin, source: &str) -> Self {
        Self {
            collection: collection.to_owned(),
            kind: kind.to_owned(),
            content: content.to_owned(),
            origin,
            source: source.to_owned(),
        }
    }
}

/// One immutable version of a chunk.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub(crate) id: ChunkId,
    pub(crate) version: u32,
    pub(crate) collection: String,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) content_hash: String,
    pub(crate) origin: Origin,
    pub(crate) source: String,
    pub(crate) confidence: Confidence,
    pub(crate) status: Status,
    pub(crate) created_at: Timestamp,
}

impl Chunk {
    /// Lineage id.
    #[must_use]
    pub fn id(&self) -> &ChunkId {
        &self.id
    }
    /// Version number, from 1.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }
    /// Collection (namespace).
    #[must_use]
    pub fn collection(&self) -> &str {
        &self.collection
    }
    /// Kind (validated open string).
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }
    /// The text.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }
    /// SHA-256 of the text, hex.
    #[must_use]
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
    /// Origin class.
    #[must_use]
    pub fn origin(&self) -> Origin {
        self.origin
    }
    /// Free-text source (tool name, URL, role, model id).
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
    /// Confidence when this version was written (always `Unconfirmed`); the current state, which links can
    /// change, is `MemoryStore::confidence`.
    #[must_use]
    pub fn confidence(&self) -> Confidence {
        self.confidence
    }
    /// Lifecycle state.
    #[must_use]
    pub fn status(&self) -> Status {
        self.status
    }
    /// When this version was written (caller's time).
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
}

/// The type of a link between chunks (CONTRACT D7). Declaration order is the listing order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum LinkKind {
    /// The source is a child of the target (a section of a document). Cycles are refused.
    PartOf,
    /// The source was derived from the target (a summary of it). Cycles are refused.
    DerivedFrom,
    /// The source cites the target.
    Cites,
    /// A loose relation; may be mutual.
    Related,
    /// The target is independent evidence for the source (different origin class and source).
    CorroboratedBy,
    /// The target is evidence against the source.
    ContradictedBy,
}

/// A directed, append-only link, attached to the source version that was current when it was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub(crate) from: ChunkId,
    pub(crate) from_version: u32,
    pub(crate) kind: LinkKind,
    pub(crate) to: ChunkId,
    pub(crate) created_at: Timestamp,
}

impl Link {
    /// Source lineage.
    #[must_use]
    pub fn from(&self) -> &ChunkId {
        &self.from
    }
    /// The source version this link belongs to.
    #[must_use]
    pub fn from_version(&self) -> u32 {
        self.from_version
    }
    /// Link type.
    #[must_use]
    pub fn kind(&self) -> LinkKind {
        self.kind
    }
    /// Target lineage.
    #[must_use]
    pub fn to(&self) -> &ChunkId {
        &self.to
    }
    /// When the link was made (caller's time).
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
}
