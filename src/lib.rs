// SPDX-License-Identifier: MIT OR Apache-2.0

//! sageant: the Knowledge Store. Pre-release; see `docs/CONTRACT.md` for the decided design.
//!
//! The core does no I/O, reads no clock and draws no randomness: time comes from the caller and ids
//! are derived from content (CONTRACT D1).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod lifecycle;
mod model;
mod screen;
mod sha256;
mod store;

pub use error::Error;
pub use lifecycle::{
    Action, AuditEvent, LinkTarget, PersonAuthority, Removed, SecretOverride, Tombstone,
};
pub use model::{
    Chunk, ChunkId, ChunkRef, Confidence, Link, LinkKind, NewChunk, Origin, Status, Timestamp,
};
#[cfg(feature = "redactant")]
pub use screen::RegisteredSecrets;
pub use screen::SecretScreen;
pub use sha256::sha256_hex;
pub use store::MemoryStore;
