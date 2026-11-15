# sageant

A durable knowledge store for people and AI agents. You put text in; it keeps every version, remembers where it came
from, finds it again with search, and lets you hide it (archive), make it unretrievable (forget) or remove it for good
(purge). An AI client can read it, and write to the collections you allow, over MCP.

## Status

Pre-1.0: the API is unstable (a breaking change moves the second version number).

## Install and first run

```
cargo install sageant-cli
```

That puts a `sageant` program on your path. There is no setup step; the first command creates the store.

```
sageant put "Owls hunt at night and see well in the dark"     # prints the new chunk's id
sageant search owls
sageant get 0a02                                              # an id prefix is enough if it is unique
sageant add ~/notes                                           # put a folder of .txt and .md files (never reads outside it)
sageant list
sageant where                                                 # where the store lives
```

The store is two SQLite files in your user data directory (`%LOCALAPPDATA%\sageant` on Windows,
`~/Library/Application Support/sageant` on macOS, `$XDG_DATA_HOME/sageant` or `~/.local/share/sageant` elsewhere).
Set `SAGEANT_HOME` or pass `--home <DIR>` to keep it somewhere else.

Other commands: `history`, `links`, `link`, `archive`, `unarchive`, `forget`, `purge`, `export`, `import`, `reindex`, `info` (which storage backend PersistAnt chose, and why), `serve`.

Back up with `sageant export --out backup.jsonl`; restore (or move to another machine) with `sageant import backup.jsonl`,
which is safe to repeat. Forgotten chunks are not in an export. `sageant --help` lists
them all.

## Removing things

- **archive** keeps a chunk but takes it out of search and listings (`sageant list --all` shows it).
- **forget** removes a chunk's content and leaves a minimal tombstone. What that means depends on the storage backend,
  and `sageant info` says which one you have (`secure delete: yes` or `no`):
  - **`sqlite-secure`** (what a new store gets, when PersistAnt offers it): the bytes are zeroed in the store files as
    they are freed, so the chunk's text and its search terms are *erased from the store files*. The tests read the raw files
    back after a forget to check it. This does not reach copies your file system, snapshots, backups or an SSD's wear
    levelling keep.
  - **`sqlite`** (a store made before this, or any backend without secure delete): the chunk is only *unretrievable*: the
    freed bytes may stay in the file until it is compacted. To move to a secure store, `sageant export`, then `sageant
    import` into a new home.
- **purge** removes everything about a chunk, tombstone included. It needs you at a terminal to type a confirmation,
  and no MCP tool can do it.

## Use it from an AI client (MCP)

`sageant serve` speaks MCP on standard input and output. It is read-only unless you name collections the client may
write to:

```json
{ "mcpServers": { "sageant": { "command": "sageant", "args": ["serve", "--allow-write", "notes"] } } }
```

The client gets ten tools (`sageant_get`, `_list`, `_search`, `_links`, `_history`, `_put`, `_supersede`, `_link`,
`_archive`, `_forget`) and no purge. Everything it reads comes back in a JSON envelope; the stored text is only ever in
the `content` field, as data. What a client writes is marked as written by an agent, never by you.

## The crates

| Crate | What it is |
|---|---|
| `sageant` | The model and rules: chunks, versions, links, confidence, deletion, an in-memory store |
| `sageant-lexical` | BM25 search with integer-only scoring, behind a `TextIndex` trait |
| `sageant-persistant` | A durable store and text index on PersistAnt key-value backends (SQLite) |
| `sageant-mcp` | The MCP server |
| `sageant-cli` | The `sageant` command |

`docs/CONTRACT.md` is the design; `docs/MUTATION.md` records the mutation testing.

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your
option.
