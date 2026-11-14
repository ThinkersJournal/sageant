// SPDX-License-Identifier: MIT OR Apache-2.0

//! sageant: the Knowledge Store. Pre-release; see `docs/CONTRACT.md` for the decided design.
//!
//! The core does no I/O, reads no clock and draws no randomness: time comes from the caller and ids
//! are derived from content (CONTRACT D1).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod lifecycle;
mod maintain;
mod model;
#[cfg(feature = "nfc")]
mod nfc;
mod reader;
mod record;
mod retrieve;
mod screen;
mod sha256;
mod store;
mod writer;

pub use error::Error;
pub use lifecycle::{
    Action, Actor, AuditEvent, LinkTarget, PersonAuthority, Removed, SecretOverride, Tombstone,
};
pub use maintain::{
    Applied, Change, Demoter, ExpireMaintainer, Maintainer, Plan, Promoter, Step, Why,
};
pub use model::{
    Chunk, ChunkId, ChunkRef, Confidence, Link, LinkKind, NewChunk, Origin, Status, Timestamp,
};
#[cfg(feature = "nfc")]
pub use nfc::{is_nfc, normalise_nfc};
pub use reader::{Filter, Reader};
pub use retrieve::{DefaultFuser, Fuser, Hit, Query, Reason, Retriever, order_hits};
#[cfg(feature = "redactant")]
pub use screen::RegisteredSecrets;
pub use screen::SecretScreen;
pub use sha256::sha256_hex;
pub use store::MemoryStore;
pub use writer::Writer;
