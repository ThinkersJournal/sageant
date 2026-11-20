// SPDX-License-Identifier: MIT OR Apache-2.0
//! Worth: a derived signal for how much a chunk is worth keeping (ROADMAP X1, docs/DESIGN-WORTH-AND-TRUST.md section 2).
//!
//! Worth is **derived**, never a field on a chunk: [`worth_of`] reads a store and computes an integer score in basis
//! points (0 to 10 000), so a chunk's content, hash and id never change because of it. It is distinct from
//! [`Confidence`](crate::Confidence) (the belief state) and from trust. The formula is `worth-v1`:
//!
//! | Signal | Weight | Cap |
//! |---|---|---|
//! | structural links (`part-of`, `derived-from`), in or out | 1500 each | 4000 |
//! | loose links (`cites`, `related`), in or out | 500 each | 2000 |
//! | recency: `3000 * H / (H + age)`, `H` the half-life | up to 3000 | 3000 |
//! | stability: `250 * (min(versions, 5) - 1)` | up to 1000 | 1000 |
//!
//! `age` is `now` minus the newest of the current version's creation time and the newest structural or loose link
//! touching the chunk, and is zero when `now` is earlier (clock skew). Evidence links (`corroborated-by`,
//! `contradicted-by`) count for nothing here, neither as neighbours nor as a touch: they belong to trust. The weights are
//! fixed (CireSnave-approved defaults, 2026-11-18); only the time unit is the caller's, through the half-life. There is
//! no read signal in this version: nothing here records or counts reads.

use crate::Error;
use crate::model::{ChunkId, LinkKind, Timestamp};
use crate::reader::Reader;

/// The id of the formula in this module. A change of formula or weights is a new id, never a silent edit.
pub const WORTH_MODEL: &str = "worth-v1";

const STRUCTURAL_BP: u64 = 1500;
const STRUCTURAL_CAP_BP: u64 = 4000;
const LOOSE_BP: u64 = 500;
const LOOSE_CAP_BP: u64 = 2000;
const RECENCY_MAX_BP: u64 = 3000;
const STABILITY_BP_PER_VERSION: u64 = 250;
const STABILITY_MAX_VERSIONS: u64 = 5;

/// The one tunable: how long, in the caller's time unit, until recency is halved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorthParams {
    half_life: u64,
}

impl WorthParams {
    /// A half-life in the caller's time unit. Zero is treated as one, so the formula never divides by zero.
    #[must_use]
    pub fn new(half_life: u64) -> Self {
        Self {
            half_life: half_life.max(1),
        }
    }

    /// The half-life in use (at least one).
    #[must_use]
    pub fn half_life(&self) -> u64 {
        self.half_life
    }
}

/// One signal's contribution to a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    pub(crate) name: &'static str,
    pub(crate) raw: u64,
    pub(crate) bp: u32,
}

impl Component {
    /// The signal's name: `structural_links`, `loose_links`, `recency` or `stability`.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }
    /// The raw measurement: a link count, an age in the caller's unit, or a version count.
    #[must_use]
    pub fn raw(&self) -> u64 {
        self.raw
    }
    /// What this signal adds to the score, in basis points.
    #[must_use]
    pub fn bp(&self) -> u32 {
        self.bp
    }
}

/// A derived worth: the score and the four components that sum to it, in a fixed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worth {
    score_bp: u32,
    components: [Component; 4],
    version: u32,
    half_life: u64,
}

impl Worth {
    /// The score in basis points, 0 to 10 000.
    #[must_use]
    pub fn score_bp(&self) -> u32 {
        self.score_bp
    }
    /// The components: structural links, loose links, recency, stability, in that order.
    #[must_use]
    pub fn components(&self) -> &[Component; 4] {
        &self.components
    }
    /// The version of the chunk this worth was computed for.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }
    /// The half-life, in the caller's time unit, it was computed with.
    #[must_use]
    pub fn half_life(&self) -> u64 {
        self.half_life
    }
    /// Rebuild a worth from stored parts. The caller (the decoder) has already checked the sum and the names.
    pub(crate) fn from_parts(
        score_bp: u32,
        components: [Component; 4],
        version: u32,
        half_life: u64,
    ) -> Self {
        Self {
            score_bp,
            components,
            version,
            half_life,
        }
    }
}

/// The four component names, in the order a [`Worth`] holds them.
pub(crate) const COMPONENT_NAMES: [&str; 4] =
    ["structural_links", "loose_links", "recency", "stability"];

/// A [`Worth`] filed against a chunk: the derived numbers and when they were computed, keyed by
/// `(chunk id, version, model id)`. Numbers only, never content; recomputable, so losing one loses only time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorthSnapshot {
    pub(crate) id: ChunkId,
    pub(crate) model_id: String,
    pub(crate) computed_at: Timestamp,
    pub(crate) worth: Worth,
}

impl WorthSnapshot {
    /// The snapshot of `worth` for `id` computed at `computed_at`, under the model [`WORTH_MODEL`]. Building one stores
    /// nothing; see [`WorthStore`].
    #[must_use]
    pub fn new(id: &ChunkId, worth: &Worth, computed_at: Timestamp) -> Self {
        Self {
            id: id.clone(),
            model_id: WORTH_MODEL.to_owned(),
            computed_at,
            worth: worth.clone(),
        }
    }
    /// The chunk it is about.
    #[must_use]
    pub fn id(&self) -> &ChunkId {
        &self.id
    }
    /// The chunk version it was computed for.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.worth.version
    }
    /// The formula that produced it, such as `worth-v1`.
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    /// When it was computed, in the caller's time.
    #[must_use]
    pub fn computed_at(&self) -> Timestamp {
        self.computed_at
    }
    /// The score and its components.
    #[must_use]
    pub fn worth(&self) -> &Worth {
        &self.worth
    }
}

/// Reading and writing worth snapshots. A snapshot is derived data: recording one is not an audited write, changes no
/// chunk, and is removed with the chunk by `forget` and `purge`.
pub trait WorthStore {
    /// File `worth` (computed by [`worth_of`]) as the snapshot for `id` at the version it was computed for, replacing
    /// any snapshot in that slot.
    ///
    /// # Errors
    /// `NotFound` or `Forgotten`; `Invalid("worth version")` when the worth is for a version the chunk has not
    /// reached; `Conflict` if the chunk changed while recording; `Backend` or `Corrupt`.
    fn record_worth(
        &mut self,
        id: &ChunkId,
        worth: &Worth,
        now: Timestamp,
    ) -> Result<WorthSnapshot, Error>;
    /// Every snapshot of `id`, ordered by version then model id.
    ///
    /// # Errors
    /// `NotFound` or `Forgotten`; `Backend` or `Corrupt`.
    fn worth_snapshots(&self, id: &ChunkId) -> Result<Vec<WorthSnapshot>, Error>;
}

/// Narrow a basis-point value that is at most 10 000 by construction.
fn bp32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// The worth of the current version of `id` at time `now`. Reads only; nothing is written and nothing about the chunk
/// changes.
///
/// # Errors
/// `NotFound` or `Forgotten` for an unknown or removed chunk, or whatever the reader reports.
pub fn worth_of(
    reader: &dyn Reader,
    id: &ChunkId,
    now: Timestamp,
    params: &WorthParams,
) -> Result<Worth, Error> {
    let current = reader.get(id)?;
    let versions = u64::from(current.version());
    let mut newest = current.created_at().0;
    let mut structural = 0u64;
    let mut loose = 0u64;
    let outgoing = reader.links_from(id)?;
    let incoming = reader.links_to(id)?;
    for link in outgoing.iter().chain(incoming.iter()) {
        match link.kind() {
            LinkKind::PartOf | LinkKind::DerivedFrom => structural += 1,
            LinkKind::Cites | LinkKind::Related => loose += 1,
            // Evidence belongs to trust: not a neighbour, not a touch.
            _ => continue,
        }
        newest = newest.max(link.created_at().0);
    }
    let age = now.0.saturating_sub(newest);
    let h = params.half_life();
    let structural_bp = (structural.saturating_mul(STRUCTURAL_BP)).min(STRUCTURAL_CAP_BP);
    let loose_bp = (loose.saturating_mul(LOOSE_BP)).min(LOOSE_CAP_BP);
    // `age + h` cannot overflow a u128; the quotient is at most RECENCY_MAX_BP because age >= 0.
    let recency_bp = u64::try_from(
        u128::from(RECENCY_MAX_BP) * u128::from(h) / (u128::from(h) + u128::from(age)),
    )
    .unwrap_or(0);
    let stability_bp = STABILITY_BP_PER_VERSION * (versions.min(STABILITY_MAX_VERSIONS) - 1);
    let components = [
        Component {
            name: "structural_links",
            raw: structural,
            bp: bp32(structural_bp),
        },
        Component {
            name: "loose_links",
            raw: loose,
            bp: bp32(loose_bp),
        },
        Component {
            name: "recency",
            raw: age,
            bp: bp32(recency_bp),
        },
        Component {
            name: "stability",
            raw: versions,
            bp: bp32(stability_bp),
        },
    ];
    let score_bp = components.iter().map(|c| c.bp).sum();
    Ok(Worth {
        score_bp,
        components,
        version: current.version(),
        half_life: h,
    })
}
