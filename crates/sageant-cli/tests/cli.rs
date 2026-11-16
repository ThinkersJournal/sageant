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
fn a_forgotten_chunk_is_unreadable_unsearchable_and_called_forgotten() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "forget this sentence");
    let f = run(home.path(), &["forget", &id, "--reason", "asked"]);
    assert!(f.status.success(), "{}", err(&f));
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
fn an_older_sqlite_home_makes_a_forgotten_chunk_unretrievable_and_says_it_is_not_erased() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "older home sentence to forget");
    // A store made before the backend record: plain SQLite, which overwrites the stored copy but can leave fragments.
    std::fs::write(home.path().join("backend"), "sqlite\n").unwrap();
    let info = out(&run(home.path(), &["info"]));
    assert!(info.contains("forget: unretrievable only"), "{info}");
    let f = run(home.path(), &["forget", &id]);
    assert!(f.status.success(), "{}", err(&f));
    let said = out(&f).to_lowercase();
    assert!(
        said.contains("unretrievable, not erased") && !said.contains("erased from"),
        "{said}"
    );
    let got = run(home.path(), &["get", &id]);
    assert_eq!(got.status.code(), Some(1));
    assert!(err(&got).contains("forgotten"), "{}", err(&got));
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
        "put",
        "get",
        "list",
        "search",
        "history",
        "links",
        "link",
        "archive",
        "forget",
        "purge",
        "add",
        "unarchive",
        "export",
        "import",
        "serve",
        "where",
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

#[test]
fn the_backend_persistant_chose_is_recorded_and_info_says_why() {
    let home = tempfile::tempdir().unwrap();
    put(home.path(), "recorded");
    let recorded = std::fs::read_to_string(home.path().join("backend")).unwrap();
    assert!(
        !recorded.trim().is_empty(),
        "the first run records the chosen backend id"
    );
    let info = out(&run(home.path(), &["info"]));
    assert!(
        info.contains(&format!("backend: {}", recorded.trim())),
        "{info}"
    );
    assert!(
        info.lines().count() > 2,
        "the reasons PersistAnt gave are listed:\n{info}"
    );
}

#[test]
fn a_store_from_before_the_record_keeps_working_and_gets_one() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "old store");
    std::fs::remove_file(home.path().join("backend")).unwrap();
    assert!(out(&run(home.path(), &["get", &id[..8]])).contains("old store"));
    assert!(home.path().join("backend").exists());
}

#[test]
fn an_unknown_recorded_backend_is_refused_without_touching_the_data() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "keep");
    std::fs::write(home.path().join("backend"), "no-such-backend\n").unwrap();
    let o = run(home.path(), &["get", &id[..8]]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("no-such-backend") && err(&o).contains("backend"),
        "{}",
        err(&o)
    );
    std::fs::remove_file(home.path().join("backend")).unwrap();
    assert!(
        out(&run(home.path(), &["get", &id[..8]])).contains("keep"),
        "the data was never touched"
    );
}

#[test]
fn a_bad_prefix_says_how_many_chunks_there_are() {
    let home = tempfile::tempdir().unwrap();
    put(home.path(), "one");
    put(home.path(), "two");
    let none = run(home.path(), &["get", "ffffffffff"]);
    assert!(err(&none).contains("2 chunks"), "{}", err(&none));
}

// ---- export, import, unarchive (row D) ----

fn export(home: &std::path::Path) -> String {
    let o = run(home, &["export"]);
    assert!(o.status.success(), "{}", err(&o));
    out(&o)
}

#[test]
fn unarchive_brings_a_chunk_back() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "come back");
    assert!(run(home.path(), &["archive", &id]).status.success());
    assert!(out(&run(home.path(), &["list"])).trim().is_empty());
    let u = run(home.path(), &["unarchive", &id[..8]]);
    assert!(u.status.success(), "{}", err(&u));
    assert!(out(&run(home.path(), &["list"])).contains(&id));
    assert!(out(&run(home.path(), &["search", "come"])).contains(&id[..8]));
}

#[test]
fn export_then_import_rebuilds_the_store_in_a_fresh_home() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let one = put(a.path(), "first note about owls");
    let two = put(a.path(), "second note about tomatoes");
    let three = put(a.path(), "archived note");
    assert!(
        run(a.path(), &["link", &one, "Related", &two])
            .status
            .success()
    );
    assert!(run(a.path(), &["archive", &three]).status.success());
    let dump = export(a.path());
    assert!(
        dump.lines().next().unwrap().contains("sageant_export"),
        "{dump}"
    );

    let file = a.path().join("dump.jsonl");
    std::fs::write(&file, &dump).unwrap();
    let imp = run(b.path(), &["import", file.to_str().unwrap()]);
    assert!(imp.status.success(), "{}", err(&imp));
    assert!(err(&imp).contains("3 chunks"), "{}", err(&imp));

    // The same ids, text, links and archive state; and search works (the index was rebuilt).
    assert!(out(&run(b.path(), &["get", &one[..8]])).contains("first note about owls"));
    assert!(out(&run(b.path(), &["links", &one])).contains(&two));
    assert!(out(&run(b.path(), &["list"])).contains(&two));
    assert!(
        !out(&run(b.path(), &["list"])).contains(&three),
        "the archived chunk stays archived"
    );
    assert!(out(&run(b.path(), &["list", "--all"])).contains(&three));
    assert_eq!(out(&run(b.path(), &["search", "owls"])).lines().count(), 1);
    // Exporting the copy gives the same dump.
    assert_eq!(export(b.path()), dump);
}

#[test]
fn every_version_of_a_chunk_travels_and_importing_twice_adds_nothing() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let id = put(a.path(), "version one");
    // A second version through the MCP-free path: supersede via the library is not on the CLI, so use history of a
    // chunk that was written twice with the same text is impossible; import a hand-written two-version export.
    let dump = format!(
        "{{\"sageant_export\":1}}\n{}",
        [
            serde_json::json!({"type":"chunk","id":id,"version":1,"collection":"notes","kind":"note","content":"version one","origin":"User","source":"cli","status":"Active","created_at":1}),
            serde_json::json!({"type":"chunk","id":id,"version":2,"collection":"notes","kind":"note","content":"version two","origin":"User","source":"cli","status":"Active","created_at":2}),
        ]
        .iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>()
    );
    let file = a.path().join("two.jsonl");
    std::fs::write(&file, dump).unwrap();
    assert!(
        run(b.path(), &["import", file.to_str().unwrap()])
            .status
            .success()
    );
    let h = out(&run(b.path(), &["history", &id]));
    assert!(
        h.contains("version one") && h.contains("version two"),
        "{h}"
    );
    let again = run(b.path(), &["import", file.to_str().unwrap()]);
    assert!(again.status.success(), "{}", err(&again));
    assert!(err(&again).contains("skipped"), "{}", err(&again));
    assert_eq!(out(&run(b.path(), &["history", &id])).lines().count(), 2);
}

#[test]
fn a_bad_import_file_is_refused_whole_and_names_the_line() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("bad.jsonl");
    std::fs::write(&file, "not json at all\n").unwrap();
    let o = run(home.path(), &["import", file.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("export"), "{}", err(&o));
    // A good header and a broken second record: nothing is imported.
    let other = tempfile::tempdir().unwrap();
    let real = put(other.path(), "x"); // the id a first version of "x" in notes really has
    let good = format!(
        "{{\"sageant_export\":1}}\n{}\n{{\"type\":\"chunk\"}}\n",
        serde_json::json!({"type":"chunk","id":real,"version":1,"collection":"notes","kind":"note","content":"x","origin":"User","source":"cli","status":"Active","created_at":1})
    );
    std::fs::write(&file, good).unwrap();
    let o = run(home.path(), &["import", file.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("line 3"), "{}", err(&o));
    assert!(
        out(&run(home.path(), &["list", "--all"])).trim().is_empty(),
        "nothing was imported"
    );
}

#[test]
fn a_forgotten_chunk_is_not_in_the_export() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "secret sentence to forget");
    put(home.path(), "keeper");
    assert!(run(home.path(), &["forget", &id]).status.success());
    let dump = export(home.path());
    assert!(
        !dump.contains("secret sentence") && !dump.contains(&id),
        "{dump}"
    );
    assert!(dump.contains("keeper"));
}

#[test]
fn an_import_record_whose_id_does_not_match_its_content_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join("lie.jsonl");
    let lie = format!(
        "{{\"sageant_export\":1}}
{}
",
        serde_json::json!({"type":"chunk","id":"ab".repeat(32),"version":1,"collection":"notes","kind":"note","content":"x","origin":"User","source":"cli","status":"Active","created_at":1})
    );
    std::fs::write(&file, lie).unwrap();
    let o = run(home.path(), &["import", file.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("does not match the content"),
        "{}",
        err(&o)
    );
}

// ---- `sageant add <folder>` (row D) ----

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn add_puts_every_text_file_in_a_folder_and_records_where_it_came_from() {
    let home = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("owls.txt"), "Owls hunt at night.");
    write(
        &notes.path().join("deep/er/garden.md"),
        "# Tomatoes\nneed sun",
    );
    write(&notes.path().join("image.png"), "not text we should read");
    write(&notes.path().join("empty.txt"), "");
    std::fs::write(notes.path().join("binary.txt"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let o = run(home.path(), &["add", notes.path().to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(err(&o).contains("added 2"), "{}", err(&o));
    assert_eq!(out(&run(home.path(), &["list"])).lines().count(), 2);
    assert_eq!(
        out(&run(home.path(), &["search", "owls"])).lines().count(),
        1
    );
    let id = out(&run(home.path(), &["search", "tomatoes"]))
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let shown = out(&run(home.path(), &["get", &id[..8]]));
    assert!(
        shown.contains("source: deep/er/garden.md"),
        "the relative path is the source:\n{shown}"
    );
    assert!(shown.contains("origin: User"), "{shown}");
}

#[test]
fn adding_the_same_folder_again_adds_nothing() {
    let home = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("a.txt"), "alpha");
    assert!(
        run(home.path(), &["add", notes.path().to_str().unwrap()])
            .status
            .success()
    );
    let again = run(home.path(), &["add", notes.path().to_str().unwrap()]);
    assert!(again.status.success());
    assert!(
        err(&again).contains("added 0") && err(&again).contains("already present"),
        "{}",
        err(&again)
    );
    assert_eq!(out(&run(home.path(), &["list"])).lines().count(), 1);
}

#[test]
fn add_refuses_a_missing_folder_and_a_file() {
    let home = tempfile::tempdir().unwrap();
    let none = run(
        home.path(),
        &["add", home.path().join("nope").to_str().unwrap()],
    );
    assert_eq!(none.status.code(), Some(1));
    assert!(err(&none).contains("folder"), "{}", err(&none));
    let file = home.path().join("f.txt");
    std::fs::write(&file, "x").unwrap();
    assert_eq!(
        run(home.path(), &["add", file.to_str().unwrap()])
            .status
            .code(),
        Some(1)
    );
}

/// Make `link` point at `target`: a symbolic link where the platform allows one, else (Windows without the
/// privilege) a directory junction, which needs none. Returns false only if neither could be made.
fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).is_ok()
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
            || Command::new("cmd")
                .args([
                    "/C",
                    "mklink",
                    "/J",
                    link.to_str().unwrap(),
                    target.to_str().unwrap(),
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
    }
}

#[test]
fn add_never_reads_outside_the_folder_through_a_link_to_another_directory() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(
        &outside.path().join("secret.txt"),
        "TOPSECRETMARKER outside the folder",
    );
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("inside.txt"), "an ordinary note");
    assert!(
        link_dir(outside.path(), &notes.path().join("escape")),
        "the test needs to be able to make a directory link (symlink or junction); it could not, so it proves nothing"
    );
    let o = run(home.path(), &["add", notes.path().to_str().unwrap()]);
    assert!(
        err(&o).contains("skipped: 1 links, 0 outside the folder"),
        "a directory link is skipped and counted as a link: {}",
        err(&o)
    );
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        out(&run(home.path(), &["search", "TOPSECRETMARKER"]))
            .trim()
            .is_empty(),
        "outside content was read"
    );
    assert_eq!(
        out(&run(home.path(), &["list"])).lines().count(),
        1,
        "only the file inside was added"
    );
    assert_eq!(
        out(&run(home.path(), &["search", "ordinary"]))
            .lines()
            .count(),
        1
    );
}

#[cfg(unix)]
#[test]
fn add_never_follows_a_symlink_to_a_file_outside_the_folder() {
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(
        &outside.path().join("secret.txt"),
        "TOPSECRETMARKER in a file outside",
    );
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("inside.txt"), "an ordinary note");
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        notes.path().join("link.txt"),
    )
    .unwrap();
    let o = run(home.path(), &["add", notes.path().to_str().unwrap()]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(
        out(&run(home.path(), &["search", "TOPSECRETMARKER"]))
            .trim()
            .is_empty()
    );
    assert!(
        err(&o).contains("link"),
        "links are skipped and said so: {}",
        err(&o)
    );
}

#[test]
fn add_skips_files_over_the_size_limit_and_says_so() {
    let home = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("small.txt"), "small");
    write(&notes.path().join("big.txt"), &"word ".repeat(300));
    let o = run(
        home.path(),
        &["add", notes.path().to_str().unwrap(), "--max-bytes", "1000"],
    );
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&run(home.path(), &["list"])).lines().count(), 1);
    assert!(err(&o).contains("too large"), "{}", err(&o));
}

#[test]
fn add_can_choose_the_collection_and_extensions() {
    let home = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("a.rs"), "fn main() {}");
    write(&notes.path().join("b.txt"), "plain");
    let o = run(
        home.path(),
        &[
            "add",
            notes.path().to_str().unwrap(),
            "--collection",
            "code",
            "--ext",
            "rs",
        ],
    );
    assert!(o.status.success(), "{}", err(&o));
    let list = out(&run(home.path(), &["list", "--collection", "code"]));
    assert_eq!(list.lines().count(), 1, "{list}");
    assert!(list.contains("code/"), "{list}");
}

#[test]
fn a_new_home_zeroes_freed_bytes_and_forget_says_what_that_does_and_does_not_cover() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "erase this sentence");
    let info = out(&run(home.path(), &["info"]));
    assert!(
        info.contains("forget: erased from the store files (freed bytes are zeroed)"),
        "{info}"
    );
    let f = run(home.path(), &["forget", &id]);
    assert!(f.status.success(), "{}", err(&f));
    let said = format!("{}{}", out(&f), err(&f)).to_lowercase();
    assert!(said.contains("erased from the store files"), "{said}");
    assert!(
        said.contains("backups") || said.contains("copies"),
        "the limits are stated: {said}"
    );
    // And the raw store files no longer hold the text.
    let mut all = Vec::new();
    for e in std::fs::read_dir(home.path()).unwrap() {
        all.extend(std::fs::read(e.unwrap().path()).unwrap_or_default());
    }
    assert!(
        !all.windows(19).any(|w| w == b"erase this sentence"),
        "the sentence is still in a file"
    );
}

#[test]
fn a_forget_through_mcp_erases_the_text_and_its_index_terms_from_the_files_too() {
    forget_through_mcp_on("sqlite-secure");
}

fn forget_through_mcp_on(backend: &str) {
    let home = tempfile::tempdir().unwrap();
    put(home.path(), "an ordinary note to create the store");
    std::fs::write(
        home.path().join("backend"),
        format!(
            "{backend}
"
        ),
    )
    .unwrap();
    let mut s = Session::start(home.path(), &["--allow-write", "notes"]);
    let text = "zqxmcpmarker unique words for the agent to write";
    let put = s.call(
        "sageant_put",
        json!({"collection": "notes", "kind": "note", "content": text}),
    );
    let body: Value =
        serde_json::from_str(put["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let id = body["id"].as_str().unwrap().to_owned();
    let raw = || {
        let mut all = Vec::new();
        for e in std::fs::read_dir(home.path()).unwrap() {
            all.extend(std::fs::read(e.unwrap().path()).unwrap_or_default());
        }
        all
    };
    let has = |bytes: &[u8]| bytes.windows(12).any(|w| w == b"zqxmcpmarker");
    assert!(
        has(&raw()),
        "{backend}: control: the marker is in the files before the forget"
    );
    let f = s.call("sageant_forget", json!({"id": id, "reason": "test"}));
    assert_ne!(f["result"]["isError"], json!(true), "{f}");
    drop(s);
    assert!(
        !has(&raw()),
        "{backend}: the text or its index terms are still in a store file after a forget through MCP"
    );
}

// ---- kill tests for the row 4 / D mutation run (docs/MUTATION.md, run 4) ----
// These were written after the run named the surviving mutants, so there was no red state; the recorded rerun of just
// those mutants is the evidence they are now caught.

#[test]
fn the_default_home_comes_from_the_platform_data_directory_when_nothing_is_set() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().to_str().unwrap().to_owned();
    let o = Command::new(env!("CARGO_BIN_EXE_sageant"))
        .arg("where")
        .env_remove("SAGEANT_HOME")
        .env("HOME", &base)
        .env("XDG_DATA_HOME", &base)
        .env("LOCALAPPDATA", &base)
        .env("APPDATA", &base)
        .env("USERPROFILE", &base)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let shown = out(&o);
    let shown = shown.trim();
    assert!(shown.starts_with(&base), "{shown} should be under {base}");
    assert!(shown.ends_with("sageant"), "{shown}");
    // With the variables empty there is nowhere to put it, and the error says how to choose a home.
    let none = Command::new(env!("CARGO_BIN_EXE_sageant"))
        .arg("where")
        .env_remove("SAGEANT_HOME")
        .env("HOME", "")
        .env("XDG_DATA_HOME", "")
        .env("LOCALAPPDATA", "")
        .env("APPDATA", "")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(none.status.code(), Some(1));
    assert!(err(&none).contains("SAGEANT_HOME"), "{}", err(&none));
}

#[test]
fn a_listing_cuts_a_long_text_after_sixty_characters_and_only_then() {
    let home = tempfile::tempdir().unwrap();
    let sixty = "a".repeat(60);
    let sixty_one = format!("{sixty}b");
    let id60 = put(home.path(), &sixty);
    let id61 = put(home.path(), &sixty_one);
    let list = out(&run(home.path(), &["list"]));
    let text_of = |id: &str| {
        list.lines()
            .find(|l| l.starts_with(id))
            .and_then(|l| l.split("v1  ").last())
            .unwrap_or("")
            .to_owned()
    };
    assert_eq!(
        text_of(&id60),
        sixty,
        "sixty characters are shown whole:
{list}"
    );
    assert_eq!(
        text_of(&id61),
        format!("{sixty}..."),
        "sixty-one are cut:
{list}"
    );
}

#[test]
fn an_old_format_index_is_rebuilt_once_and_the_old_file_is_deleted() {
    let home = tempfile::tempdir().unwrap();
    put(home.path(), "owls hunt at night");
    put(home.path(), "owls and moles");
    // Stand in for a home written by 0.3.19 and earlier: an `index.db` beside the store and no `index2.db`.
    std::fs::remove_file(home.path().join("index2.db")).unwrap();
    std::fs::write(home.path().join("index.db"), b"old format, never opened").unwrap();
    let r = run(home.path(), &["search", "owls"]);
    assert!(r.status.success(), "{}", err(&r));
    assert_eq!(out(&r).lines().count(), 2, "{}", out(&r));
    assert!(
        err(&r).contains("rebuilding the search index"),
        "{}",
        err(&r)
    );
    assert!(
        !home.path().join("index.db").exists(),
        "the old index must be gone"
    );
    assert!(home.path().join("index2.db").exists());
    // The second open does not rebuild or say anything.
    let again = run(home.path(), &["search", "owls"]);
    assert_eq!(out(&again), out(&r));
    assert!(!err(&again).contains("rebuilding"), "{}", err(&again));
}

#[test]
fn a_chunk_forgotten_after_the_upgrade_leaves_no_old_index_behind_to_keep_its_terms() {
    let home = tempfile::tempdir().unwrap();
    let id = put(home.path(), "zebrafinch secret sentence");
    std::fs::remove_file(home.path().join("index2.db")).unwrap();
    std::fs::write(home.path().join("index.db"), b"old").unwrap();
    assert!(run(home.path(), &["forget", &id[..8]]).status.success());
    assert!(!home.path().join("index.db").exists());
    let files: Vec<u8> = std::fs::read_dir(home.path())
        .unwrap()
        .flat_map(|e| std::fs::read(e.unwrap().path()).unwrap_or_default())
        .collect();
    assert!(
        !files.windows(9).any(|w| w == b"zebrafinc"),
        "the forgotten text is still in a file under the home"
    );
}

#[test]
fn reindex_reports_how_many_versions_it_indexed_and_search_heals_a_missing_index() {
    let home = tempfile::tempdir().unwrap();
    put(home.path(), "one searchable note");
    put(home.path(), "two searchable notes");
    put(home.path(), "three searchable notes");
    let r = run(home.path(), &["reindex"]);
    assert!(r.status.success(), "{}", err(&r));
    assert!(out(&r).contains("indexed 3 chunk versions"), "{}", out(&r));
    // No index at all: a search rebuilds it from the store.
    std::fs::remove_file(home.path().join("index2.db")).unwrap();
    assert_eq!(
        out(&run(home.path(), &["search", "searchable"]))
            .lines()
            .count(),
        3
    );
    // An empty store has nothing to rebuild and finds nothing (and is not an error).
    let empty = tempfile::tempdir().unwrap();
    let none = run(empty.path(), &["search", "anything"]);
    assert!(none.status.success(), "{}", err(&none));
    assert!(out(&none).trim().is_empty());
    let zero = run(empty.path(), &["reindex"]);
    assert!(
        out(&zero).contains("indexed 0 chunk versions"),
        "{}",
        out(&zero)
    );
}

#[test]
fn only_archived_chunks_are_marked_archived_in_a_full_listing() {
    let home = tempfile::tempdir().unwrap();
    let kept = put(home.path(), "a kept note");
    let gone = put(home.path(), "an archived note");
    assert!(run(home.path(), &["archive", &gone]).status.success());
    let list = out(&run(home.path(), &["list", "--all"]));
    let line = |id: &str| list.lines().find(|l| l.contains(id)).unwrap().to_owned();
    assert!(line(&gone).contains("[archived]"), "{list}");
    assert!(!line(&kept).contains("[archived]"), "{list}");
}

#[test]
fn every_origin_survives_an_export_and_import() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let mut records = vec!["{\"sageant_export\":1}".to_owned()];
    let mut ids = Vec::new();
    for (i, origin) in ["User", "Tool", "Web", "Agent", "Model"].iter().enumerate() {
        let text = format!("note number {i} from {origin}");
        let src = tempfile::tempdir().unwrap();
        let id = put(src.path(), &text);
        ids.push((id.clone(), *origin));
        records.push(
            serde_json::json!({"type":"chunk","id":id,"version":1,"collection":"notes","kind":"note","content":text,"origin":origin,"source":"s","status":"Active","created_at":i + 1})
                .to_string(),
        );
    }
    let file = a.path().join("origins.jsonl");
    std::fs::write(&file, records.join("\n")).unwrap();
    let imp = run(b.path(), &["import", file.to_str().unwrap()]);
    assert!(imp.status.success(), "{}", err(&imp));
    assert!(
        err(&imp).contains("imported 5 chunks (5 versions), 0 links; skipped 0"),
        "{}",
        err(&imp)
    );
    for (id, origin) in ids {
        let shown = out(&run(b.path(), &["get", &id[..8]]));
        assert!(
            shown.contains(&format!("origin: {origin}")),
            "{origin}:\n{shown}"
        );
    }
}

#[test]
fn import_and_export_report_exact_counts_and_a_second_import_counts_what_it_skips() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let one = put(a.path(), "first exported note");
    let two = put(a.path(), "second exported note");
    assert!(
        run(a.path(), &["link", &one, "Related", &two])
            .status
            .success()
    );
    let ex = run(
        a.path(),
        &[
            "export",
            "--out",
            a.path().join("e.jsonl").to_str().unwrap(),
        ],
    );
    assert!(
        err(&ex).contains("exported 3 records"),
        "2 chunks and 1 link: {}",
        err(&ex)
    );
    let file = a.path().join("e.jsonl");
    let first = run(b.path(), &["import", file.to_str().unwrap()]);
    assert!(
        err(&first).contains("imported 2 chunks (2 versions), 1 links; skipped 0"),
        "{}",
        err(&first)
    );
    let again = run(b.path(), &["import", file.to_str().unwrap()]);
    assert!(
        err(&again).contains("imported 0 chunks (0 versions), 0 links; skipped 3"),
        "{}",
        err(&again)
    );
}

#[test]
fn add_reports_exact_counts_for_each_kind_of_skip() {
    let home = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("one.txt"), "first note");
    write(&notes.path().join("sub/two.md"), "second note");
    write(&notes.path().join("big.txt"), &"word ".repeat(300));
    write(&notes.path().join("empty.txt"), "   ");
    write(&notes.path().join("skip.rs"), "fn main() {}");
    std::fs::write(notes.path().join("bad.txt"), [0xff, 0xfe]).unwrap();
    let first = run(
        home.path(),
        &["add", notes.path().to_str().unwrap(), "--max-bytes", "1000"],
    );
    assert!(first.status.success(), "{}", err(&first));
    assert!(
        err(&first).contains("added 2 (0 already present); skipped: 0 links, 0 outside the folder, 1 too large, 1 unreadable, 2 other files"),
        "{}",
        err(&first)
    );
    let again = run(
        home.path(),
        &["add", notes.path().to_str().unwrap(), "--max-bytes", "1000"],
    );
    assert!(
        err(&again).contains("added 0 (2 already present)"),
        "{}",
        err(&again)
    );
    // A file of exactly the limit is taken; one byte over is not.
    let edge = tempfile::tempdir().unwrap();
    write(&edge.path().join("exact.txt"), &"x".repeat(10));
    write(&edge.path().join("over.txt"), &"y".repeat(11));
    let o = run(
        home.path(),
        &["add", edge.path().to_str().unwrap(), "--max-bytes", "10"],
    );
    assert!(
        err(&o).contains("added 1 ") && err(&o).contains("1 too large"),
        "{}",
        err(&o)
    );
}

#[cfg(unix)]
#[test]
fn add_counts_a_skipped_link() {
    let home = tempfile::tempdir().unwrap();
    let notes = tempfile::tempdir().unwrap();
    write(&notes.path().join("a.txt"), "note");
    std::os::unix::fs::symlink(notes.path().join("a.txt"), notes.path().join("ln.txt")).unwrap();
    let o = run(home.path(), &["add", notes.path().to_str().unwrap()]);
    assert!(err(&o).contains("1 links"), "{}", err(&o));
}

#[test]
fn a_write_is_attributed_to_the_user_named_by_the_environment() {
    use persistant::registry::Endpoint;
    use sageant_persistant::PersistStore;
    let actor_for = |user: &str, username: &str| {
        let home = tempfile::tempdir().unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_sageant"))
            .args(["put", "who wrote this"])
            .env("SAGEANT_HOME", home.path())
            .env("USER", user)
            .env("USERNAME", username)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", err(&o));
        let backend = std::fs::read_to_string(home.path().join("backend")).unwrap();
        let store = PersistStore::open(
            backend.trim(),
            &Endpoint::Path(home.path().join("store.db")),
        )
        .unwrap();
        let log = store.audit().unwrap();
        log[0].actor().to_owned()
    };
    assert_eq!(actor_for("alice", "bob"), "cli/alice");
    assert_eq!(actor_for("", "bob"), "cli/bob");
    assert_eq!(actor_for("", ""), "cli/user");
}
