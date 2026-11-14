// SPDX-License-Identifier: MIT OR Apache-2.0
//! Slice 10 of ROADMAP row 3: Promoter, Demoter and the plan-then-apply Maintainer (CONTRACT D12).
//! Written before the code. A plan is data: it can be shown, refused and tested without a store; applying it goes
//! through a `Writer`, so every decision is audited under an `Actor`.

use sageant::{
    Action, Actor, Change, Chunk, ChunkId, ChunkRef, Demoter, Error, ExpireMaintainer, Hit,
    LinkKind, Maintainer, MemoryStore, NewChunk, Origin, Plan, Promoter, Reader, Reason, Status,
    Step, Timestamp, Why, Writer,
};

fn actor() -> Actor {
    Actor::new("maintainer", "m-1")
}

fn put(s: &mut MemoryStore, text: &str, at: u64) -> ChunkId {
    s.put(
        NewChunk::new("n", "note", text, Origin::User, "cli"),
        Timestamp(at),
    )
    .unwrap()
    .id
}

#[test]
fn the_expire_maintainer_plans_archives_for_old_active_chunks_and_changes_nothing() {
    let mut s = MemoryStore::new();
    let old = put(&mut s, "old", 10);
    let _new = put(&mut s, "new", 100);
    let archived = put(&mut s, "already", 5);
    s.archive(&archived, "x", "r", Timestamp(6)).unwrap();
    let audit_before = s.audit().len();
    let plan = ExpireMaintainer::new(Timestamp(50)).plan(&s);
    assert_eq!(s.audit().len(), audit_before); // planning wrote nothing
    assert_eq!(plan.steps().len(), 1);
    assert_eq!(plan.steps()[0].change(), &Change::Archive { id: old });
    assert_eq!(plan.steps()[0].why(), &Why::Expired);
}

#[test]
fn applying_a_plan_writes_through_the_writer_and_audits_each_step_under_the_actor() {
    let mut s = MemoryStore::new();
    let old = put(&mut s, "old", 10);
    let plan = ExpireMaintainer::new(Timestamp(50)).plan(&s);
    let out = ExpireMaintainer::new(Timestamp(50)).apply(&mut s, &plan, &actor(), Timestamp(60));
    assert_eq!(out.applied(), 1);
    assert!(out.failed().is_empty());
    assert_eq!(s.get(&old).unwrap().status(), Status::Archived);
    let e = s.audit().last().unwrap();
    assert_eq!(e.action(), Action::Archive);
    assert_eq!(e.actor(), "maintainer/m-1");
    assert_eq!(e.reason(), "expired");
    // Re-planning finds nothing left to do.
    assert!(
        ExpireMaintainer::new(Timestamp(50))
            .plan(&s)
            .steps()
            .is_empty()
    );
}

#[test]
fn a_hand_built_plan_can_link_and_supersede_with_typed_reasons() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 1);
    let b = put(&mut s, "beta", 1);
    let plan = Plan::new(vec![
        Step::new(
            Change::Link {
                from: a.clone(),
                kind: LinkKind::Related,
                to: b.clone(),
            },
            Why::Duplicate { of: b.clone() },
        ),
        Step::new(
            Change::Supersede {
                id: a.clone(),
                content: "alpha, tidied".into(),
                origin: Origin::Model,
                source: "m".into(),
            },
            Why::Consolidated {
                from: vec![a.clone(), b.clone()],
            },
        ),
    ]);
    let out = ExpireMaintainer::new(Timestamp(0)).apply(&mut s, &plan, &actor(), Timestamp(2));
    assert_eq!((out.applied(), out.failed().len()), (2, 0));
    assert_eq!(s.get(&a).unwrap().version(), 2);
    assert_eq!(s.links_from(&a).unwrap().len(), 1);
    let notes: Vec<String> = s
        .audit()
        .iter()
        .filter(|e| e.action() == Action::Note)
        .map(|e| e.reason().to_owned())
        .collect();
    assert_eq!(
        notes,
        vec!["duplicate".to_owned(), "consolidated".to_owned()]
    );
    assert!(
        s.audit()
            .iter()
            .all(|e| e.actor() == "maintainer/m-1" || e.actor() == "User")
    );
}

#[test]
fn a_failing_step_is_reported_and_does_not_stop_the_rest() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 1);
    let ghost = MemoryStore::new()
        .put(
            NewChunk::new("q", "note", "ghost", Origin::User, "c"),
            Timestamp(1),
        )
        .unwrap()
        .id;
    let plan = Plan::new(vec![
        Step::new(Change::Archive { id: ghost }, Why::Expired),
        Step::new(Change::Archive { id: a.clone() }, Why::Expired),
    ]);
    let out = ExpireMaintainer::new(Timestamp(0)).apply(&mut s, &plan, &actor(), Timestamp(2));
    assert_eq!(out.applied(), 1);
    assert_eq!(out.failed().len(), 1);
    assert_eq!(out.failed()[0].0, 0); // the step index
    assert_eq!(out.failed()[0].1, Error::NotFound);
    assert_eq!(s.get(&a).unwrap().status(), Status::Archived);
}

#[test]
fn an_empty_actor_applies_nothing() {
    let mut s = MemoryStore::new();
    let a = put(&mut s, "alpha", 1);
    let plan = Plan::new(vec![Step::new(
        Change::Archive { id: a.clone() },
        Why::Expired,
    )]);
    let out =
        ExpireMaintainer::new(Timestamp(0)).apply(&mut s, &plan, &Actor::new("", ""), Timestamp(2));
    assert_eq!(out.applied(), 0);
    assert_eq!(s.get(&a).unwrap().status(), Status::Active);
}

struct Recent;
impl Promoter for Recent {
    fn promote(&self, ctx: &[u8], reader: &dyn Reader) -> Vec<Hit> {
        let _ = ctx;
        reader
            .list(&sageant::Filter::new())
            .into_iter()
            .map(|c| Hit::new(c, None, Reason::Recency))
            .collect()
    }
}

#[test]
fn a_promoter_is_a_retriever_with_no_write_power() {
    let mut s = MemoryStore::new();
    put(&mut s, "one", 1);
    let hits = Recent.promote(b"working memory", &s);
    assert_eq!(hits.len(), 1);
}

struct Summarise;
impl Demoter for Summarise {
    fn demote(
        &self,
        item: &Chunk,
        actor: &Actor,
        at: Timestamp,
        w: &mut dyn Writer,
    ) -> Result<ChunkRef, Error> {
        let summary = w.put(
            NewChunk::new(
                item.collection(),
                "summary",
                &format!("summary of {}", item.content()),
                Origin::Model,
                "summariser",
            ),
            actor,
            at,
        )?;
        w.link(&summary.id, LinkKind::DerivedFrom, item.id(), actor, at)?;
        Ok(summary)
    }
}

#[test]
fn a_demoter_writes_a_derived_chunk_and_never_edits_the_original() {
    let mut s = MemoryStore::new();
    let orig = put(&mut s, "long text", 1);
    // The harness reads the item first and hands the demoter a copy plus the writer.
    let item = s.get(&orig).unwrap().clone();
    let sum = Summarise
        .demote(&item, &actor(), Timestamp(2), &mut s)
        .unwrap();
    assert_eq!(s.get(&orig).unwrap().content(), "long text");
    assert_eq!(s.get(&orig).unwrap().version(), 1);
    assert_eq!(
        s.links_from(&sum.id).unwrap()[0].kind(),
        LinkKind::DerivedFrom
    );
    assert_eq!(s.links_from(&sum.id).unwrap()[0].to(), &orig);
}
