// SPDX-License-Identifier: MIT OR Apache-2.0
//! What each command does. Failures are a message and an exit code: 1 for a failed request, 2 for a refused one.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use persistant::registry::Endpoint;
use rmcp::ServiceExt;
use sageant::{
    Actor, ChunkId, ChunkRef, Error, Filter, LinkKind, NewChunk, Origin, PersonAuthority, Query,
    Retriever, Status, Timestamp, Writer,
};
use sageant_lexical::{LexicalRetriever, TextIndex};
use sageant_mcp::{Grants, McpServer, Tools};
use sageant_persistant::{KvTextIndex, PersistStore};

/// A failed command.
#[derive(Debug)]
pub struct Failure {
    pub code: u8,
    pub message: String,
}

fn fail(message: impl Into<String>) -> Failure {
    Failure {
        code: 1,
        message: message.into(),
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        fail(match e {
            Error::NotFound => "no such chunk".to_owned(),
            Error::Forgotten => "that chunk was forgotten (it is unretrievable)".to_owned(),
            other => other.to_string(),
        })
    }
}

/// The commands, free of the argument parser.
#[derive(Debug)]
pub enum Command {
    Put {
        text: String,
        collection: String,
        kind: String,
        source: String,
    },
    Get {
        id: String,
        version: Option<u32>,
    },
    List {
        collection: Option<String>,
        all: bool,
    },
    Search {
        query: String,
        limit: usize,
    },
    History {
        id: String,
    },
    Links {
        id: String,
    },
    Link {
        from: String,
        kind: String,
        to: String,
    },
    Archive {
        id: String,
        reason: String,
    },
    Forget {
        id: String,
        reason: String,
    },
    Purge {
        id: String,
    },
    Serve {
        allow_write: Vec<String>,
    },
    Reindex,
    Where,
}

/// Where the store lives when `--home` and `SAGEANT_HOME` are not given.
fn default_home() -> Result<PathBuf, Failure> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").or_else(|| var("APPDATA"))
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|h| h.join(".local").join("share")))
    };
    base.map(|b| b.join("sageant"))
        .ok_or_else(|| fail("cannot find a data directory; pass --home <DIR> or set SAGEANT_HOME"))
}

fn now() -> Timestamp {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    Timestamp(ms)
}

fn snippet(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 60 {
        let cut: String = flat.chars().take(60).collect();
        format!("{cut}...")
    } else {
        flat
    }
}

struct App {
    home: PathBuf,
    store: PersistStore,
    retriever: LexicalRetriever<KvTextIndex>,
    actor: Actor,
}

impl App {
    fn open(home: &Path) -> Result<Self, Failure> {
        std::fs::create_dir_all(home)
            .map_err(|e| fail(format!("cannot create {}: {e}", home.display())))?;
        let store = PersistStore::open("sqlite", &Endpoint::Path(home.join("store.db")))?;
        let index = KvTextIndex::open("sqlite", &Endpoint::Path(home.join("index.db")))?;
        let who = ["USER", "USERNAME"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "user".to_owned());
        Ok(Self {
            home: home.to_owned(),
            store,
            retriever: LexicalRetriever::new(index),
            actor: Actor::new("cli", &who),
        })
    }

    /// A full id, or the one chunk an id prefix names.
    fn resolve(&self, text: &str) -> Result<ChunkId, Failure> {
        if text.len() == 64
            && let Ok(id) = ChunkId::parse(text)
        {
            return Ok(id);
        }
        let all = self.store.list(&Filter::new().include_archived())?;
        let mut hits: Vec<&ChunkRef> = all
            .iter()
            .filter(|r| r.id.as_str().starts_with(text))
            .collect();
        hits.dedup_by(|a, b| a.id == b.id);
        match hits.as_slice() {
            [] => Err(fail(format!("no chunk starts with {text:?}"))),
            [one] => Ok(one.id.clone()),
            _ => Err(fail(format!(
                "{text:?} matches more than one chunk; type more of the id"
            ))),
        }
    }

    fn reindex(&mut self) -> Result<usize, Failure> {
        let mut n = 0;
        for r in self
            .store
            .list(&Filter::new().include_archived().include_superseded())?
        {
            let chunk = self.store.get_version(&r.id, r.version)?;
            self.retriever.index_mut().index(&r, chunk.content())?;
            n += 1;
        }
        Ok(n)
    }
}

pub fn run(home: Option<PathBuf>, command: Command) -> Result<(), Failure> {
    let home = match home {
        Some(h) => h,
        None => default_home()?,
    };
    if matches!(command, Command::Where) {
        println!("{}", home.display());
        return Ok(());
    }
    let mut app = App::open(&home)?;
    let out = &mut std::io::stdout();
    match command {
        Command::Put {
            text,
            collection,
            kind,
            source,
        } => put(&mut app, text, &collection, &kind, &source),
        Command::Get { id, version } => get(&app, &id, version),
        Command::List { collection, all } => list(&app, collection.as_deref(), all),
        Command::Search { query, limit } => search(&mut app, &query, limit),
        Command::History { id } => history(&app, &id),
        Command::Links { id } => links(&app, &id),
        Command::Link { from, kind, to } => {
            let (from, to) = (app.resolve(&from)?, app.resolve(&to)?);
            let kind = LinkKind::parse(&kind).map_err(|_| {
                fail("unknown link kind; use PartOf, DerivedFrom, Cites, Related, CorroboratedBy or ContradictedBy")
            })?;
            let actor = app.actor.clone();
            Writer::link(&mut app.store, &from, kind, &to, &actor, now())?;
            Ok(())
        }
        Command::Archive { id, reason } => {
            let id = app.resolve(&id)?;
            let actor = app.actor.clone();
            app.store
                .set_status(&id, Status::Archived, &actor, &reason, now())?;
            Ok(())
        }
        Command::Forget { id, reason } => {
            let id = app.resolve(&id)?;
            let actor = app.actor.clone();
            Writer::forget(&mut app.store, &id, &actor, &reason, None, now())?;
            app.retriever.index_mut().remove(&id)?;
            writeln!(
                out,
                "forgotten: {id} is now unretrievable",
                id = id.as_str()
            )
            .ok();
            Ok(())
        }
        Command::Purge { id } => purge(&mut app, &id),
        Command::Serve { allow_write } => serve(app, &allow_write),
        Command::Reindex => {
            let n = app.reindex()?;
            writeln!(out, "indexed {n} chunk versions").ok();
            Ok(())
        }
        Command::Where => Ok(()),
    }
}

fn put(
    app: &mut App,
    text: String,
    collection: &str,
    kind: &str,
    source: &str,
) -> Result<(), Failure> {
    let text = if text == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| fail(format!("cannot read standard input: {e}")))?;
        s.trim_end().to_owned()
    } else {
        text
    };
    let actor = app.actor.clone();
    let r = Writer::put(
        &mut app.store,
        NewChunk::new(collection, kind, &text, Origin::User, source),
        &actor,
        now(),
    )?;
    app.retriever.index_mut().index(&r, &text)?;
    println!("{}", r.id.as_str());
    eprintln!("stored in {collection} (version {})", r.version);
    Ok(())
}

fn get(app: &App, id: &str, version: Option<u32>) -> Result<(), Failure> {
    let id = app.resolve(id)?;
    let chunk = match version {
        Some(v) => app.store.get_version(&id, v)?,
        None => app.store.get(&id)?,
    };
    let confidence = app.store.confidence(&id)?;
    println!("id: {}", chunk.id().as_str());
    println!("version: {}", chunk.version());
    println!("collection: {}", chunk.collection());
    println!("kind: {}", chunk.kind());
    println!("origin: {:?}", chunk.origin());
    println!("confidence: {confidence:?}");
    println!("status: {:?}", chunk.status());
    println!("source: {}", chunk.source());
    println!("created: {}", chunk.created_at().0);
    println!();
    println!("{}", chunk.content());
    Ok(())
}

fn list(app: &App, collection: Option<&str>, all: bool) -> Result<(), Failure> {
    let mut filter = Filter::new();
    if let Some(c) = collection {
        filter = filter.collection(c);
    }
    if all {
        filter = filter.include_archived();
    }
    for r in app.store.list(&filter)? {
        let chunk = app.store.get_version(&r.id, r.version)?;
        let mark = if chunk.status() == Status::Archived {
            " [archived]"
        } else {
            ""
        };
        println!(
            "{}  {}/{}  v{}  {}{mark}",
            r.id.as_str(),
            chunk.collection(),
            chunk.kind(),
            r.version,
            snippet(chunk.content())
        );
    }
    Ok(())
}

fn search(app: &mut App, query: &str, limit: usize) -> Result<(), Failure> {
    if app.retriever.index_mut().stats()?.docs == 0 && !app.store.list(&Filter::new())?.is_empty() {
        app.reindex()?;
    }
    for hit in app
        .retriever
        .retrieve(&Query::new(query).limit(limit), &app.store)?
    {
        let chunk = app
            .store
            .get_version(&hit.chunk().id, hit.chunk().version)?;
        println!(
            "{:>6}  {}  {}",
            hit.score().unwrap_or(0),
            hit.chunk().id.as_str(),
            snippet(chunk.content())
        );
    }
    Ok(())
}

fn history(app: &App, id: &str) -> Result<(), Failure> {
    let id = app.resolve(id)?;
    for chunk in app.store.history(&id)? {
        println!(
            "v{}  created {}  {}",
            chunk.version(),
            chunk.created_at().0,
            snippet(chunk.content())
        );
    }
    Ok(())
}

fn links(app: &App, id: &str) -> Result<(), Failure> {
    let id = app.resolve(id)?;
    for l in app.store.links_from(&id)? {
        println!(
            "{} --{}--> {}",
            l.from().as_str(),
            l.kind().name(),
            l.to().as_str()
        );
    }
    for l in app.store.links_to(&id)? {
        println!(
            "{} --{}--> {}",
            l.from().as_str(),
            l.kind().name(),
            l.to().as_str()
        );
    }
    Ok(())
}

fn purge(app: &mut App, id: &str) -> Result<(), Failure> {
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(Failure {
            code: 2,
            message:
                "purge needs a person at a terminal to type a confirmation; nothing was removed"
                    .to_owned(),
        });
    }
    let id = app.resolve(id)?;
    eprintln!(
        "This removes everything about {} for good, its tombstone included.",
        id.as_str()
    );
    eprint!("Type `{}` to confirm: ", confirmation_for(&id));
    std::io::stderr().flush().ok();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .map_err(|e| fail(format!("cannot read your answer: {e}")))?;
    purge_confirmed(app, &id, &line)
}

/// What a person must type to purge `id`.
fn confirmation_for(id: &ChunkId) -> String {
    format!("purge {}", &id.as_str()[..8])
}

/// Purge `id` if `typed` is exactly the confirmation; otherwise change nothing.
fn purge_confirmed(app: &mut App, id: &ChunkId, typed: &str) -> Result<(), Failure> {
    if typed.trim() != confirmation_for(id) {
        return Err(Failure {
            code: 2,
            message: "that is not the confirmation; nothing was removed".to_owned(),
        });
    }
    let how = format!("typed-confirmation:{}", app.actor.label());
    Writer::purge(
        &mut app.store,
        id,
        &PersonAuthority::from_person_action(&how),
        now(),
    )?;
    app.retriever.index_mut().remove(id)?;
    println!("purged {}", id.as_str());
    Ok(())
}

fn serve(app: App, allow_write: &[String]) -> Result<(), Failure> {
    let App {
        home,
        store,
        retriever: _,
        actor: _,
    } = app;
    let index = KvTextIndex::open("sqlite", &Endpoint::Path(home.join("index.db")))?;
    let tools = Tools::new(
        store,
        index,
        Grants::writable(allow_write),
        Actor::new("mcp", "stdio"),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| fail(format!("cannot start the runtime: {e}")))?;
    runtime.block_on(async {
        let running = McpServer::new(tools)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| fail(format!("cannot start the MCP server: {e}")))?;
        running
            .waiting()
            .await
            .map_err(|e| fail(format!("the MCP server stopped: {e}")))?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_one_chunk() -> (tempfile::TempDir, App, ChunkId) {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::open(dir.path()).unwrap();
        let actor = app.actor.clone();
        let r = Writer::put(
            &mut app.store,
            NewChunk::new("notes", "note", "remove me", Origin::User, "t"),
            &actor,
            Timestamp(1),
        )
        .unwrap();
        (dir, app, r.id)
    }

    #[test]
    fn a_wrong_confirmation_removes_nothing() {
        let (_dir, mut app, id) = app_with_one_chunk();
        for typed in ["", "purge", "yes", "purge 00000000", "PURGE abcdefgh"] {
            let e = purge_confirmed(&mut app, &id, typed).unwrap_err();
            assert_eq!(e.code, 2, "{typed:?}");
        }
        assert!(app.store.get(&id).is_ok());
    }

    #[test]
    fn the_exact_confirmation_purges_chunk_tombstone_and_index_entry() {
        let (_dir, mut app, id) = app_with_one_chunk();
        let actor = app.actor.clone();
        Writer::forget(&mut app.store, &id, &actor, "r", None, Timestamp(2)).unwrap();
        let typed = format!(
            "  {}
",
            confirmation_for(&id)
        );
        purge_confirmed(&mut app, &id, &typed).unwrap();
        assert!(app.store.tombstone(&id).unwrap().is_none());
        assert_eq!(app.store.get(&id).err(), Some(Error::NotFound));
        let log = app.store.audit().unwrap();
        let last = log.last().unwrap();
        assert_eq!(last.action(), sageant::Action::Purge);
        assert!(last.actor().starts_with("typed-confirmation:cli/"));
    }
}
