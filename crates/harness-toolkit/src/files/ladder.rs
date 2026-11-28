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
//! `tests/parity/ladder.rs`. Unlike R1, which takes the first match, a step
//! that matches more than one place stops the ladder (deviation: see
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

/// How a step compares a file line with a line of `find`.
type Same = fn(&str, &str) -> bool;

/// A line without its ending.
fn bare(line: &str) -> &str {
    line.strip_suffix('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or(line)
}

/// Every start index where `pattern` matches `lines` under `same`. Like
/// R1's search, a pattern longer than the input matches nowhere.
fn starts(lines: &[&str], pattern: &[&str], same: impl Fn(&str, &str) -> bool) -> Vec<usize> {
    if pattern.is_empty() || pattern.len() > lines.len() {
        return Vec::new();
    }
    (0..=lines.len() - pattern.len())
        .filter(|&i| {
            pattern
                .iter()
                .enumerate()
                .all(|(p, pat)| same(bare(lines[i + p]), pat))
        })
        .collect()
}

/// Find the one place `find` names in `content` and put `replace` there.
/// An empty `find` matches nowhere.
pub fn replace_one(content: &str, find: &str, replace: &str) -> Found {
    if find.is_empty() {
        return Found::None;
    }
    let exact: Vec<usize> = content.match_indices(find).map(|(at, _)| at).collect();
    let line_of = |at: usize| content[..at].matches('\n').count() + 1;
    match exact.as_slice() {
        [at] => {
            let mut text = String::with_capacity(content.len() + replace.len());
            text.push_str(&content[..*at]);
            text.push_str(replace);
            text.push_str(&content[at + find.len()..]);
            return Found::One {
                text,
                step: Step::Exact,
                line: line_of(*at),
            };
        }
        [] => {}
        many => return Found::Many(many.iter().map(|&at| line_of(at)).collect()),
    }

    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let pattern: Vec<&str> = find.lines().collect();
    let steps: [(Step, Same); 3] = [
        (Step::TrimEnd, |a, b| a.trim_end() == b.trim_end()),
        (Step::Trim, |a, b| a.trim() == b.trim()),
        (Step::Normalised, |a, b| normalise(a) == normalise(b)),
    ];
    for (step, same) in steps {
        match starts(&lines, &pattern, same).as_slice() {
            [] => continue,
            [i] => {
                let end = i + pattern.len();
                let mut text: String = lines[..*i].concat();
                text.push_str(replace);
                let had_ending = lines[end - 1].ends_with('\n');
                if had_ending && !replace.is_empty() && !replace.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&lines[end..].concat());
                return Found::One {
                    text,
                    step,
                    line: i + 1,
                };
            }
            many => return Found::Many(many.iter().map(|i| i + 1).collect()),
        }
    }
    Found::None
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
    fn empty_find_and_no_match_are_none() {
        assert_eq!(replace_one("abc", "", "z"), Found::None);
        assert_eq!(replace_one("abc", "q", "z"), Found::None);
    }
}
