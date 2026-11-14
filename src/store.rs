// SPDX-License-Identifier: MIT OR Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use crate::Error;
use crate::lifecycle::{
    Action, AuditEvent, LinkTarget, PersonAuthority, Removed, SecretOverride, Tombstone,
};
use crate::model::{
    Chunk, ChunkId, ChunkRef, Confidence, Link, LinkKind, NewChunk, Origin, Status, Timestamp,
};
use crate::reader::{Filter, Reader};
use crate::screen::SecretScreen;
use crate::sha256::sha256_hex;

/// The in-memory reference store. Ordered maps only, so no hash order can reach any output (CONTRACT D16).
#[derive(Default)]
pub struct MemoryStore {
    screen: Option<Box<dyn SecretScreen>>,
    lineages: BTreeMap<ChunkId, Vec<Chunk>>,
    links: BTreeMap<ChunkId, Vec<Link>>,
    caps: BTreeMap<String, usize>,
    tombstones: BTreeMap<ChunkId, Tombstone>,
    review: BTreeSet<ChunkId>,
    audit: Vec<AuditEvent>,
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

    /// Screen `texts`. `Ok(Some(class))` means the screen flagged `class` and `allow` covers it: the caller records
    /// the override once the write has happened.
    fn run_screen(
        &mut self,
        texts: &[&str],
        allow: Option<&SecretOverride>,
        at: Timestamp,
    ) -> Result<Option<&'static str>, Error> {
        if allow.is_some_and(|a| a.by().is_empty()) {
            return Err(Error::Invalid("override"));
        }
        let mut hit = None;
        if let Some(screen) = &self.screen {
            hit = texts.iter().find_map(|t| screen.screen(t));
        }
        match hit {
            None => Ok(None),
            Some(class) if allow.is_some_and(|a| a.class() == class) => Ok(Some(class)),
            Some(class) => {
                self.record(at, Action::Refused, None, "", &format!("secret:{class}"));
                Err(Error::SecretRefused { class })
            }
        }
    }

    fn record(
        &mut self,
        at: Timestamp,
        action: Action,
        id: Option<&ChunkId>,
        actor: &str,
        reason: &str,
    ) {
        self.audit.push(AuditEvent {
            at,
            action,
            id: id.cloned(),
            actor: actor.to_owned(),
            reason: reason.to_owned(),
        });
    }

    fn record_override(
        &mut self,
        class: Option<&'static str>,
        allow: Option<&SecretOverride>,
        id: &ChunkId,
        at: Timestamp,
    ) {
        if let (Some(class), Some(allow)) = (class, allow) {
            let reason = format!("secret-override:{class}");
            self.record(at, Action::Overridden, Some(id), allow.by(), &reason);
        }
    }

    /// The lineage, or `Forgotten` / `NotFound` saying why it is not there.
    fn lineage(&self, id: &ChunkId) -> Result<&Vec<Chunk>, Error> {
        match self.lineages.get(id) {
            Some(v) => Ok(v),
            None if self.tombstones.contains_key(id) => Err(Error::Forgotten),
            None => Err(Error::NotFound),
        }
    }

    /// The audit log, oldest first. It never holds content.
    #[must_use]
    pub fn audit(&self) -> &[AuditEvent] {
        &self.audit
    }

    /// The tombstone of a forgotten lineage, if it is currently forgotten.
    #[must_use]
    pub fn tombstone(&self, id: &ChunkId) -> Option<&Tombstone> {
        self.tombstones.get(id)
    }

    /// Whether a link's target is still stored.
    #[must_use]
    pub fn link_target(&self, link: &Link) -> LinkTarget {
        if self.lineages.contains_key(&link.to) {
            LinkTarget::Present
        } else {
            LinkTarget::Removed
        }
    }

    /// Whether a chunk derived from something later forgotten or purged is flagged for review.
    ///
    /// # Errors
    /// `NotFound` or `Forgotten`.
    pub fn needs_review(&self, id: &ChunkId) -> Result<bool, Error> {
        self.lineage(id)?;
        Ok(self.review.contains(id))
    }

    /// Every string the store holds, for tests that prove content is gone. Not for production use.
    #[doc(hidden)]
    #[must_use]
    pub fn dump_all_text(&self) -> String {
        format!(
            "{:?}{:?}{:?}{:?}{:?}",
            self.lineages, self.links, self.tombstones, self.review, self.audit
        )
    }

    /// Mark a chunk archived: kept, out of retrieval. New versions stay archived.
    ///
    /// # Errors
    /// `NotFound`, `Forgotten`, or `BadState` if already archived.
    pub fn archive(
        &mut self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        at: Timestamp,
    ) -> Result<(), Error> {
        if self
            .lineage(id)?
            .last()
            .is_some_and(|c| c.status == Status::Archived)
        {
            return Err(Error::BadState("already archived"));
        }
        if let Some(v) = self.lineages.get_mut(id) {
            for c in v {
                c.status = Status::Archived;
            }
        }
        self.record(at, Action::Archive, Some(id), actor, reason);
        Ok(())
    }

    /// Remove a chunk's content (every version), its source, its hash and its outgoing links, leaving a minimal
    /// tombstone. Links pointing at it stay and read as removed; chunks derived from it are flagged for review.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten` if it is already forgotten.
    pub fn forget(
        &mut self,
        id: &ChunkId,
        actor: &str,
        reason: &str,
        keyed_hash: Option<&str>,
        at: Timestamp,
    ) -> Result<Removed, Error> {
        let removed = self.lineage(id)?;
        let tombstone = Tombstone {
            id: id.clone(),
            collection: removed
                .first()
                .map(|c| c.collection.clone())
                .unwrap_or_default(),
            versions_removed: u32::try_from(removed.len()).unwrap_or(u32::MAX),
            at,
            actor: actor.to_owned(),
            reason: reason.to_owned(),
            keyed_hash: keyed_hash.map(str::to_owned),
        };
        self.lineages.remove(id);
        self.links.remove(id);
        self.flag_derived_from(id);
        self.tombstones.insert(id.clone(), tombstone);
        self.record(at, Action::Forget, Some(id), actor, reason);
        Ok(Removed::Unretrievable)
    }

    fn flag_derived_from(&mut self, id: &ChunkId) {
        let derived: Vec<ChunkId> = self
            .links
            .values()
            .flatten()
            .filter(|l| l.kind == LinkKind::DerivedFrom && &l.to == id)
            .map(|l| l.from.clone())
            .collect();
        self.review.extend(derived);
    }

    /// Remove everything about a chunk, including its tombstone. Needs a [`PersonAuthority`].
    ///
    /// # Errors
    /// `NotFound` if there is neither a chunk nor a tombstone.
    pub fn purge(
        &mut self,
        id: &ChunkId,
        authority: &PersonAuthority,
        at: Timestamp,
    ) -> Result<(), Error> {
        if !self.lineages.contains_key(id) && !self.tombstones.contains_key(id) {
            return Err(Error::NotFound);
        }
        self.lineages.remove(id);
        self.links.remove(id);
        self.flag_derived_from(id);
        self.tombstones.remove(id);
        self.record(at, Action::Purge, Some(id), authority.how(), "");
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
        self.put_inner(new, None, at)
    }

    /// [`put`](Self::put), allowing text the screen flags as the class `allow` names. Audited.
    ///
    /// # Errors
    /// As `put`; `Invalid("override")` if `allow` names nobody; `SecretRefused` if the flagged class differs.
    pub fn put_overriding(
        &mut self,
        new: NewChunk,
        allow: &SecretOverride,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        self.put_inner(new, Some(allow), at)
    }

    fn put_inner(
        &mut self,
        new: NewChunk,
        allow: Option<&SecretOverride>,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        if new.collection.is_empty() {
            return Err(Error::Invalid("collection"));
        }
        if new.kind.is_empty() {
            return Err(Error::Invalid("kind"));
        }
        check_text(&new.content)?;
        self.check_cap(&new.collection, &new.content)?;
        let overridden = self.run_screen(&[&new.content, &new.source], allow, at)?;
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
        // The audit log names the origin class, never the free-text source (which may hold a URL or a name).
        let actor = format!("{:?}", chunk.origin);
        self.lineages.insert(id.clone(), vec![chunk]);
        self.tombstones.remove(&id);
        self.record(at, Action::Put, Some(&id), &actor, "");
        self.record_override(overridden, allow, &id, at);
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
        self.supersede_inner(id, content, origin, source, None, at)
    }

    /// [`supersede`](Self::supersede), allowing text the screen flags as the class `allow` names. Audited.
    ///
    /// # Errors
    /// As `supersede`; `Invalid("override")` if `allow` names nobody; `SecretRefused` if the class differs.
    pub fn supersede_overriding(
        &mut self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        allow: &SecretOverride,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        self.supersede_inner(id, content, origin, source, Some(allow), at)
    }

    fn supersede_inner(
        &mut self,
        id: &ChunkId,
        content: &str,
        origin: Origin,
        source: &str,
        allow: Option<&SecretOverride>,
        at: Timestamp,
    ) -> Result<ChunkRef, Error> {
        check_text(content)?;
        let current = self.lineage(id)?.last().ok_or(Error::NotFound)?;
        let collection = current.collection.clone();
        let kind = current.kind.clone();
        let version = current.version + 1;
        let status = current.status;
        let same_text = current.content_hash == sha256_hex(content.as_bytes());
        let current_version = current.version;
        self.check_cap(&collection, content)?;
        let overridden = self.run_screen(&[content, source], allow, at)?;
        if same_text {
            return Err(Error::Conflict {
                expected: current_version,
                found: current_version,
            });
        }
        let next = Chunk {
            id: id.clone(),
            version,
            collection,
            kind,
            content: content.to_owned(),
            content_hash: sha256_hex(content.as_bytes()),
            origin,
            source: source.to_owned(),
            confidence: Confidence::Unconfirmed,
            status, // a new version of an archived chunk stays archived
            created_at: at,
        };
        if let Some(v) = self.lineages.get_mut(id) {
            v.push(next);
        }
        self.record(at, Action::Supersede, Some(id), &format!("{origin:?}"), "");
        self.record_override(overridden, allow, id, at);
        Ok(ChunkRef {
            id: id.clone(),
            version,
        })
    }

    /// The current (latest) version, archived or not.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten` if only a tombstone remains.
    pub fn get(&self, id: &ChunkId) -> Result<&Chunk, Error> {
        self.lineage(id)?.last().ok_or(Error::NotFound)
    }

    /// A specific version, from 1.
    ///
    /// # Errors
    /// `NotFound` for an unknown id or version; `Forgotten`.
    pub fn get_version(&self, id: &ChunkId, version: u32) -> Result<&Chunk, Error> {
        let index = usize::try_from(version)
            .ok()
            .and_then(|v| v.checked_sub(1))
            .ok_or(Error::NotFound)?;
        self.lineage(id)?.get(index).ok_or(Error::NotFound)
    }

    /// Every version, oldest first.
    ///
    /// # Errors
    /// `NotFound`, or `Forgotten`.
    pub fn history(&self, id: &ChunkId) -> Result<&[Chunk], Error> {
        self.lineage(id).map(Vec::as_slice)
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
        self.record(at, Action::Link, Some(from), "", &format!("{kind:?}"));
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
            // Evidence that was forgotten or purged counts for nothing, either way.
            let Ok(other) = self.get(&link.to) else {
                continue;
            };
            match link.kind {
                LinkKind::ContradictedBy => return Ok(Confidence::Contradicted),
                LinkKind::CorroboratedBy => {
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

impl Reader for MemoryStore {
    fn get(&self, id: &ChunkId) -> Result<&Chunk, Error> {
        Self::get(self, id)
    }
    fn get_version(&self, id: &ChunkId, version: u32) -> Result<&Chunk, Error> {
        Self::get_version(self, id, version)
    }
    fn history(&self, id: &ChunkId) -> Result<&[Chunk], Error> {
        Self::history(self, id)
    }
    fn links_from(&self, id: &ChunkId) -> Result<Vec<&Link>, Error> {
        Self::links_from(self, id)
    }
    fn links_to(&self, id: &ChunkId) -> Result<Vec<&Link>, Error> {
        Self::links_to(self, id)
    }
    fn confidence(&self, id: &ChunkId) -> Result<Confidence, Error> {
        Self::confidence(self, id)
    }
    fn list(&self, filter: &Filter) -> Vec<ChunkRef> {
        let mut out = Vec::new();
        for (id, versions) in &self.lineages {
            let confidence = Self::confidence(self, id).unwrap_or(Confidence::Unconfirmed);
            let newest_first = versions.iter().rev();
            for (i, c) in newest_first.enumerate() {
                if i > 0 && !filter.include_superseded {
                    break;
                }
                if filter.matches(c, confidence) {
                    out.push(ChunkRef {
                        id: id.clone(),
                        version: c.version,
                    });
                }
            }
        }
        if let Some(n) = filter.limit {
            out.truncate(n);
        }
        out
    }
}
