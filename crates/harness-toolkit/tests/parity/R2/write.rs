// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parity vectors for `write_file`: the write cases of R2's `edit.rs`
//! tests at R2's pinned commit (Apache-2.0; see THIRD-PARTY-NOTICES.md),
//! run through the files service. The paths and texts are R2's.

use harness_core::{AppId, CallId, SessionId};
use harness_toolkit::files::contract::wait;
use harness_toolkit::files::{resolve, ChangeId, FileSystem, MemoryFs};

fn write(fs: &MemoryFs, path: &str, text: &str) {
    let app = AppId::from("chat");
    let at = resolve(&app, &fs.linked(&app), &format!("Design/{path}")).unwrap();
    let by = ChangeId {
        session: SessionId::from("s1"),
        call: CallId::from("cal_0001"),
    };
    wait(fs.write(&at, text, false, &by)).unwrap();
}

fn linked() -> MemoryFs {
    let fs = MemoryFs::new();
    fs.link("chat", "f1", "Design", false);
    fs
}

// R2: test_file_write_new.
#[test]
fn write_new() {
    let fs = linked();
    write(&fs, "new_file.txt", "Hello, world!\nLine 2");
    assert_eq!(
        fs.get("f1", "new_file.txt").as_deref(),
        Some("Hello, world!\nLine 2")
    );
}

// R2: test_file_write_overwrite.
#[test]
fn write_overwrite() {
    let fs = linked();
    fs.put("f1", "existing.txt", "old content");
    write(&fs, "existing.txt", "new content");
    assert_eq!(fs.get("f1", "existing.txt").as_deref(), Some("new content"));
}

// R2: test_file_write_creates_dirs.
#[test]
fn write_creates_dirs() {
    let fs = linked();
    write(&fs, "a/b/c/file.txt", "nested");
    assert_eq!(fs.get("f1", "a/b/c/file.txt").as_deref(), Some("nested"));
}
