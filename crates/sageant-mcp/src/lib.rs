// SPDX-License-Identifier: MIT OR Apache-2.0
//! An MCP server for the sageant knowledge store (CONTRACT D13, D14).
//!
//! The tool logic lives here as plain functions over any [`Reader`] and [`Writer`], so it is tested without a
//! transport; the MCP wiring is a thin layer on top.
//!
//! **Injection safety (D14).** Retrieved content is data. Every chunk is served in a fixed JSON envelope whose
//! `content` field holds the stored text as one JSON string; the text is never concatenated into prose, never
//! appears in any other field, and never appears in an error. The server never follows an instruction found in a
//! chunk and never expands links on its own.
//!
//! **Write grants (D13).** Reads are on. A write tool works only on a collection the server was started with a grant
//! for. `purge` is not a tool: it needs a person (the CLI, with a typed confirmation). A client writes as
//! `Origin::Agent`, never as the user.
//!
//! **Size.** Responses are capped by an estimated token budget (four bytes to the token); a cut is always reported
//! (`truncated`, and for a chunk `content_bytes`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod server;

pub use server::{McpServer, tool_definitions};

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use sageant::{
    Actor, Chunk, ChunkId, ChunkRef, Confidence, Error, Filter, Link, LinkKind, NewChunk, Origin,
    Query, Reader, Retriever, Status, Timestamp, Writer,
};
use sageant_lexical::{LexicalRetriever, TextIndex};
use serde::Deserialize;
use serde_json::{Value, json};

/// The ten tools, in the order the contract lists them. There is no purge.
#[must_use]
pub fn tool_names() -> [&'static str; 10] {
    [
        "sageant_get",
        "sageant_list",
        "sageant_search",
        "sageant_links",
        "sageant_history",
        "sageant_put",
        "sageant_supersede",
        "sageant_link",
        "sageant_archive",
        "sageant_forget",
    ]
}

/// Which collections a client may write to. Reads are always allowed.
#[derive(Debug, Clone, Default)]
pub struct Grants {
    write: BTreeSet<String>,
}

impl Grants {
    /// No write grant at all (the default).
    #[must_use]
    pub fn read_only() -> Self {
        Self::default()
    }

    /// A write grant for exactly the named collections.
    #[must_use]
    pub fn writable<I, T>(collections: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: AsRef<str>,
    {
        Self {
            write: collections
                .into_iter()
                .map(|c| c.as_ref().to_owned())
                .collect(),
        }
    }

    /// Whether writes to `collection` are allowed.
    #[must_use]
    pub fn allows(&self, collection: &str) -> bool {
        self.write.contains(collection)
    }
}

/// How big a response may be.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    max_tokens: usize,
}

impl Limits {
    /// A budget of `n` tokens, estimated at four bytes each.
    #[must_use]
    pub fn max_tokens(n: usize) -> Self {
        Self { max_tokens: n }
    }

    fn bytes(self) -> usize {
        self.max_tokens.saturating_mul(4)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::max_tokens(4000)
    }
}

/// Why a tool call failed. None of these carries stored content.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolError {
    /// No such tool.
    UnknownTool(String),
    /// A missing, mistyped or malformed argument.
    BadRequest(String),
    /// A write to a collection the server has no grant for.
    NotAuthorised {
        /// The collection.
        collection: String,
    },
    /// The store refused.
    Store(Error),
}

impl ToolError {
    /// A short stable code for the client.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownTool(_) => "unknown_tool",
            Self::BadRequest(_) => "bad_request",
            Self::NotAuthorised { .. } => "not_authorised",
            Self::Store(e) => match e {
                Error::NotFound => "not_found",
                Error::Forgotten => "forgotten",
                Error::Conflict { .. } => "conflict",
                Error::SecretRefused { .. } => "secret_refused",
                Error::TooLarge { .. } => "too_large",
                Error::NotNormalised { .. } => "not_normalised",
                Error::LinkCycle => "link_cycle",
                Error::DuplicateLink => "duplicate_link",
                Error::BadState(_) => "bad_state",
                Error::Invalid(_) => "invalid",
                Error::Corrupt(_) => "corrupt",
                Error::Backend(_) => "backend",
                _ => "error",
            },
        }
    }

    /// The error as the JSON a client receives.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let message = match self {
            Self::UnknownTool(n) => format!("unknown tool: {n}"),
            Self::BadRequest(m) => m.clone(),
            Self::NotAuthorised { collection } => {
                format!("no write grant for collection {collection}")
            }
            Self::Store(e) => e.to_string(),
        };
        json!({"error": {"code": self.code(), "message": message}})
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.to_json()["error"]["message"].as_str().unwrap_or("")
        )
    }
}

impl std::error::Error for ToolError {}

impl From<Error> for ToolError {
    fn from(e: Error) -> Self {
        Self::Store(e)
    }
}

fn args<T: for<'de> Deserialize<'de>>(v: Value) -> Result<T, ToolError> {
    serde_json::from_value(v).map_err(|e| ToolError::BadRequest(e.to_string()))
}

fn chunk_id(text: &str) -> Result<ChunkId, ToolError> {
    ChunkId::parse(text).map_err(|_| ToolError::BadRequest("id is not a chunk id".to_owned()))
}

fn origin_from(text: &str) -> Result<Origin, ToolError> {
    match text {
        "User" => Ok(Origin::User),
        "Tool" => Ok(Origin::Tool),
        "Web" => Ok(Origin::Web),
        "Agent" => Ok(Origin::Agent),
        "Model" => Ok(Origin::Model),
        _ => Err(ToolError::BadRequest("unknown origin".to_owned())),
    }
}

fn confidence_from(text: &str) -> Result<Confidence, ToolError> {
    match text {
        "Unconfirmed" => Ok(Confidence::Unconfirmed),
        "Corroborated" => Ok(Confidence::Corroborated),
        "Contradicted" => Ok(Confidence::Contradicted),
        _ => Err(ToolError::BadRequest("unknown confidence".to_owned())),
    }
}

fn status_name(s: Status) -> &'static str {
    match s {
        Status::Archived => "Archived",
        _ => "Active",
    }
}

/// The leading values whose serialised sizes (and one separating byte each) fit in `budget` bytes.
fn within_budget(values: Vec<Value>, budget: usize) -> Vec<Value> {
    let mut used = 0;
    let mut kept = Vec::new();
    for v in values {
        used += serde_json::to_string(&v).map_or(0, |s| s.len()) + 1;
        if used > budget {
            break;
        }
        kept.push(v);
    }
    kept
}

/// The longest prefix of `text` whose JSON encoding fits in `budget` bytes.
fn fit(text: &str, budget: usize) -> &str {
    if serde_json::to_string(text).map_or(usize::MAX, |s| s.len()) <= budget {
        return text;
    }
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .collect();
    let (mut lo, mut hi) = (0, bounds.len() - 1); // bounds[lo] fits, bounds[hi] does not
    while hi - lo > 1 {
        let mid = usize::midpoint(lo, hi);
        if serde_json::to_string(&text[..bounds[mid]]).map_or(usize::MAX, |s| s.len()) <= budget {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    &text[..bounds[lo]]
}

#[derive(Deserialize)]
struct GetArgs {
    id: String,
    version: Option<u32>,
}

#[derive(Deserialize)]
struct ListArgs {
    collection: Option<String>,
    kind: Option<String>,
    origin: Option<String>,
    confidence: Option<String>,
    #[serde(default)]
    include_archived: bool,
    #[serde(default)]
    include_superseded: bool,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    collection: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
struct PutArgs {
    collection: String,
    kind: String,
    content: String,
    source: Option<String>,
}

#[derive(Deserialize)]
struct SupersedeArgs {
    id: String,
    content: String,
    source: Option<String>,
}

#[derive(Deserialize)]
struct LinkArgs {
    from: String,
    kind: String,
    to: String,
}

#[derive(Deserialize)]
struct ReasonArgs {
    id: String,
    reason: Option<String>,
}

type Clock = Box<dyn Fn() -> Timestamp + Send + Sync>;

/// The ten tools over a store and a text index.
pub struct Tools<S, I> {
    store: Mutex<S>,
    retriever: Mutex<LexicalRetriever<I>>,
    grants: Grants,
    limits: Limits,
    actor: Actor,
    clock: Clock,
}

impl<S, I> std::fmt::Debug for Tools<S, I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tools")
            .field("grants", &self.grants)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<S, I> Tools<S, I>
where
    S: Reader + Writer,
    I: TextIndex,
{
    /// Tools over `store`, searching with `index`. Writes are attributed to `actor`.
    #[must_use]
    pub fn new(store: S, index: I, grants: Grants, actor: Actor) -> Self {
        Self {
            store: Mutex::new(store),
            retriever: Mutex::new(LexicalRetriever::new(index)),
            grants,
            limits: Limits::default(),
            actor,
            clock: Box::new(|| {
                let ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
                Timestamp(ms)
            }),
        }
    }

    /// Use these response limits.
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Use this clock for the time of every write (the default is the system clock in milliseconds).
    #[must_use]
    pub fn with_clock(mut self, clock: impl Fn() -> Timestamp + Send + Sync + 'static) -> Self {
        self.clock = Box::new(clock);
        self
    }

    /// Run `f` on the store, for inspection (an audit log, say) by the host that owns the server.
    pub fn with_store<R>(&self, f: impl FnOnce(&mut S) -> R) -> R {
        f(&mut lock(&self.store))
    }

    fn now(&self) -> Timestamp {
        (self.clock)()
    }

    fn gate(&self, collection: &str) -> Result<(), ToolError> {
        if self.grants.allows(collection) {
            Ok(())
        } else {
            Err(ToolError::NotAuthorised {
                collection: collection.to_owned(),
            })
        }
    }

    /// Run the named tool on JSON arguments.
    ///
    /// # Errors
    /// [`ToolError`]; its JSON never carries stored content.
    pub fn call(&self, tool: &str, arguments: Value) -> Result<Value, ToolError> {
        match tool {
            "sageant_get" => self.get(args(arguments)?),
            "sageant_list" => self.list(args(arguments)?),
            "sageant_search" => self.search(args(arguments)?),
            "sageant_links" => self.links(&args::<IdArgs>(arguments)?.id),
            "sageant_history" => self.history(&args::<IdArgs>(arguments)?.id),
            "sageant_put" => self.put(args(arguments)?),
            "sageant_supersede" => self.supersede(args(arguments)?),
            "sageant_link" => self.link(args(arguments)?),
            "sageant_archive" => self.archive(args(arguments)?),
            "sageant_forget" => self.forget(args(arguments)?),
            other => Err(ToolError::UnknownTool(other.to_owned())),
        }
    }

    /// The envelope for one chunk version, with its content cut to fit `budget` bytes of JSON.
    fn envelope(
        &self,
        store: &S,
        chunk: &Chunk,
        with_content: bool,
        budget: usize,
    ) -> Result<Value, ToolError> {
        let confidence = store.confidence(chunk.id())?;
        let mut v = json!({
            "id": chunk.id().as_str(),
            "version": chunk.version(),
            "collection": chunk.collection(),
            "kind": chunk.kind(),
            "origin": format!("{:?}", chunk.origin()),
            "confidence": format!("{confidence:?}"),
            "status": status_name(chunk.status()),
            "source": chunk.source(),
            "created_at": chunk.created_at().0,
        });
        if with_content {
            let overhead = serde_json::to_string(&v).map_or(0, |s| s.len()) + 64;
            let shown = fit(chunk.content(), budget.saturating_sub(overhead));
            v["content"] = Value::String(shown.to_owned());
            v["truncated"] = Value::Bool(shown.len() < chunk.content().len());
            v["content_bytes"] = json!(chunk.content().len());
        }
        Ok(v)
    }

    fn get(&self, a: GetArgs) -> Result<Value, ToolError> {
        let id = chunk_id(&a.id)?;
        let store = lock(&self.store);
        let chunk = match a.version {
            Some(v) => store.get_version(&id, v)?,
            None => store.get(&id)?,
        };
        self.envelope(&store, &chunk, true, self.limits.bytes())
    }

    fn list(&self, a: ListArgs) -> Result<Value, ToolError> {
        let mut filter = Filter::new();
        if let Some(c) = &a.collection {
            filter = filter.collection(c);
        }
        if let Some(k) = &a.kind {
            filter = filter.kind(k);
        }
        if let Some(o) = &a.origin {
            filter = filter.origin(origin_from(o)?);
        }
        if let Some(c) = &a.confidence {
            filter = filter.confidence(confidence_from(c)?);
        }
        if a.include_archived {
            filter = filter.include_archived();
        }
        if a.include_superseded {
            filter = filter.include_superseded();
        }
        if let Some(n) = a.limit {
            filter = filter.limit(n);
        }
        let store = lock(&self.store);
        let refs = store.list(&filter)?;
        let total = refs.len();
        let mut all = Vec::new();
        for r in &refs {
            let chunk = store.get_version(&r.id, r.version)?;
            all.push(self.envelope(&store, &chunk, false, 0)?);
        }
        let items = within_budget(all, self.limits.bytes());
        Ok(json!({"truncated": items.len() < total, "total": total, "items": items}))
    }

    fn search(&self, a: SearchArgs) -> Result<Value, ToolError> {
        let mut query = Query::new(&a.query);
        if let Some(c) = &a.collection {
            query = query.filter(Filter::new().collection(c));
        }
        if let Some(n) = a.limit {
            query = query.limit(n);
        }
        let store = lock(&self.store);
        let found = lock(&self.retriever).retrieve(&query, &*store)?;
        let total = found.len();
        let each = self.limits.bytes() / 2;
        let mut all = Vec::new();
        for h in &found {
            let chunk = store.get_version(&h.chunk().id, h.chunk().version)?;
            let mut v = self.envelope(&store, &chunk, true, each)?;
            v["score"] = json!(h.score());
            v["matched"] = match h.reason() {
                sageant::Reason::Lexical { terms } => json!(terms),
                _ => json!([]),
            };
            all.push(v);
        }
        let hits = within_budget(all, self.limits.bytes());
        Ok(json!({"truncated": hits.len() < total, "total": total, "hits": hits}))
    }

    fn link_json(l: &Link) -> Value {
        json!({
            "from": l.from().as_str(),
            "from_version": l.from_version(),
            "kind": l.kind().name(),
            "to": l.to().as_str(),
            "created_at": l.created_at().0,
        })
    }

    fn links(&self, id: &str) -> Result<Value, ToolError> {
        let id = chunk_id(id)?;
        let store = lock(&self.store);
        let from: Vec<Value> = store.links_from(&id)?.iter().map(Self::link_json).collect();
        let to: Vec<Value> = store.links_to(&id)?.iter().map(Self::link_json).collect();
        Ok(json!({"from": from, "to": to}))
    }

    fn history(&self, id: &str) -> Result<Value, ToolError> {
        let id = chunk_id(id)?;
        let store = lock(&self.store);
        let all = store.history(&id)?;
        let each = self.limits.bytes() / 2;
        let mut built = Vec::new();
        for chunk in &all {
            built.push(self.envelope(&store, chunk, true, each)?);
        }
        let versions = within_budget(built, self.limits.bytes());
        Ok(json!({"truncated": versions.len() < all.len(), "versions": versions}))
    }

    fn index_version(&self, store: &S, r: &ChunkRef) -> Result<(), ToolError> {
        let chunk = store.get_version(&r.id, r.version)?;
        lock(&self.retriever)
            .index_mut()
            .index(r, chunk.content())?;
        Ok(())
    }

    fn put(&self, a: PutArgs) -> Result<Value, ToolError> {
        self.gate(&a.collection)?;
        let source = a.source.as_deref().unwrap_or("mcp");
        let new = NewChunk::new(&a.collection, &a.kind, &a.content, Origin::Agent, source);
        let mut store = lock(&self.store);
        let r = store.put(new, &self.actor, self.now())?;
        self.index_version(&store, &r)?;
        Ok(json!({"id": r.id.as_str(), "version": r.version}))
    }

    fn supersede(&self, a: SupersedeArgs) -> Result<Value, ToolError> {
        let id = chunk_id(&a.id)?;
        let mut store = lock(&self.store);
        self.gate(store.get(&id)?.collection())?;
        let source = a.source.as_deref().unwrap_or("mcp");
        let r = store.supersede(
            &id,
            &a.content,
            Origin::Agent,
            source,
            &self.actor,
            self.now(),
        )?;
        self.index_version(&store, &r)?;
        Ok(json!({"id": r.id.as_str(), "version": r.version}))
    }

    fn link(&self, a: LinkArgs) -> Result<Value, ToolError> {
        let (from, to) = (chunk_id(&a.from)?, chunk_id(&a.to)?);
        let kind = LinkKind::parse(&a.kind)
            .map_err(|_| ToolError::BadRequest("unknown link kind".to_owned()))?;
        let mut store = lock(&self.store);
        self.gate(store.get(&from)?.collection())?;
        store.link(&from, kind, &to, &self.actor, self.now())?;
        Ok(json!({"linked": true}))
    }

    fn archive(&self, a: ReasonArgs) -> Result<Value, ToolError> {
        let id = chunk_id(&a.id)?;
        let mut store = lock(&self.store);
        self.gate(store.get(&id)?.collection())?;
        store.set_status(
            &id,
            Status::Archived,
            &self.actor,
            a.reason.as_deref().unwrap_or(""),
            self.now(),
        )?;
        Ok(json!({"status": "archived", "id": id.as_str()}))
    }

    fn forget(&self, a: ReasonArgs) -> Result<Value, ToolError> {
        let id = chunk_id(&a.id)?;
        let mut store = lock(&self.store);
        self.gate(store.get(&id)?.collection())?;
        store.forget(
            &id,
            &self.actor,
            a.reason.as_deref().unwrap_or(""),
            None,
            self.now(),
        )?;
        lock(&self.retriever).index_mut().remove(&id)?;
        Ok(json!({"status": "unretrievable", "id": id.as_str()}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_keeps_exactly_the_items_that_fit_with_a_separator_each() {
        // "aa" serialises to 4 bytes, plus one for the separator: 5 each.
        let v = || vec![json!("aa"), json!("aa"), json!("aa")];
        assert_eq!(within_budget(v(), 15).len(), 3);
        assert_eq!(within_budget(v(), 14).len(), 2);
        assert_eq!(within_budget(v(), 10).len(), 2);
        assert_eq!(within_budget(v(), 9).len(), 1);
        assert_eq!(within_budget(v(), 4).len(), 0);
        assert_eq!(within_budget(vec![], 100).len(), 0);
    }

    #[test]
    fn fit_returns_the_longest_prefix_whose_json_encoding_fits() {
        assert_eq!(fit("hello", 7), "hello"); // "hello" with its quotes is 7 bytes
        assert_eq!(fit("hello", 6), "hell");
        assert_eq!(fit("hello", 3), "h");
        assert_eq!(fit("hello", 2), "");
        assert_eq!(fit("hello", 0), "");
        // A character is never split: "é" is two bytes in UTF-8, so it needs 4 with the quotes.
        assert_eq!(fit("éé", 4), "é");
        assert_eq!(fit("éé", 3), "");
        // Escapes count: a quote is two bytes in JSON.
        assert_eq!(fit("a\"b", 6), "a\"b");
        assert_eq!(fit("a\"b", 5), "a\"");
        assert_eq!(fit("a\"b", 4), "a");
        assert_eq!(fit("", 2), "");
    }

    #[test]
    fn every_store_error_has_its_own_code_and_a_message() {
        let cases: [(Error, &str); 12] = [
            (Error::NotFound, "not_found"),
            (Error::Forgotten, "forgotten"),
            (
                Error::Conflict {
                    expected: 1,
                    found: 2,
                },
                "conflict",
            ),
            (Error::SecretRefused { class: "k" }, "secret_refused"),
            (
                Error::TooLarge {
                    measured: 2,
                    cap: 1,
                },
                "too_large",
            ),
            (Error::NotNormalised { at: 0 }, "not_normalised"),
            (Error::LinkCycle, "link_cycle"),
            (Error::DuplicateLink, "duplicate_link"),
            (Error::BadState("x"), "bad_state"),
            (Error::Invalid("x"), "invalid"),
            (Error::Corrupt("x"), "corrupt"),
            (Error::Backend("x".to_owned()), "backend"),
        ];
        for (e, code) in cases {
            let t = ToolError::Store(e);
            assert_eq!(t.code(), code);
            assert!(!t.to_string().is_empty(), "{code} has a message");
            assert_eq!(t.to_json()["error"]["code"], code);
        }
        assert_eq!(ToolError::BadRequest("why".into()).to_string(), "why");
        assert!(
            ToolError::UnknownTool("zz".into())
                .to_string()
                .contains("zz")
        );
        assert!(
            ToolError::NotAuthorised {
                collection: "c".into()
            }
            .to_string()
            .contains('c')
        );
    }

    #[test]
    fn names_parse_to_their_values_and_nothing_else_does() {
        for (n, o) in [
            ("User", Origin::User),
            ("Tool", Origin::Tool),
            ("Web", Origin::Web),
            ("Agent", Origin::Agent),
            ("Model", Origin::Model),
        ] {
            assert_eq!(origin_from(n).unwrap(), o);
        }
        assert!(origin_from("user").is_err());
        for (n, c) in [
            ("Unconfirmed", Confidence::Unconfirmed),
            ("Corroborated", Confidence::Corroborated),
            ("Contradicted", Confidence::Contradicted),
        ] {
            assert_eq!(confidence_from(n).unwrap(), c);
        }
        assert!(confidence_from("sure").is_err());
        assert_eq!(status_name(Status::Archived), "Archived");
        assert_eq!(status_name(Status::Active), "Active");
    }

    #[test]
    fn grants_and_debug_output_say_what_they_hold() {
        assert!(!Grants::read_only().allows("notes"));
        assert!(Grants::writable(["notes"]).allows("notes"));
        assert!(!Grants::writable(["notes"]).allows("other"));
        let t = Tools::new(
            sageant::MemoryStore::new(),
            sageant_lexical::MemoryIndex::default(),
            Grants::writable(["notes"]),
            Actor::new("a", "b"),
        );
        let shown = format!("{t:?}");
        assert!(
            shown.contains("Tools") && shown.contains("notes"),
            "{shown}"
        );
    }
}
