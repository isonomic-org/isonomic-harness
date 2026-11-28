// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files kit over the memory store.

use harness_core::future::BoxFuture;
use harness_core::kits::files_errors as code;
use harness_core::*;
use harness_toolkit::files::{files_kit, MemoryFs};
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

fn poll<T>(f: BoxFuture<'_, T>) -> T {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let mut f = f;
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("the memory store is ready at once"),
    }
}

struct Kit {
    fs: Arc<MemoryFs>,
    tools: Vec<Arc<dyn Tool>>,
    next: std::cell::Cell<u32>,
}

fn kit() -> Kit {
    let fs = Arc::new(MemoryFs::new());
    fs.link("chat", "f1", "Design", false);
    fs.link("chat", "f2", "Notes", true);
    fs.put("f1", "notes.md", "one\ntwo\nthree\n");
    fs.put("f1", "icons/a.svg", "<svg/>");
    fs.put("f1", ".git/config", "[core]");
    fs.put("f2", "todo.md", "milk\n");
    Kit {
        tools: files_kit(fs.clone()),
        fs,
        next: std::cell::Cell::new(0),
    }
}

fn cx(k: &Kit, cap: Option<usize>) -> ToolCtx {
    k.next.set(k.next.get() + 1);
    ToolCtx {
        session: SessionId::from("s1"),
        app: AppId::from("chat"),
        call: CallId::from(format!("c{}", k.next.get()).as_str()),
        now: 0,
        child: None,
        result_cap: cap,
    }
}

fn tool<'a>(k: &'a Kit, name: &str) -> &'a Arc<dyn Tool> {
    k.tools.iter().find(|t| t.spec().name == name).unwrap()
}

/// Resolve, then run: what the host does after policy allows the call.
fn call_cap(k: &Kit, name: &str, input: Value, cap: Option<usize>) -> Result<String, String> {
    let t = tool(k, name);
    let c = cx(k, cap);
    if let Err(e) = poll(t.access(&input, &c).expect("a kit tool resolves")) {
        return Err(e.error.unwrap().code);
    }
    match poll(t.call(&input, &c)) {
        ToolOutput::Done(ToolDone {
            outcome: ToolOutcome::Ok,
            content,
            ..
        }) => Ok(content
            .iter()
            .map(|b| match b {
                Block::Text { text } => text.clone(),
                other => format!("{other:?}"),
            })
            .collect()),
        ToolOutput::Done(d) => Err(d.error.unwrap().code),
        ToolOutput::Ask(_) => panic!("files tools never ask"),
    }
}

fn call(k: &Kit, name: &str, input: Value) -> Result<String, String> {
    call_cap(k, name, input, None)
}

fn args<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::object(pairs)
}

fn s(v: &str) -> Value {
    Value::from(v)
}

#[test]
fn seven_tools_in_spec_order_offered_only_with_a_link() {
    let k = kit();
    let names: Vec<String> = k.tools.iter().map(|t| t.spec().name).collect();
    assert_eq!(
        names,
        [
            "list_dir",
            "read_file",
            "search_files",
            "write_file",
            "edit_file",
            "move_path",
            "trash_path"
        ]
    );
    assert!(k.tools.iter().all(|t| t.offered(&AppId::from("chat"))));
    assert!(!k.tools[0].offered(&AppId::from("other")));
}

#[test]
fn access_names_the_folder_and_previews_whole_values() {
    let k = kit();
    let t = tool(&k, "edit_file");
    let input = args([
        ("path", s("Design/notes.md")),
        ("find", s("two")),
        ("replace", s("2")),
    ]);
    let a = poll(t.access(&input, &cx(&k, None)).unwrap()).unwrap();
    assert_eq!(
        (a.scope.kind, a.scope.id.as_str()),
        (ScopeKind::Folder, "f1")
    );
    assert_eq!(a.preview[1].value, "two");
    assert!(a.reversible);
    k.fs.irreversible("f1");
    let a = poll(t.access(&input, &cx(&k, None)).unwrap()).unwrap();
    assert!(!a.reversible);
}

#[test]
fn a_call_without_its_access_check_is_refused() {
    let k = kit();
    let input = args([("path", s("Design/notes.md"))]);
    let out = poll(tool(&k, "read_file").call(&input, &cx(&k, None)));
    assert!(
        matches!(out, ToolOutput::Done(d) if d.error.as_ref().unwrap().code == "tool.unresolved")
    );
}

#[test]
fn paths_outside_links_read_only_and_protected_are_refused() {
    let k = kit();
    let read = |p: &str| call(&k, "read_file", args([("path", s(p))]));
    assert_eq!(read("/etc/passwd"), Err(code::NOT_LINKED.into()));
    assert_eq!(read("Design/../x"), Err(code::OUTSIDE_FOLDER.into()));
    assert_eq!(read("Design/.git/config"), Err(code::PROTECTED.into()));
    assert_eq!(read("Design/nope.md"), Err(code::NOT_FOUND.into()));
    let write = |p: &str| call(&k, "write_file", args([("path", s(p)), ("text", s("x"))]));
    assert_eq!(write("Notes/todo.md"), Err(code::READ_ONLY.into()));
    assert_eq!(write("Design/AGENTS.md"), Err(code::PROTECTED.into()));
    assert_eq!(write("Design"), Err(code::PROTECTED.into()));
    assert_eq!(k.fs.get("f2", "todo.md").as_deref(), Some("milk\n"));
}

#[test]
fn list_dir_lists_folders_and_pages_with_after() {
    let k = kit();
    let top = call(&k, "list_dir", args([("path", s(""))])).unwrap();
    assert!(
        top.contains("Design/") && top.contains("Notes/ (read only)"),
        "{top}"
    );
    let d = call(&k, "list_dir", args([("path", s("Design"))])).unwrap();
    assert!(d.contains("icons/") && d.contains("notes.md  14 bytes") && !d.contains(".git/config"));
    for i in 0..40 {
        k.fs.put("f1", &format!("many/file-{i:02}.txt"), "x");
    }
    let first = call_cap(
        &k,
        "list_dir",
        args([("path", s("Design/many"))]),
        Some(512),
    )
    .unwrap();
    let after = first
        .lines()
        .rev()
        .find(|l| l.contains("after = "))
        .unwrap();
    let last = after
        .rsplit("after = ")
        .next()
        .unwrap()
        .trim_end_matches(" to go on.");
    let next = call_cap(
        &k,
        "list_dir",
        args([("path", s("Design/many")), ("after", s(last))]),
        Some(512),
    )
    .unwrap();
    assert!(!next.contains(&format!("{last}  ")) && first.contains(&format!("{last}  ")));
}

/// Invariant 9: every page fits the cap, and pages cover the content with
/// no gap and no overlap, long lines included.
#[test]
fn read_pages_fit_and_cover_the_text() {
    let k = kit();
    let long = "é".repeat(700);
    let content: String = (1..=120)
        .map(|i| format!("line {i} {}\n", if i % 17 == 0 { &long } else { "" }))
        .collect();
    k.fs.put("f1", "big.txt", &content);
    let cap = 1024;
    let mut from = 1;
    let mut seen = String::new();
    loop {
        let page = call_cap(
            &k,
            "read_file",
            args([
                ("path", s("Design/big.txt")),
                ("from_line", Value::from(from as u64)),
            ]),
            Some(cap),
        )
        .unwrap();
        assert!(page.len() <= cap, "{} > {cap}", page.len());
        let (header, body) = page.split_once('\n').unwrap();
        seen.push_str(body);
        match header.split("from_line = ").nth(1) {
            Some(n) => from = n.trim_end_matches('.').parse().unwrap(),
            None => break,
        }
    }
    let joined: String = content.lines().collect();
    assert_eq!(seen.replace('\n', ""), joined);
    let some = call(
        &k,
        "read_file",
        args([
            ("path", s("Design/notes.md")),
            ("from_line", Value::from(2u64)),
            ("lines", Value::from(1u64)),
        ]),
    )
    .unwrap();
    assert_eq!(
        some,
        "Design/notes.md, lines 2 to 2 of 3. Go on with from_line = 3.\ntwo\n"
    );
}

#[test]
fn search_by_name_and_text_skips_vcs() {
    let k = kit();
    let names = call(
        &k,
        "search_files",
        args([("path", s("Design")), ("query", s("SVG"))]),
    )
    .unwrap();
    assert_eq!(names, "Design/icons/a.svg\n");
    let found = call(
        &k,
        "search_files",
        args([("path", s("Design")), ("query", s("tw")), ("in", s("text"))]),
    )
    .unwrap();
    assert_eq!(found, "Design/notes.md:2: two\n");
    let vcs = call(
        &k,
        "search_files",
        args([
            ("path", s("Design")),
            ("query", s("core")),
            ("in", s("text")),
        ]),
    )
    .unwrap();
    assert!(vcs.starts_with("Nothing under Design"));
}

#[test]
fn write_and_append() {
    let k = kit();
    call(
        &k,
        "write_file",
        args([("path", s("Design/new/a.md")), ("text", s("hi"))]),
    )
    .unwrap();
    call(
        &k,
        "write_file",
        args([
            ("path", s("Design/new/a.md")),
            ("text", s("!")),
            ("append", Value::Bool(true)),
        ]),
    )
    .unwrap();
    assert_eq!(k.fs.get("f1", "new/a.md").as_deref(), Some("hi!"));
}

#[test]
fn edit_uses_the_ladder_and_needs_one_place() {
    let k = kit();
    let edit = |f: &str, r: &str| {
        call(
            &k,
            "edit_file",
            args([
                ("path", s("Design/notes.md")),
                ("find", s(f)),
                ("replace", s(r)),
            ]),
        )
    };
    assert_eq!(
        edit("two", "2").unwrap(),
        "Edited Design/notes.md at line 2."
    );
    assert_eq!(
        k.fs.get("f1", "notes.md").as_deref(),
        Some("one\n2\nthree\n")
    );
    assert_eq!(edit("zzz", "y"), Err(code::NO_MATCH.into()));
    k.fs.put("f1", "notes.md", "x\ny\nx\n");
    assert_eq!(edit("x", "z"), Err(code::MANY_MATCHES.into()));
    assert_eq!(
        edit("x\ny", "x\ny").unwrap(),
        "No change: find and replace are the same."
    );
    k.fs.put("f1", "notes.md", "  a = \u{201C}b\u{201D}  \nc\n");
    assert!(edit("a = \"b\"", "a = 1").unwrap().contains("typographic"));
    assert_eq!(k.fs.get("f1", "notes.md").as_deref(), Some("a = 1\nc\n"));
}

#[test]
fn move_never_replaces_and_stays_in_one_folder() {
    let k = kit();
    let mv = |a: &str, b: &str| call(&k, "move_path", args([("from", s(a)), ("to", s(b))]));
    assert_eq!(
        mv("Design/icons", "Design/art/icons").unwrap(),
        "Moved Design/icons to Design/art/icons."
    );
    assert_eq!(k.fs.get("f1", "art/icons/a.svg").as_deref(), Some("<svg/>"));
    assert_eq!(
        mv("Design/notes.md", "Design/art/icons/a.svg"),
        Err(code::EXISTS.into())
    );
    assert_eq!(
        mv("Design/notes.md", "Notes/n.md"),
        Err(code::CROSS_FOLDER.into())
    );
    assert_eq!(
        mv("Design/gone.md", "Design/x.md"),
        Err(code::NOT_FOUND.into())
    );
}

#[test]
fn trash_keeps_what_it_removes() {
    let k = kit();
    call(&k, "trash_path", args([("path", s("Design/icons"))])).unwrap();
    assert_eq!(k.fs.get("f1", "icons/a.svg"), None);
    assert_eq!(k.fs.trashed(), ["f1/icons"]);
    assert_eq!(
        call(&k, "trash_path", args([("path", s("Design/icons"))])),
        Err(code::NOT_FOUND.into())
    );
}
