// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! What each files tool does once its places are resolved. Every result
//! is built to fit `cap`.

use super::super::ladder::{replace_one, Found, Step};
use super::super::lines::{display_lines, display_starts, Source};
use super::super::text::{clip, date, Out};
use super::super::{ChangeId, FsError, Place};
use super::{flag, number, text, FilesTool, READ_MAX, SHOWN_PATH};
use harness_core::kits::files_errors as code;
use harness_core::Value;
use harness_tools::ToolCtx;

impl FilesTool {
    pub(super) async fn list(
        &self,
        input: &Value,
        cx: &ToolCtx,
        at: &Place,
        cap: usize,
    ) -> Result<String, FsError> {
        let after = text(input, "after");
        let root = text(input, "path").trim().is_empty();
        let rows: Vec<(String, String)> = if root {
            // By label, so that `after` continues a long listing.
            let mut linked = self.fs.linked(&cx.app);
            linked.sort_by(|a, b| a.label.cmp(&b.label));
            linked
                .into_iter()
                .map(|l| {
                    let ro = if l.read_only { " (read only)" } else { "" };
                    (
                        l.label.clone(),
                        format!("{}/{ro}\n", clip(&l.label, SHOWN_PATH)),
                    )
                })
                .collect()
        } else {
            self.fs
                .list(at)
                .await?
                .into_iter()
                .map(|e| {
                    let name = clip(&e.name, SHOWN_PATH);
                    let row = if e.dir {
                        format!("{name}/\n")
                    } else {
                        format!("{name}  {} bytes  {}\n", e.size, date(e.modified))
                    };
                    (e.name, row)
                })
                .collect()
        };
        let title = if root {
            "Linked folders:\n".to_owned()
        } else {
            format!("{}:\n", clip(&at.logical(), SHOWN_PATH))
        };
        let rest: Vec<&(String, String)> = rows
            .iter()
            .filter(|(name, _)| after.is_empty() || name.as_str() > after)
            .collect();
        let more = |left: usize, last: &str| {
            format!("{left} more. Call again with after = {last} to go on.\n")
        };
        // Room for the closing line, which names the last entry shown in
        // full: `after` continues from exactly that name.
        let longest = rest.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        let room = more(rest.len(), &"x".repeat(longest)).len();
        let mut out = Out::new(cap);
        out.add(&title, room);
        for (shown, (name, row)) in rest.iter().enumerate() {
            if !out.add(row, room) {
                if shown == 0 {
                    return Err(FsError::new(
                        code::TOO_LARGE,
                        format!(
                            "A name in this folder is too long to list: {}",
                            clip(name, 80)
                        ),
                    ));
                }
                out.add(&more(rest.len() - shown, &rest[shown - 1].0), 0);
                return Ok(out.buf);
            }
        }
        if rest.is_empty() {
            out.add("(empty)\n", 0);
        }
        Ok(out.buf)
    }

    pub(super) async fn read(
        &self,
        input: &Value,
        at: &Place,
        cap: usize,
    ) -> Result<String, FsError> {
        let content = self.fs.read(at, READ_MAX).await?;
        let lines = display_lines(&content);
        let total = lines.len();
        let path = clip(&at.logical(), SHOWN_PATH);
        if total == 0 {
            return Ok(format!("{path} is empty.\n"));
        }
        let from = number(input, "from_line").unwrap_or(1);
        if from > total {
            return Ok(format!(
                "{path} has {total} lines; line {from} is past the end.\n"
            ));
        }
        let header = |last: usize| {
            let more = if last < total {
                format!(" Go on with from_line = {}.", last + 1)
            } else {
                String::new()
            };
            format!("{path}, lines {from} to {last} of {total}.{more}\n")
        };
        let room = header(total.saturating_sub(1)).len() + 1;
        let want = number(input, "lines").unwrap_or(total);
        let mut out = Out::new(cap);
        let mut last = from - 1;
        for line in lines.iter().skip(from - 1).take(want) {
            if !out.add(line, room) {
                break;
            }
            last += 1;
        }
        Ok(format!("{}{}", header(last), out.buf))
    }

    pub(super) async fn search(
        &self,
        input: &Value,
        at: &Place,
        cap: usize,
    ) -> Result<String, FsError> {
        let query = text(input, "query");
        if query.trim().is_empty() {
            return Ok("Give a query to find.".into());
        }
        let in_text = text(input, "in") == "text";
        let hits = self.fs.search(at, query, in_text).await?;
        if hits.is_empty() {
            return Ok(format!(
                "Nothing under {} matches.\n",
                clip(&at.logical(), SHOWN_PATH)
            ));
        }
        let more = |left: usize| {
            format!(
                "{left} more matches are not shown; search a smaller folder or a longer query.\n"
            )
        };
        let room = more(hits.len()).len();
        let mut out = Out::new(cap);
        for (n, h) in hits.iter().enumerate() {
            let path = clip(&format!("{}/{}", at.label, h.path), SHOWN_PATH);
            let row = match &h.line {
                Some((line, body)) => format!("{path}:{line}: {}\n", clip(body.trim(), 200)),
                None if h.dir => format!("{path}/\n"),
                None => format!("{path}\n"),
            };
            if !out.add(&row, room) {
                out.add(&more(hits.len() - n), 0);
                break;
            }
        }
        Ok(out.buf)
    }

    pub(super) async fn write(
        &self,
        input: &Value,
        at: &Place,
        by: &ChangeId,
    ) -> Result<String, FsError> {
        let append = flag(input, "append");
        let body = text(input, "text");
        let before = self.fs.stat(at).await?.map(|e| e.size);
        self.fs.write(at, body, append, by).await?;
        let path = clip(&at.logical(), SHOWN_PATH);
        let len = body.len();
        Ok(match (before, append) {
            (None, _) => format!("Created {path} ({len} bytes)."),
            (Some(old), true) => format!(
                "Appended {len} bytes to {path} (now {} bytes).",
                old + len as u64
            ),
            (Some(old), false) => format!("Replaced {path} ({len} bytes, was {old})."),
        })
    }

    pub(super) async fn edit(
        &self,
        input: &Value,
        at: &Place,
        by: &ChangeId,
        cap: usize,
    ) -> Result<String, FsError> {
        let (find, replace) = (text(input, "find"), text(input, "replace"));
        let path = clip(&at.logical(), SHOWN_PATH);
        if find == replace {
            return Ok("No change: find and replace are the same.".into());
        }
        let content = self.fs.read(at, READ_MAX).await?;
        match replace_one(&content, find, replace) {
            Found::One { text, step, line } => {
                self.fs.write(at, &text, false, by).await?;
                let how = match step {
                    Step::Exact => "the exact text",
                    Step::TrimEnd => "whole lines, ignoring trailing spaces",
                    Step::Trim => "whole lines, ignoring spaces at line ends",
                    Step::Normalised => "whole lines, ignoring spaces and typographic punctuation",
                };
                let starts = display_starts(&text);
                let Some(first) = starts.get(line - 1).copied() else {
                    return Ok(format!(
                        "Edited {path}, matched by {how}. The matched lines were removed from the end."
                    ));
                };
                let new_lines = display_lines(&text);
                let count = Source::parse(replace).lines.len().max(1);
                let end = starts
                    .get(line - 1 + count)
                    .map_or(new_lines.len(), |s| s - 1)
                    .max(first);
                let head =
                    format!("Edited {path}, matched by {how}. Lines {first} to {end} now read:\n");
                let mut out = Out::new(cap);
                out.add(&head, 0);
                for l in new_lines.iter().take(end).skip(first - 1) {
                    if !out.add(l, 4) {
                        out.add("...\n", 0);
                        break;
                    }
                }
                Ok(out.buf)
            }
            Found::None => Err(FsError::new(
                code::NO_MATCH,
                format!("find matched nowhere in {path}. Copy it from the file."),
            )),
            Found::Many(lines) => {
                let starts = display_starts(&content);
                let mut on: Vec<usize> = lines
                    .iter()
                    .map(|l| starts.get(l - 1).copied().unwrap_or(*l))
                    .collect();
                on.dedup();
                let list: Vec<String> = on.iter().take(20).map(usize::to_string).collect();
                Err(FsError::new(
                    code::MANY_MATCHES,
                    format!(
                        "find matched {} places, on lines {}. Give more lines so it matches one.",
                        lines.len(),
                        list.join(", ")
                    ),
                ))
            }
        }
    }
}
