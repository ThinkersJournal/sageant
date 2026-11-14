// SPDX-License-Identifier: MIT OR Apache-2.0
//! Stored records (CONTRACT D17): a canonical, versioned byte format for chunks and links.
//!
//! A record is the header line, then each field as `name byte_len\n` followed by exactly that many bytes and a
//! newline. Fields come in one fixed order, so `decode` then `encode` is the identity and a decoder can reject
//! anything non-canonical by comparing. Decoding verifies what it can: the content hash, that the text is one the
//! store would accept, and the id of a first version. A corrupt or forged record is `Error::Corrupt`, never trusted.

use crate::Error;
use crate::model::{Chunk, ChunkId, Confidence, Link, LinkKind, Origin, Status, Timestamp};
use crate::sha256::sha256_hex;
use crate::store::check_text;

const CHUNK_HEADER: &str = "sageant-chunk/1\n";
const LINK_HEADER: &str = "sageant-link/1\n";

fn put_field(out: &mut Vec<u8>, name: &str, value: &[u8]) {
    out.extend_from_slice(name.as_bytes());
    out.push(b' ');
    out.extend_from_slice(value.len().to_string().as_bytes());
    out.push(b'\n');
    out.extend_from_slice(value);
    out.push(b'\n');
}

fn origin_name(o: Origin) -> &'static str {
    match o {
        Origin::User => "User",
        Origin::Tool => "Tool",
        Origin::Web => "Web",
        Origin::Agent => "Agent",
        Origin::Model => "Model",
    }
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Active => "Active",
        Status::Archived => "Archived",
    }
}

fn kind_name(k: LinkKind) -> &'static str {
    match k {
        LinkKind::PartOf => "PartOf",
        LinkKind::DerivedFrom => "DerivedFrom",
        LinkKind::Cites => "Cites",
        LinkKind::Related => "Related",
        LinkKind::CorroboratedBy => "CorroboratedBy",
        LinkKind::ContradictedBy => "ContradictedBy",
    }
}

struct Cursor<'a> {
    rest: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn header(&mut self, header: &str) -> Result<(), Error> {
        self.rest = self
            .rest
            .strip_prefix(header.as_bytes())
            .ok_or(Error::Corrupt("header"))?;
        Ok(())
    }

    /// The next field, which must be called `name`.
    fn field(&mut self, name: &str) -> Result<&'a str, Error> {
        let end = self
            .rest
            .iter()
            .position(|&b| b == b'\n')
            .ok_or(Error::Corrupt("truncated"))?;
        let line = std::str::from_utf8(&self.rest[..end]).map_err(|_| Error::Corrupt("text"))?;
        let len: usize = line
            .strip_prefix(name)
            .and_then(|r| r.strip_prefix(' '))
            .and_then(|n| n.parse().ok())
            .ok_or(Error::Corrupt("field"))?;
        let body = &self.rest[end + 1..];
        let value = body.get(..len).ok_or(Error::Corrupt("truncated"))?;
        if body.get(len) != Some(&b'\n') {
            return Err(Error::Corrupt("truncated"));
        }
        self.rest = &body[len + 1..];
        std::str::from_utf8(value).map_err(|_| Error::Corrupt("text"))
    }

    fn finish(&self) -> Result<(), Error> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(Error::Corrupt("trailing bytes"))
        }
    }
}

fn number<T: core::str::FromStr>(text: &str) -> Result<T, Error> {
    text.parse().map_err(|_| Error::Corrupt("number"))
}

fn id(text: &str) -> Result<ChunkId, Error> {
    ChunkId::parse(text).map_err(|_| Error::Corrupt("id"))
}

impl Chunk {
    /// The canonical stored form of this version.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = CHUNK_HEADER.as_bytes().to_vec();
        put_field(&mut out, "id", self.id.as_str().as_bytes());
        put_field(&mut out, "version", self.version.to_string().as_bytes());
        put_field(&mut out, "collection", self.collection.as_bytes());
        put_field(&mut out, "kind", self.kind.as_bytes());
        put_field(&mut out, "content", self.content.as_bytes());
        put_field(&mut out, "content_hash", self.content_hash.as_bytes());
        put_field(&mut out, "origin", origin_name(self.origin).as_bytes());
        put_field(&mut out, "source", self.source.as_bytes());
        put_field(&mut out, "status", status_name(self.status).as_bytes());
        put_field(
            &mut out,
            "created_at",
            self.created_at.0.to_string().as_bytes(),
        );
        out
    }

    /// Read a stored version back, verifying it.
    ///
    /// # Errors
    /// `Corrupt` for anything that is not exactly what `encode` writes, or whose content hash, text or (for a first
    /// version) id does not check out.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut cur = Cursor { rest: bytes };
        cur.header(CHUNK_HEADER)?;
        let chunk_id = id(cur.field("id")?)?;
        let version: u32 = number(cur.field("version")?)?;
        let collection = cur.field("collection")?.to_owned();
        let kind = cur.field("kind")?.to_owned();
        let content = cur.field("content")?.to_owned();
        let content_hash = cur.field("content_hash")?.to_owned();
        let origin = match cur.field("origin")? {
            "User" => Origin::User,
            "Tool" => Origin::Tool,
            "Web" => Origin::Web,
            "Agent" => Origin::Agent,
            "Model" => Origin::Model,
            _ => return Err(Error::Corrupt("origin")),
        };
        let source = cur.field("source")?.to_owned();
        let status = match cur.field("status")? {
            "Active" => Status::Active,
            "Archived" => Status::Archived,
            _ => return Err(Error::Corrupt("status")),
        };
        let created_at = Timestamp(number(cur.field("created_at")?)?);
        cur.finish()?;
        if version == 0 || collection.is_empty() || kind.is_empty() {
            return Err(Error::Corrupt("fields"));
        }
        if check_text(&content).is_err() {
            return Err(Error::Corrupt("content"));
        }
        if sha256_hex(content.as_bytes()) != content_hash {
            return Err(Error::Corrupt("content hash"));
        }
        if version == 1 && ChunkId::derive(&collection, &content_hash) != chunk_id {
            return Err(Error::Corrupt("id"));
        }
        let chunk = Self {
            id: chunk_id,
            version,
            collection,
            kind,
            content,
            content_hash,
            origin,
            source,
            confidence: Confidence::Unconfirmed,
            status,
            created_at,
        };
        if chunk.encode() != bytes {
            return Err(Error::Corrupt("not canonical"));
        }
        Ok(chunk)
    }
}

impl Link {
    /// The canonical stored form of this link.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = LINK_HEADER.as_bytes().to_vec();
        put_field(&mut out, "from", self.from.as_str().as_bytes());
        put_field(
            &mut out,
            "from_version",
            self.from_version.to_string().as_bytes(),
        );
        put_field(&mut out, "kind", kind_name(self.kind).as_bytes());
        put_field(&mut out, "to", self.to.as_str().as_bytes());
        put_field(
            &mut out,
            "created_at",
            self.created_at.0.to_string().as_bytes(),
        );
        out
    }

    /// Read a stored link back, verifying it.
    ///
    /// # Errors
    /// `Corrupt` for anything that is not exactly what `encode` writes.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut cur = Cursor { rest: bytes };
        cur.header(LINK_HEADER)?;
        let from = id(cur.field("from")?)?;
        let from_version: u32 = number(cur.field("from_version")?)?;
        let kind = match cur.field("kind")? {
            "PartOf" => LinkKind::PartOf,
            "DerivedFrom" => LinkKind::DerivedFrom,
            "Cites" => LinkKind::Cites,
            "Related" => LinkKind::Related,
            "CorroboratedBy" => LinkKind::CorroboratedBy,
            "ContradictedBy" => LinkKind::ContradictedBy,
            _ => return Err(Error::Corrupt("kind")),
        };
        let to = id(cur.field("to")?)?;
        let created_at = Timestamp(number(cur.field("created_at")?)?);
        cur.finish()?;
        if from_version == 0 {
            return Err(Error::Corrupt("fields"));
        }
        let link = Self {
            from,
            from_version,
            kind,
            to,
            created_at,
        };
        if link.encode() != bytes {
            return Err(Error::Corrupt("not canonical"));
        }
        Ok(link)
    }
}
