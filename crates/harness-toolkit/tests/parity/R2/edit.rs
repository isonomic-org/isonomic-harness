// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parity vectors for `edit_file`: the edit cases of R2's `edit.rs` tests
//! at R2's pinned commit (Apache-2.0; see THIRD-PARTY-NOTICES.md), run
//! through `replace_one`. The contents and the expected texts are R2's.
//! No code is taken from R2: its replacement is the ladder's first step.

use harness_toolkit::files::ladder::{replace_one, Found, Step};

fn replaced(content: &str, find: &str, replace: &str) -> Option<String> {
    match replace_one(content, find, replace) {
        Found::One { text, step, .. } => {
            assert_eq!(step, Step::Exact);
            Some(text)
        }
        _ => None,
    }
}

// R2: test_file_edit_single_match.
#[test]
fn single_match() {
    let text = replaced(
        "fn foo() {\n    println!(\"hello\");\n}",
        "println!(\"hello\");",
        "println!(\"world\");",
    )
    .unwrap();
    assert!(text.contains("println!(\"world\");"));
    assert!(!text.contains("println!(\"hello\");"));
}

// R2: test_file_edit_no_match.
#[test]
fn no_match() {
    assert!(matches!(
        replace_one("some content", "nonexistent", "replacement"),
        Found::None
    ));
}

// R2: test_file_edit_multiple_matches. R2 names the first two places; the
// ladder names every line.
#[test]
fn multiple_matches() {
    match replace_one("foo\nbar\nfoo\nbaz", "foo", "qux") {
        Found::Many(lines) => assert_eq!(lines, [1, 3]),
        other => panic!("{other:?}"),
    }
}

// R2: test_file_edit_delete.
#[test]
fn delete() {
    assert_eq!(
        replaced("keep\ndelete me\nkeep", "\ndelete me", "").as_deref(),
        Some("keep\nkeep")
    );
}

// R2: test_string_replace_normal_edit_still_succeeds.
#[test]
fn normal_edit() {
    assert_eq!(
        replaced("hello world\n", "hello", "goodbye").as_deref(),
        Some("goodbye world\n")
    );
}

// R2: test_string_replace_identical_before_after_is_error. R2 fails the
// call; here the text comes back the same, and `edit_file` answers "No
// change" without writing (recorded as a difference).
#[test]
fn identical_find_and_replace_changes_nothing() {
    assert_eq!(
        replaced("hello world\n", "hello", "hello").as_deref(),
        Some("hello world\n")
    );
}
