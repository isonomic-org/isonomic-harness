// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lines, as every files tool counts them. A file's lines keep their own
//! endings (`\n`, `\r\n` or a lone `\r`), and the first ending is the
//! file's preferred one for inserted lines. The line model is ported from
//! R1's `text_file.rs` at its pinned commit (Apache-2.0; see
//! THIRD-PARTY-NOTICES.md).
//!
//! A line longer than [`LONG_LINE`] bytes is shown as several display
//! lines. Every number a files tool shows or takes (the read header,
//! `from_line`, search hits, edit results) counts display lines, which are
//! the file's own lines unless one is that long. Display lines, endings
//! included, concatenate to the file exactly.

/// The longest display line, in bytes.
pub const LONG_LINE: usize = 512;

/// A line ending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// `\n`.
    Lf,
    /// `\r\n`.
    CrLf,
    /// A lone `\r`.
    Cr,
}

impl Ending {
    /// Its text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// One line: its text and its ending, if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line<'a> {
    /// The text, without the ending.
    pub text: &'a str,
    /// The ending; `None` for an unterminated last line.
    pub ending: Option<Ending>,
}

/// A file split into lines, each keeping its ending.
#[derive(Clone, Debug)]
pub struct Source<'a> {
    /// The lines, in order.
    pub lines: Vec<Line<'a>>,
    /// The first ending in the file, or `\n` when it has none.
    pub preferred: Ending,
}

impl<'a> Source<'a> {
    /// Split `content` into lines, keeping each ending.
    pub fn parse(content: &'a str) -> Self {
        let bytes = content.as_bytes();
        let mut lines = Vec::new();
        let mut preferred = None;
        let (mut start, mut at) = (0, 0);
        while at < bytes.len() {
            let (ending, len) = match bytes[at] {
                b'\r' if bytes.get(at + 1) == Some(&b'\n') => (Ending::CrLf, 2),
                b'\r' => (Ending::Cr, 1),
                b'\n' => (Ending::Lf, 1),
                _ => {
                    at += 1;
                    continue;
                }
            };
            preferred.get_or_insert(ending);
            lines.push(Line {
                text: &content[start..at],
                ending: Some(ending),
            });
            at += len;
            start = at;
        }
        if start < content.len() {
            lines.push(Line {
                text: &content[start..],
                ending: None,
            });
        }
        Self {
            lines,
            preferred: preferred.unwrap_or(Ending::Lf),
        }
    }

    /// The lines' texts.
    pub fn texts(&self) -> Vec<&'a str> {
        self.lines.iter().map(|l| l.text).collect()
    }
}

/// Cut one line, ending included, into display lines of at most
/// [`LONG_LINE`] bytes, at character boundaries.
fn cut<'a>(mut rest: &'a str, out: &mut Vec<&'a str>) {
    while rest.len() > LONG_LINE {
        let mut at = LONG_LINE;
        while !rest.is_char_boundary(at) {
            at -= 1;
        }
        // Never part a `\r\n`.
        if rest.as_bytes()[at - 1] == b'\r' && rest.as_bytes().get(at) == Some(&b'\n') {
            at -= 1;
        }
        out.push(&rest[..at]);
        rest = &rest[at..];
    }
    out.push(rest);
}

/// Each of the file's lines with its ending, as one slice.
fn whole_lines(content: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut offset = 0;
    for line in Source::parse(content).lines {
        let len = line.text.len() + line.ending.map_or(0, |e| e.as_str().len());
        out.push(&content[offset..offset + len]);
        offset += len;
    }
    out
}

/// The display lines of `content`, each with its ending where it has one:
/// together they are `content` exactly.
pub fn display_lines(content: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in whole_lines(content) {
        cut(line, &mut out);
    }
    out
}

/// For each of the file's lines (from 0), the display line it starts on
/// (from 1).
pub fn display_starts(content: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut next = 1;
    let mut pieces = Vec::new();
    for line in whole_lines(content) {
        starts.push(next);
        pieces.clear();
        cut(line, &mut pieces);
        next += pieces.len();
    }
    starts
}

/// The display lines that hold `needle` (already in lower case), without
/// case: each one's number (from 1) and its text without the ending. A
/// match that spans two pieces of a long line is reported on the first.
pub fn matching_lines(content: &str, needle: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut number = 0;
    let mut pieces = Vec::new();
    for line in whole_lines(content) {
        pieces.clear();
        cut(line, &mut pieces);
        let first = number + 1;
        number += pieces.len();
        let bare = |p: &str| p.trim_end_matches(['\r', '\n']).to_owned();
        let mut found = false;
        for (i, piece) in pieces.iter().enumerate() {
            if piece.to_lowercase().contains(needle) {
                out.push((first + i, bare(piece)));
                found = true;
            }
        }
        if !found && pieces.len() > 1 && line.to_lowercase().contains(needle) {
            out.push((first, bare(pieces[0])));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endings_are_kept_and_the_first_is_preferred() {
        let s = Source::parse("a\r\nb\rc\nd");
        assert_eq!(s.texts(), ["a", "b", "c", "d"]);
        assert_eq!(s.preferred, Ending::CrLf);
        assert_eq!(s.lines[3].ending, None);
        assert_eq!(Source::parse("").lines.len(), 0);
        assert_eq!(Source::parse("x").preferred, Ending::Lf);
    }

    #[test]
    fn display_lines_cover_the_content_and_agree_with_starts() {
        let long = "é".repeat(LONG_LINE);
        let content = format!("one\r\n{long}\rtwo\nend");
        let d = display_lines(&content);
        assert_eq!(d.concat(), content);
        assert!(d.iter().all(|l| l.len() <= LONG_LINE));
        let starts = display_starts(&content);
        assert_eq!(starts.len(), 4);
        assert_eq!(d[starts[2] - 1], "two\n");
        assert_eq!(d[starts[3] - 1], "end");
        // A `\r\n` is never parted, and a hit names the piece it is in.
        let wide = format!(
            "{}\r\n{}needle\n",
            "a".repeat(LONG_LINE - 1),
            "b".repeat(600)
        );
        let d = display_lines(&wide);
        assert_eq!(d.concat(), wide);
        assert!(d[0].ends_with('a') && d[1] == "\r\n");
        let hits = matching_lines(&wide, "needle");
        assert_eq!(hits.len(), 1);
        assert_eq!(d[hits[0].0 - 1].trim_end(), hits[0].1);
    }
}
