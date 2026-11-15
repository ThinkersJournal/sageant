// SPDX-License-Identifier: MIT OR Apache-2.0
//! The link rules as pure functions (CONTRACT D6, D7), shared by every store so none copies them.

use crate::Error;
use crate::model::{Chunk, ChunkId, Confidence, LinkKind};

/// The confidence of `me` given its outgoing links (CONTRACT D6): `Contradicted` if any contradicting evidence is
/// still stored; else `Corroborated` if some corroborating chunk has a different origin class *and* a different
/// source; else `Unconfirmed`.
///
/// Each item is a link's kind and the chunk it points to, or `None` when that chunk was forgotten or purged; such
/// removed evidence counts for nothing, either way. Order does not matter.
#[must_use]
pub fn derive_confidence(me: &Chunk, links: &[(LinkKind, Option<&Chunk>)]) -> Confidence {
    let mut corroborated = false;
    for (kind, target) in links {
        let Some(other) = target else {
            continue;
        };
        match kind {
            LinkKind::ContradictedBy => return Confidence::Contradicted,
            LinkKind::CorroboratedBy => {
                corroborated |= other.origin != me.origin && other.source != me.source;
            }
            _ => {}
        }
    }
    if corroborated {
        Confidence::Corroborated
    } else {
        Confidence::Unconfirmed
    }
}

/// Whether `goal` can be reached from `start` by following edges, where `next` lists the targets of one node's
/// edges of the kind being checked. A node reaches itself. Terminates on cyclic data. Used to refuse a link that
/// would close a cycle in `part-of` or `derived-from`.
///
/// # Errors
/// Whatever `next` returns.
pub fn closes_cycle(
    start: &ChunkId,
    goal: &ChunkId,
    next: &mut dyn FnMut(&ChunkId) -> Result<Vec<ChunkId>, Error>,
) -> Result<bool, Error> {
    let mut seen: Vec<ChunkId> = Vec::new();
    let mut stack = vec![start.clone()];
    while let Some(node) = stack.pop() {
        if &node == goal {
            return Ok(true);
        }
        if seen.contains(&node) {
            continue;
        }
        stack.extend(next(&node)?);
        seen.push(node);
    }
    Ok(false)
}
