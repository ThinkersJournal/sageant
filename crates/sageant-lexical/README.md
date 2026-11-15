# sageant-lexical

Lexical (word) search for [sageant](https://crates.io/crates/sageant): a `TextIndex` trait, an in-memory index, and
a BM25 retriever whose scoring uses integer arithmetic only, so a given corpus and query score identically on every
machine.

- `tokenize` and `term_counts`: lowercase, split on anything not alphanumeric.
- `TextIndex`: where postings live. `MemoryIndex` is the reference implementation; `sageant-persistant` has a durable
  one. Every method is fallible, so a backend can report failure.
- `LexicalRetriever`: BM25 (k1 1.2, b 0.75) as a `sageant::Retriever`. The index says what matches; the store you pass
  at query time says what is visible, so archived, superseded and forgotten chunks never surface.

```rust
use sageant::{MemoryStore, NewChunk, Origin, Query, Retriever, Timestamp};
use sageant_lexical::{LexicalRetriever, MemoryIndex, TextIndex};

let mut store = MemoryStore::new();
let mut index = MemoryIndex::default();
let text = "the borrow checker enforces ownership";
let chunk = store.put(NewChunk::new("notes", "note", text, Origin::User, "example"), Timestamp(1)).unwrap();
index.index(&chunk, text).unwrap();

let hits = LexicalRetriever::new(index).retrieve(&Query::new("ownership"), &store).unwrap();
assert_eq!(hits.len(), 1);
```

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT licence (`LICENSE-MIT`), at your
option.
