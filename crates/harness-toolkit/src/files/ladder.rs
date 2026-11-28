// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The match ladder `edit_file` uses to find the one place to replace
//! (`docs/kits.md`). Each step is tried only when the stricter ones found
//! nothing, and each must find exactly one place: exact text, then whole
//! lines ignoring trailing spaces, then whole lines ignoring spaces at both
//! ends, then whole lines with typographic punctuation folded to ASCII.
//!
//! Steps two to four, the punctuation table and their test cases are
//! ported from R1 at its pinned commit (Apache-2.0); see
//! THIRD-PARTY-NOTICES.md. R1's tests are parity vectors in
//! `tests/parity/R1/`. Unlike R1, which takes the first match, a step that
//! matches more than one place stops the ladder (deviation: see
//! `docs/kits-deviations.md`).

/// Which step found the place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The exact text.
    Exact,
    /// Whole lines, ignoring trailing spaces.
    TrimEnd,
    /// Whole lines, ignoring spaces at both ends.
    Trim,
    /// Whole lines, with typographic dashes, quotes and spaces folded.
    Normalised,
}

/// What the ladder found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// Exactly one place: the new content, the step, and the first line
    /// replaced (from 1).
    One {
        /// The content with the place replaced.
        text: String,
        /// The step that found it.
        step: Step,
        /// The first line replaced, from 1.
        line: usize,
    },
    /// Nowhere.
    None,
    /// More than one place, at these lines (from 1), at the first step
    /// that matched anything.
    Many(Vec<usize>),
}

/// Fold common typographic punctuation to ASCII after trimming, so text
/// typed in plain ASCII still finds lines with dashes, curly quotes or odd
/// spaces. The table is R1's.
fn normalise(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => '-',
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => ' ',
            other => other,
        })
        .collect()
}

use super::lines::{Line, Source};
use std::borrow::Cow;

/// How a step reads a line before it compares: two lines are the same
/// at a step when their keys are equal.
type Key = for<'a> fn(&'a str) -> Cow<'a, str>;

/// Every start index where `pattern` matches `lines`, key for key. Like
/// R1's search, a pattern longer than the input matches nowhere. R1 tries
/// each start in turn; this is one pass with a failure table, so the work
/// grows with the file plus the pattern, not with their product.
fn starts(lines: &[Cow<'_, str>], pattern: &[Cow<'_, str>]) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > lines.len() {
        return Vec::new();
    }
    let mut back = vec![0; pattern.len()];
    let mut k = 0;
    for i in 1..pattern.len() {
        while k > 0 && pattern[i] != pattern[k] {
            k = back[k - 1];
        }
        if pattern[i] == pattern[k] {
            k += 1;
        }
        back[i] = k;
    }
    let mut out = Vec::new();
    let mut k = 0;
    for (i, line) in lines.iter().enumerate() {
        while k > 0 && *line != pattern[k] {
            k = back[k - 1];
        }
        if *line == pattern[k] {
            k += 1;
        }
        if k == pattern.len() {
            out.push(i + 1 - k);
            k = back[k - 1];
        }
    }
    out
}

/// Each line's key at a step.
fn keys<'a>(lines: &[&'a str], key: Key) -> Vec<Cow<'a, str>> {
    lines.iter().map(|l| key(l)).collect()
}

/// Every byte offset where `find` occurs, overlapping ones included.
fn occurrences(content: &str, find: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = content[from..].find(find).map(|i| i + from) {
        out.push(at);
        let step = content[at..].chars().next().map_or(1, char::len_utf8);
        from = at + step;
    }
    out
}

/// The byte offset each of the file's lines starts at.
fn line_starts(source: &Source<'_>) -> Vec<usize> {
    let mut at = 0;
    source
        .lines
        .iter()
        .map(|l| {
            let start = at;
            at += l.text.len() + l.ending.map_or(0, |e| e.as_str().len());
            start
        })
        .collect()
}

/// The line (from 1) that byte offset `at` falls on.
fn line_of(starts: &[usize], at: usize) -> usize {
    starts.partition_point(|&s| s <= at).max(1)
}

/// Find the one place `find` names in `content` and put `replace` there.
/// An empty `find` matches nowhere. Line numbers are the file's own lines,
/// from 1.
pub fn replace_one(content: &str, find: &str, replace: &str) -> Found {
    if find.is_empty() {
        return Found::None;
    }
    let source = Source::parse(content);
    let offsets = line_starts(&source);
    match occurrences(content, find).as_slice() {
        [at] => {
            let mut text = String::with_capacity(content.len() + replace.len());
            text.push_str(&content[..*at]);
            text.push_str(replace);
            text.push_str(&content[at + find.len()..]);
            return Found::One {
                text,
                step: Step::Exact,
                line: line_of(&offsets, *at),
            };
        }
        [] => {}
        many => return Found::Many(many.iter().map(|&at| line_of(&offsets, at)).collect()),
    }

    let lines = source.texts();
    let pattern = Source::parse(find).texts();
    let steps: [(Step, Key); 3] = [
        (Step::TrimEnd, |l| Cow::Borrowed(l.trim_end())),
        (Step::Trim, |l| Cow::Borrowed(l.trim())),
        (Step::Normalised, |l| Cow::Owned(normalise(l))),
    ];
    for (step, key) in steps {
        match starts(&keys(&lines, key), &keys(&pattern, key)).as_slice() {
            [] => continue,
            [i] => {
                return Found::One {
                    text: splice(&source, *i, pattern.len(), replace),
                    step,
                    line: i + 1,
                }
            }
            many => return Found::Many(many.iter().map(|i| i + 1).collect()),
        }
    }
    Found::None
}

/// Rebuild the file with lines `at..at + len` replaced by `replace`'s
/// lines. As in R1, unchanged lines keep their endings and inserted lines
/// take the file's preferred ending; unlike R1, the last inserted line ends
/// only where the last replaced line did, so no ending is added to a file.
fn splice(source: &Source<'_>, at: usize, len: usize, replace: &str) -> String {
    let mut out = String::new();
    let push = |out: &mut String, l: &Line<'_>| {
        out.push_str(l.text);
        if let Some(e) = l.ending {
            out.push_str(e.as_str());
        }
    };
    for l in &source.lines[..at] {
        push(&mut out, l);
    }
    let inserted = Source::parse(replace).texts();
    let last_ended = source.lines[at + len - 1].ending.is_some();
    for (n, text) in inserted.iter().enumerate() {
        out.push_str(text);
        if n + 1 < inserted.len() || last_ended {
            out.push_str(source.preferred.as_str());
        }
    }
    for l in &source.lines[at + len..] {
        push(&mut out, l);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_keeps_the_rest_and_line_endings() {
        let got = replace_one("a\n  b  \nc\n", "b", "B");
        assert_eq!(
            got,
            Found::One {
                text: "a\n  B  \nc\n".into(),
                step: Step::Exact,
                line: 2
            }
        );
        let got = replace_one("a\n  b  \nc\r\n", "b\nc", "B\nC");
        assert_eq!(
            got,
            Found::One {
                text: "a\nB\nC\n".into(),
                step: Step::Trim,
                line: 2
            }
        );
    }

    #[test]
    fn many_stops_the_ladder() {
        assert_eq!(replace_one("x\ny\nx\n", "x", "z"), Found::Many(vec![1, 3]));
        assert_eq!(
            replace_one(" x \n x \n", "x\n", "z"),
            Found::Many(vec![1, 2])
        );
    }

    #[test]
    fn line_steps_keep_the_file_s_endings() {
        let one = |c: &str, f: &str, r: &str| match replace_one(c, f, r) {
            Found::One { text, .. } => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(one("a\r\n b \r\nc\r\n", "b\nc", "B\nC"), "a\r\nB\r\nC\r\n");
        assert_eq!(one("a\n b", "b\n", "B"), "a\nB");
        assert_eq!(one("a\n b \nc\n", "b\n", ""), "a\nc\n");
        assert_eq!(one("a\rb \rc", "b", "B"), "a\rB \rc");
    }

    #[test]
    fn overlapping_exact_matches_count_as_places() {
        assert_eq!(replace_one("aaa", "aa", "b"), Found::Many(vec![1, 1]));
        assert_eq!(
            replace_one("x\naa\n", "aa", "b"),
            Found::One {
                text: "x\nb\n".into(),
                step: Step::Exact,
                line: 2
            }
        );
    }

    /// A find that matches very many places answers quickly: line numbers
    /// come from one pass over the file, not one per match.
    #[test]
    fn many_matches_in_a_large_file_stay_fast() {
        let content = "e e e e e e e e\r\n".repeat(120_000);
        let began = std::time::Instant::now();
        let Found::Many(lines) = replace_one(&content, "e", "E") else {
            panic!("many places");
        };
        assert_eq!(lines.len(), 960_000);
        assert_eq!((lines[0], lines[8], lines[959_999]), (1, 2, 120_000));
        assert!(began.elapsed().as_secs() < 20);
    }

    #[test]
    fn line_steps_with_a_long_find_stay_fast() {
        // Every start matches all of a long find but its last line: trying
        // each start in turn would cost the file's lines times the find's.
        let content = " x\n".repeat(400_000);
        let find = format!("{}y", "x\n".repeat(20_000));
        let began = std::time::Instant::now();
        assert_eq!(replace_one(&content, &find, "z"), Found::None);
        assert!(began.elapsed().as_secs() < 20);
        // The same search still finds its places.
        let find = "x\n".repeat(399_999);
        assert_eq!(replace_one(&content, &find, "z"), Found::Many(vec![1, 2]));
        let one = format!("{content}  y \n");
        let Found::One { step, line, .. } = replace_one(&one, "x\ny", "z") else {
            panic!("one place");
        };
        assert_eq!((step, line), (Step::Trim, 400_000));
    }

    #[test]
    fn empty_find_and_no_match_are_none() {
        assert_eq!(replace_one("abc", "", "z"), Found::None);
        assert_eq!(replace_one("abc", "q", "z"), Found::None);
    }
}
