// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the two files-kit fixtures share: the linked folders, the files
//! before each session, and small builders. The results in kits-files-read
//! and kits-files-write are what the tools return over this seed;
//! harness-toolkit's tests replay every recorded call to check.

use super::build::*;
use crate::recorder::{Ctx, Recorder};
use harness_core::access::{PreviewItem, Scope};
use harness_core::kits::FILES_SPECS;
use harness_core::*;

/// The linked folders, as (id, label, read only, changes can be undone).
pub const LINKS: &[(&str, &str, bool, bool)] = &[
    ("f1", "Design", false, true),
    ("f2", "Archive", true, true),
    ("f3", "Drafts", false, true),
    ("f4", "Camera", false, false),
];

/// The files before each session, as (folder id, path, text).
pub const SEED: &[(&str, &str, &str)] = &[
    (
        "f1",
        "brief.md",
        "Brief\n\nGoal: a calmer home screen.\nOwner: Sam\nReview: Friday\n",
    ),
    ("f1", "icons/home.svg", "<svg/>"),
    ("f1", "icons/logo.bin", "\u{0}\u{1}"),
    ("f1", "notes/homework.md", "Check the home icon.\n"),
    ("f2", "old.md", "Old notes\n"),
    ("f3", "plan.md", "Plan\n"),
    ("f4", "shots.md", "Shot list\n"),
];

/// The result cap the sessions' tools were given.
pub const CAP: usize = 8 * 1024;

pub(super) fn spec(name: &str) -> ToolSpec {
    FILES_SPECS
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.tool_spec())
        .unwrap_or_else(|| panic!("{name} is a files tool"))
}

pub(super) fn offered() -> Vec<ToolSpec> {
    FILES_SPECS.iter().map(|k| k.tool_spec()).collect()
}

pub(super) fn folders() -> Vec<String> {
    LINKS
        .iter()
        .map(|(_, label, ..)| (*label).to_owned())
        .collect()
}

pub(super) fn args<const N: usize>(pairs: [(&str, &str); N]) -> Value {
    Value::object(pairs.map(|(k, v)| (k, Value::from(v))))
}

pub(super) fn scope(id: &str) -> Scope {
    let label = LINKS.iter().find(|l| l.0 == id).map_or(id, |l| l.1);
    Scope {
        kind: ScopeKind::Folder,
        id: id.into(),
        label: label.into(),
    }
}

pub(super) fn items<const N: usize>(pairs: [(&str, &str); N]) -> Vec<PreviewItem> {
    pairs
        .map(|(label, value)| PreviewItem {
            label: label.into(),
            value: value.into(),
        })
        .to_vec()
}

/// The meta a files result carries: the scope, the backend, the path (and
/// where a move went) and, for a change that was made (`changed`) and can
/// be undone, the call that holds its pre-image.
pub(super) fn meta(id: &str, tool: &str, input: &Value, changed: bool) -> Option<Value> {
    let field = |k: &str| input.get(k).and_then(Value::as_str);
    let path = field("path").or(field("from")).unwrap_or("");
    let label = path.split('/').next().filter(|l| !l.is_empty());
    let link = LINKS
        .iter()
        .find(|l| Some(l.1) == label)
        .unwrap_or(&LINKS[0]);
    let shown = if path.is_empty() { link.1 } else { path };
    let mut facts = vec![
        ("scope", Value::from(link.0)),
        ("backend", Value::from("host")),
        ("path", Value::from(shown)),
    ];
    if let Some(to) = field("to") {
        facts.push(("to", Value::from(to)));
    }
    let changes = FILES_SPECS.iter().any(|s| s.name == tool && !s.core);
    if changed && changes && link.3 {
        facts.push(("pre_image", Value::from(id)));
    }
    Some(Value::object(facts))
}

/// A call and its successful result.
pub(super) fn ok(r: &mut Recorder, s: &mut Ctx, id: &str, tool: &str, input: Value, out: &str) {
    let done = ToolResult {
        meta: meta(id, tool, &input, true),
        ..result(id, ToolOutcome::Ok, text(out))
    };
    r.call(s, call(id, &spec(tool), input, None));
    r.result(s, done, 30);
}

/// A call and its failure. A failure to resolve (the path is not plainly
/// inside a writable linked folder) happens before the run and carries no
/// meta; any other failure is the run's.
pub(super) fn fails(
    r: &mut Recorder,
    s: &mut Ctx,
    id: &str,
    tool: &str,
    input: Value,
    code: &str,
    message: &str,
) {
    let unresolved = [
        "files.not_linked",
        "files.outside_folder",
        "files.read_only",
        "files.protected",
    ]
    .contains(&code);
    let failed = ToolResult {
        error: Some(ErrorInfo::new(code, message)),
        meta: if unresolved {
            None
        } else {
            meta(id, tool, &input, false)
        },
        ..result(id, ToolOutcome::Error, text(message))
    };
    r.call(s, call(id, &spec(tool), input, None));
    r.result(s, failed, 0);
}

pub(super) fn open(r: &mut Recorder, s: &mut Ctx, says: &str, first: bool) {
    r.turn(s, Cause::User, None);
    r.user(s, says);
    r.context_with(s, None, folders());
    r.step(s, Purpose::Reply, first.then(offered));
    r.reply(s, text("Let me look."), StopReason::ToolUse, None);
}

pub(super) fn close(r: &mut Recorder, s: &mut Ctx, answer: &str) {
    r.end_step(s, StepOutcome::ToolCalls);
    r.step(s, Purpose::Reply, None);
    r.reply(s, text(answer), StopReason::End, None);
    r.end_step(s, StepOutcome::Complete);
    r.end_turn(s, TurnStop::Done, None);
    r.advance(20_000);
}
