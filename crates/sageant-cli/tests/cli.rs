// SPDX-License-Identifier: MIT OR Apache-2.0
//! The `sageant` command line. Written before the code. The first-run path is a release gate: a fresh install
//! must work with no setup step, so every test starts from an empty home directory.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

fn sageant(home: &std::path::Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sageant"));
    c.env("SAGEANT_HOME", home).stdin(Stdio::null());
    c
}

fn run(home: &std::path::Path, args: &[&str]) -> Output {
    sageant(home).args(args).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// `put` prints the new chunk's full id on stdout and nothing else there.
fn put(home: &std::path::Path, text: &str) -> String {
    let o = run(home, &["put", text]);
    assert!(o.status.success(), "{}", err(&o));
    let id = out(&o).trim().to_owned();
    assert_eq!(id.len(), 64, "{id:?}");
    id
}

#[test]
fn a_fresh_install_works_with_no_setup() {
    let home = tempfile::tempdir().unwrap();
    // No `init`: the very first command creates what it needs.
    let id = put(home.path(), "the borrow checker enforces ownership");
    put(home.path(), "tomatoes need sun");

    let got = run(home.path(), &["get", &id]);
    assert!(got.status.success(), "{}", err(&got));
    let text = out(&got);
    assert!(text.contains("the borrow checker enforces ownership"));
    assert!(
        text.contains("origin: User") && text.contains("collection: notes"),
        "{text}"
    );

    let list = out(&run(home.path(), &["list"]));
    assert_eq!(list.lines().count(), 2, "{list}");
    assert!(list.contains(&id));

    let found = out(&run(home.path(), &["search", "ownership"]));
    assert_eq!(found.lines().count(), 1, "{found}");
    assert!(found.contains(&id[..8]));
}

#[test]
fn an_id_prefix_is_enough_and_an_ambiguous_or_unknown_one_says_so() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "alpha");
    let short = &id[..10];
    assert!(out(&run(home.path(), &["get", short])).contains("alpha"));
    let none = run(home.path(), &["get", "ffffffff"]);
    assert_eq!(none.status.code(), Some(1));
    assert!(err(&none).contains("no chunk"), "{}", err(&none));
    // An empty prefix matches everything once there is more than one chunk.
    put(home.path(), "beta");
    let many = run(home.path(), &["get", ""]);
    assert_eq!(many.status.code(), Some(1));
    assert!(err(&many).contains("more than one"), "{}", err(&many));
}

#[test]
fn forget_makes_a_chunk_unretrievable_and_says_exactly_that() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "forget this sentence");
    let f = run(home.path(), &["forget", &id, "--reason", "asked"]);
    assert!(f.status.success(), "{}", err(&f));
    let said = format!("{}{}", out(&f), err(&f)).to_lowercase();
    assert!(
        said.contains("unretrievable") && !said.contains("erased"),
        "{said}"
    );
    let got = run(home.path(), &["get", &id]);
    assert_eq!(got.status.code(), Some(1));
    assert!(err(&got).contains("forgotten"), "{}", err(&got));
    assert!(
        out(&run(home.path(), &["search", "sentence"]))
            .trim()
            .is_empty()
    );
}

#[test]
fn purge_needs_a_person_at_a_terminal() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "keep me");
    // stdin is not a terminal here, so there is no person to confirm.
    let p = run(home.path(), &["purge", &id]);
    assert_eq!(p.status.code(), Some(2));
    assert!(err(&p).contains("terminal"), "{}", err(&p));
    assert!(
        out(&run(home.path(), &["get", &id])).contains("keep me"),
        "a refused purge changes nothing"
    );
}

#[test]
fn archive_hides_a_chunk_from_search_and_list_until_asked() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "archived words");
    assert!(run(home.path(), &["archive", &id]).status.success());
    assert!(
        out(&run(home.path(), &["search", "archived"]))
            .trim()
            .is_empty()
    );
    assert!(out(&run(home.path(), &["list"])).trim().is_empty());
    assert!(out(&run(home.path(), &["list", "--all"])).contains(&id));
}

#[test]
fn history_links_and_where_work() {
    let home = tempfile::tempdir().unwrap();
    let a = put(home.path(), "first");
    let b = put(home.path(), "second");
    assert!(
        run(home.path(), &["link", &a, "Related", &b])
            .status
            .success()
    );
    let l = out(&run(home.path(), &["links", &a]));
    assert!(l.contains("Related") && l.contains(&b), "{l}");
    let w = out(&run(home.path(), &["where"]));
    assert!(w.trim().starts_with(home.path().to_str().unwrap()), "{w}");
}

#[test]
fn help_lists_every_command_and_an_unknown_one_fails() {
    let home = tempfile::tempdir().unwrap();
    let help = out(&run(home.path(), &["--help"]));
    for c in [
        "put", "get", "list", "search", "history", "links", "link", "archive", "forget", "purge",
        "serve", "where",
    ] {
        assert!(help.contains(c), "help should mention {c}:\n{help}");
    }
    assert!(!run(home.path(), &["frobnicate"]).status.success());
}

/// A line-oriented JSON-RPC session with `sageant serve`.
struct Session {
    child: std::process::Child,
    lines: std::sync::mpsc::Receiver<String>,
    next: u64,
}

impl Session {
    fn start(home: &std::path::Path, extra: &[&str]) -> Self {
        let mut child = sageant(home)
            .arg("serve")
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        let mut s = Self {
            child,
            lines,
            next: 1,
        };
        let init = s.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}),
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        s.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        s
    }

    fn send(&mut self, v: &Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{v}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(20))
                .expect("a reply in time");
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == id {
                return v;
            }
        }
    }

    fn call(&mut self, tool: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": args}))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn serve_speaks_mcp_with_ten_tools_no_purge_and_writes_only_where_granted() {
    let home = tempfile::tempdir().unwrap();
    let mut s = Session::start(home.path(), &["--allow-write", "notes"]);
    let tools = s.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 10, "{names:?}");
    assert!(names.iter().all(|n| !n.contains("purge")));

    let put = s.call(
        "sageant_put",
        json!({"collection": "notes", "kind": "note", "content": "from an agent"}),
    );
    assert_ne!(put["result"]["isError"], json!(true), "{put}");
    let body: Value =
        serde_json::from_str(put["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let id = body["id"].as_str().unwrap().to_owned();

    // No grant for another collection.
    let denied = s.call(
        "sageant_put",
        json!({"collection": "private", "kind": "note", "content": "x"}),
    );
    assert_eq!(denied["result"]["isError"], json!(true), "{denied}");
    drop(s);

    // The agent's chunk is in the same store the command line reads, marked as an agent's.
    let got = out(&run(home.path(), &["get", &id]));
    assert!(
        got.contains("from an agent") && got.contains("origin: Agent"),
        "{got}"
    );
}

#[test]
fn serve_without_a_grant_is_read_only() {
    let home = tempfile::tempdir().unwrap();
    put(home.path(), "already here");
    let mut s = Session::start(home.path(), &[]);
    let denied = s.call(
        "sageant_put",
        json!({"collection": "notes", "kind": "note", "content": "x"}),
    );
    assert_eq!(denied["result"]["isError"], json!(true), "{denied}");
    let found = s.call("sageant_search", json!({"query": "already"}));
    let body: Value =
        serde_json::from_str(found["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        body["hits"].as_array().unwrap().len(),
        1,
        "the server sees what the command line stored"
    );
}
