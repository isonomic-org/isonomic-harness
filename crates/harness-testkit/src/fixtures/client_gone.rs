//! client-gone: an assistant panel calls a client tool whose app has
//! disconnected. The session moves to awaiting client, the call times out
//! as a readable failure, the reply explains, and when the app reconnects
//! the session is active again and the tool works.

use super::build::*;
use crate::recorder::Recorder;
use crate::tools;
use harness_core::*;

const PROMPT: &str = "You help edit the open document. Use the document tools to make changes.";

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "docs", "assistant-panel", Profile::Standard);
    r.preset_prompt(&mut s, PROMPT);
    let insert = tools::insert_block();
    let heading = || {
        Value::object([
            (
                "block",
                Value::object([
                    ("level", Value::from(2u64)),
                    ("text", Value::from("Overview")),
                    ("type", Value::from("heading")),
                ]),
            ),
            ("position", Value::from("start")),
        ])
    };

    // Turn 1: the app is gone when the call is made.
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "Add a heading called Overview at the top.");
    r.context(&mut s, None);
    r.step(&mut s, Purpose::Reply, Some(vec![insert.clone()]));
    r.reply(
        &mut s,
        text("Adding the heading."),
        StopReason::ToolUse,
        None,
    );
    r.call(&mut s, call("cal_0001", &insert, heading(), None));
    let deadline = r.now() + 30_000;
    r.emit(
        &s,
        Event::SessionStatus(SessionStatus {
            status: Status::AwaitingClient,
            reason: Some("The app that runs insert_block is not connected.".into()),
            calls: vec![CallId::from("cal_0001")],
            deadline: Some(deadline),
        }),
    );
    let mut timeout = result(
        "cal_0001",
        ToolOutcome::Timeout,
        text("insert_block did not run: the document app is not connected. Nothing was changed."),
    );
    timeout.error = Some(ErrorInfo::new("client.gone", "The app is not connected."));
    r.result(&mut s, timeout, 30_000);
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("I couldn't reach the document, so nothing changed. Once it's open again, ask me and I'll add the heading."),
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);

    // The app reconnects: the session resumes.
    r.advance(45_000);
    s.turn = None;
    r.emit(
        &s,
        Event::SessionStatus(SessionStatus {
            status: Status::Active,
            reason: Some("The app reconnected.".into()),
            calls: vec![],
            deadline: None,
        }),
    );

    // Turn 2: the same tool works.
    r.advance(5_000);
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "It's open again. Try now.");
    r.context(&mut s, None);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Adding the heading."),
        StopReason::ToolUse,
        None,
    );
    r.call(&mut s, call("cal_0002", &insert, heading(), None));
    r.result(
        &mut s,
        result(
            "cal_0002",
            ToolOutcome::Ok,
            text("Inserted a level-2 heading at the top."),
        ),
        120,
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Done. The document now starts with an **Overview** heading."),
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);
    r.finish()
}
