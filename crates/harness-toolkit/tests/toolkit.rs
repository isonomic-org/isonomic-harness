//! The Milestone 0 toolkit and the service fakes.

use harness_core::future::BoxFuture;
use harness_core::*;
use harness_toolkit::services::*;
use harness_toolkit::*;
use harness_tools::{Tool, ToolCtx, ToolOutput};
use std::sync::Arc;

fn poll<T>(f: BoxFuture<'_, T>) -> T {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let mut f = f;
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("fakes are ready at once"),
    }
}

fn cx(session: &str) -> ToolCtx {
    ToolCtx {
        session: SessionId::from(session),
        app: AppId::from("chat"),
        call: CallId::from("cal_0001"),
        now: 1_794_756_600_000,
        child: None,
    }
}

fn text(out: ToolOutput) -> (ToolOutcome, String) {
    match out {
        ToolOutput::Done(d) => (
            d.outcome,
            d.content
                .iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect(),
        ),
        ToolOutput::Ask(q) => panic!("unexpected question {q:?}"),
    }
}

fn input(pairs: Vec<(&str, Value)>) -> Value {
    Value::object(pairs)
}

#[test]
fn the_toolkit_offers_four_tools() {
    let zone = Arc::new(FixedZone {
        name: "UTC".into(),
        offset_minutes: 0,
    });
    let names: Vec<String> = toolkit(zone).iter().map(|t| t.spec().name).collect();
    assert_eq!(names, ["ask_user", "todo", "plan", "now"]);
}

#[test]
fn ask_user_becomes_a_question() {
    let tool = AskUser;
    let i = input(vec![
        ("prompt", Value::from("Which day?")),
        (
            "options",
            Value::Array(vec![
                Value::from("Tuesday"),
                Value::object([
                    ("id", Value::from("wed")),
                    ("label", Value::from("Wednesday")),
                ]),
            ]),
        ),
    ]);
    let ToolOutput::Ask(q) = poll(tool.call(&i, &cx("s"))) else {
        panic!("expected a question")
    };
    assert_eq!(q.prompt, "Which day?");
    assert_eq!(q.options[0].id, "1");
    assert_eq!(q.options[1].id, "wed");
    assert!(q.free_text && !q.multiple);
    let bad = input(vec![("prompt", Value::from(" "))]);
    assert_eq!(text(poll(tool.call(&bad, &cx("s")))).0, ToolOutcome::Error);
}

#[test]
fn todo_keeps_a_list_per_session_and_replays_from_the_log() {
    let tool = Todo::default();
    let add = |item: &str| {
        input(vec![
            ("action", Value::from("add")),
            ("item", Value::from(item)),
        ])
    };
    assert_eq!(
        text(poll(tool.call(&add("Book a room"), &cx("a")))).1,
        "Added."
    );
    poll(tool.call(&add("Send the agenda"), &cx("a")));
    poll(tool.call(&add("Other session"), &cx("b")));
    let done = input(vec![
        ("action", Value::from("done")),
        ("item", Value::from("2")),
    ]);
    assert_eq!(text(poll(tool.call(&done, &cx("a")))).1, "Checked off.");
    let list = input(vec![("action", Value::from("list"))]);
    assert_eq!(
        text(poll(tool.call(&list, &cx("a")))).1,
        "1. [ ] Book a room\n2. [x] Send the agenda"
    );
    let missing = input(vec![
        ("action", Value::from("done")),
        ("item", Value::from("Fly")),
    ]);
    assert_eq!(
        text(poll(tool.call(&missing, &cx("a")))).0,
        ToolOutcome::Error
    );

    // The fixture's calls rebuild the list.
    let fixture = harness_testkit_free_events();
    let fresh = Todo::default();
    fresh.replay(&SessionId::from("a"), &fixture);
    assert_eq!(fresh.render(&SessionId::from("a")), "1. [ ] Book a room");
}

/// A todo call and its result, as a log would hold them.
fn harness_testkit_free_events() -> Vec<SessionEvent> {
    let ev = |seq, event| SessionEvent {
        session: SessionId::from("a"),
        seq,
        at: 0,
        turn: None,
        step: None,
        event,
    };
    let call = |id: &str, item: &str| ToolCall {
        call: CallId::from(id),
        tool: "todo".into(),
        kind: ToolKind::Engine,
        input: input(vec![
            ("action", Value::from("add")),
            ("item", Value::from(item)),
        ]),
        input_raw: None,
        effects: vec![],
        batch: None,
        child: None,
    };
    let result = |id: &str, outcome| ToolResult {
        call: CallId::from(id),
        outcome,
        content: vec![],
        error: None,
        truncated: None,
        duration_ms: None,
    };
    vec![
        ev(1, Event::ToolCall(call("c1", "Book a room"))),
        ev(2, Event::ToolResult(result("c1", ToolOutcome::Ok))),
        ev(3, Event::ToolCall(call("c2", "Never ran"))),
        ev(4, Event::ToolResult(result("c2", ToolOutcome::Denied))),
    ]
}

#[test]
fn plan_records_steps() {
    let tool = PlanTool::default();
    let i = input(vec![
        (
            "steps",
            Value::Array(vec![Value::from("Collect"), Value::from("Summarize")]),
        ),
        ("current", Value::from(2u64)),
    ]);
    assert_eq!(
        text(poll(tool.call(&i, &cx("a")))).1,
        "Plan recorded: 2 steps."
    );
    assert_eq!(
        tool.render(&SessionId::from("a")).unwrap(),
        "  1. Collect\n→ 2. Summarize"
    );
    let bad = input(vec![("steps", Value::Array(vec![]))]);
    assert_eq!(text(poll(tool.call(&bad, &cx("a")))).0, ToolOutcome::Error);
}

#[test]
fn now_formats_in_the_persons_zone() {
    let tool = Now::new(Arc::new(FixedZone {
        name: "America/New_York".into(),
        offset_minutes: -300,
    }));
    assert_eq!(
        text(poll(tool.call(&Value::Null, &cx("a")))).1,
        "Sunday, 15 November 2026, 10:30 (America/New_York, UTC-05:00)"
    );
    assert_eq!(
        Now::format(0, "Asia/Kolkata", 330),
        "Thursday, 1 January 1970, 05:30 (Asia/Kolkata, UTC+05:30)"
    );
    assert_eq!(
        Now::format(951_782_400_000, "UTC", 0),
        "Tuesday, 29 February 2000, 00:00 (UTC, UTC+00:00)"
    );
}

#[test]
fn service_fakes_keep_owners_apart() {
    let a = SessionId::from("a");
    let store = FakeArtifacts::default();
    let v1 = poll(store.put(&a, None, "Notes", "text/markdown", b"one".to_vec())).unwrap();
    let v2 = poll(store.put(
        &a,
        Some(v1.artifact.clone()),
        "Notes",
        "text/markdown",
        b"two".to_vec(),
    ))
    .unwrap();
    assert_eq!((v1.version, v2.version), (1, 2));
    assert_eq!(v2.op, ArtifactOp::Update);
    assert_eq!(poll(store.get(&a, &v1.artifact)).unwrap(), b"two");
    assert!(poll(store.get(&SessionId::from("b"), &v1.artifact)).is_err());

    let web = FakeEgress {
        pages: vec![Page {
            url: "https://example.test/a".into(),
            title: "Lead times".into(),
            text: "Stable this week.".into(),
        }],
    };
    assert_eq!(poll(web.search("lead")).unwrap().len(), 1);
    assert!(poll(web.read("https://example.test/b")).is_err());

    let mem = FakeMemory::default();
    let chat = AppId::from("chat");
    poll(mem.remember(&chat, "Prefers mornings")).unwrap();
    assert_eq!(poll(mem.recall(&chat, "morning")).unwrap().len(), 1);
    assert!(poll(mem.recall(&AppId::from("docs"), "morning"))
        .unwrap()
        .is_empty());

    let mut conns = FakeConnections::default();
    conns.by_owner.insert(
        chat.clone(),
        vec![Connection {
            handle: "cal".into(),
            label: "Calendar".into(),
            operations: vec!["list".into()],
        }],
    );
    assert!(poll(conns.call(&chat, "cal", "list", Value::Null)).is_ok());
    assert!(poll(conns.call(&chat, "cal", "delete", Value::Null)).is_err());
    assert!(poll(conns.list(&AppId::from("docs"))).is_empty());
}
