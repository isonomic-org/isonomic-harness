// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seven files tools over a [`FileSystem`]. `access` resolves each path
//! once and keeps the places for the call, so the run never looks a name
//! up a second time; the run then works on those places only.

use super::ladder::{replace_one, Found, Step};
use super::{resolve, Entry, FileSystem, FsError, Place};
use harness_core::access::{Access, PreviewItem, Scope};
use harness_core::kits::{files_errors as code, kit_spec, KitSpec};
use harness_core::offer::KitMember;
use harness_core::{future::BoxFuture, CallId, Number, Remember, ScopeKind, Value};
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The default result cap, in bytes, when the capability table gives none.
pub const DEFAULT_CAP: usize = 8 * 1024;
/// The largest file the tools read whole.
pub const READ_MAX: usize = 4 * 1024 * 1024;
/// How many files a text search reads at most.
const SEARCH_FILES: usize = 2_000;

/// One files tool.
pub struct FilesTool {
    spec: &'static KitSpec,
    fs: Arc<dyn FileSystem>,
    places: Mutex<HashMap<CallId, Vec<Place>>>,
}

/// The files kit over `fs`, in the specs' order.
pub fn files_kit(fs: Arc<dyn FileSystem>) -> Vec<Arc<dyn Tool>> {
    harness_core::kits::FILES_SPECS
        .iter()
        .filter_map(|s| kit_spec(s.name))
        .map(|spec| {
            Arc::new(FilesTool {
                spec,
                fs: fs.clone(),
                places: Mutex::new(HashMap::new()),
            }) as Arc<dyn Tool>
        })
        .collect()
}

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

fn number(input: &Value, key: &str) -> Option<usize> {
    match input.get(key) {
        Some(Value::Number(Number::Unsigned(n))) if *n > 0 => usize::try_from(*n).ok(),
        _ => None,
    }
}

fn fail(e: FsError) -> ToolDone {
    ToolDone::error(e.code, &e.message)
}

fn item(label: &str, value: impl Into<String>) -> PreviewItem {
    PreviewItem {
        label: label.into(),
        value: value.into(),
    }
}

/// Split `content` into display lines: its lines, with any line longer
/// than `width` bytes split at character boundaries. Pages count display
/// lines, so every page fits and pages cover the text with no gap.
fn display_lines(content: &str, width: usize) -> Vec<&str> {
    let mut out = Vec::new();
    for line in content.lines() {
        let mut rest = line;
        while rest.len() > width {
            let mut cut = width;
            while !rest.is_char_boundary(cut) {
                cut -= 1;
            }
            out.push(&rest[..cut]);
            rest = &rest[cut..];
        }
        out.push(rest);
    }
    out
}

impl FilesTool {
    fn name(&self) -> &'static str {
        self.spec.name
    }

    fn changes(&self) -> bool {
        !self.spec.core
    }

    fn cap(cx: &ToolCtx) -> usize {
        cx.result_cap.unwrap_or(DEFAULT_CAP).max(256)
    }

    /// Resolve the call's paths and check what a change may touch.
    fn places(&self, input: &Value, cx: &ToolCtx) -> Result<Vec<Place>, FsError> {
        let linked = self.fs.linked(&cx.app);
        let at = |p: &str| resolve(&linked, p);
        let places = match self.name() {
            "list_dir" if text(input, "path").trim().is_empty() => {
                let first = linked
                    .first()
                    .ok_or_else(|| FsError::new(code::NOT_LINKED, "No folders are linked."))?;
                vec![Place {
                    folder: first.id.clone(),
                    label: first.label.clone(),
                    read_only: first.read_only,
                    path: String::new(),
                }]
            }
            "move_path" => {
                let (from, to) = (at(text(input, "from"))?, at(text(input, "to"))?);
                if from.folder != to.folder {
                    return Err(FsError::new(
                        code::CROSS_FOLDER,
                        "Moves stay inside one linked folder.",
                    ));
                }
                vec![from, to]
            }
            _ => vec![at(text(input, "path"))?],
        };
        if self.changes() {
            for p in &places {
                if p.read_only {
                    return Err(FsError::new(
                        code::READ_ONLY,
                        format!("{} is linked for reading only.", p.label),
                    ));
                }
                if p.protected() || p.path.is_empty() {
                    return Err(FsError::new(
                        code::PROTECTED,
                        format!("{} is not changed by these tools.", p.logical()),
                    ));
                }
            }
        } else if places.iter().any(Place::in_vcs) {
            return Err(FsError::new(
                code::PROTECTED,
                "Version-control metadata is not read by these tools.",
            ));
        }
        Ok(places)
    }

    fn preview(&self, input: &Value, places: &[Place]) -> Vec<PreviewItem> {
        let path = places[0].logical();
        match self.name() {
            "write_file" => {
                let mode = if input.get("append") == Some(&Value::Bool(true)) {
                    "Add to the end"
                } else {
                    "Create or replace"
                };
                vec![
                    item("Path", path),
                    item(mode, format!("{} bytes", text(input, "text").len())),
                ]
            }
            "edit_file" => vec![
                item("Path", path),
                item("Find", text(input, "find")),
                item("Replace with", text(input, "replace")),
            ],
            "move_path" => vec![item("From", path), item("To", places[1].logical())],
            "trash_path" => vec![item("Move to the Trash", path)],
            _ => vec![item("Path", path)],
        }
    }

    async fn run(&self, input: &Value, cx: &ToolCtx, places: Vec<Place>) -> ToolDone {
        let at = &places[0];
        let done = match self.name() {
            "list_dir" => self.list(input, cx, at).await,
            "read_file" => self.read(input, cx, at).await,
            "search_files" => self.search(input, cx, at).await,
            "write_file" => {
                let append = input.get("append") == Some(&Value::Bool(true));
                let body = text(input, "text");
                self.fs.write(at, body, append).await.map(|()| {
                    let verb = if append { "Added" } else { "Wrote" };
                    format!("{verb} {} bytes to {}.", body.len(), at.logical())
                })
            }
            "edit_file" => self.edit(input, at).await,
            "move_path" => {
                let to = &places[1];
                self.fs
                    .rename(at, to)
                    .await
                    .map(|()| format!("Moved {} to {}.", at.logical(), to.logical()))
            }
            "trash_path" => self
                .fs
                .trash(at)
                .await
                .map(|()| format!("Moved {} to the Trash.", at.logical())),
            other => Ok(format!("{other} is not a files tool.")),
        };
        match done {
            Ok(t) => ToolDone::ok(t),
            Err(e) => fail(e),
        }
    }

    async fn list(&self, input: &Value, cx: &ToolCtx, at: &Place) -> Result<String, FsError> {
        let cap = Self::cap(cx);
        if text(input, "path").trim().is_empty() {
            let mut out = String::from("Linked folders:\n");
            for l in self.fs.linked(&cx.app) {
                let ro = if l.read_only { " (read only)" } else { "" };
                out.push_str(&format!("{}/{ro}\n", l.label));
            }
            return Ok(out);
        }
        let after = text(input, "after");
        let entries: Vec<Entry> = self.fs.list(at).await?;
        let mut out = format!("{}:\n", at.logical());
        let rest: Vec<&Entry> = entries
            .iter()
            .filter(|e| after.is_empty() || e.name.as_str() > after)
            .collect();
        for (shown, e) in rest.iter().enumerate() {
            let line = if e.dir {
                format!("{}/\n", e.name)
            } else {
                format!("{}  {} bytes  {}\n", e.name, e.size, e.modified)
            };
            if shown > 0 && out.len() + line.len() + 80 > cap {
                let last = &rest[shown - 1].name;
                out.push_str(&format!(
                    "{} more. Call again with after = {last} to go on.\n",
                    rest.len() - shown
                ));
                return Ok(out);
            }
            out.push_str(&line);
        }
        if rest.is_empty() {
            out.push_str("(empty)\n");
        }
        Ok(out)
    }

    async fn read(&self, input: &Value, cx: &ToolCtx, at: &Place) -> Result<String, FsError> {
        let content = self.fs.read(at, READ_MAX).await?;
        let cap = Self::cap(cx);
        let lines = display_lines(&content, cap / 2);
        let total = lines.len();
        let from = number(input, "from_line").unwrap_or(1);
        if total == 0 {
            return Ok(format!("{} is empty.\n", at.logical()));
        }
        if from > total {
            return Ok(format!(
                "{} has {total} lines; line {from} is past the end.\n",
                at.logical()
            ));
        }
        let want = number(input, "lines").unwrap_or(total);
        let header_room = 160;
        let mut body = String::new();
        let mut last = from - 1;
        for line in lines.iter().skip(from - 1).take(want) {
            if body.len() + line.len() + 1 + header_room > cap && last >= from {
                break;
            }
            body.push_str(line);
            body.push('\n');
            last += 1;
        }
        let more = if last < total {
            format!(" Go on with from_line = {}.", last + 1)
        } else {
            String::new()
        };
        Ok(format!(
            "{}, lines {from} to {last} of {total}.{more}\n{body}",
            at.logical()
        ))
    }

    async fn search(&self, input: &Value, cx: &ToolCtx, at: &Place) -> Result<String, FsError> {
        let cap = Self::cap(cx);
        let query = text(input, "query");
        if query.is_empty() {
            return Ok("Give a query to find.".into());
        }
        let in_text = text(input, "in") == "text";
        let needle = query.to_lowercase();
        let mut out = String::new();
        let mut hits = 0;
        let mut read = 0;
        let mut stack = vec![at.clone()];
        while let Some(dir) = stack.pop() {
            for e in self.fs.list(&dir).await? {
                let path = if dir.path.is_empty() {
                    e.name.clone()
                } else {
                    format!("{}/{}", dir.path, e.name)
                };
                let child = Place {
                    path,
                    ..dir.clone()
                };
                if child.in_vcs() {
                    continue;
                }
                let mut found = Vec::new();
                if !in_text && e.name.to_lowercase().contains(&needle) {
                    found.push(format!(
                        "{}{}\n",
                        child.logical(),
                        if e.dir { "/" } else { "" }
                    ));
                }
                if e.dir {
                    stack.push(child);
                } else if in_text && read < SEARCH_FILES {
                    read += 1;
                    if let Ok(body) = self.fs.read(&child, READ_MAX).await {
                        for (n, line) in body.lines().enumerate() {
                            if line.to_lowercase().contains(&needle) {
                                let shown: String = line.trim().chars().take(200).collect();
                                found.push(format!("{}:{}: {shown}\n", child.logical(), n + 1));
                            }
                        }
                    }
                }
                for f in found {
                    if out.len() + f.len() + 80 > cap {
                        out.push_str("More matches are not shown; search a smaller folder.\n");
                        return Ok(out);
                    }
                    out.push_str(&f);
                    hits += 1;
                }
            }
        }
        if hits == 0 {
            return Ok(format!("Nothing under {} matches {query}.\n", at.logical()));
        }
        Ok(out)
    }

    async fn edit(&self, input: &Value, at: &Place) -> Result<String, FsError> {
        let (find, replace) = (text(input, "find"), text(input, "replace"));
        if find == replace {
            return Ok("No change: find and replace are the same.".into());
        }
        let content = self.fs.read(at, READ_MAX).await?;
        match replace_one(&content, find, replace) {
            Found::One { text, step, line } => {
                self.fs.write(at, &text, false).await?;
                let how = match step {
                    Step::Exact => "",
                    Step::TrimEnd => ", ignoring trailing spaces",
                    Step::Trim => ", ignoring spaces at line ends",
                    Step::Normalised => ", ignoring spaces and typographic punctuation",
                };
                Ok(format!("Edited {} at line {line}{how}.", at.logical()))
            }
            Found::None => Err(FsError::new(
                code::NO_MATCH,
                format!(
                    "find matched nowhere in {}. Copy it from the file.",
                    at.logical()
                ),
            )),
            Found::Many(lines) => {
                let list: Vec<String> = lines.iter().map(usize::to_string).collect();
                Err(FsError::new(
                    code::MANY_MATCHES,
                    format!(
                        "find matched {} places, at lines {}. Give more lines so it matches one.",
                        lines.len(),
                        list.join(", ")
                    ),
                ))
            }
        }
    }
}

impl Tool for FilesTool {
    fn spec(&self) -> harness_core::ToolSpec {
        self.spec.tool_spec()
    }

    fn kit(&self) -> Option<KitMember> {
        Some(self.spec.member())
    }

    fn offered(&self, app: &harness_core::AppId) -> bool {
        !self.fs.linked(app).is_empty()
    }

    fn access<'a>(
        &'a self,
        input: &'a Value,
        cx: &'a ToolCtx,
    ) -> Option<BoxFuture<'a, Result<Access, ToolDone>>> {
        Some(Box::pin(async move {
            let places = self.places(input, cx).map_err(fail)?;
            let first = &places[0];
            let access = Access {
                scope: Scope {
                    kind: ScopeKind::Folder,
                    id: first.folder.clone(),
                    label: first.label.clone(),
                },
                preview: self.preview(input, &places),
                reversible: !self.changes() || self.fs.reversible(&first.folder),
                remember: Remember::Session,
                confined: false,
                effects: None,
            };
            self.places
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(cx.call.clone(), places);
            Ok(access)
        }))
    }

    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let kept = self
                .places
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&cx.call);
            let places = match kept {
                Some(p) => p,
                None => {
                    return ToolOutput::Done(ToolDone::error(
                        "tool.unresolved",
                        "The call ran without its access check.",
                    ))
                }
            };
            ToolOutput::Done(self.run(input, cx, places).await)
        })
    }
}
