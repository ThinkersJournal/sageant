// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant export` and `sageant import`: a plain JSON-lines file of every version of every chunk and every link,
//! for backup and for moving a store. Forgotten chunks are gone and are not in it; neither are tombstones or the audit
//! log. Importing is idempotent: what is already there is skipped.
//!
//! Format: the first line is `{"sageant_export":1}`; then one JSON object per line, either
//! `{"type":"chunk","id","version","collection","kind","content","origin","source","status","created_at"}` (one per
//! version, oldest first) or `{"type":"link","from","from_version","kind","to","created_at"}`.
//! Provenance (origin, source, time) is carried as written, because it is your own data being restored.

use std::collections::BTreeMap;
use std::io::Write;

use sageant::{
    Chunk, ChunkId, Error, Filter, LinkKind, NewChunk, Origin, Status, Timestamp, Writer,
};
use sageant_lexical::TextIndex;
use serde_json::{Value, json};

use super::{App, Failure, fail};

const HEADER: &str = "{\"sageant_export\":1}";

fn origin_from(name: &str) -> Option<Origin> {
    Some(match name {
        "User" => Origin::User,
        "Tool" => Origin::Tool,
        "Web" => Origin::Web,
        "Agent" => Origin::Agent,
        "Model" => Origin::Model,
        _ => return None,
    })
}

/// Every line of the export, as text (the header first).
pub fn export(app: &App) -> Result<Vec<String>, Failure> {
    let mut refs = app
        .store
        .list(&Filter::new().include_archived().include_superseded())?;
    refs.sort_by(|a, b| (&a.id, a.version).cmp(&(&b.id, b.version)));
    let mut lines = vec![HEADER.to_owned()];
    let mut ids: Vec<ChunkId> = Vec::new();
    for r in &refs {
        let c = app.store.get_version(&r.id, r.version)?;
        if ids.last() != Some(&r.id) {
            ids.push(r.id.clone());
        }
        lines.push(
            json!({
                "type": "chunk", "id": c.id().as_str(), "version": c.version(),
                "collection": c.collection(), "kind": c.kind(), "content": c.content(),
                "origin": format!("{:?}", c.origin()), "source": c.source(),
                "status": format!("{:?}", c.status()), "created_at": c.created_at().0,
            })
            .to_string(),
        );
    }
    for id in &ids {
        for l in app.store.links_from(id)? {
            lines.push(
                json!({
                    "type": "link", "from": l.from().as_str(), "from_version": l.from_version(),
                    "kind": l.kind().name(), "to": l.to().as_str(), "created_at": l.created_at().0,
                })
                .to_string(),
            );
        }
    }
    Ok(lines)
}

struct ChunkLine {
    line: usize,
    id: ChunkId,
    version: u32,
    collection: String,
    kind: String,
    content: String,
    origin: Origin,
    source: String,
    archived: bool,
    at: Timestamp,
}

struct LinkLine {
    from: ChunkId,
    kind: LinkKind,
    to: ChunkId,
    at: Timestamp,
}

fn text(v: &Value, key: &str, n: usize) -> Result<String, Failure> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| fail(format!("line {n}: missing or non-text \"{key}\"")))
}

fn number(v: &Value, key: &str, n: usize) -> Result<u64, Failure> {
    v[key]
        .as_u64()
        .ok_or_else(|| fail(format!("line {n}: missing or non-number \"{key}\"")))
}

fn id_of(v: &Value, key: &str, n: usize) -> Result<ChunkId, Failure> {
    ChunkId::parse(&text(v, key, n)?)
        .map_err(|_| fail(format!("line {n}: \"{key}\" is not a chunk id")))
}

/// Read the whole file before changing anything: a bad file imports nothing.
fn parse(input: &str) -> Result<(Vec<ChunkLine>, Vec<LinkLine>), Failure> {
    let mut lines = input.lines().enumerate().map(|(i, l)| (i + 1, l.trim()));
    match lines.next() {
        Some((_, first))
            if serde_json::from_str::<Value>(first).is_ok_and(|v| v["sageant_export"] == 1) => {}
        _ => {
            return Err(fail(
                "this is not a sageant export (the first line must be {\"sageant_export\":1}); make one with `sageant export`",
            ));
        }
    }
    let (mut chunks, mut links) = (Vec::new(), Vec::new());
    for (n, line) in lines {
        if line.is_empty() {
            continue;
        }
        let v: Value =
            serde_json::from_str(line).map_err(|e| fail(format!("line {n}: not JSON: {e}")))?;
        match v["type"].as_str() {
            Some("chunk") => {
                let origin_name = text(&v, "origin", n)?;
                let item = ChunkLine {
                    line: n,
                    id: id_of(&v, "id", n)?,
                    version: u32::try_from(number(&v, "version", n)?)
                        .map_err(|_| fail(format!("line {n}: version out of range")))?,
                    collection: text(&v, "collection", n)?,
                    kind: text(&v, "kind", n)?,
                    content: text(&v, "content", n)?,
                    origin: origin_from(&origin_name)
                        .ok_or_else(|| fail(format!("line {n}: unknown origin {origin_name:?}")))?,
                    source: text(&v, "source", n)?,
                    archived: text(&v, "status", n)? == "Archived",
                    at: Timestamp(number(&v, "created_at", n)?),
                };
                if item.version == 1 {
                    // The id of a first version is derived from its content: refuse a record that lies about it.
                    let new = NewChunk::new(
                        &item.collection,
                        &item.kind,
                        &item.content,
                        item.origin,
                        &item.source,
                    );
                    let made = Chunk::first_version(new, item.at)
                        .map_err(|e| fail(format!("line {n}: {e}")))?;
                    if made.id() != &item.id {
                        return Err(fail(format!("line {n}: the id does not match the content")));
                    }
                }
                chunks.push(item);
            }
            Some("link") => {
                let kind_name = text(&v, "kind", n)?;
                links.push(LinkLine {
                    from: id_of(&v, "from", n)?,
                    kind: LinkKind::parse(&kind_name)
                        .map_err(|_| fail(format!("line {n}: unknown link kind {kind_name:?}")))?,
                    to: id_of(&v, "to", n)?,
                    at: Timestamp(number(&v, "created_at", n)?),
                });
            }
            _ => {
                return Err(fail(format!(
                    "line {n}: \"type\" must be \"chunk\" or \"link\""
                )));
            }
        }
    }
    Ok((chunks, links))
}

/// What an import did.
#[derive(Debug, Default)]
pub struct Summary {
    pub chunks: usize,
    pub versions: usize,
    pub links: usize,
    pub skipped: usize,
}

pub fn import(app: &mut App, input: &str) -> Result<Summary, Failure> {
    let (mut chunks, links) = parse(input)?;
    chunks.sort_by(|a, b| (&a.id, a.version, a.line).cmp(&(&b.id, b.version, b.line)));
    let actor = app.actor.clone();
    let mut done = Summary::default();
    let mut archived: BTreeMap<ChunkId, bool> = BTreeMap::new();
    for c in &chunks {
        archived.insert(c.id.clone(), c.archived);
        let result = if c.version == 1 {
            let new = NewChunk::new(&c.collection, &c.kind, &c.content, c.origin, &c.source);
            Writer::put(&mut app.store, new, &actor, c.at)
        } else {
            Writer::supersede(
                &mut app.store,
                &c.id,
                &c.content,
                c.origin,
                &c.source,
                &actor,
                c.at,
            )
        };
        match result {
            Ok(r) => {
                app.retriever.index_mut().index(&r, &c.content)?;
                done.versions += 1;
                if c.version == 1 {
                    done.chunks += 1;
                }
            }
            // Already there (same text), or an earlier version is missing: leave it alone.
            Err(Error::Conflict { .. } | Error::NotFound | Error::Forgotten) => done.skipped += 1,
            Err(e) => return Err(Failure::from(e)),
        }
    }
    for (id, is_archived) in archived {
        if is_archived {
            match Writer::set_status(
                &mut app.store,
                &id,
                Status::Archived,
                &actor,
                "import",
                Timestamp(0),
            ) {
                Ok(()) | Err(Error::BadState(_) | Error::NotFound | Error::Forgotten) => {}
                Err(e) => return Err(Failure::from(e)),
            }
        }
    }
    for l in &links {
        match Writer::link(&mut app.store, &l.from, l.kind, &l.to, &actor, l.at) {
            Ok(()) => done.links += 1,
            Err(Error::DuplicateLink | Error::NotFound | Error::Forgotten | Error::LinkCycle) => {
                done.skipped += 1
            }
            Err(e) => return Err(Failure::from(e)),
        }
    }
    Ok(done)
}

/// Write the export to `path`, or to `out` when there is none.
pub fn write_export(
    app: &App,
    path: Option<&std::path::Path>,
    out: &mut dyn Write,
) -> Result<usize, Failure> {
    let lines = export(app)?;
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    match path {
        Some(p) => std::fs::write(p, body)
            .map_err(|e| fail(format!("cannot write {}: {e}", p.display())))?,
        None => out
            .write_all(body.as_bytes())
            .map_err(|e| fail(format!("cannot write: {e}")))?,
    }
    Ok(lines.len() - 1)
}
