// SPDX-License-Identifier: MIT OR Apache-2.0
use std::collections::BTreeMap;

use crate::Error;
use crate::model::{
    Chunk, ChunkId, ChunkRef, Confidence, Link, LinkKind, NewChunk, Origin, Status, Timestamp,
};
use crate::screen::SecretScreen;
use crate::sha256::sha256_hex;

/// The in-memory reference store. Ordered maps only, so no hash order can reach any output (CONTRACT D16).
#[derive(Default)]
pub struct MemoryStore {
    screen: Option<Box<dyn SecretScreen>>,
    lineages: BTreeMap<ChunkId, Vec<Chunk>>,
    links: BTreeMap<ChunkId, Vec<Link>>,
    caps: BTreeMap<String, usize>,
}

/// Core validates only what it can verify: ASCII is NFC; anything else is refused until the `nfc` helper exists (D4).
fn check_text(content: &str) -> Result<(), Error> {
    if content.is_empty() {
        return Err(Error::Invalid("content"));
    }
    match content.bytes().position(|b| !b.is_ascii()) {
        Some(at) => Err(Error::NotNormalised { at }),
        None => Ok(()),
    }
}

impl core::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryStore")
            .field("versions", &self.len())
            .field("screened", &self.screen.is_some())
            .finish()
    }
}

impl MemoryStore {
    /// Set the secret screen run on the content and source of every write. Applies to later writes.
    pub fn set_screen(&mut self, screen: Box<dyn SecretScreen>) {
        self.screen = Some(screen);
    }

    fn run_screen(&self, texts: &[&str]) -> Result<(), Error> {
        if let Some(screen) = &self.screen {
            for text in texts {
                if let Some(class) = screen.screen(text) {
                    return Err(Error::SecretRefused { class });
                }
            }
        }
        Ok(())
    }

    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of chunk versions held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lineages.values().map(Vec::len).sum()
    }

    /// True when nothing is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lineages.is_empty()
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

    /// Store a new chunk as version 1 of a new lineage.
    ///
    /// # Errors
    /// `Invalid`, `NotNormalised`, `TooLarge`, or `Conflict` when the same text already exists in the collection.
    pub fn put(&mut self, new: NewChunk, at: Timestamp) -> Result<ChunkRef, Error> {
        if new.collection.is_empty() {
            return Err(Error::Invalid("collection"));
        }
        if new.kind.is_empty() {
            return Err(Error::Invalid("kind"));
        }
        check_text(&new.content)?;
        self.check_cap(&new.collection, &new.content)?;
        self.run_screen(&[&new.content, &new.source])?;
        let content_hash = sha256_hex(new.content.as_bytes());
        let id = ChunkId::derive(&new.collection, &content_hash);
        if let Some(existing) = self.lineages.get(&id) {
            return Err(Error::Conflict {
                expected: 0,
                found: u32::try_from(existing.len()).unwrap_or(u32::MAX),
            });
        }
        let chunk = Chunk {
            id: id.clone(),
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
        };
        self.lineages.insert(id.clone(), vec![chunk]);
        Ok(ChunkRef { id, version: 1 })
    }

    /// Append a new version of an existing lineage; the old version stays readable.
    ///
    /// # Errors
    /// `NotFound`, `NotNormalised`, `TooLarge`, `Invalid`, or `Conflict` when the text is unchanged.
    pub fn supersede(
        &mut self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        check_text(content)?;
        let current = self
            .lineages
            .get(id)
            .and_then(|v| v.last())
            .ok_or(Error::NotFound)?;
        self.check_cap(&current.collection, content)?;
        self.run_screen(&[content, source])?;
        let content_hash = sha256_hex(content.as_bytes());
        if content_hash == current.content_hash {
            return Err(Error::Conflict {
                expected: current.version,
                found: current.version,
            });
        }
        let next = Chunk {
            id: id.clone(),
            version: current.version + 1,
            collection: current.collection.clone(),
            kind: current.kind.clone(),
            content: content.to_owned(),
            content_hash,
            origin,
            source: source.to_owned(),
            confidence: Confidence::Unconfirmed,
            status: Status::Active,
            created_at: at,
        };
        let version = next.version;
        if let Some(v) = self.lineages.get_mut(id) {
            v.push(next);
        }
        Ok(ChunkRef {
            id: id.clone(),
            version,
        })
    }

    /// The current (latest) version.
    ///
    /// # Errors
    /// `NotFound`.
    pub fn get(&self, id: &ChunkId) -> Result<&Chunk, Error> {
        self.lineages
            .get(id)
            .and_then(|v| v.last())
            .ok_or(Error::NotFound)
    }

    /// A specific version, from 1.
    ///
    /// # Errors
    /// `NotFound` for an unknown id or version.
    pub fn get_version(&self, id: &ChunkId, version: u32) -> Result<&Chunk, Error> {
        let index = usize::try_from(version)
            .ok()
            .and_then(|v| v.checked_sub(1))
            .ok_or(Error::NotFound)?;
        self.lineages
            .get(id)
            .and_then(|v| v.get(index))
            .ok_or(Error::NotFound)
    }

    /// Every version, oldest first.
    ///
    /// # Errors
    /// `NotFound`.
    pub fn history(&self, id: &ChunkId) -> Result<&[Chunk], Error> {
        self.lineages
            .get(id)
            .map(Vec::as_slice)
            .ok_or(Error::NotFound)
    }

    /// Link `from` to `to`, attached to `from`'s current version.
    ///
    /// # Errors
    /// `NotFound` if either lineage is unknown; `LinkCycle` for a self-link or a cycle in `part-of` or
    /// `derived-from`; `DuplicateLink` for the same (source, kind, target). A refused link changes nothing.
    pub fn link(
        &mut self,
        from: &ChunkId,
        kind: LinkKind,
        to: &ChunkId,
        at: Timestamp,
    ) -> Result<(), Error> {
        let from_version = self.get(from)?.version;
        self.get(to)?;
        if from == to {
            return Err(Error::LinkCycle);
        }
        let out = self.links.get(from).map_or(&[][..], Vec::as_slice);
        if out.iter().any(|l| l.kind == kind && &l.to == to) {
            return Err(Error::DuplicateLink);
        }
        if matches!(kind, LinkKind::PartOf | LinkKind::DerivedFrom) && self.reaches(to, from, kind)
        {
            return Err(Error::LinkCycle);
        }
        let list = self.links.entry(from.clone()).or_default();
        list.push(Link {
            from: from.clone(),
            from_version,
            kind,
            to: to.clone(),
            created_at: at,
        });
        list.sort_by(|a, b| (a.kind, &a.to).cmp(&(b.kind, &b.to)));
        Ok(())
    }

    /// Whether `goal` can be reached from `start` along edges of `kind`.
    fn reaches(&self, start: &ChunkId, goal: &ChunkId, kind: LinkKind) -> bool {
        let mut seen: Vec<&ChunkId> = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            if node == goal {
                return true;
            }
            if seen.contains(&node) {
                continue;
            }
            seen.push(node);
            if let Some(out) = self.links.get(node) {
                stack.extend(out.iter().filter(|l| l.kind == kind).map(|l| &l.to));
            }
        }
        false
    }

    /// Links made from `id`, ordered by (kind, target id).
    ///
    /// # Errors
    /// `NotFound`.
    pub fn links_from(&self, id: &ChunkId) -> Result<Vec<&Link>, Error> {
        self.get(id)?;
        Ok(self
            .links
            .get(id)
            .map_or_else(Vec::new, |v| v.iter().collect()))
    }

    /// Links pointing at `id`, ordered by (kind, source id).
    ///
    /// # Errors
    /// `NotFound`.
    pub fn links_to(&self, id: &ChunkId) -> Result<Vec<&Link>, Error> {
        self.get(id)?;
        let mut found: Vec<&Link> = self
            .links
            .values()
            .flatten()
            .filter(|l| &l.to == id)
            .collect();
        found.sort_by(|a, b| (a.kind, &a.from).cmp(&(b.kind, &b.from)));
        Ok(found)
    }

    /// The current confidence of `id`, derived from its links (CONTRACT D6): `Contradicted` if anything
    /// contradicts it; else `Corroborated` if some corroborating chunk has a different origin class *and* a
    /// different source; else `Unconfirmed`.
    ///
    /// # Errors
    /// `NotFound`.
    pub fn confidence(&self, id: &ChunkId) -> Result<Confidence, Error> {
        let me = self.get(id)?;
        let mut corroborated = false;
        for link in self.links.get(id).into_iter().flatten() {
            match link.kind {
                LinkKind::ContradictedBy => return Ok(Confidence::Contradicted),
                LinkKind::CorroboratedBy => {
                    let other = self.get(&link.to)?;
                    corroborated |= other.origin != me.origin && other.source != me.source;
                }
                _ => {}
            }
        }
        Ok(if corroborated {
            Confidence::Corroborated
        } else {
            Confidence::Unconfirmed
        })
    }
}
