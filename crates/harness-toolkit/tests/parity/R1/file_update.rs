// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parity vectors for file updates: R1's `file_update_tests.rs` cases at
//! R1's pinned commit (Apache-2.0; see THIRD-PARTY-NOTICES.md). R1 applies
//! a patch of chunks; here each chunk is one `edit_file` replacement
//! through the ladder, and an insertion at the end of the file is
//! `write_file` with append. The starting and resulting contents are R1's.

use harness_core::{AppId, CallId, SessionId};
use harness_toolkit::files::contract::wait;
use harness_toolkit::files::ladder::{replace_one, Found};
use harness_toolkit::files::{resolve, ChangeId, FileSystem, MemoryFs};

/// Apply each (find, replace) in order; every one must match one place.
fn apply(content: &str, edits: &[(&str, &str)]) -> String {
    let mut text = content.to_owned();
    for (find, replace) in edits {
        text = match replace_one(&text, find, replace) {
            Found::One { text, .. } => text,
            other => panic!("{find:?}: {other:?}"),
        };
    }
    text
}

// R1: test_unified_diff.
#[test]
fn two_chunks_replace_two_lines() {
    let got = apply(
        "foo\nbar\nbaz\nqux\n",
        &[("foo\nbar\n", "foo\nBAR\n"), ("baz\nqux\n", "baz\nQUX\n")],
    );
    assert_eq!(got, "foo\nBAR\nbaz\nQUX\n");
}

// R1: test_unified_diff_first_line_replacement.
#[test]
fn first_line_replacement() {
    let got = apply("foo\nbar\nbaz\n", &[("foo\nbar\n", "FOO\nbar\n")]);
    assert_eq!(got, "FOO\nbar\nbaz\n");
}

// R1: test_unified_diff_last_line_replacement.
#[test]
fn last_line_replacement() {
    let got = apply("foo\nbar\nbaz\n", &[("foo\nbar\nbaz\n", "foo\nbar\nBAZ\n")]);
    assert_eq!(got, "foo\nbar\nBAZ\n");
}

// R1: test_unified_diff_insert_at_eof. An insertion at the end is an
// append here.
#[test]
fn insert_at_end_of_file() {
    let fs = MemoryFs::new();
    fs.link("chat", "f1", "Design", false);
    fs.put("f1", "insert.txt", "foo\nbar\nbaz\n");
    let linked = fs.linked(&AppId::from("chat"));
    let at = resolve(&AppId::from("chat"), &linked, "Design/insert.txt").unwrap();
    let by = ChangeId {
        session: SessionId::from("s1"),
        call: CallId::from("cal_0001"),
    };
    wait(fs.write(&at, "quux\n", true, &by)).unwrap();
    assert_eq!(
        fs.get("f1", "insert.txt").as_deref(),
        Some("foo\nbar\nbaz\nquux\n")
    );
}

// R1: test_unified_diff_interleaved_changes.
#[test]
fn interleaved_changes() {
    let got = apply(
        "a\nb\nc\nd\ne\nf\n",
        &[
            ("a\nb\n", "a\nB\n"),
            ("d\ne\n", "d\nE\n"),
            ("f\n", "f\ng\n"),
        ],
    );
    assert_eq!(got, "a\nB\nc\nd\nE\nf\ng\n");
}
