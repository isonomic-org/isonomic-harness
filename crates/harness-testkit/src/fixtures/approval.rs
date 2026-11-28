// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! approval: an outward action approved in the shell's sheet, one denied by
//! the owning app, an ambiguous answer treated as a denial, and an ask-user
//! question answered. The person opted in to location.

use super::build::*;
use crate::recorder::{Ctx, Recorder};
use crate::tools;
use harness_core::*;

const PROMPT: &str =
    "You are a helpful assistant. Be accurate and concise, and use Markdown when it helps.";
const PLACE: Option<&str> = Some("Lisbon, Portugal");

fn gated(
    r: &mut Recorder,
    s: &mut Ctx,
    call_id: &str,
    tool: &ToolSpec,
    input: Value,
    summary: &str,
) -> ApprovalId {
    r.call(s, call(call_id, tool, input, None));
    let approval = ApprovalId::new(r.id("apv"));
    let expires = r.now() + 300_000;
    r.emit(
        s,
        Event::ApprovalRequest(ApprovalRequest {
            approval: approval.clone(),
            call: Some(CallId::from(call_id)),
            summary: summary.to_owned(),
            rule: ApprovalRule::ActsOutward,
            effects: vec![Effect::ActsOutward],
            expires_at: Some(expires),
            scope: None,
            preview: Vec::new(),
            reversible: None,
            remember: None,
        }),
    );
    approval
}

fn decide(r: &mut Recorder, s: &Ctx, approval: ApprovalId, d: ApprovalDecision) {
    r.advance(4_000);
    r.emit(
        s,
        Event::ApprovalDecision(ApprovalDecision { approval, ..d }),
    );
}

fn decision(
    decision: Decision,
    basis: Basis,
    by: Decider,
    answer: Option<&str>,
) -> ApprovalDecision {
    ApprovalDecision {
        remember: None,
        approval: ApprovalId::from(""),
        decision,
        basis,
        by,
        answer: answer.map(str::to_owned),
        reason: None,
    }
}

fn answer_step(r: &mut Recorder, s: &mut Ctx, answer: &str) {
    r.step(s, Purpose::Reply, None);
    r.reply(s, text(answer), StopReason::End, None);
    r.end_step(s, StepOutcome::Complete);
    r.end_turn(s, TurnStop::Done, None);
}

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);
    r.preset_prompt(&mut s, PROMPT);
    let email = tools::send_email();
    let post = tools::post_message();
    let mut offered = vec![email.clone(), post.clone()];
    offered.extend(tools::toolkit());

    // Turn 1: approved in the shell's approvals sheet.
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "Email Sam the agenda for Thursday.");
    r.context(&mut s, PLACE);
    r.step(&mut s, Purpose::Reply, Some(offered));
    r.reply(&mut s, text("I'll send it now."), StopReason::ToolUse, None);
    let input = Value::object([
        ("to", Value::from("sam@example.com")),
        ("subject", Value::from("Thursday agenda")),
        (
            "body",
            Value::from(
                "Hi Sam, the agenda: welcome, first book vote, scheduling. See you Thursday.",
            ),
        ),
    ]);
    let a = gated(
        &mut r,
        &mut s,
        "cal_0001",
        &email,
        input,
        "Send an email to sam@example.com: \"Thursday agenda\"",
    );
    let shell = by(DeciderScope::Shell, None);
    decide(
        &mut r,
        &s,
        a,
        decision(Decision::Allow, Basis::User, shell.clone(), None),
    );
    r.result(
        &mut s,
        result("cal_0001", ToolOutcome::Ok, text("Sent.")),
        700,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    answer_step(&mut r, &mut s, "Sent the Thursday agenda to Sam.");

    // Turn 2: denied by the app that owns the session.
    r.advance(30_000);
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "Also email the whole team list.");
    r.context(&mut s, PLACE);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Sending it to the team list."),
        StopReason::ToolUse,
        None,
    );
    let input = Value::object([
        ("to", Value::from("team@example.com")),
        ("subject", Value::from("Thursday agenda")),
        (
            "body",
            Value::from("Hi all, the agenda: welcome, first book vote, scheduling."),
        ),
    ]);
    let a = gated(
        &mut r,
        &mut s,
        "cal_0002",
        &email,
        input,
        "Send an email to team@example.com: \"Thursday agenda\"",
    );
    let app = by(DeciderScope::App, Some("chat"));
    decide(
        &mut r,
        &s,
        a,
        decision(Decision::Deny, Basis::User, app.clone(), None),
    );
    r.result(
        &mut s,
        result(
            "cal_0002",
            ToolOutcome::Denied,
            text("The person denied this action. Do not retry it unless they ask."),
        ),
        0,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    answer_step(&mut r, &mut s, "Understood, I won't email the team list.");

    // Turn 3: an ambiguous answer is a denial.
    r.advance(30_000);
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "Post the agenda in the neighborhood channel.");
    r.context(&mut s, PLACE);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Posting it to the channel."),
        StopReason::ToolUse,
        None,
    );
    let input = Value::object([
        ("channel", Value::from("neighborhood")),
        (
            "text",
            Value::from("Book club Thursday at 7: welcome, first book vote, scheduling."),
        ),
    ]);
    let a = gated(
        &mut r,
        &mut s,
        "cal_0003",
        &post,
        input,
        "Post a message in #neighborhood",
    );
    decide(
        &mut r,
        &s,
        a,
        decision(
            Decision::Deny,
            Basis::Ambiguous,
            shell,
            Some("hmm, maybe later?"),
        ),
    );
    r.result(
        &mut s,
        result(
            "cal_0003",
            ToolOutcome::Denied,
            text("The approval answer was ambiguous, so the action was not taken."),
        ),
        0,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    answer_step(
        &mut r,
        &mut s,
        "I didn't post it. Say \"post it\" whenever you're ready.",
    );

    // Turn 4: an ask-user question, answered.
    r.advance(30_000);
    r.turn(&mut s, Cause::User, None);
    r.user(
        &mut s,
        "Put booking a room for the next meeting on my list.",
    );
    r.context(&mut s, PLACE);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Which day should I note?"),
        StopReason::ToolUse,
        None,
    );
    let options = [("tue", "Tuesday"), ("wed", "Wednesday")];
    let input = Value::object([
        (
            "prompt",
            Value::from("Which day works for the next meeting?"),
        ),
        (
            "options",
            Value::from(options.iter().map(|(_, l)| *l).collect::<Vec<_>>()),
        ),
    ]);
    r.call(&mut s, call("cal_0004", &tools::ask_user(), input, None));
    let q = QuestionId::new(r.id("qst"));
    r.emit(
        &s,
        Event::Question(Question {
            question: q.clone(),
            call: Some(CallId::from("cal_0004")),
            prompt: "Which day works for the next meeting?".into(),
            options: options
                .iter()
                .map(|(id, label)| Choice {
                    id: (*id).into(),
                    label: (*label).into(),
                })
                .collect(),
            multiple: false,
            free_text: true,
        }),
    );
    r.advance(6_000);
    r.emit(
        &s,
        Event::Answer(Answer {
            question: q,
            selected: vec!["wed".into()],
            text: None,
            skipped: false,
            by: app,
        }),
    );
    r.result(
        &mut s,
        result("cal_0004", ToolOutcome::Ok, text("Wednesday")),
        0,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(&mut s, text("Adding it."), StopReason::ToolUse, None);
    let input = Value::object([
        ("action", Value::from("add")),
        ("item", Value::from("Book a room for Wednesday's meeting")),
    ]);
    r.call(&mut s, call("cal_0005", &tools::todo(), input, None));
    r.result(
        &mut s,
        result("cal_0005", ToolOutcome::Ok, text("Added.")),
        40,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    answer_step(
        &mut r,
        &mut s,
        "Done: \"Book a room for Wednesday's meeting\" is on your list.",
    );
    r.finish()
}
