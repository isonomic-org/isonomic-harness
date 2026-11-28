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
    cx_as("s1", "chat", &format!("c{}", k.next.get()), cap)
}

fn cx_as(session: &str, app: &str, call: &str, cap: Option<usize>) -> ToolCtx {
    ToolCtx {
        session: SessionId::from(session),
        app: AppId::from(app),
        call: CallId::from(call),
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
    // A write shows the whole text, never a count.
    let long = "x".repeat(5000);
    let input = args([("path", s("Design/a.md")), ("text", s(&long))]);
    let a = poll(
        tool(&k, "write_file")
            .access(&input, &cx(&k, None))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(a.preview[1].value, long);
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

/// Call ids restart in every session, so the places a call resolved are
/// kept per session and call: another session's call with the same id
/// never swaps them.
#[test]
fn sessions_with_the_same_call_id_keep_their_own_places() {
    let k = kit();
    k.fs.link("other", "f9", "Secret", false);
    k.fs.put("f9", "a.md", "TOP SECRET");
    k.fs.put("f1", "a.md", "plain");
    let t = tool(&k, "read_file");
    let mine = cx_as("s1", "chat", "cal_0001", None);
    let theirs = cx_as("s2", "other", "cal_0001", None);
    let input = args([("path", s("Design/a.md"))]);
    let secret = args([("path", s("Secret/a.md"))]);
    poll(t.access(&input, &mine).unwrap()).unwrap();
    poll(t.access(&secret, &theirs).unwrap()).unwrap();
    let text = |out: ToolOutput| match out {
        ToolOutput::Done(d) => format!("{:?}", d.content),
        ToolOutput::Ask(_) => String::new(),
    };
    let got = text(poll(t.call(&input, &mine)));
    assert!(got.contains("plain") && !got.contains("SECRET"), "{got}");
    assert!(text(poll(t.call(&secret, &theirs))).contains("TOP SECRET"));
    // And a write approved for one folder cannot land in the other.
    let w = tool(&k, "write_file");
    let input = args([("path", s("Design/a.md")), ("text", s("mine"))]);
    let other = args([("path", s("Secret/a.md")), ("text", s("theirs"))]);
    poll(w.access(&input, &mine).unwrap()).unwrap();
    poll(w.access(&other, &theirs).unwrap()).unwrap();
    poll(w.call(&input, &mine));
    assert_eq!(k.fs.get("f9", "a.md").as_deref(), Some("TOP SECRET"));
    assert_eq!(k.fs.get("f1", "a.md").as_deref(), Some("mine"));
}

#[test]
fn one_session_never_drops_the_places_another_resolved() {
    let k = kit();
    let read = tool(&k, "read_file");
    let input = args([("path", s("Design/notes.md"))]);
    let mine = cx_as("s1", "chat", "cal_0001", None);
    poll(read.access(&input, &mine).unwrap()).unwrap();
    for n in 0..300 {
        let other = cx_as("s2", "chat", &format!("c{n}"), None);
        poll(read.access(&input, &other).unwrap()).unwrap();
    }
    let ToolOutput::Done(done) = poll(read.call(&input, &mine)) else {
        panic!("files tools never ask");
    };
    assert!(done.error.is_none(), "{:?}", done.error);
    // The busy session lost its own oldest.
    let oldest = cx_as("s2", "chat", "c0", None);
    let ToolOutput::Done(done) = poll(read.call(&input, &oldest)) else {
        panic!("files tools never ask");
    };
    assert_eq!(done.error.unwrap().code, "tool.unresolved");
}

#[test]
fn a_folder_unlinked_or_made_read_only_after_resolving_is_refused_at_run() {
    let k = kit();
    let w = tool(&k, "write_file");
    let input = args([("path", s("Design/notes.md")), ("text", s("x"))]);
    let c = cx(&k, None);
    poll(w.access(&input, &c).unwrap()).unwrap();
    k.fs.unlink("chat", "f1");
    let out = poll(w.call(&input, &c));
    assert!(
        matches!(out, ToolOutput::Done(d) if d.error.as_ref().unwrap().code == code::NOT_LINKED)
    );
    assert_eq!(
        k.fs.get("f1", "notes.md").as_deref(),
        Some("one\ntwo\nthree\n")
    );
}

#[test]
fn paths_outside_links_read_only_and_protected_are_refused() {
    let k = kit();
    k.fs.symlink("f1", "out", None);
    let read = |p: &str| call(&k, "read_file", args([("path", s(p))]));
    assert_eq!(read("/etc/passwd"), Err(code::NOT_LINKED.into()));
    assert_eq!(read("Design/../x"), Err(code::OUTSIDE_FOLDER.into()));
    assert_eq!(read("Design/out/passwd"), Err(code::OUTSIDE_FOLDER.into()));
    assert_eq!(read("Design/.git/config"), Err(code::PROTECTED.into()));
    assert_eq!(read("Design/nope.md"), Err(code::NOT_FOUND.into()));
    let write = |p: &str| call(&k, "write_file", args([("path", s(p)), ("text", s("x"))]));
    assert_eq!(write("Notes/todo.md"), Err(code::READ_ONLY.into()));
    assert_eq!(write("Design/AGENTS.md"), Err(code::PROTECTED.into()));
    assert_eq!(write("Design/deep/agents.md"), Err(code::PROTECTED.into()));
    assert_eq!(write("Design"), Err(code::PROTECTED.into()));
    assert_eq!(write("Design/out/x"), Err(code::OUTSIDE_FOLDER.into()));
    assert_eq!(k.fs.get("f2", "todo.md").as_deref(), Some("milk\n"));
    let bad = call(
        &k,
        "search_files",
        args([("path", s("Design")), ("query", s("x")), ("in", s("regex"))]),
    );
    assert_eq!(bad, Err("tool.input".into()));
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
    assert!(
        d.contains("icons/") && d.contains("notes.md  14 bytes  1970-01-01 00:00"),
        "{d}"
    );
    for i in 0..60 {
        k.fs.put("f1", &format!("many/file-{i:02}.txt"), "x");
    }
    let mut seen = Vec::new();
    let mut after = String::new();
    loop {
        let page = call_cap(
            &k,
            "list_dir",
            args([("path", s("Design/many")), ("after", s(&after))]),
            Some(1024),
        )
        .unwrap();
        assert!(page.len() <= 1024);
        seen.extend(
            page.lines()
                .filter(|l| l.starts_with("file-"))
                .map(|l| l.split("  ").next().unwrap().to_owned()),
        );
        match page.split("after = ").nth(1) {
            Some(rest) => after = rest.trim_end().trim_end_matches(" to go on.").to_owned(),
            None => break,
        }
    }
    let want: Vec<String> = (0..60).map(|i| format!("file-{i:02}.txt")).collect();
    assert_eq!(seen, want);
}

#[test]
fn read_shows_a_range_and_says_where_to_go_on() {
    let k = kit();
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
    let past = call(
        &k,
        "read_file",
        args([
            ("path", s("Design/notes.md")),
            ("from_line", Value::from(9u64)),
        ]),
    )
    .unwrap();
    assert!(past.contains("past the end"));
}

/// A search hit's line number is the one `read_file` shows, even after a
/// line long enough to be split.
#[test]
fn search_edit_and_read_count_lines_the_same_way() {
    let k = kit();
    let long = "y".repeat(1300);
    k.fs.put("f1", "wide.txt", &format!("{long}\nneedle here\nend\n"));
    let found = call(
        &k,
        "search_files",
        args([
            ("path", s("Design")),
            ("query", s("needle")),
            ("in", s("text")),
        ]),
    )
    .unwrap();
    assert_eq!(found, "Design/wide.txt:4: needle here\n");
    let page = call(
        &k,
        "read_file",
        args([
            ("path", s("Design/wide.txt")),
            ("from_line", Value::from(4u64)),
            ("lines", Value::from(1u64)),
        ]),
    )
    .unwrap();
    assert!(page.ends_with("\nneedle here\n"), "{page}");
    let edited = call(
        &k,
        "edit_file",
        args([
            ("path", s("Design/wide.txt")),
            ("find", s("needle")),
            ("replace", s("pin")),
        ]),
    )
    .unwrap();
    assert!(
        edited.contains("Lines 4 to 4 now read:\npin here\n"),
        "{edited}"
    );
    // A hit late in a long line names the display line it is on, and a
    // read from that line shows it.
    let tail = format!("{}tail-mark\nlast\n", "z".repeat(1200));
    k.fs.put("f1", "late.txt", &tail);
    let found = call(
        &k,
        "search_files",
        args([
            ("path", s("Design")),
            ("query", s("tail-mark")),
            ("in", s("text")),
        ]),
    )
    .unwrap();
    assert!(found.starts_with("Design/late.txt:3: "), "{found}");
    let page = call(
        &k,
        "read_file",
        args([
            ("path", s("Design/late.txt")),
            ("from_line", Value::from(3u64)),
            ("lines", Value::from(1u64)),
        ]),
    )
    .unwrap();
    assert!(page.contains("tail-mark"), "{page}");
    // An edit that removes the file's last lines says so.
    let cut = call(
        &k,
        "edit_file",
        args([
            ("path", s("Design/late.txt")),
            ("find", s("last\n")),
            ("replace", s("")),
        ]),
    )
    .unwrap();
    assert!(cut.contains("removed from the end"), "{cut}");
    let none = call(
        &k,
        "search_files",
        args([("path", s("Design/nowhere")), ("query", s("x"))]),
    );
    assert!(none.unwrap_err().contains("files.not_found"));
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
    let slip = call(
        &k,
        "search_files",
        args([("path", s("Design")), ("query", s("nts"))]),
    )
    .unwrap();
    assert_eq!(slip, "Design/notes.md\n");
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
    assert_eq!(vcs, "Nothing under Design matches.\n");
}

#[test]
fn write_says_created_replaced_or_appended_with_sizes() {
    let k = kit();
    let write = |text: &str, append: bool| {
        call(
            &k,
            "write_file",
            args([
                ("path", s("Design/new/a.md")),
                ("text", s(text)),
                ("append", Value::Bool(append)),
            ]),
        )
        .unwrap()
    };
    assert_eq!(write("hi", false), "Created Design/new/a.md (2 bytes).");
    assert_eq!(
        write("!", true),
        "Appended 1 bytes to Design/new/a.md (now 3 bytes)."
    );
    assert_eq!(k.fs.get("f1", "new/a.md").as_deref(), Some("hi!"));
    assert_eq!(
        write("bye", false),
        "Replaced Design/new/a.md (3 bytes, was 3)."
    );
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
        "Edited Design/notes.md, matched by the exact text. Lines 2 to 2 now read:\n2\n"
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
    k.fs.put("f1", "notes.md", "  a = \u{201C}b\u{201D}  \r\nc\r\n");
    let fuzzy = edit("a = \"b\"", "a = 1\nb = 2").unwrap();
    assert!(
        fuzzy.contains("typographic") && fuzzy.contains("Lines 1 to 2"),
        "{fuzzy}"
    );
    assert_eq!(
        k.fs.get("f1", "notes.md").as_deref(),
        Some("a = 1\r\nb = 2\r\nc\r\n")
    );
}

#[test]
fn move_never_replaces_and_stays_in_one_folder() {
    let k = kit();
    let mv = |a: &str, b: &str| call(&k, "move_path", args([("from", s(a)), ("to", s(b))]));
    assert_eq!(
        mv("Design/icons", "Design/art/icons").unwrap(),
        "Moved to Design/art/icons."
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
    assert_eq!(
        call(&k, "trash_path", args([("path", s("Design/icons"))])).unwrap(),
        "Moved Design/icons to the Trash."
    );
    assert_eq!(k.fs.get("f1", "icons/a.svg"), None);
    assert_eq!(k.fs.trashed(), ["f1/icons"]);
    assert_eq!(
        call(&k, "trash_path", args([("path", s("Design/icons"))])),
        Err(code::NOT_FOUND.into())
    );
}
