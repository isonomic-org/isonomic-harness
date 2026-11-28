// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! kits-files-write: the files kit's four changing tools under ask-first
//! grants. An edit is approved for the session, and the next change in
//! that folder runs without asking; a change in another folder asks again;
//! a change that cannot be undone asks and offers "once" only, and the next
//! one there asks again; a denied change is not made; and a read-only
//! folder and a find that matches twice fail as results without asking.

use super::build::*;
use super::kits_files::*;
use crate::recorder::{Ctx, Recorder};
use harness_core::access::PreviewItem;
use harness_core::*;

struct Ask<'a> {
    id: &'a str,
    tool: &'a str,
    input: Value,
    folder: &'a str,
    preview: Vec<PreviewItem>,
    reversible: bool,
}

/// A call that asks; the person answers `decision`, remembering `kept`.
fn asked(r: &mut Recorder, s: &mut Ctx, ask: Ask<'_>, decision: Decision, kept: Option<Remember>) {
    let tool = spec(ask.tool);
    r.call(s, call(ask.id, &tool, ask.input.clone(), None));
    let approval = ApprovalId::new(r.id("apv"));
    let summary = format!(
        "{} {}",
        ask.tool,
        serde_json::to_string(&ask.input).unwrap_or_default()
    );
    let offer = if ask.reversible {
        Remember::Session
    } else {
        Remember::Once
    };
    r.emit(
        s,
        Event::ApprovalRequest(ApprovalRequest {
            approval: approval.clone(),
            call: Some(CallId::from(ask.id)),
            summary,
            rule: ApprovalRule::Grant,
            effects: vec![Effect::ModifiesData],
            expires_at: Some(r.now() + 300_000),
            scope: Some(scope(ask.folder)),
            preview: ask.preview,
            reversible: Some(ask.reversible),
            remember: Some(offer),
        }),
    );
    r.advance(3_000);
    r.emit(
        s,
        Event::ApprovalDecision(ApprovalDecision {
            approval,
            decision,
            basis: Basis::User,
            by: by(DeciderScope::Shell, None),
            answer: None,
            reason: None,
            remember: kept,
        }),
    );
}

fn done(r: &mut Recorder, s: &mut Ctx, id: &str, tool: &str, input: &Value, out: &str) {
    let done = ToolResult {
        meta: meta(id, tool, input, true),
        ..result(id, ToolOutcome::Ok, text(out))
    };
    r.result(s, done, 30);
}

/// The fixture's events.
pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);

    // Turn 1: asks, approved for the session.
    open(&mut r, &mut s, "Make Alex the owner of the brief.", true);
    let edit = |find: &str, replace: &str| {
        args([
            ("path", "Design/brief.md"),
            ("find", find),
            ("replace", replace),
        ])
    };
    asked(
        &mut r,
        &mut s,
        Ask {
            id: "cal_0001",
            tool: "edit_file",
            input: edit("Owner: Sam", "Owner: Alex"),
            folder: "f1",
            preview: items([
                ("Path", "Design/brief.md"),
                ("Find", "Owner: Sam"),
                ("Replace with", "Owner: Alex"),
            ]),
            reversible: true,
        },
        Decision::Allow,
        Some(Remember::Session),
    );
    done(
        &mut r,
        &mut s,
        "cal_0001",
        "edit_file",
        &edit("Owner: Sam", "Owner: Alex"),
        "Edited Design/brief.md, matched by the exact text. Lines 4 to 4 now read:\nOwner: Alex\n",
    );
    close(&mut r, &mut s, "Alex is now the owner in the brief.");

    // Turn 2: the same folder runs without asking; failures are results.
    open(
        &mut r,
        &mut s,
        "Say calm, not calmer. Add a status file, tidy the icons, and copy the brief to Archive.",
        false,
    );
    ok(
        &mut r,
        &mut s,
        "cal_0002",
        "edit_file",
        edit("calmer", "calm"),
        "Edited Design/brief.md, matched by the exact text. Lines 3 to 3 now read:\nGoal: a calm home screen.\n",
    );
    let status = args([("path", "Design/status.md"), ("text", "Status: draft\n")]);
    ok(
        &mut r,
        &mut s,
        "cal_0003",
        "write_file",
        status,
        "Created Design/status.md (14 bytes).",
    );
    let more = Value::object([
        ("path", Value::from("Design/status.md")),
        ("text", Value::from("Next: review\n")),
        ("append", Value::Bool(true)),
    ]);
    ok(
        &mut r,
        &mut s,
        "cal_0004",
        "write_file",
        more,
        "Appended 13 bytes to Design/status.md (now 27 bytes).",
    );
    let mv = args([
        ("from", "Design/icons/home.svg"),
        ("to", "Design/art/home.svg"),
    ]);
    ok(
        &mut r,
        &mut s,
        "cal_0005",
        "move_path",
        mv,
        "Moved to Design/art/home.svg.",
    );
    ok(
        &mut r,
        &mut s,
        "cal_0006",
        "trash_path",
        args([("path", "Design/icons/logo.bin")]),
        "Moved Design/icons/logo.bin to the Trash.",
    );
    let copy = args([("path", "Archive/brief.md"), ("text", "Brief\n")]);
    fails(
        &mut r,
        &mut s,
        "cal_0007",
        "write_file",
        copy,
        "files.read_only",
        "Archive is linked for reading only.",
    );
    fails(
        &mut r,
        &mut s,
        "cal_0008",
        "edit_file",
        edit("e", "E"),
        "files.many_matches",
        "find matched 8 places, on lines 1, 3, 4, 5. Give more lines so it matches one.",
    );
    let clash = args([("from", "Design/status.md"), ("to", "Design/brief.md")]);
    fails(
        &mut r,
        &mut s,
        "cal_0009",
        "move_path",
        clash,
        "files.exists",
        "Something is already at Design/brief.md.",
    );
    close(
        &mut r,
        &mut s,
        "The brief says calm, status.md is there, and the icons are tidied. Archive is linked for reading only, so I could not copy the brief there.",
    );

    // Turn 3: another folder asks again; this one is denied.
    open(
        &mut r,
        &mut s,
        "Start the plan in Drafts with a first step.",
        false,
    );
    let plan = Value::object([
        ("path", Value::from("Drafts/plan.md")),
        ("text", Value::from("1. Agree the goal\n")),
        ("append", Value::Bool(true)),
    ]);
    asked(
        &mut r,
        &mut s,
        Ask {
            id: "cal_0010",
            tool: "write_file",
            input: plan,
            folder: "f3",
            preview: items([
                ("Path", "Drafts/plan.md"),
                ("Add to the end", "1. Agree the goal\n"),
            ]),
            reversible: true,
        },
        Decision::Deny,
        None,
    );
    r.result(
        &mut s,
        result(
            "cal_0010",
            ToolOutcome::Denied,
            text("The person denied this action. Do not retry it unless they ask."),
        ),
        0,
    );
    close(&mut r, &mut s, "Understood, I left the plan as it was.");

    // Turn 4: a change that cannot be undone offers "once" only.
    open(
        &mut r,
        &mut s,
        "Add the sunset and the moonrise to the shot list.",
        false,
    );
    let shot = Value::object([
        ("path", Value::from("Camera/shots.md")),
        ("text", Value::from("Sunset\n")),
        ("append", Value::Bool(true)),
    ]);
    asked(
        &mut r,
        &mut s,
        Ask {
            id: "cal_0011",
            tool: "write_file",
            input: shot.clone(),
            folder: "f4",
            preview: items([("Path", "Camera/shots.md"), ("Add to the end", "Sunset\n")]),
            reversible: false,
        },
        Decision::Allow,
        Some(Remember::Once),
    );
    done(
        &mut r,
        &mut s,
        "cal_0011",
        "write_file",
        &shot,
        "Appended 7 bytes to Camera/shots.md (now 17 bytes).",
    );
    // The approval was for that call only: the next change there asks again.
    let moon = Value::object([
        ("path", Value::from("Camera/shots.md")),
        ("text", Value::from("Moonrise\n")),
        ("append", Value::Bool(true)),
    ]);
    asked(
        &mut r,
        &mut s,
        Ask {
            id: "cal_0012",
            tool: "write_file",
            input: moon.clone(),
            folder: "f4",
            preview: items([
                ("Path", "Camera/shots.md"),
                ("Add to the end", "Moonrise\n"),
            ]),
            reversible: false,
        },
        Decision::Allow,
        Some(Remember::Once),
    );
    done(
        &mut r,
        &mut s,
        "cal_0012",
        "write_file",
        &moon,
        "Appended 9 bytes to Camera/shots.md (now 26 bytes).",
    );
    close(&mut r, &mut s, "Sunset and moonrise are on the shot list.");
    r.finish()
}
