// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant`: a durable knowledge store on the command line, and an MCP server over the same store.
//!
//! There is no setup step: the first command creates the data directory (see `sageant where`).

#![forbid(unsafe_code)]

mod app;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// A durable knowledge store: put text in, search it, link it, forget it; serve it to an AI over MCP.
#[derive(Debug, Parser)]
#[command(name = "sageant", version)]
struct Cli {
    /// Where the store lives. Defaults to your user data directory (see `sageant where`).
    #[arg(long, global = true, env = "SAGEANT_HOME", value_name = "DIR")]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Store text (use `-` to read it from standard input). Prints the new chunk's id.
    Put {
        /// The text, or `-` for standard input.
        text: String,
        /// The collection to put it in.
        #[arg(long, default_value = "notes")]
        collection: String,
        /// What kind of thing it is.
        #[arg(long, default_value = "note")]
        kind: String,
        /// Where it came from, in your words.
        #[arg(long, default_value = "cli")]
        source: String,
    },
    /// Show a chunk. An id prefix is enough if it is unique.
    Get {
        /// The chunk id or a unique prefix of it.
        id: String,
        /// A specific version instead of the current one.
        #[arg(long)]
        version: Option<u32>,
    },
    /// List chunks (archived ones only with --all).
    List {
        /// Only this collection.
        #[arg(long)]
        collection: Option<String>,
        /// Include archived chunks.
        #[arg(long)]
        all: bool,
    },
    /// Search the text of your chunks.
    Search {
        /// What to look for.
        query: String,
        /// Show at most this many.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Every version of a chunk.
    History {
        /// The chunk id or a unique prefix of it.
        id: String,
    },
    /// Links from and to a chunk.
    Links {
        /// The chunk id or a unique prefix of it.
        id: String,
    },
    /// Link one chunk to another.
    Link {
        /// The source chunk.
        from: String,
        /// PartOf, DerivedFrom, Cites, Related, CorroboratedBy or ContradictedBy.
        kind: String,
        /// The target chunk.
        to: String,
    },
    /// Keep a chunk but take it out of search and listings.
    Archive {
        /// The chunk id or a unique prefix of it.
        id: String,
        /// Why.
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Make a chunk unretrievable. A minimal tombstone is left; the storage backend may keep old pages until it is
    /// compacted, so this is "unretrievable", not "erased".
    Forget {
        /// The chunk id or a unique prefix of it.
        id: String,
        /// Why.
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Remove everything about a chunk, tombstone included. Needs you at a terminal to type a confirmation.
    Purge {
        /// The chunk id or a unique prefix of it.
        id: String,
    },
    /// Serve the store over MCP on standard input and output, for an AI client. Read-only unless you grant writes.
    Serve {
        /// Let the client write to this collection (repeatable).
        #[arg(long = "allow-write", value_name = "COLLECTION")]
        allow_write: Vec<String>,
    },
    /// Bring an archived chunk back into search and listings.
    Unarchive {
        /// The chunk id or a unique prefix of it.
        id: String,
        /// Why.
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Write every chunk version and link as JSON lines (to standard output, or --out FILE). Forgotten chunks are not
    /// in it.
    Export {
        /// Write to this file instead of standard output.
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Read a file made by `export`. Safe to repeat: what is already there is skipped. A bad file imports nothing.
    Import {
        /// The export file.
        file: PathBuf,
    },
    /// Put the text files of a folder in the store (never reading outside it: links are skipped).
    Add {
        /// The folder.
        folder: PathBuf,
        /// The collection to put them in.
        #[arg(long, default_value = "notes")]
        collection: String,
        /// What kind of thing they are.
        #[arg(long, default_value = "note")]
        kind: String,
        /// File extensions to take (repeat or comma-separate).
        #[arg(long = "ext", value_delimiter = ',', default_values_t = ["txt".to_owned(), "md".to_owned()])]
        extensions: Vec<String>,
        /// Skip files larger than this many bytes.
        #[arg(long, default_value_t = 1_048_576)]
        max_bytes: u64,
    },
    /// Rebuild the search index from the store.
    Reindex,
    /// Show where the store is and which storage backend PersistAnt chose for it, and why.
    Info,
    /// Print the data directory.
    Where,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match app::run(cli.home, cli.command.into()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sageant: {}", e.message);
            ExitCode::from(e.code)
        }
    }
}

impl From<Command> for app::Command {
    fn from(c: Command) -> Self {
        match c {
            Command::Put {
                text,
                collection,
                kind,
                source,
            } => Self::Put {
                text,
                collection,
                kind,
                source,
            },
            Command::Get { id, version } => Self::Get { id, version },
            Command::List { collection, all } => Self::List { collection, all },
            Command::Search { query, limit } => Self::Search { query, limit },
            Command::History { id } => Self::History { id },
            Command::Links { id } => Self::Links { id },
            Command::Link { from, kind, to } => Self::Link { from, kind, to },
            Command::Archive { id, reason } => Self::Archive { id, reason },
            Command::Forget { id, reason } => Self::Forget { id, reason },
            Command::Purge { id } => Self::Purge { id },
            Command::Serve { allow_write } => Self::Serve { allow_write },
            Command::Unarchive { id, reason } => Self::Unarchive { id, reason },
            Command::Export { out } => Self::Export { out },
            Command::Import { file } => Self::Import { file },
            Command::Add {
                folder,
                collection,
                kind,
                extensions,
                max_bytes,
            } => Self::Add {
                folder,
                collection,
                kind,
                extensions,
                max_bytes,
            },
            Command::Reindex => Self::Reindex,
            Command::Info => Self::Info,
            Command::Where => Self::Where,
        }
    }
}
