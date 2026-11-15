# sageant-cli

The `sageant` command: a durable knowledge store for people and AI agents. You put text in; it keeps every version and
where it came from, finds it again, and lets you archive it, make it unretrievable (forget) or remove it for good
(purge). An AI client can read it, and write to the collections you allow, over MCP.

```
cargo install sageant-cli
sageant put "Owls hunt at night and see well in the dark"   # prints the new chunk's id
sageant search owls
sageant get 0a02                                            # an id prefix is enough if it is unique
```

There is no setup step: the first command creates the store in your user data directory (`sageant where` prints it;
set `SAGEANT_HOME` or pass `--home <DIR>` to move it). `sageant --help` lists the commands: `put get list search history
links link archive forget purge reindex info where serve`.

`purge` needs you at a terminal to type a confirmation. `forget` makes a chunk unretrievable and does not claim the bytes
are erased.

For an AI client: `sageant serve --allow-write notes` speaks MCP on standard input and output, read-only unless you name
collections it may write to.

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your
option.
