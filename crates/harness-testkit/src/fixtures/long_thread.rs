// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! long-thread: one interactive session of exactly 10,000 events. Mostly
//! short exchanges, a tool call every tenth turn, request context every third
//! turn and a title after the first. Generated, not committed.

use super::build::*;
use crate::recorder::{Ctx, Recorder};
use crate::tools;
use harness_core::*;

/// The number of events in long-thread.
pub const EVENTS: usize = 10_000;

const TOPICS: &[&str] = &[
    "a weeknight dinner",
    "a reading list",
    "a budget for the trip",
    "the garden beds",
    "a birthday message",
    "a packing list",
    "a workout plan",
    "a cover letter",
];

fn simple(r: &mut Recorder, s: &mut Ctx, i: usize, with_context: bool) {
    let topic = TOPICS[i % TOPICS.len()];
    r.advance(20_000);
    r.turn(s, Cause::User, None);
    r.user(s, &format!("Message {i}: help me think about {topic}."));
    if with_context {
        r.context(s, None);
    }
    r.step(s, Purpose::Reply, None);
    r.reply(
        s,
        text(&format!("Reply {i}: for {topic}, start with what matters most, keep it simple and adjust next time.")),
        StopReason::End,
        None,
    );
    r.end_step(s, StepOutcome::Complete);
    r.end_turn(s, TurnStop::Done, None);
}

fn with_tool(r: &mut Recorder, s: &mut Ctx, i: usize, with_context: bool) {
    r.advance(20_000);
    r.turn(s, Cause::User, None);
    r.user(s, &format!("Message {i}: what time is it?"));
    if with_context {
        r.context(s, None);
    }
    r.step(s, Purpose::Reply, None);
    r.reply(s, text("Checking."), StopReason::ToolUse, None);
    let id = r.id("cal");
    r.call(s, call(&id, &tools::now(), Value::object::<&str>([]), None));
    r.result(s, result(&id, ToolOutcome::Ok, text("09:41")), 5);
    r.end_step(s, StepOutcome::ToolCalls);
    r.step(s, Purpose::Reply, None);
    r.reply(
        s,
        text(&format!("Reply {i}: it's 09:41.")),
        StopReason::End,
        None,
    );
    r.end_step(s, StepOutcome::Complete);
    r.end_turn(s, TurnStop::Done, None);
}

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);
    r.preset_prompt(&mut s, "You are a helpful assistant.");
    let mut i = 0;
    loop {
        let left = EVENTS - r.events.len();
        let ctx = i % 3 == 0;
        let tool = i % 10 == 9;
        let size = if tool { 11 } else { 6 } + usize::from(ctx);
        if left < size + 7 {
            break;
        }
        if i == 0 {
            r.turn(&mut s, Cause::User, None);
            r.user(&mut s, "Message 0: help me plan my week.");
            r.context(&mut s, None);
            r.step(&mut s, Purpose::Reply, Some(tools::toolkit()));
            r.reply(&mut s, text("Reply 0: list your fixed commitments first, then the three things that matter most."), StopReason::End, None);
            r.end_step(&mut s, StepOutcome::Complete);
            r.end_turn(&mut s, TurnStop::Done, None);
            r.title_step(&mut s, "Planning the week");
        } else if tool {
            with_tool(&mut r, &mut s, i, ctx);
        } else {
            simple(&mut r, &mut s, i, ctx);
        }
        i += 1;
    }
    // Close out with plain turns (6 or 7 events), then renames for any
    // remainder under 6.
    while EVENTS - r.events.len() >= 6 {
        let left = EVENTS - r.events.len();
        simple(&mut r, &mut s, i, left == 7 || left >= 13);
        i += 1;
    }
    let mut n = 1;
    while r.events.len() < EVENTS {
        s.turn = None;
        r.advance(1_000);
        r.emit_title(
            &mut s,
            &format!("Planning the week ({n})"),
            TitleSource::User,
        );
        n += 1;
    }
    r.finish()
}
