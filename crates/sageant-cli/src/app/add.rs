// SPDX-License-Identifier: MIT OR Apache-2.0
//! `sageant add FOLDER`: put the text files of a folder into the store.
//!
//! **It never reads outside the folder.** The walk is over real directory entries only: a symbolic link (to a file or
//! a directory) is skipped and counted, never followed, so a link inside the folder cannot lead anywhere else, and
//! nothing can loop. A Windows directory junction (not reported as a link by the platform's file type) is caught by the
//! second check: before a file is read, its fully resolved path must still lie inside the folder's resolved path. A
//! name like `..` cannot occur in a directory listing; the folder argument itself is whatever the person typed.

use std::path::{Path, PathBuf};

use sageant::{Error, NewChunk, Origin, Writer};
use sageant_lexical::TextIndex;

use super::{App, Failure, fail, now};

/// What `add` did.
#[derive(Debug, Default)]
pub struct Summary {
    pub added: usize,
    pub present: usize,
    pub links: usize,
    pub outside: usize,
    pub too_large: usize,
    pub unreadable: usize,
    pub other: usize,
}

/// Where to look and what to take.
pub struct Options {
    pub collection: String,
    pub kind: String,
    pub extensions: Vec<String>,
    pub max_bytes: u64,
}

fn wanted(path: &Path, extensions: &[String]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// The files under `root` that may be read, in a stable order, counting what was refused.
fn walk(
    root: &Path,
    canonical_root: &Path,
    summary: &mut Summary,
) -> Result<Vec<PathBuf>, Failure> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| fail(format!("cannot read {}: {e}", dir.display())))?;
        let mut names: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        names.sort();
        for path in names {
            // `symlink_metadata` does not follow: a link is seen as a link.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                summary.unreadable += 1;
                continue;
            };
            let kind = meta.file_type();
            if kind.is_symlink() {
                summary.links += 1;
            } else if kind.is_dir() {
                // A junction looks like a directory; its resolved path gives it away.
                match std::fs::canonicalize(&path) {
                    Ok(real) if real.starts_with(canonical_root) => pending.push(path),
                    _ => summary.outside += 1,
                }
            } else if kind.is_file() {
                files.push(path);
            } else {
                summary.other += 1;
            }
        }
    }
    files.sort();
    Ok(files)
}

pub fn add(app: &mut App, folder: &Path, options: &Options) -> Result<Summary, Failure> {
    if !folder.is_dir() {
        return Err(fail(format!("{} is not a folder", folder.display())));
    }
    let canonical_root = std::fs::canonicalize(folder)
        .map_err(|e| fail(format!("cannot resolve {}: {e}", folder.display())))?;
    let mut summary = Summary::default();
    let files = walk(folder, &canonical_root, &mut summary)?;
    let actor = app.actor.clone();
    for path in files {
        if !wanted(&path, &options.extensions) {
            summary.other += 1;
            continue;
        }
        // Re-check at the moment of reading: the file must still resolve to somewhere inside the folder.
        match std::fs::canonicalize(&path) {
            Ok(real) if real.starts_with(&canonical_root) => {}
            _ => {
                summary.outside += 1;
                continue;
            }
        }
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if size > options.max_bytes {
            summary.too_large += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            summary.unreadable += 1;
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            summary.unreadable += 1;
            continue;
        };
        if text.trim().is_empty() {
            summary.other += 1;
            continue;
        }
        let relative = path
            .strip_prefix(folder)
            .unwrap_or(&path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let new = NewChunk::new(
            &options.collection,
            &options.kind,
            &text,
            Origin::User,
            &relative,
        );
        match Writer::put(&mut app.store, new, &actor, now()) {
            Ok(r) => {
                app.retriever.index_mut().index(&r, &text)?;
                summary.added += 1;
            }
            Err(Error::Conflict { .. }) => summary.present += 1,
            Err(Error::NotNormalised { .. } | Error::TooLarge { .. } | Error::Invalid(_)) => {
                summary.unreadable += 1
            }
            Err(e) => return Err(Failure::from(e)),
        }
    }
    Ok(summary)
}
