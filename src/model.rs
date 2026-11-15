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

/// Lifecycle state of a stored chunk (CONTRACT D8). A forgotten chunk is not stored, so it has no status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Status {
    /// Retrievable.
    Active,
    /// Kept, but out of retrieval.
    Archived,
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

    /// Read an id from its hex text.
    ///
    /// # Errors
    /// `Invalid("id")` unless it is exactly 64 lowercase hex characters.
    pub fn parse(text: &str) -> Result<Self, crate::Error> {
        if text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            Ok(Self(text.to_owned()))
        } else {
            Err(crate::Error::Invalid("id"))
        }
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
    /// Build version 1 of a new lineage from `new`: validate it, hash it and derive its id, exactly as
    /// `MemoryStore::put` does (a store adds its own caps, screen and duplicate checks around this).
    ///
    /// # Errors
    /// `Invalid("collection")`, `Invalid("kind")`, `Invalid("content")` or `NotNormalised`.
    pub fn first_version(new: NewChunk, at: Timestamp) -> Result<Self, crate::Error> {
        if new.collection.is_empty() {
            return Err(crate::Error::Invalid("collection"));
        }
        if new.kind.is_empty() {
            return Err(crate::Error::Invalid("kind"));
        }
        crate::store::check_text(&new.content)?;
        let content_hash = sha256_hex(new.content.as_bytes());
        Ok(Self {
            id: ChunkId::derive(&new.collection, &content_hash),
            version: 1,
            collection: new.collection,
            kind: new.kind,
            content: new.content,
            content_hash,
            origin: new.origin,
            source: new.source,
            confidence: Confidence::Unconfirmed,
            status: Status::Active,
            created_at: at,
        })
    }

    /// Build the version after this one, as `MemoryStore::supersede` does. It keeps the lineage id, collection,
    /// kind and status (a new version of an archived chunk stays archived).
    ///
    /// # Errors
    /// `Invalid("content")`, `NotNormalised`, or `Conflict` when the text is unchanged.
    pub fn next_version(
        &self,
        content: &str,
        origin: Origin,
        source: &str,
        at: Timestamp,
    ) -> Result<Self, crate::Error> {
        crate::store::check_text(content)?;
        let content_hash = sha256_hex(content.as_bytes());
        if content_hash == self.content_hash {
            return Err(crate::Error::Conflict {
                expected: self.version,
                found: self.version,
            });
        }
        Ok(Self {
            id: self.id.clone(),
            version: self.version + 1,
            collection: self.collection.clone(),
            kind: self.kind.clone(),
            content: content.to_owned(),
            content_hash,
            origin,
            source: source.to_owned(),
            confidence: Confidence::Unconfirmed,
            status: self.status,
            created_at: at,
        })
    }

    /// This version with its status set to `status`; everything else, including the content hash, is unchanged.
    #[must_use]
    pub fn with_status(&self, status: Status) -> Self {
        let mut next = self.clone();
        next.status = status;
        next
    }

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

impl LinkKind {
    /// The kind's stable name, as stored and as the MCP tools spell it.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::PartOf => "PartOf",
            Self::DerivedFrom => "DerivedFrom",
            Self::Cites => "Cites",
            Self::Related => "Related",
            Self::CorroboratedBy => "CorroboratedBy",
            Self::ContradictedBy => "ContradictedBy",
        }
    }

    /// Read a kind from its [`name`](Self::name).
    ///
    /// # Errors
    /// `Invalid("link kind")` for anything else.
    pub fn parse(text: &str) -> Result<Self, crate::Error> {
        [
            Self::PartOf,
            Self::DerivedFrom,
            Self::Cites,
            Self::Related,
            Self::CorroboratedBy,
            Self::ContradictedBy,
        ]
        .into_iter()
        .find(|k| k.name() == text)
        .ok_or(crate::Error::Invalid("link kind"))
    }
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
    /// A link, as a store builds one after it has checked the rules.
    #[must_use]
    pub fn new(
        from: ChunkId,
        from_version: u32,
        kind: LinkKind,
        to: ChunkId,
        created_at: Timestamp,
    ) -> Self {
        Self {
            from,
            from_version,
            kind,
            to,
            created_at,
        }
    }

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
