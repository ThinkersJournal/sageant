// SPDX-License-Identifier: MIT OR Apache-2.0
//! Promoter, Demoter and the plan-then-apply Maintainer (CONTRACT D12). The store offers the interfaces; the harness
//! decides when to call them. No model calls happen here: a harness that wants summaries supplies its own `Demoter`.

use crate::Error;
use crate::lifecycle::Actor;
use crate::model::{Chunk, ChunkId, ChunkRef, LinkKind, Origin, Status, Timestamp};
use crate::reader::{Filter, Reader};
use crate::retrieve::Hit;
use crate::writer::Writer;

/// A retriever with a working-memory target and no extra power: it reads, it cannot write.
pub trait Promoter {
    /// The hits to promote, given opaque caller context.
    ///
    /// # Errors
    /// Whatever the reader reports.
    fn promote(&self, ctx: &[u8], reader: &dyn Reader) -> Result<Vec<Hit>, Error>;
}

/// Writes a derived chunk for `item` (for example a summary) and never edits `item`. The harness reads the item
/// first and hands the demoter a copy plus the writer.
pub trait Demoter {
    /// Store the demoted form, linked `derived-from` the original, and return its reference.
    ///
    /// # Errors
    /// Whatever the writer refuses.
    fn demote(
        &self,
        item: &Chunk,
        actor: &Actor,
        at: Timestamp,
        writer: &mut dyn Writer,
    ) -> Result<ChunkRef, Error>;
}

/// What a maintenance step does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Change {
    /// Archive a lineage.
    Archive {
        /// The lineage.
        id: ChunkId,
    },
    /// Link two lineages.
    Link {
        /// Link source.
        from: ChunkId,
        /// Link kind.
        kind: LinkKind,
        /// Link target.
        to: ChunkId,
    },
    /// Append a new version.
    Supersede {
        /// The lineage.
        id: ChunkId,
        /// The new text.
        content: String,
        /// Where the new text came from.
        origin: Origin,
        /// Its source.
        source: String,
    },
}

/// Why a step is in the plan: a typed reason, logged with the write.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// Same content as another chunk.
    Duplicate {
        /// The chunk kept.
        of: ChunkId,
    },
    /// Older than the maintainer's cut-off.
    Expired,
    /// Several chunks merged into one.
    Consolidated {
        /// The chunks merged.
        from: Vec<ChunkId>,
    },
    /// Anything else, in the planner's words.
    Custom(String),
}

impl Why {
    /// A short, content-free label for the audit log.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Duplicate { .. } => "duplicate".to_owned(),
            Self::Expired => "expired".to_owned(),
            Self::Consolidated { .. } => "consolidated".to_owned(),
            Self::Custom(s) => s.clone(),
        }
    }
}

/// One planned change with its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    change: Change,
    why: Why,
}

impl Step {
    /// A step.
    #[must_use]
    pub fn new(change: Change, why: Why) -> Self {
        Self { change, why }
    }
    /// What it does.
    #[must_use]
    pub fn change(&self) -> &Change {
        &self.change
    }
    /// Why.
    #[must_use]
    pub fn why(&self) -> &Why {
        &self.why
    }
}

/// An ordered list of steps. It is data: it can be shown, logged, refused and unit-tested without a store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    steps: Vec<Step>,
}

impl Plan {
    /// A plan of `steps`, applied in order.
    #[must_use]
    pub fn new(steps: Vec<Step>) -> Self {
        Self { steps }
    }
    /// The steps.
    #[must_use]
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }
}

/// What applying a plan did. A failed step is reported, never hidden, and does not stop the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    applied: usize,
    failed: Vec<(usize, Error)>,
}

impl Applied {
    /// How many steps were written.
    #[must_use]
    pub fn applied(&self) -> usize {
        self.applied
    }
    /// The steps that failed, as (step index, error).
    #[must_use]
    pub fn failed(&self) -> &[(usize, Error)] {
        &self.failed
    }
}

/// Plans maintenance (dedup, consolidate, expire, re-embed) and applies a plan. A plan is read-only work over a
/// `Reader`; only `apply` writes, through a `Writer`, and every write is audited under the `Actor`.
pub trait Maintainer {
    /// What this maintainer would do. Reads only.
    ///
    /// # Errors
    /// Whatever the reader reports.
    fn plan(&self, reader: &dyn Reader) -> Result<Plan, Error>;

    /// Execute `plan` in order through `writer`, auditing each step under `actor`: an archive carries its `Why` label as the reason, a link or supersede is
    /// followed by a `note` carrying it.
    fn apply(&self, writer: &mut dyn Writer, plan: &Plan, actor: &Actor, at: Timestamp) -> Applied {
        let mut out = Applied {
            applied: 0,
            failed: Vec::new(),
        };
        for (index, step) in plan.steps.iter().enumerate() {
            let reason = step.why.label();
            let result = match &step.change {
                Change::Archive { id } => {
                    writer.set_status(id, Status::Archived, actor, &reason, at)
                }
                Change::Link { from, kind, to } => writer
                    .link(from, *kind, to, actor, at)
                    .and_then(|()| writer.note(from, actor, &reason, at)),
                Change::Supersede {
                    id,
                    content,
                    origin,
                    source,
                } => writer
                    .supersede(id, content, *origin, source, actor, at)
                    .and_then(|_| writer.note(id, actor, &reason, at)),
            };
            match result {
                Ok(()) => out.applied += 1,
                Err(e) => out.failed.push((index, e)),
            }
        }
        out
    }
}

/// A reference maintainer: plans to archive every active chunk whose current version was created before `before`.
#[derive(Debug, Clone, Copy)]
pub struct ExpireMaintainer {
    before: Timestamp,
}

impl ExpireMaintainer {
    /// Expire what was created before `before` (caller's time).
    #[must_use]
    pub fn new(before: Timestamp) -> Self {
        Self { before }
    }
}

impl Maintainer for ExpireMaintainer {
    fn plan(&self, reader: &dyn Reader) -> Result<Plan, Error> {
        // "Created before 0" is nothing; guard it so `created_until(0)` cannot match chunks created at 0.
        let Some(last) = self.before.0.checked_sub(1) else {
            return Ok(Plan::default());
        };
        let steps = reader
            .list(&Filter::new().created_until(Timestamp(last)))?
            .into_iter()
            .map(|c| Step::new(Change::Archive { id: c.id }, Why::Expired))
            .collect();
        Ok(Plan::new(steps))
    }
}
