//! Tool calls: batches, guards, approvals, the three-flag rule, questions,
//! pre_tool denials, cancel mid-tool, client-gone, and compaction.

mod common;
use common::*;
use harness_core::*;
use harness_log::Part;
use harness_loop::*;
use harness_policy::Autonomy;
use harness_tools::{QuestionSpec, ToolDone, ToolOutput};

fn kit() -> Vec<ToolSpec> {
    vec![
        spec("now", ToolKind::Engine, &[], Concurrency::Parallel),
        spec("lookup", ToolKind::Engine, &[], Concurrency::Parallel),
        spec(
            "read_note",
            ToolKind::Engine,
            &[Effect::ReadsPrivateData],
            Concurrency::Parallel,
        ),
        spec(
            "fetch_page",
            ToolKind::Engine,
            &[Effect::IngestsUntrustedContent],
            Concurrency::Parallel,
        ),
        spec(
            "send_email",
            ToolKind::Engine,
            &[Effect::ActsOutward],
            Concurrency::Serial,
        ),
        spec("ask_user", ToolKind::Engine, &[], Concurrency::Serial),
        spec("insert_block", ToolKind::Client, &[], Concurrency::Serial),
    ]
}

fn driver() -> Driver {
    Driver::with_profile(Config::new(preset("test"), None, kit()), Profile::Intel)
}

fn results(d: &Driver) -> Vec<ToolResult> {
    d.own()
        .iter()
        .filter_map(|e| match &e.event {
            Event::ToolResult(r) => Some(r.clone()),
            _ => None,
        })
        .collect()
}

fn call_of(d: &Driver, i: usize) -> CallId {
    d.runs[i].0.call.clone()
}

#[test]
fn parallel_calls_share_a_batch_and_results_follow_call_order() {
    let mut d = driver();
    d.user("Time and the weather?");
    d.call_tools(&[("now", "{}"), ("lookup", r#"{"q":"weather"}"#)]);
    assert_eq!(d.runs.len(), 2, "both run at once");
    assert_eq!(d.runs[0].0.batch, Some(0));
    // Results arrive out of order.
    d.tool_ok(&call_of(&d, 1), "Sunny");
    assert_eq!(d.requests.len(), 1);
    d.tool_ok(&call_of(&d, 0), "10:00");
    assert_eq!(d.requests.len(), 2, "next step after the batch");
    let tool_msg = d.last_request().messages.last().unwrap().clone();
    let order: Vec<String> = tool_msg
        .content
        .iter()
        .filter_map(|p| match p {
            Part::ToolResult { call, .. } => Some(call.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(order, ["cal_0001", "cal_0002"]);
    d.reply("10:00 and sunny.");
    d.check();
}

#[test]
fn bad_input_and_unknown_tools_become_readable_results() {
    let mut d = driver();
    d.user("Go.");
    d.call_tools(&[("lookup", "{not json"), ("teleport", "{}")]);
    assert!(d.runs.is_empty(), "nothing ran");
    let r = results(&d);
    assert_eq!(r.len(), 2);
    assert!(r.iter().all(|r| r.outcome == ToolOutcome::Error));
    assert_eq!(d.requests.len(), 2, "the model sees the failures");
    d.check();
}

#[test]
fn outward_actions_ask_and_decisions_round_trip() {
    let mut d = driver();
    d.user("Email Sam.");
    d.call_tools(&[("send_email", r#"{"to":"sam"}"#)]);
    let req = d
        .last_of(|e| match e {
            Event::ApprovalRequest(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.rule, ApprovalRule::ActsOutward);
    assert!(d.runs.is_empty());
    let shell = Decider {
        scope: DeciderScope::Shell,
        app: None,
    };
    d.send(Input::Decision(ApprovalDecision {
        approval: req.approval.clone(),
        decision: Decision::Allow,
        basis: Basis::User,
        by: shell.clone(),
        answer: None,
        reason: None,
    }));
    assert_eq!(d.runs.len(), 1);
    d.tool_ok(&call_of(&d, 0), "Sent.");
    d.reply("Sent.");

    // An ambiguous answer is a denial the model can read.
    d.user("Email the team.");
    d.call_tools(&[("send_email", r#"{"to":"team"}"#)]);
    let req = d
        .last_of(|e| match e {
            Event::ApprovalRequest(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    let (decision, basis, answer) =
        harness_policy::interpret(&harness_policy::DecisionInput::Text("maybe later".into()));
    d.send(Input::Decision(ApprovalDecision {
        approval: req.approval,
        decision,
        basis,
        by: shell,
        answer,
        reason: None,
    }));
    let r = results(&d).pop().unwrap();
    assert_eq!(r.outcome, ToolOutcome::Denied);
    assert_eq!(d.runs.len(), 1, "the denied call never ran");
    let seen = texts(&d.last_request().messages).join("|");
    assert!(seen.contains("unclear"), "{seen}");
    d.reply("I didn't send it.");

    // Unanswered, it times out as a denial.
    d.user("Email Kim.");
    d.call_tools(&[("send_email", r#"{"to":"kim"}"#)]);
    d.advance(60_001);
    d.send(Input::Tick);
    let decided = d
        .last_of(|e| match e {
            Event::ApprovalDecision(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(decided.basis, Basis::Timeout);
    d.reply("No approval came.");
    d.check();
}

#[test]
fn the_three_flag_rule_makes_a_preapproved_outward_tool_ask() {
    let mut cfg = Config::new(preset("test"), Some(Profile::Intel), kit());
    cfg.preset.policy.policy.autonomy = Autonomy::Trusted;
    cfg.preset.policy.policy.allow_outward = vec!["send_email".into()];
    let mut d = Driver::with_profile(cfg, Profile::Intel);
    d.user("Send it.");
    d.call_tools(&[("send_email", r#"{"to":"a"}"#)]);
    assert_eq!(d.runs.len(), 1, "trusted and pre-approved: runs");
    d.tool_ok(&call_of(&d, 0), "Sent.");
    d.call_tools(&[
        ("read_note", r#"{"id":"n1"}"#),
        ("fetch_page", r#"{"url":"x"}"#),
    ]);
    d.tool_ok(&call_of(&d, 1), "private");
    d.tool_ok(&call_of(&d, 2), "untrusted");
    d.call_tools(&[("send_email", r#"{"to":"b"}"#)]);
    let req = d
        .last_of(|e| match e {
            Event::ApprovalRequest(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(req.rule, ApprovalRule::ThreeFlag);
    assert_eq!(d.runs.len(), 3);
}

#[test]
fn questions_round_trip_and_pre_tool_denials_are_readable() {
    struct NoLookups;
    impl Interceptor for NoLookups {
        fn pre_tool(&mut self, call: &ToolCall) -> Link {
            if call.tool == "lookup" {
                Link::Deny("Lookups are off in this workspace.".into())
            } else {
                Link::Pass
            }
        }
    }
    let mut d = driver();
    d.s.add_link(Box::new(NoLookups));
    d.user("Plan a meeting.");
    d.call_tools(&[("ask_user", r#"{"prompt":"Which day?"}"#)]);
    let call = call_of(&d, 0);
    d.send(Input::Tool {
        call: call.clone(),
        output: ToolOutput::Ask(QuestionSpec {
            prompt: "Which day?".into(),
            options: vec![
                Choice {
                    id: "tue".into(),
                    label: "Tuesday".into(),
                },
                Choice {
                    id: "wed".into(),
                    label: "Wednesday".into(),
                },
            ],
            multiple: false,
            free_text: true,
        }),
    });
    let q = d
        .last_of(|e| match e {
            Event::Question(q) => Some(q.clone()),
            _ => None,
        })
        .unwrap();
    d.send(Input::Answer(Answer {
        question: q.question,
        selected: vec!["wed".into()],
        text: None,
        skipped: false,
        by: Decider {
            scope: DeciderScope::App,
            app: Some(AppId::from("chat")),
        },
    }));
    assert_eq!(results(&d)[0].content, vec![Block::text("Wednesday")]);
    d.call_tools(&[("lookup", r#"{"q":"rooms"}"#)]);
    let r = results(&d).pop().unwrap();
    assert_eq!(r.outcome, ToolOutcome::Denied);
    assert!(texts(&d.last_request().messages)
        .join("|")
        .contains("Lookups are off"));
    d.check();
}

#[test]
fn cancel_mid_tool_seals_every_outstanding_call() {
    let mut d = driver();
    d.user("Look things up, then email.");
    d.call_tools(&[
        ("lookup", r#"{"q":"a"}"#),
        ("lookup", r#"{"q":"b"}"#),
        ("send_email", r#"{"to":"x"}"#),
    ]);
    d.tool_ok(&call_of(&d, 0), "A");
    let outs = d.send(Input::Cancel);
    assert!(outs.contains(&Output::CancelTool(call_of(&d, 1))));
    let r = results(&d);
    assert_eq!(r.len(), 3, "no orphan calls");
    assert_eq!(r[1].outcome, ToolOutcome::Cancelled);
    let end = d
        .last_of(|e| match e {
            Event::TurnEnd(t) => Some(t.stop.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(end, TurnStop::Cancelled);
    // A late result is refused.
    let late = d.tool_ok(&call_of(&d, 1), "B");
    assert_eq!(late, [Output::Refused(Refusal::NotOpen)]);
    d.check();
}

#[test]
fn client_tools_wait_time_out_and_resume() {
    let mut d = driver();
    d.send(Input::ClientGone);
    d.user("Add a heading.");
    d.call_tools(&[("insert_block", r#"{"text":"Overview"}"#)]);
    let status = d
        .last_of(|e| match e {
            Event::SessionStatus(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(status.status, Status::AwaitingClient);
    assert_eq!(status.calls, vec![call_of(&d, 0)]);
    d.advance(5_001);
    d.send(Input::Tick);
    let r = results(&d).pop().unwrap();
    assert_eq!(r.outcome, ToolOutcome::Timeout);
    assert_eq!(r.error.unwrap().code, "client.gone");
    d.reply("The editor isn't open, so I couldn't add it.");
    d.send(Input::ClientBack);
    let status = d
        .last_of(|e| match e {
            Event::SessionStatus(s) => Some(s.status.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(status, Status::Active);
    d.user("Try now.");
    d.call_tools(&[("insert_block", r#"{"text":"Overview"}"#)]);
    d.tool_ok(&call_of(&d, 1), "Inserted.");
    assert_eq!(results(&d).pop().unwrap().outcome, ToolOutcome::Ok);
    d.check();
}

#[test]
fn compaction_trims_long_results_and_replay_stays_exact() {
    let mut d = driver();
    d.user("Read the long note.");
    d.call_tools(&[("read_note", r#"{"id":"long"}"#)]);
    let long: String = (0..900).map(|i| format!("line {i:04}\n")).collect();
    d.send(Input::Tool {
        call: call_of(&d, 0),
        output: ToolOutput::Done(ToolDone::ok(long)),
    });
    let c = d
        .last_of(|e| match e {
            Event::Compacted(c) => Some(c.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(c.trimmed.len(), 1);
    assert!(c.tokens_after <= 2_000 && c.tokens_before > 2_000);
    assert!(d.last_request().estimate_tokens() <= 2_000);
    d.reply("It lists 900 lines.");
    d.check();
}
