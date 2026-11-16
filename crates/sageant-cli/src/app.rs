// SPDX-License-Identifier: MIT OR Apache-2.0
//! What each command does. Failures are a message and an exit code: 1 for a failed request, 2 for a refused one.

use std::io::{BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod add;
mod exchange;

use persistant::registry::Endpoint;
use rmcp::ServiceExt;
use sageant::{
    Actor, ChunkId, ChunkRef, Error, Filter, LinkKind, NewChunk, Origin, PersonAuthority, Query,
    Retriever, Status, Timestamp, Writer,
};
use sageant_lexical::{LexicalRetriever, TextIndex};
use sageant_mcp::{Grants, McpServer, Tools};
use sageant_persistant::{PersistStore, PersistTextIndex};

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
    Info,
    Where,
    Unarchive {
        id: String,
        reason: String,
    },
    Export {
        out: Option<PathBuf>,
    },
    Import {
        file: PathBuf,
    },
    Add {
        folder: PathBuf,
        collection: String,
        kind: String,
        extensions: Vec<String>,
        max_bytes: u64,
    },
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

/// What `forget` did, in the words its backend has earned (the README table is the single source): "erased" only where
/// the raw store files were read back in the tests and do not hold the text.
fn forget_message(id: &str, erasure: sageant_persistant::Erasure) -> String {
    use sageant_persistant::Erasure;
    match erasure {
        Erasure::Zeroed => format!(
            "forgotten: {id} is erased from the store files (its bytes were zeroed as they were freed). Copies your file system, snapshots or backups keep are not touched."
        ),
        _ => format!(
            "forgotten: {id} is now unretrievable, not erased: its stored copy was overwritten where the database allows it, but this database can leave stale fragments of it in the file. `sageant export`, then `sageant import` into a new home gives you a store that zeroes freed bytes."
        ),
    }
}

fn erasure_summary(erasure: sageant_persistant::Erasure) -> &'static str {
    use sageant_persistant::Erasure;
    match erasure {
        Erasure::Zeroed => "erased from the store files (freed bytes are zeroed)",
        _ => {
            "unretrievable only (the stored copy is overwritten where possible, but stale fragments can remain)"
        }
    }
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

/// The backend this home uses and the reasons PersistAnt gave when it was chosen (empty if recorded earlier).
///
/// PersistAnt picks the backend the first time, from what the store declares it needs; the id is written to
/// `<home>/backend` and used from then on, so a later change in PersistAnt's ranking can never make an existing
/// store be read by a different engine. A store made before this record existed (it has `store.db` and no `backend`
/// file) was SQLite, which is what is recorded for it.
fn backend_for(home: &Path) -> Result<(String, Vec<String>), Failure> {
    let record = home.join("backend");
    if let Ok(text) = std::fs::read_to_string(&record) {
        let id = text.trim().to_owned();
        if id.is_empty() {
            return Err(fail(format!(
                "{} is empty; delete it to have PersistAnt choose a backend again (your data is untouched)",
                record.display()
            )));
        }
        return Ok((id, Vec::new()));
    }
    let (id, why) = if home.join("store.db").exists() {
        (
            "sqlite".to_owned(),
            vec!["recorded for a store made before the backend was recorded".to_owned()],
        )
    } else {
        let (_, chosen) = PersistStore::open_chosen(&Endpoint::Path(home.join("store.db")))
            .map_err(|e| fail(format!("no storage backend is available: {e}")))?;
        (chosen.backend().to_owned(), chosen.why().to_vec())
    };
    std::fs::write(
        &record,
        format!(
            "{id}
"
        ),
    )
    .map_err(|e| fail(format!("cannot write {}: {e}", record.display())))?;
    Ok((id, why))
}

/// Delete a database file and the journal, WAL and shared-memory files SQLite keeps beside it.
fn remove_index_files(path: &Path) {
    let name = path.to_string_lossy().into_owned();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{name}{suffix}"));
    }
}

struct App {
    home: PathBuf,
    backend: String,
    store: PersistStore,
    retriever: LexicalRetriever<PersistTextIndex>,
    actor: Actor,
}

impl App {
    fn open(home: &Path) -> Result<Self, Failure> {
        std::fs::create_dir_all(home)
            .map_err(|e| fail(format!("cannot create {}: {e}", home.display())))?;
        let (backend, _) = backend_for(home)?;
        let wrong = |e: Error| {
            fail(format!(
                "cannot open the {backend:?} storage backend recorded in {}: {e} (delete that file to have PersistAnt choose again; your data is untouched)",
                home.join("backend").display()
            ))
        };
        let store =
            PersistStore::open(&backend, &Endpoint::Path(home.join("store.db"))).map_err(wrong)?;
        // The search index lives in `index2.db` (PersistAnt's text layer). The first open after the upgrade finds the old
        // `index.db` (or no index at all), rebuilds the index from the store, and only then deletes the old file: it is
        // derived data, and leaving it would keep the terms of chunks forgotten from now on.
        let old = home.join("index.db");
        let new = home.join("index2.db");
        let rebuild = old.exists() || !new.exists();
        if old.exists() {
            remove_index_files(&new);
        }
        let index = PersistTextIndex::open(&backend, &Endpoint::Path(new)).map_err(wrong)?;
        let who = ["USER", "USERNAME"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "user".to_owned());
        let mut app = Self {
            home: home.to_owned(),
            backend,
            store,
            retriever: LexicalRetriever::new(index),
            actor: Actor::new("cli", &who),
        };
        if rebuild {
            if old.exists() {
                eprintln!("sageant: rebuilding the search index in the new format (one time)");
            }
            app.reindex()?;
            remove_index_files(&old);
        }
        Ok(app)
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
            [] => {
                let n = all
                    .iter()
                    .map(|r| &r.id)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len();
                Err(fail(format!(
                    "no chunk starts with {text:?} ({n} chunks exist; `sageant list` shows them)"
                )))
            }
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
            let erasure = sageant_persistant::backend_erasure(&app.backend)
                .unwrap_or(sageant_persistant::Erasure::Unretrievable);
            writeln!(out, "{}", forget_message(id.as_str(), erasure)).ok();
            Ok(())
        }
        Command::Purge { id } => purge(&mut app, &id),
        Command::Serve { allow_write } => serve(app, &allow_write),
        Command::Reindex => {
            let n = app.reindex()?;
            writeln!(out, "indexed {n} chunk versions").ok();
            Ok(())
        }
        Command::Info => {
            let (backend, _) = backend_for(&app.home)?;
            writeln!(out, "data: {}", app.home.display()).ok();
            writeln!(out, "backend: {backend}").ok();
            let erasure = sageant_persistant::backend_erasure(&backend)
                .unwrap_or(sageant_persistant::Erasure::Unretrievable);
            writeln!(out, "forget: {}", erasure_summary(erasure)).ok();
            match sageant_persistant::explain_backend(&backend) {
                Ok(lines) => {
                    for line in lines {
                        writeln!(out, "  {line}").ok();
                    }
                }
                Err(e) => writeln!(out, "  warning: {e}").ok().unwrap_or(()),
            }
            Ok(())
        }
        Command::Unarchive { id, reason } => {
            let id = app.resolve(&id)?;
            let actor = app.actor.clone();
            Writer::set_status(&mut app.store, &id, Status::Active, &actor, &reason, now())?;
            Ok(())
        }
        Command::Export { out: path } => {
            let n = exchange::write_export(&app, path.as_deref(), out)?;
            eprintln!("exported {n} records");
            Ok(())
        }
        Command::Import { file } => {
            let input = std::fs::read_to_string(&file)
                .map_err(|e| fail(format!("cannot read {}: {e}", file.display())))?;
            let s = exchange::import(&mut app, &input)?;
            eprintln!(
                "imported {} chunks ({} versions), {} links; skipped {} already present or not applicable",
                s.chunks, s.versions, s.links, s.skipped
            );
            Ok(())
        }
        Command::Add {
            folder,
            collection,
            kind,
            extensions,
            max_bytes,
        } => {
            let options = add::Options {
                collection,
                kind,
                extensions,
                max_bytes,
            };
            let s = add::add(&mut app, &folder, &options)?;
            eprintln!(
                "added {} ({} already present); skipped: {} links, {} outside the folder, {} too large, {} unreadable, {} other files",
                s.added, s.present, s.links, s.outside, s.too_large, s.unreadable, s.other
            );
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
        backend,
        store,
        retriever: _,
        actor: _,
    } = app;
    // The recorded backend, not a hard-coded one: a forget through MCP must erase the index terms on a secure store too.
    let index = PersistTextIndex::open(&backend, &Endpoint::Path(home.join("index2.db")))?;
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
        for typed in [
            "",
            "purge",
            "yes",
            "xyzzy",
            "purge 00000000",
            "PURGE abcdefgh",
        ] {
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

    #[test]
    fn the_word_erased_is_used_only_for_backends_that_guarantee_it() {
        use sageant_persistant::Erasure;
        let said = forget_message("abcd", Erasure::Zeroed);
        assert!(
            said.contains("erased from the store files") && said.contains("backups"),
            "{said}"
        );
        assert!(erasure_summary(Erasure::Zeroed).starts_with("erased"));
        let said = forget_message("abcd", Erasure::Unretrievable);
        assert!(
            said.contains("unretrievable, not erased") && !said.contains("erased from"),
            "{said}"
        );
        assert!(erasure_summary(Erasure::Unretrievable).starts_with("unretrievable"));
    }
}
