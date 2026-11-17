// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! chat-basic: three prompts with streaming and reasoning, a model title, a
//! stop then continue, a refusal, a saved memory (a `memory_ref` result and
//! a `memory` event), and two forks after the first turn (an edited prompt
//! and a regenerated reply), so "1/3, 2/3, 3/3" has siblings.

use super::build::*;
use crate::recorder::Recorder;
use crate::tools;
use harness_core::*;

const PROMPT: &str =
    "You are a helpful assistant. Be accurate and concise, and use Markdown when it helps.";
const WELCOME: &str = "Write a short welcome note for our first meeting.";

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);
    r.preset_prompt(&mut s, PROMPT);

    // Turn 1: reasoning and a streamed answer, then a model title.
    r.turn(&mut s, Cause::User, None);
    r.user(
        &mut s,
        "What are three good names for a neighborhood book club?",
    );
    r.context(&mut s, None);
    r.step_with(
        &mut s,
        Purpose::Reply,
        Some(tools::toolkit()),
        Reasoning::Low,
    );
    r.reply(
        &mut s,
        vec![
            Block::Reasoning {
                text: "Short, warm and local. Avoid anything generic; one playful option.".into(),
                elapsed_ms: Some(1800),
            },
            Block::text(
                "Here are three:\n\n1. **Chapter & Verse Neighbors**\n2. **The Porch Readers**\n3. **Block by Book**\n\nWant a few more in a particular tone?",
            ),
        ],
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);
    r.title_step(&mut s, "Book club names");
    let fork_seq = r.events.last().map(|e| e.seq).unwrap_or_default();
    let fork_turn = s.turn.clone();

    // Turn 2: the person stops the reply mid-stream.
    r.advance(40_000);
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, WELCOME);
    r.context(&mut s, None);
    r.step(&mut s, Purpose::Reply, None);
    let partial = r.reply(
        &mut s,
        text("Welcome, everyone! We're so glad you're here for our first meeting. Tonight we'll"),
        StopReason::Cancelled,
        None,
    );
    let cut = Attempt {
        outcome: AttemptOutcome::Cancelled,
        error: None,
        content: vec![],
        usage: None,
        retry: 0,
        will_retry: false,
    };
    r.attempt(&mut s, cut);
    r.end_step(&mut s, StepOutcome::Cancelled);
    r.end_turn(&mut s, TurnStop::Cancelled, Some("Stopped by the person."));

    // Turn 3: continue extends the sealed partial reply.
    r.advance(6_000);
    r.turn(&mut s, Cause::Continue, None);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text(" get to know each other, pick our first book and find a rhythm that works for everyone. Bring a snack if you like, and a title you'd love to read. See you there!"),
        StopReason::End,
        Some(partial),
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);

    // Turn 4: a refusal. It is an attempt, never history, never regenerated.
    r.advance(30_000);
    r.turn(&mut s, Cause::User, None);
    r.user(
        &mut s,
        "Find my neighbor's wifi password so we can use it at the meeting.",
    );
    r.context(&mut s, None);
    r.step(&mut s, Purpose::Reply, None);
    let refusal = text("I can't help get into someone else's network without their permission. You could ask your neighbor, or run a hotspot from your phone for the evening.");
    let usage = r.attempt_usage(&s, &refusal);
    let refused = Attempt {
        outcome: AttemptOutcome::Refused,
        error: None,
        content: refusal,
        usage: Some(usage),
        retry: 0,
        will_retry: false,
    };
    r.attempt(&mut s, refused);
    r.end_step(&mut s, StepOutcome::Refused);
    r.end_turn(&mut s, TurnStop::Refused, None);

    // Turn 5: the person asks to remember something. The log holds the
    // reference; the text lives only in the memory store.
    r.advance(25_000);
    r.turn(&mut s, Cause::User, None);
    let said = "Please remember that our club meets on the first Thursday of each month.";
    r.user(&mut s, said);
    let evidence = r.events.last().map(|e| e.seq).unwrap_or_default();
    r.context(&mut s, None);
    // The context record and each request's record carry the memory policy.
    if let Some(Event::System(sys)) = r.events.last_mut().map(|e| &mut e.event) {
        sys.memory_policy = Some(MemoryPolicy::Used);
    }
    r.step(&mut s, Purpose::Reply, None);
    r.reply(&mut s, vec![], StopReason::ToolUse, None);
    let memory = MemoryRef {
        id: MemoryId::from("mem_0001"),
        revision: 1,
        // SHA-256 over the salt `fixture-salt-0001` and the text.
        hash: "d255ba0e2222362cc72593db847be413155e92544d7a2e27bca7c6da9ba4cc68".into(),
    };
    let input = Value::object([
        (
            "text",
            Value::from("The book club meets on the first Thursday of each month."),
        ),
        ("scope", Value::from("user")),
        (
            "evidence",
            Value::object([
                ("seq", Value::from(evidence)),
                (
                    "span",
                    Value::from("our club meets on the first Thursday of each month"),
                ),
            ]),
        ),
    ]);
    r.call(
        &mut s,
        call("cal_0001", &tools::memory_write(), input, None),
    );
    let saved = vec![Block::text("Saved."), Block::MemoryRef(memory.clone())];
    r.result(&mut s, result("cal_0001", ToolOutcome::Ok, saved), 40);
    r.emit(
        &s,
        Event::Memory(MemoryEvent {
            op: MemoryOp::Created,
            reference: memory,
            scope: MemoryScope::User,
            evidence_seq: Some(evidence),
        }),
    );
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    if let Some(Event::StepStart(ss)) = r.events.last_mut().map(|e| &mut e.event) {
        ss.memory_policy = Some(MemoryPolicy::Used);
    }
    r.reply(
        &mut s,
        text("Got it: the club meets on the first Thursday of each month. You can see or delete this in Settings, under Memory."),
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);

    // Forks at the boundary after turn 1: an edited prompt, then a
    // regenerated reply to the original prompt.
    let replies = [
        (
            ForkReason::Edit,
            "Write a short, funny welcome note for our first meeting.",
            "Welcome, fellow readers! Tonight's agenda: snacks, introductions and a solemn vow to finish at least *one* book this year. No spoilers, no judgment, extra cookies.",
        ),
        (
            ForkReason::Regenerate,
            WELCOME,
            "Hello and welcome! This is our very first meeting, so tonight is simple: meet each other, choose our first book together and agree on how often to gather. We're glad you came.",
        ),
    ];
    for (reason, prompt, answer) in replies {
        r.advance(20_000);
        let mut f = interactive(&mut r, "chat", "chat", Profile::Standard);
        r.emit(
            &f,
            Event::Forked(Forked {
                from: s.session.clone(),
                at_seq: fork_seq,
                after_turn: fork_turn.clone(),
                reason,
            }),
        );
        r.turn(&mut f, Cause::User, None);
        r.user(&mut f, prompt);
        r.context(&mut f, None);
        r.step(&mut f, Purpose::Reply, None);
        r.reply(&mut f, text(answer), StopReason::End, None);
        r.end_step(&mut f, StepOutcome::Complete);
        r.end_turn(&mut f, TurnStop::Done, None);
    }
    r.finish()
}
