// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! autonomous-run: an agent woken by a schedule on two days. On the second
//! run its history outgrows the budget, so it compacts (trims two long
//! results, then summarizes the first run), delegates a task to a fake
//! child session, and stops cleanly when its step budget runs out.

use super::build::*;
use crate::recorder::{Ctx, Recorder};
use crate::tools;
use harness_core::*;

const PROMPT: &str =
    "You are a research agent. Work through the task with your tools and report briefly.";
const CHILD_PROMPT: &str =
    "You are a subagent. Do only the task you were given and report briefly.";

fn agent(r: &mut Recorder, class: SessionClass, parent: Option<ParentRef>) -> Ctx {
    let id = SessionId::new(r.id("ses"));
    agent_as(r, id, class, parent)
}

fn agent_as(
    r: &mut Recorder,
    id: SessionId,
    class: SessionClass,
    parent: Option<ParentRef>,
) -> Ctx {
    r.session_as(
        id,
        SessionStart {
            class,
            app: AppId::from("agents"),
            principal: Principal {
                kind: PrincipalKind::Agent,
                id: Some("agt_0001".into()),
            },
            preset: PresetRef {
                id: PresetId::from("test"),
                version: 1,
            },
            parent,
            profile: Some(Profile::Standard),
            model: None,
        },
        "local:small",
    )
}

fn items(day: &str, n: usize) -> String {
    let mut out = format!("{n} new items since {day}:\n");
    for i in 1..=n {
        out.push_str(&format!(
            "{i}. Supplier update {i}: lead times, stock levels and a note on delivery windows for the coming weeks.\n"
        ));
    }
    out
}

fn collect(r: &mut Recorder, s: &mut Ctx, call_id: &str, source: &str, since: &str, n: usize) {
    let input = Value::object([
        ("since", Value::from(since)),
        ("source", Value::from(source)),
    ]);
    r.call(s, call(call_id, &tools::collect_items(), input, None));
    r.result(
        s,
        result(call_id, ToolOutcome::Ok, text(&items(since, n))),
        900,
    );
}

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut a = agent(&mut r, SessionClass::Autonomous, None);
    r.preset_prompt(&mut a, PROMPT);
    let wake = || {
        Some(Wake {
            source: WakeSource::Schedule,
            id: "daily-digest".into(),
        })
    };
    let offered = vec![tools::collect_items(), tools::delegate()];

    // Day 1.
    r.turn(&mut a, Cause::Wake, wake());
    r.context(&mut a, None);
    r.step(&mut a, Purpose::Reply, Some(offered));
    r.reply(
        &mut a,
        text("Collecting today's items."),
        StopReason::ToolUse,
        None,
    );
    collect(
        &mut r,
        &mut a,
        "cal_0001",
        "supplier-inbox",
        "2026-11-13",
        14,
    );
    r.end_step(&mut a, StepOutcome::ToolCalls);
    r.step(&mut a, Purpose::Reply, None);
    r.reply(&mut a, text("Digest: 14 supplier updates. Lead times are stable; two suppliers flag slower deliveries next week."), StopReason::End, None);
    r.end_step(&mut a, StepOutcome::Complete);
    r.end_turn(&mut a, TurnStop::Done, None);
    let day1 = a.turn.clone().expect("turn");

    // Day 2.
    r.advance(86_400_000 - 60_000);
    r.turn(&mut a, Cause::Wake, wake());
    r.context(&mut a, None);
    r.step(&mut a, Purpose::Reply, None);
    r.reply(
        &mut a,
        text("Collecting today's items."),
        StopReason::ToolUse,
        None,
    );
    collect(
        &mut r,
        &mut a,
        "cal_0002",
        "supplier-inbox",
        "2026-11-14",
        18,
    );
    r.end_step(&mut a, StepOutcome::ToolCalls);

    // Compaction: trim the long results first, then summarize day 1.
    r.step(&mut a, Purpose::Summary, None);
    r.reply(&mut a, text("Day 1 (14 Nov): collected 14 supplier updates; lead times stable; two suppliers expect slower deliveries next week."), StopReason::End, None);
    r.end_step(&mut a, StepOutcome::Complete);
    let through = r
        .events
        .iter()
        .rev()
        .find(|e| matches!(e.event, Event::ToolResult(_)))
        .map(|e| e.seq)
        .unwrap_or_default();
    let trim = |call: &str, n: usize, day: &str| Trim {
        call: CallId::from(call),
        original_bytes: items(day, n).len() as u64,
        head_bytes: 400,
        tail_bytes: 200,
    };
    r.emit(
        &a,
        Event::Compacted(Compacted {
            through_seq: through,
            trimmed: vec![trim("cal_0001", 14, "2026-11-13"), trim("cal_0002", 18, "2026-11-14")],
            summary: Some(Summary {
                text: "Day 1 (14 Nov): collected 14 supplier updates; lead times stable; two suppliers expect slower deliveries next week.".into(),
                turns: vec![day1],
            }),
            tokens_before: 7_400,
            tokens_after: 2_100,
        }),
    );

    // Delegate to a child session.
    r.step(&mut a, Purpose::Reply, None);
    r.reply(
        &mut a,
        text("Asking a subagent to check the three supplier sites."),
        StopReason::ToolUse,
        None,
    );
    let task = "Check the three supplier sites for price changes since yesterday.";
    let child = SessionId::new(r.id("ses"));
    let mut ask = call(
        "cal_0003",
        &tools::delegate(),
        Value::object([("task", Value::from(task))]),
        None,
    );
    ask.child = Some(child.clone());
    r.call(&mut a, ask);
    let parent = Some(ParentRef {
        session: a.session.clone(),
        call: CallId::from("cal_0003"),
    });
    let mut d = agent_as(&mut r, child, SessionClass::Delegated, parent);
    r.preset_prompt(&mut d, CHILD_PROMPT);
    r.turn(&mut d, Cause::Delegate, None);
    r.user(&mut d, task);
    r.step(&mut d, Purpose::Reply, Some(vec![tools::collect_items()]));
    r.reply(
        &mut d,
        text("No price changes on any of the three sites since yesterday."),
        StopReason::End,
        None,
    );
    r.end_step(&mut d, StepOutcome::Complete);
    r.end_turn(&mut d, TurnStop::Done, None);
    let report = format!(
        "Subagent {} reported: no price changes on any of the three sites since yesterday.",
        d.session
    );
    r.result(
        &mut a,
        result("cal_0003", ToolOutcome::Ok, text(&report)),
        50,
    );
    r.end_step(&mut a, StepOutcome::ToolCalls);

    // The third step uses the last of the step budget; the turn stops cleanly.
    r.step(&mut a, Purpose::Reply, None);
    r.reply(
        &mut a,
        text("Checking the calendar for deliveries."),
        StopReason::ToolUse,
        None,
    );
    collect(&mut r, &mut a, "cal_0004", "calendar", "2026-11-14", 3);
    r.end_step(&mut a, StepOutcome::ToolCalls);
    r.end_turn_with(
        &mut a,
        TurnStop::Budget,
        Some("Step budget reached."),
        Some(BudgetHit {
            kind: BudgetKind::Steps,
            limit: 3,
            used: 3,
        }),
    );
    r.finish()
}
