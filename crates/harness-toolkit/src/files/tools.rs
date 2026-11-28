// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seven files tools over a [`FileSystem`]. `access` resolves each path
//! once and keeps the places for that session's call, so the run never
//! looks a name up a second time; the service checks them again as it
//! acts. Every result fits the session's cap (invariant 9).

use super::text::clip;
use super::{resolve, FileSystem, FsError, Place};
use harness_core::access::{Access, PreviewItem, Scope};
use harness_core::kits::{files_errors as code, kit_spec, KitSpec};
use harness_core::offer::KitMember;
use harness_core::{future::BoxFuture, CallId, Number, Remember, ScopeKind, SessionId, Value};
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The default result cap, in bytes, when the capability table gives none.
pub const DEFAULT_CAP: usize = 8 * 1024;
/// The smallest cap the tools work to; a smaller one is raised to it.
pub const MIN_CAP: usize = 1024;
/// The largest file the tools read whole.
pub const READ_MAX: usize = 4 * 1024 * 1024;
/// Resolved calls kept at most; the oldest is dropped first.
const KEPT: usize = 256;
/// The most of a path a header or a listing line shows.
const SHOWN_PATH: usize = 160;

mod run;

type Key = (SessionId, CallId);

/// One files tool.
pub struct FilesTool {
    spec: &'static KitSpec,
    fs: Arc<dyn FileSystem>,
    places: Mutex<VecDeque<(Key, Vec<Place>)>>,
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
                places: Mutex::new(VecDeque::new()),
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

fn flag(input: &Value, key: &str) -> bool {
    input.get(key) == Some(&Value::Bool(true))
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

impl FilesTool {
    fn name(&self) -> &'static str {
        self.spec.name
    }

    fn changes(&self) -> bool {
        !self.spec.core
    }

    fn cap(cx: &ToolCtx) -> usize {
        cx.result_cap.unwrap_or(DEFAULT_CAP).max(MIN_CAP)
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, VecDeque<(Key, Vec<Place>)>> {
        self.places.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Resolve the call's paths and check what a change may touch.
    fn places(&self, input: &Value, cx: &ToolCtx) -> Result<Vec<Place>, FsError> {
        let linked = self.fs.linked(&cx.app);
        let at = |p: &str| resolve(&cx.app, &linked, p);
        let places = match self.name() {
            "list_dir" if text(input, "path").trim().is_empty() => {
                let first = linked
                    .first()
                    .ok_or_else(|| FsError::new(code::NOT_LINKED, "No folders are linked."))?;
                vec![Place {
                    app: cx.app.clone(),
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
        if self.name() == "search_files" && !["", "names", "text"].contains(&text(input, "in")) {
            return Err(FsError::new("tool.input", "in is names or text."));
        }
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
                let mode = if flag(input, "append") {
                    "Add to the end"
                } else {
                    "Create or replace with"
                };
                vec![item("Path", path), item(mode, text(input, "text"))]
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

    /// Facts for the person's view and the audit, never for the model:
    /// the scope, the backend, the path (and where a move went), and for a
    /// change that can be undone, the call that holds its pre-image.
    fn meta(&self, cx: &ToolCtx, places: &[Place]) -> Value {
        let at = &places[0];
        let mut facts = vec![
            ("scope", Value::from(at.folder.as_str())),
            ("backend", Value::from("host")),
            ("path", Value::from(at.logical())),
        ];
        if let Some(to) = places.get(1) {
            facts.push(("to", Value::from(to.logical())));
        }
        if self.changes() && self.fs.reversible(&at.folder) {
            facts.push(("pre_image", Value::from(cx.call.as_str())));
        }
        Value::object(facts)
    }

    async fn run(&self, input: &Value, cx: &ToolCtx, places: Vec<Place>) -> ToolDone {
        let cap = Self::cap(cx);
        let at = &places[0];
        let done = match self.name() {
            "list_dir" => self.list(input, cx, at, cap).await,
            "read_file" => self.read(input, at, cap).await,
            "search_files" => self.search(input, at, cap).await,
            "write_file" => self.write(input, at, &cx.call).await,
            "edit_file" => self.edit(input, at, &cx.call, cap).await,
            "move_path" => {
                let to = &places[1];
                self.fs
                    .rename(at, to, &cx.call)
                    .await
                    .map(|()| format!("Moved to {}.", clip(&to.logical(), SHOWN_PATH)))
            }
            "trash_path" => self
                .fs
                .trash(at, &cx.call)
                .await
                .map(|()| format!("Moved {} to the Trash.", clip(&at.logical(), SHOWN_PATH))),
            other => Ok(format!("{other} is not a files tool.")),
        };
        match done {
            Ok(t) => ToolDone::ok(clip(&t, cap)),
            Err(e) => fail(FsError::new(e.code, clip(&e.message, cap))),
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
            let places = self
                .places(input, cx)
                .map_err(|e| fail(FsError::new(e.code, clip(&e.message, Self::cap(cx)))))?;
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
            let key = (cx.session.clone(), cx.call.clone());
            let mut kept = self.kept();
            kept.retain(|(k, _)| *k != key);
            if kept.len() >= KEPT {
                kept.pop_front();
            }
            kept.push_back((key, places));
            Ok(access)
        }))
    }

    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let key = (cx.session.clone(), cx.call.clone());
            let places = {
                let mut kept = self.kept();
                let i = kept.iter().position(|(k, _)| *k == key);
                i.and_then(|i| kept.remove(i)).map(|(_, p)| p)
            };
            let Some(places) = places else {
                return ToolOutput::Done(ToolDone::error(
                    "tool.unresolved",
                    "The call ran without its access check.",
                ));
            };
            if places.iter().any(|p| p.app != cx.app) {
                return ToolOutput::Done(ToolDone::error(
                    "tool.unresolved",
                    "The call's places were resolved for another app.",
                ));
            }
            let meta = self.meta(cx, &places);
            let mut done = self.run(input, cx, places).await;
            done.meta = Some(meta);
            ToolOutput::Done(done)
        })
    }
}
