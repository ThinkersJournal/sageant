// SPDX-License-Identifier: MIT OR Apache-2.0
//! crates.io rejects a package at upload for metadata that `cargo publish --dry-run` does not check. 0.3.7 went out
//! for three of five crates because `sageant-mcp` had a 22-character keyword. This test holds the limits for every
//! package in the workspace: at most 5 keywords, each 1 to 20 characters, ASCII letters, digits, `-`, `_` or `+`,
//! starting with a letter or digit; categories from a known list; a description; a readme and licence files.
//! Written before the fix (it failed on `sageant-mcp`'s keyword).

use std::path::{Path, PathBuf};

/// Category slugs this project uses (all valid on crates.io; add one only after checking crates.io/category_slugs).
const CATEGORIES: &[&str] = &[
    "algorithms",
    "command-line-utilities",
    "data-structures",
    "database-implementations",
    "development-tools",
    "network-programming",
    "text-processing",
];

fn manifests() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out = vec![root.join("Cargo.toml")];
    if let Ok(dir) = std::fs::read_dir(root.join("crates")) {
        let mut crates: Vec<PathBuf> = dir
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("Cargo.toml"))
            .collect();
        crates.sort();
        out.extend(crates);
    }
    out
}

/// The string items of `key = [ ... ]`, on one line or across several (cargo rewrites the manifest it packages).
fn list(toml: &str, key: &str) -> Vec<String> {
    let start = toml
        .lines()
        .position(|l| l.trim_start().starts_with(&format!("{key} =")))
        .unwrap_or_else(|| panic!("no `{key}` line"));
    let mut text = String::new();
    for line in toml.lines().skip(start) {
        text.push_str(line);
        text.push('\n');
        if line.contains(']') {
            break;
        }
    }
    let inner = text
        .split_once('[')
        .and_then(|(_, r)| r.rsplit_once(']'))
        .map_or("", |(i, _)| i);
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn has(toml: &str, key: &str) -> bool {
    toml.lines().any(|l| {
        l.trim_start().starts_with(&format!("{key} ="))
            || l.trim_start().starts_with(&format!("{key}.workspace"))
    })
}

#[test]
fn every_package_has_metadata_crates_io_will_accept() {
    for manifest in manifests() {
        let name = manifest
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let toml = std::fs::read_to_string(&manifest).unwrap();
        let keywords = list(&toml, "keywords");
        assert!(
            !keywords.is_empty() && keywords.len() <= 5,
            "{name}: 1 to 5 keywords, got {keywords:?}"
        );
        for k in &keywords {
            assert!(
                k.len() <= 20,
                "{name}: keyword {k:?} is {} characters; crates.io allows 20",
                k.len()
            );
            let mut chars = k.chars();
            assert!(
                chars.next().is_some_and(|c| c.is_ascii_alphanumeric()),
                "{name}: keyword {k:?} must start with a letter or digit"
            );
            assert!(
                k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_+".contains(c)),
                "{name}: keyword {k:?} has a character crates.io refuses"
            );
        }
        let categories = list(&toml, "categories");
        assert!(
            !categories.is_empty() && categories.len() <= 5,
            "{name}: categories {categories:?}"
        );
        for c in &categories {
            assert!(
                CATEGORIES.contains(&c.as_str()),
                "{name}: category {c:?} is not in the checked list"
            );
        }
        for key in ["description", "license", "repository", "readme"] {
            assert!(has(&toml, key), "{name}: missing `{key}`");
        }
        assert!(
            !toml.contains("publish = false"),
            "{name}: still publish = false"
        );
        let dir = manifest.parent().unwrap();
        for file in ["README.md", "LICENSE-MIT", "LICENSE-APACHE"] {
            assert!(dir.join(file).exists(), "{name}: missing {file}");
        }
    }
}
