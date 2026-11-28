// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! kits-files: the files kit on one linked folder and one read-only one.
//! A listing and a paged read run under a read grant without asking; an
//! edit under an ask-first grant is approved for the session, and a second
//! edit runs without asking; a write to the read-only folder and a find
//! that matches twice fail as results. The results are what the tools
//! return over [`SEED`]; harness-toolkit's tests replay the calls to check.

use super::build::*;
use crate::recorder::{Ctx, Recorder};
use harness_core::access::{PreviewItem, Scope};
use harness_core::kits::FILES_SPECS;
use harness_core::*;

/// The linked folders, as (id, label, read only).
pub const LINKS: &[(&str, &str, bool)] = &[("f1", "Design", false), ("f2", "Archive", true)];

/// The files before the session, as (folder id, path, text).
pub const SEED: &[(&str, &str, &str)] = &[
    (
        "f1",
        "brief.md",
        "Brief\n\nGoal: a calmer home screen.\nOwner: Sam\n",
    ),
    ("f1", "icons/home.svg", "<svg/>"),
    ("f2", "old.md", "Old notes\n"),
];

/// The result cap the session's tools were given.
pub const CAP: usize = 8 * 1024;

fn spec(name: &str) -> ToolSpec {
    FILES_SPECS
        .iter()
        .find(|s| s.name == name)
        .map(|s| s.tool_spec())
        .unwrap_or_else(|| panic!("{name} is a files tool"))
}

fn args<const N: usize>(pairs: [(&str, &str); N]) -> Value {
    Value::object(pairs.map(|(k, v)| (k, Value::from(v))))
}

fn design() -> Scope {
    Scope {
        kind: ScopeKind::Folder,
        id: "f1".into(),
        label: "Design".into(),
    }
}

fn failed(call: &str, code: &str, message: &str) -> ToolResult {
    ToolResult {
        error: Some(ErrorInfo::new(code, message)),
        ..result(call, ToolOutcome::Error, text(message))
    }
}

fn reply_and_end(r: &mut Recorder, s: &mut Ctx, answer: &str) {
    r.step(s, Purpose::Reply, None);
    r.reply(s, text(answer), StopReason::End, None);
    r.end_step(s, StepOutcome::Complete);
    r.end_turn(s, TurnStop::Done, None);
}

fn edit(r: &mut Recorder, s: &mut Ctx, id: &str, find: &str, replace: &str) -> Value {
    let input = args([
        ("path", "Design/brief.md"),
        ("find", find),
        ("replace", replace),
    ]);
    r.call(s, call(id, &spec("edit_file"), input.clone(), None));
    input
}

/// The fixture's events.
pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);
    let offered: Vec<ToolSpec> = FILES_SPECS.iter().map(|k| k.tool_spec()).collect();
    let folders = vec!["Design".to_string(), "Archive".to_string()];

    // Turn 1: read without asking.
    r.turn(&mut s, Cause::User, None);
    r.user(
        &mut s,
        "What is in my design folder, and what does the brief say?",
    );
    r.context_with(&mut s, None, folders.clone());
    r.step(&mut s, Purpose::Reply, Some(offered));
    r.reply(&mut s, text("Let me look."), StopReason::ToolUse, None);
    r.call(
        &mut s,
        call(
            "cal_0001",
            &spec("list_dir"),
            args([("path", "Design")]),
            Some(1),
        ),
    );
    let read = Value::object([
        ("path", Value::from("Design/brief.md")),
        ("lines", Value::from(3u64)),
    ]);
    r.call(&mut s, call("cal_0002", &spec("read_file"), read, Some(1)));
    r.result(
        &mut s,
        result(
            "cal_0001",
            ToolOutcome::Ok,
            text("Design:\nbrief.md  46 bytes  0\nicons/\n"),
        ),
        40,
    );
    r.result(
        &mut s,
        result(
            "cal_0002",
            ToolOutcome::Ok,
            text("Design/brief.md, lines 1 to 3 of 4. Go on with from_line = 4.\nBrief\n\nGoal: a calmer home screen.\n"),
        ),
        40,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    reply_and_end(
        &mut r,
        &mut s,
        "Design holds brief.md and an icons folder. The brief's goal is a calmer home screen.",
    );

    // Turn 2: an edit under an ask-first grant, approved for the session.
    r.advance(20_000);
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "Make Alex the owner.");
    r.context_with(&mut s, None, folders.clone());
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Changing the owner."),
        StopReason::ToolUse,
        None,
    );
    let input = edit(&mut r, &mut s, "cal_0003", "Owner: Sam", "Owner: Alex");
    let approval = ApprovalId::new(r.id("apv"));
    let summary = format!(
        "edit_file {}",
        serde_json::to_string(&input).unwrap_or_default()
    );
    r.emit(
        &s,
        Event::ApprovalRequest(ApprovalRequest {
            approval: approval.clone(),
            call: Some(CallId::from("cal_0003")),
            summary,
            rule: ApprovalRule::Grant,
            effects: vec![Effect::ModifiesData],
            expires_at: Some(r.now() + 300_000),
            scope: Some(design()),
            preview: vec![
                PreviewItem {
                    label: "Path".into(),
                    value: "Design/brief.md".into(),
                },
                PreviewItem {
                    label: "Find".into(),
                    value: "Owner: Sam".into(),
                },
                PreviewItem {
                    label: "Replace with".into(),
                    value: "Owner: Alex".into(),
                },
            ],
            reversible: Some(true),
            remember: Some(Remember::Session),
        }),
    );
    r.advance(3_000);
    r.emit(
        &s,
        Event::ApprovalDecision(ApprovalDecision {
            approval,
            decision: Decision::Allow,
            basis: Basis::User,
            by: by(DeciderScope::Shell, None),
            answer: None,
            reason: None,
            remember: Some(Remember::Session),
        }),
    );
    r.result(
        &mut s,
        result(
            "cal_0003",
            ToolOutcome::Ok,
            text("Edited Design/brief.md at line 4."),
        ),
        30,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    reply_and_end(&mut r, &mut s, "Alex is now the owner in the brief.");

    // Turn 3: remembered for the session; then two failures as results.
    r.advance(20_000);
    r.turn(&mut s, Cause::User, None);
    r.user(
        &mut s,
        "Say calm instead of calmer, and copy the brief to Archive.",
    );
    r.context_with(&mut s, None, folders);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(&mut s, text("On it."), StopReason::ToolUse, None);
    edit(&mut r, &mut s, "cal_0004", "calmer", "calm");
    r.result(
        &mut s,
        result(
            "cal_0004",
            ToolOutcome::Ok,
            text("Edited Design/brief.md at line 3."),
        ),
        30,
    );
    r.call(
        &mut s,
        call(
            "cal_0005",
            &spec("write_file"),
            args([("path", "Archive/brief.md"), ("text", "Brief\n")]),
            None,
        ),
    );
    r.result(
        &mut s,
        failed(
            "cal_0005",
            "files.read_only",
            "Archive is linked for reading only.",
        ),
        0,
    );
    edit(&mut r, &mut s, "cal_0006", "e", "E");
    r.result(
        &mut s,
        failed(
            "cal_0006",
            "files.many_matches",
            "find matched 6 places, on lines 1, 3, 4. Give more lines so it matches one.",
        ),
        10,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    reply_and_end(
        &mut r,
        &mut s,
        "The brief now says calm. Archive is linked for reading only, so I could not copy it there.",
    );
    r.finish()
}
