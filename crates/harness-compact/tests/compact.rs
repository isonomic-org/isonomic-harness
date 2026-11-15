//! Compaction: trims first (oldest first), summaries only when allowed, and
//! the recorded event reproduces the planned request exactly.

use harness_compact::*;
use harness_core::*;
use harness_log::history::{history, request_for};
use harness_log::Part;
use harness_testkit::tools::read_note;
use harness_testkit::{Ctx, Recorder};

fn long(n: usize) -> String {
    (0..)
        .map(|i| format!("line {i:05}\n"))
        .take(n / 11)
        .collect()
}

/// A session with one turn per result size, then an open turn.
fn session(sizes: &[usize]) -> (Recorder, Ctx) {
    let mut r = Recorder::new();
    let mut cx = r.session(
        SessionStart {
            class: SessionClass::Interactive,
            app: AppId::from("chat"),
            principal: Principal {
                kind: PrincipalKind::User,
                id: None,
            },
            preset: PresetRef {
                id: PresetId::from("test"),
                version: 1,
            },
            parent: None,
            profile: Some(Profile::Standard),
        },
        "local:small",
    );
    r.preset_prompt(&mut cx, "You are a test assistant.");
    for (i, n) in sizes.iter().enumerate() {
        r.turn(&mut cx, Cause::User, None);
        r.user(&mut cx, &format!("Read note {i}."));
        r.step(&mut cx, Purpose::Reply, Some(vec![read_note()]));
        r.reply(&mut cx, vec![], StopReason::ToolUse, None);
        let call = format!("cal_{i:04}");
        r.call(
            &mut cx,
            ToolCall {
                call: CallId::from(call.as_str()),
                tool: "read_note".into(),
                kind: ToolKind::Engine,
                input: Value::object([("id", Value::from(format!("n{i}")))]),
                input_raw: None,
                effects: vec![Effect::ReadsPrivateData],
                batch: None,
                child: None,
            },
        );
        r.result(
            &mut cx,
            ToolResult {
                call: CallId::from(call.as_str()),
                outcome: ToolOutcome::Ok,
                content: vec![Block::text(long(*n))],
                error: None,
                truncated: None,
                duration_ms: None,
            },
            30,
        );
        r.end_step(&mut cx, StepOutcome::ToolCalls);
        r.step(&mut cx, Purpose::Reply, None);
        r.reply(
            &mut cx,
            vec![Block::text(format!("Note {i} read."))],
            StopReason::End,
            None,
        );
        r.end_step(&mut cx, StepOutcome::Complete);
        r.end_turn(&mut cx, TurnStop::Done, None);
    }
    r.turn(&mut cx, Cause::User, None);
    r.user(&mut cx, "What did the notes say?");
    (r, cx)
}

fn probe(cx: &Ctx) -> Probe {
    Probe {
        session: cx.session.clone(),
        turn: cx.turn.clone().unwrap(),
        start: StepStart {
            purpose: Purpose::Reply,
            memory_policy: None,
            model: "local:small".into(),
            params: Params {
                max_output_tokens: 2048,
                reasoning: Reasoning::Off,
                options: Default::default(),
            },
            tools: None,
        },
    }
}

fn cfg(budget: u64, summaries: bool) -> Config {
    Config {
        budget_tokens: budget,
        trim_over_bytes: 2_000,
        head_bytes: 400,
        tail_bytes: 200,
        summaries,
        keep_turns: 1,
    }
}

/// Record the real reply step and return its request's estimate.
fn real_estimate(r: &mut Recorder, cx: &mut Ctx) -> u64 {
    r.step(cx, Purpose::Reply, None);
    let step = cx.step.clone().unwrap();
    request_for(&r.events, &step).unwrap().estimate_tokens()
}

#[test]
fn fits_when_under_budget() {
    let (r, cx) = session(&[6_000, 6_000]);
    let p = probe(&cx);
    let tokens = estimate(&r.events, &p, None);
    assert!(tokens > 3_000);
    assert_eq!(
        plan(&r.events, &p, &cfg(tokens, true)),
        Plan::Fits { tokens }
    );
}

#[test]
fn trims_oldest_first_and_replay_is_exact() {
    let (mut r, mut cx) = session(&[6_000, 500, 6_000]);
    let p = probe(&cx);
    let before = estimate(&r.events, &p, None);
    // Trimming one result saves about 1,350 tokens; ask for half that.
    let Plan::Compact(c) = plan(&r.events, &p, &cfg(before - 700, true)) else {
        panic!("expected trims only");
    };
    assert_eq!(c.trimmed.len(), 1);
    assert_eq!(c.trimmed[0].call, CallId::from("cal_0000"));
    assert_eq!(c.trimmed[0].original_bytes, 5_995);
    assert_eq!(c.tokens_before, before);
    assert!(c.tokens_after <= before - 700);
    assert!(c.summary.is_none());

    r.emit(&cx, Event::Compacted(c.clone()));
    assert_eq!(real_estimate(&mut r, &mut cx), c.tokens_after);

    // Trimmed results are not trimmed again.
    let p2 = probe(&cx);
    let Plan::Compact(c2) = plan(&r.events[..r.events.len() - 1], &p2, &cfg(1, false)) else {
        panic!("expected trims only");
    };
    assert_eq!(c2.trimmed.len(), 1);
    assert_eq!(c2.trimmed[0].call, CallId::from("cal_0002"));
}

#[test]
fn no_summaries_on_profiles_that_forbid_them() {
    let (r, cx) = session(&[6_000, 6_000, 6_000]);
    let p = probe(&cx);
    let Plan::Compact(c) = plan(&r.events, &p, &cfg(100, false)) else {
        panic!("expected trims only");
    };
    assert_eq!(c.trimmed.len(), 3);
    assert!(c.tokens_after > 100, "still over budget, and runs as is");
}

#[test]
fn summarizes_after_trimming_when_allowed() {
    let (mut r, mut cx) = session(&[6_000, 6_000, 6_000]);
    let p = probe(&cx);
    let Plan::Summarize {
        trim,
        turns,
        tokens_before,
    } = plan(&r.events, &p, &cfg(100, true))
    else {
        panic!("expected a summary");
    };
    let trim = trim.unwrap();
    assert_eq!(trim.trimmed.len(), 3);
    // Three completed turns, the most recent kept verbatim.
    let done: Vec<TurnId> = r
        .events
        .iter()
        .filter(|e| matches!(e.event, Event::TurnEnd(_)))
        .filter_map(|e| e.turn.clone())
        .collect();
    assert_eq!(turns, done[..2]);

    r.emit(&cx, Event::Compacted(trim));
    r.step(&mut cx, Purpose::Summary, None);
    let text = "Notes 0 and 1 were read.";
    r.reply(&mut cx, vec![Block::text(text)], StopReason::End, None);
    r.end_step(&mut cx, StepOutcome::Complete);
    let c = summarized(&r.events, &p, turns.clone(), text.into(), tokens_before);
    assert!(c.tokens_after < tokens_before);
    r.emit(&cx, Event::Compacted(c.clone()));
    assert_eq!(real_estimate(&mut r, &mut cx), c.tokens_after);

    // The summary stands in for the two older turns; the third stays.
    let msgs = history(&r.events, None);
    let summaries: Vec<&Part> = msgs
        .iter()
        .flat_map(|m| &m.content)
        .filter(|p| matches!(p, Part::Summary { .. }))
        .collect();
    assert_eq!(summaries.len(), 1);
    let users: Vec<String> = msgs
        .iter()
        .flat_map(|m| &m.content)
        .filter_map(|p| match p {
            Part::Text { text } if text.starts_with("Read note") => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(users, ["Read note 2."]);

    // Nothing left to summarize: the next plan only trims (none left).
    assert!(summarizable(&r.events, &p.turn, 1).is_empty());
}
