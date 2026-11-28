// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parity vectors for the match ladder: R1's test cases for its sequence
//! search, at R1's pinned commit (Apache-2.0; see THIRD-PARTY-NOTICES.md),
//! run through `replace_one`.

use harness_toolkit::files::ladder::{replace_one, Found, Step};

fn step(content: &str, find: &str) -> Option<(Step, usize)> {
    match replace_one(content, find, "X") {
        Found::One { step, line, .. } => Some((step, line)),
        _ => None,
    }
}

// Parity with R1: an exact sequence is found where it starts.
#[test]
fn exact_match_finds_sequence() {
    assert_eq!(step("foo\nbar\nbaz\n", "bar\nbaz"), Some((Step::Exact, 2)));
}

// Parity with R1: trailing whitespace is ignored at step two.
#[test]
fn trim_end_ignores_trailing_whitespace() {
    assert_eq!(
        step("foo   \nbar\t\t\n", "foo\nbar"),
        Some((Step::TrimEnd, 1))
    );
}

// Parity with R1: leading and trailing whitespace is ignored at step three.
#[test]
fn trim_ignores_leading_and_trailing_whitespace() {
    assert_eq!(
        step("    foo   \n   bar\t\n", "foo\nbar"),
        Some((Step::Trim, 1))
    );
}

// Parity with R1: a pattern longer than the input matches nowhere and
// does not panic.
#[test]
fn pattern_longer_than_input_is_none() {
    assert_eq!(
        replace_one("just one line", "too\nmany\nlines", "X"),
        Found::None
    );
}

// Parity with R1: typographic punctuation is folded at the last step.
#[test]
fn normalised_folds_dashes_quotes_and_spaces() {
    let content = "let a = \u{201C}x\u{201D} \u{2014} y;\nend\n";
    assert_eq!(
        step(content, "let a = \"x\" - y;"),
        Some((Step::Normalised, 1))
    );
}
