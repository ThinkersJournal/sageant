# sageant-mcp

An MCP server for the [sageant](https://crates.io/crates/sageant) knowledge store.

- **Ten tools**: `sageant_get`, `_list`, `_search`, `_links`, `_history`, `_put`, `_supersede`, `_link`, `_archive`,
  `_forget`. There is no purge: removing everything about a chunk needs a person (see `sageant-cli`).
- **Write grants**: reads are on; a write works only on a collection you granted at start-up (`Grants::writable`). A
  client writes as origin "agent", attributed to the actor you configure.
- **Injection-safe envelope**: every chunk is a JSON object; the stored text is only ever in its `content` field, as
  data. Errors carry a code and never content.
- **Budgeted**: responses are capped by an estimated token budget and say when they were cut.

`Tools` is the logic over any `sageant::Reader + Writer` and `sageant_lexical::TextIndex`; `McpServer` wires it to MCP
with [rmcp](https://crates.io/crates/rmcp). Most people want the ready-made `sageant serve` in `sageant-cli`.

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your
option.
