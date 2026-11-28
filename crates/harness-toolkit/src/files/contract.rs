// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files contract: what every [`FileSystem`] must do, whatever a
//! caller puts in a [`Place`]. [`MemoryFs`] passes it here; the engine's
//! service runs the same suite over real folders. A service gives the
//! suite a [`Bench`] to arrange folders with.

use super::{ChangeId, FileSystem, FsResult, MemoryFs, Place};
use harness_core::future::BoxFuture;
use harness_core::kits::files_errors as code;
use harness_core::{AppId, CallId, SessionId};

/// What the suite needs beside the service: a way to arrange and inspect
/// folders from outside the kit.
pub trait Bench: FileSystem {
    /// Link a folder for `app`.
    fn link(&self, app: &str, id: &str, label: &str, read_only: bool);
    /// Unlink it.
    fn unlink(&self, app: &str, id: &str);
    /// Put a file, making its parents.
    fn put(&self, folder: &str, path: &str, text: &str);
    /// Put a link on disk: into a folder, or (`None`) out of every folder.
    fn symlink(&self, folder: &str, path: &str, to: Option<(&str, &str)>);
    /// Every path in a folder with its text (`None` for a folder).
    fn snapshot(&self, folder: &str) -> Vec<(String, Option<String>)>;
}

impl Bench for MemoryFs {
    fn link(&self, app: &str, id: &str, label: &str, read_only: bool) {
        MemoryFs::link(self, app, id, label, read_only);
    }
    fn unlink(&self, app: &str, id: &str) {
        MemoryFs::unlink(self, app, id);
    }
    fn put(&self, folder: &str, path: &str, text: &str) {
        MemoryFs::put(self, folder, path, text);
    }
    fn symlink(&self, folder: &str, path: &str, to: Option<(&str, &str)>) {
        MemoryFs::symlink(self, folder, path, to);
    }
    fn snapshot(&self, folder: &str) -> Vec<(String, Option<String>)> {
        MemoryFs::snapshot(self, folder)
    }
}

/// Run a future to its end on this thread.
pub fn wait<T>(mut f: BoxFuture<'_, T>) -> T {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn at(app: &str, folder: &str, label: &str, path: &str) -> Place {
    Place {
        app: AppId::from(app),
        folder: folder.into(),
        label: label.into(),
        read_only: false,
        path: path.into(),
    }
}

fn call(n: u32) -> ChangeId {
    call_in("s1", n)
}

fn call_in(session: &str, n: u32) -> ChangeId {
    ChangeId {
        session: SessionId::from(session),
        call: CallId::from(format!("cal_{n:04}").as_str()),
    }
}

fn err<T: std::fmt::Debug>(r: FsResult<T>) -> &'static str {
    r.expect_err("the service must refuse this").code
}

/// A bench with Design (writable) and Archive (read only) linked for
/// `chat`, and Secret linked for `other`.
fn arranged<B: Bench>(make: &impl Fn() -> B) -> B {
    let b = make();
    b.link("chat", "f1", "Design", false);
    b.link("chat", "f2", "Archive", true);
    b.link("other", "f3", "Secret", false);
    b.put("f1", "notes.md", "one\ntwo\n");
    b.put("f1", "icons/home.svg", "<svg/>");
    b.put("f1", ".git/config", "[core]");
    b.put("f2", "old.md", "old\n");
    b.put("f3", "key.txt", "TOP SECRET");
    b
}

/// The whole suite. `make` gives an empty bench each time.
pub fn run<B: Bench>(make: impl Fn() -> B) {
    links_are_per_app_and_checked_at_every_call(&make);
    read_only_is_checked_when_the_call_runs(&make);
    links_on_disk_never_lead_out(&make);
    protected_places_are_never_changed(&make);
    writes_are_whole_and_reads_are_text(&make);
    moves_never_replace(&make);
    search_finds_names_and_text(&make);
    every_change_can_be_undone(&make);
    changes_are_kept_per_session(&make);
    version_control_is_closed_through_a_link(&make);
}

fn links_are_per_app_and_checked_at_every_call<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    // A place naming another app's folder gains nothing.
    let stolen = at("chat", "f3", "Secret", "key.txt");
    assert_eq!(err(wait(b.read(&stolen, 1 << 20))), code::NOT_LINKED);
    assert_eq!(
        err(wait(b.write(&stolen, "x", false, &call(1)))),
        code::NOT_LINKED
    );
    assert_eq!(
        err(wait(b.list(&at("chat", "f3", "Secret", "")))),
        code::NOT_LINKED
    );
    assert_eq!(err(wait(b.search(&stolen, "k", false))), code::NOT_LINKED);
    assert_eq!(b.snapshot("f3").len(), 1);
    // A folder unlinked after resolving is gone for the call.
    let notes = at("chat", "f1", "Design", "notes.md");
    assert_eq!(wait(b.read(&notes, 1 << 20)).as_deref(), Ok("one\ntwo\n"));
    b.unlink("chat", "f1");
    assert_eq!(err(wait(b.read(&notes, 1 << 20))), code::NOT_LINKED);
    assert_eq!(err(wait(b.trash(&notes, &call(2)))), code::NOT_LINKED);
}

fn read_only_is_checked_when_the_call_runs<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    // The place says writable; the link says read only. The link wins.
    let old = at("chat", "f2", "Archive", "old.md");
    assert_eq!(
        err(wait(b.write(&old, "x", false, &call(1)))),
        code::READ_ONLY
    );
    assert_eq!(err(wait(b.trash(&old, &call(2)))), code::READ_ONLY);
    let to = at("chat", "f2", "Archive", "new.md");
    assert_eq!(err(wait(b.rename(&old, &to, &call(3)))), code::READ_ONLY);
    assert_eq!(wait(b.read(&old, 1 << 20)).as_deref(), Ok("old\n"));
}

fn links_on_disk_never_lead_out<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    b.symlink("f1", "out", None);
    b.symlink("f1", "peer", Some(("f3", "")));
    b.symlink("f1", "in", Some(("f1", "icons")));
    for p in ["out", "out/passwd", "peer/key.txt"] {
        let place = at("chat", "f1", "Design", p);
        assert_eq!(
            err(wait(b.read(&place, 1 << 20))),
            code::OUTSIDE_FOLDER,
            "{p}"
        );
        assert_eq!(
            err(wait(b.write(&place, "x", false, &call(1)))),
            code::OUTSIDE_FOLDER,
            "{p}"
        );
    }
    // A link that stays inside is followed.
    let inside = at("chat", "f1", "Design", "in/home.svg");
    assert_eq!(wait(b.read(&inside, 1 << 20)).as_deref(), Ok("<svg/>"));
    assert_eq!(b.snapshot("f3").len(), 1);
}

fn protected_places_are_never_changed<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    b.symlink("f1", "vcs", Some(("f1", ".git")));
    b.put("f1", "sub/AGENTS.md", "rules");
    for p in [
        ".git/config",
        ".GIT/hooks/x",
        "vcs/config",
        "AGENTS.md",
        "sub/agents.md",
        "",
    ] {
        let place = at("chat", "f1", "Design", p);
        assert_eq!(
            err(wait(b.write(&place, "x", false, &call(1)))),
            code::PROTECTED,
            "{p}"
        );
    }
    let hooks = at("chat", "f1", "Design", ".git");
    assert_eq!(err(wait(b.trash(&hooks, &call(2)))), code::PROTECTED);
    // A folder that holds a protected place is not moved or trashed.
    let sub = at("chat", "f1", "Design", "sub");
    let away = at("chat", "f1", "Design", "elsewhere");
    assert_eq!(err(wait(b.rename(&sub, &away, &call(4)))), code::PROTECTED);
    assert_eq!(err(wait(b.trash(&sub, &call(5)))), code::PROTECTED);
    let notes = at("chat", "f1", "Design", "notes.md");
    let into = at("chat", "f1", "Design", ".git/notes.md");
    assert_eq!(
        err(wait(b.rename(&notes, &into, &call(3)))),
        code::PROTECTED
    );
}

fn writes_are_whole_and_reads_are_text<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    let new = at("chat", "f1", "Design", "deep/er/a.md");
    wait(b.write(&new, "hi", false, &call(1))).expect("a new file in new folders");
    wait(b.write(&new, "!", true, &call(2))).expect("an append");
    assert_eq!(wait(b.read(&new, 1 << 20)).as_deref(), Ok("hi!"));
    assert_eq!(err(wait(b.read(&new, 2))), code::TOO_LARGE);
    b.put("f1", "blob.bin", "a\0b");
    let blob = at("chat", "f1", "Design", "blob.bin");
    assert_eq!(err(wait(b.read(&blob, 1 << 20))), code::NOT_TEXT);
    let gone = at("chat", "f1", "Design", "gone.md");
    assert_eq!(err(wait(b.read(&gone, 1 << 20))), code::NOT_FOUND);
    assert_eq!(wait(b.stat(&gone)), Ok(None));
    let names: Vec<String> = wait(b.list(&at("chat", "f1", "Design", "")))
        .expect("the folder lists")
        .into_iter()
        .map(|e| e.name)
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    let icons = at("chat", "f1", "Design", "icons");
    assert_eq!(
        err(wait(b.write(&icons, "x", false, &call(3)))),
        code::EXISTS
    );
}

fn moves_never_replace<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    let notes = at("chat", "f1", "Design", "notes.md");
    let svg = at("chat", "f1", "Design", "icons/home.svg");
    assert_eq!(err(wait(b.rename(&notes, &svg, &call(1)))), code::EXISTS);
    assert_eq!(wait(b.read(&svg, 1 << 20)).as_deref(), Ok("<svg/>"));
    let across = at("chat", "f2", "Archive", "notes.md");
    assert_eq!(
        err(wait(b.rename(&notes, &across, &call(2)))),
        code::CROSS_FOLDER
    );
    let gone = at("chat", "f1", "Design", "gone.md");
    let to = at("chat", "f1", "Design", "here.md");
    assert_eq!(err(wait(b.rename(&gone, &to, &call(3)))), code::NOT_FOUND);
    let icons = at("chat", "f1", "Design", "icons");
    let art = at("chat", "f1", "Design", "art/icons");
    wait(b.rename(&icons, &art, &call(4))).expect("a folder moves with what it holds");
    let moved = at("chat", "f1", "Design", "art/icons/home.svg");
    assert_eq!(wait(b.read(&moved, 1 << 20)).as_deref(), Ok("<svg/>"));
}

fn search_finds_names_and_text<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    b.put("f1", "notes-old.md", "two again\n");
    let root = at("chat", "f1", "Design", "");
    // A slip still finds both.
    let slip = wait(b.search(&root, "nts", false)).expect("a name search");
    let mut paths: Vec<&str> = slip.iter().map(|h| h.path.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, ["notes-old.md", "notes.md"]);
    // The closer name comes first.
    let exact = wait(b.search(&root, "notes.md", false)).expect("a name search");
    assert_eq!(exact.first().map(|h| h.path.as_str()), Some("notes.md"));
    assert!(wait(b.search(&root, "config", false))
        .expect("a search")
        .is_empty());
    let hits = wait(b.search(&root, "TWO", true)).expect("a text search");
    let found: Vec<(&str, usize)> = hits
        .iter()
        .map(|h| (h.path.as_str(), h.line.as_ref().map_or(0, |l| l.0)))
        .collect();
    assert_eq!(found, [("notes-old.md", 1), ("notes.md", 2)]);
    assert!(wait(b.search(&root, "core", true))
        .expect("a search")
        .is_empty());
}

/// Invariant 7: changes, then their undos in reverse order, leave the
/// folder as it began.
fn every_change_can_be_undone<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    let app = AppId::from("chat");
    let before = b.snapshot("f1");
    let notes = at("chat", "f1", "Design", "notes.md");
    let new = at("chat", "f1", "Design", "made/new.md");
    let moved = at("chat", "f1", "Design", "made/moved.md");
    let icons = at("chat", "f1", "Design", "icons");
    wait(b.write(&notes, "changed", false, &call(1))).expect("replace");
    wait(b.write(&notes, " more", true, &call(2))).expect("append");
    wait(b.write(&new, "new", false, &call(3))).expect("create");
    wait(b.rename(&new, &moved, &call(4))).expect("move");
    wait(b.trash(&icons, &call(5))).expect("trash");
    assert_ne!(b.snapshot("f1"), before);
    // Another app cannot undo this app's call.
    wait(b.undo(&AppId::from("other"), &call(5))).expect("nothing to undo");
    assert_eq!(
        err(wait(
            b.read(&at("chat", "f1", "Design", "icons/home.svg"), 9)
        )),
        code::NOT_FOUND
    );
    for n in (1..=5).rev() {
        wait(b.undo(&app, &call(n))).expect("undo");
    }
    assert_eq!(b.snapshot("f1"), before);
}

/// Call ids restart in every session: two sessions' changes with the same
/// call id are two changes, and undoing one leaves the other.
fn changes_are_kept_per_session<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    let app = AppId::from("chat");
    let notes = at("chat", "f1", "Design", "notes.md");
    let home = at("chat", "f1", "Design", "icons/home.svg");
    wait(b.write(&notes, "first", false, &call_in("s1", 1))).expect("one session");
    wait(b.write(&home, "second", false, &call_in("s2", 1))).expect("another session");
    wait(b.undo(&app, &call_in("s2", 1))).expect("undo");
    assert_eq!(wait(b.read(&notes, 99)).expect("read"), "first");
    assert_eq!(wait(b.read(&home, 99)).expect("read"), "<svg/>");
    wait(b.undo(&app, &call_in("s1", 1))).expect("undo");
    assert_eq!(wait(b.read(&notes, 99)).expect("read"), "one\ntwo\n");
}

/// A link inside the folder never opens version-control metadata.
fn version_control_is_closed_through_a_link<B: Bench>(make: &impl Fn() -> B) {
    let b = arranged(make);
    b.symlink("f1", "peek", Some(("f1", ".git")));
    b.symlink("f1", "conf", Some(("f1", ".git/config")));
    for path in ["peek/config", "conf"] {
        let place = at("chat", "f1", "Design", path);
        assert_eq!(err(wait(b.read(&place, 99))), code::PROTECTED, "{path}");
    }
    let peek = at("chat", "f1", "Design", "peek");
    assert_eq!(err(wait(b.list(&peek))), code::PROTECTED);
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_memory_store_passes_the_contract() {
        super::run(super::MemoryFs::new);
    }
}
