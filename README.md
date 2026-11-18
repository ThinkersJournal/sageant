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
- **forget** removes a chunk's content and leaves a minimal tombstone. `sageant info` says how your backend gets rid of
  the bytes, and this table is the single source for what each means (the other docs link here rather than restate it):

  | Backend (PersistAnt id) | Who has it | What `forget` does | Checked by |
  |---|---|---|---|
  | `sqlite-secure` | a new store | **erased from the store files** in one atomic step: freed bytes are zeroed, for the chunk's text and its search terms | tests that read every database file back after a forget (`crates/sageant-persistant/tests/erase.rs`, and a forget through a real MCP session in the CLI tests) |
  | `sqlite`, `sqlite-full` (WAL) | an older store, or one you name | **unretrievable, not erased**. The forget is one atomic batch that also overwrites the stored copy with PersistAnt's `BatchOp::DeleteErased`, which clears the text of a few chunks, but SQLite can leave stale fragments of a record in pages it split or rebalanced earlier, and forgetting eight or thirty chunks measurably leaves copies of some of them (PersistAnt records `kv.delete_erased` as off here for the same reason) | tests that read the files back at 1, 3, 8 and 30 chunks: zero left at 1 and 3, copies left at 8 and 30 (a test pins this, so if it ever stops being true the table is re-measured before it changes); with the overwrite switched off the text is left even at 3 (the control) |
  | any other backend that qualifies | only if you name it | **unretrievable only**: the freed bytes may stay in the file | not claimed |
  | `memory`, `fs` | cannot be used: PersistAnt records no atomic batches for them | n/a | n/a |

  Only `sqlite-secure` is claimed to erase, and that is checked at up to thirty chunks. None of them reaches copies your file system, snapshots, backups or an SSD's wear levelling keep.
- **purge** removes everything about a chunk, tombstone included. It needs you at a terminal to type a confirmation,
  and no MCP tool can do it.

A note on what is shared: `forget` and `purge` act on this store only. Anything you have exported, copied or shared outside it, and anything shared publicly, should be assumed to stay known; anything shared publicly should be assumed to remain among public knowledge forever. What has been communicated cannot be stopped from circulating, and sageant does not try to engineer around that. Today data leave a store through `export`, the commands that print it and the MCP tools you grant; sageant has no public sharing.

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
