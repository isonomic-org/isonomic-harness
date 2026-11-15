//! harness-log against the fixtures: JSONL, migrations and the history rules.

use harness_core::future::fnv1a64;
use harness_core::*;
use harness_log::*;
use harness_testkit::{fixtures, schema_check};
use std::collections::BTreeMap;

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    // Every MemoryStore future is ready on first poll.
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("memory store future pending"),
    }
}

/// Each session's resolved history, keyed by session.
fn resolved(events: &[SessionEvent]) -> BTreeMap<SessionId, Vec<SessionEvent>> {
    let mut own: BTreeMap<SessionId, Vec<SessionEvent>> = BTreeMap::new();
    for e in events {
        own.entry(e.session.clone()).or_default().push(e.clone());
    }
    let mut out = BTreeMap::new();
    for (id, log) in &own {
        let parent = log.iter().find_map(|e| match &e.event {
            Event::Forked(f) => Some(f.from.clone()),
            _ => None,
        });
        let full = match parent {
            Some(p) => resolve_fork(&own[&p], log),
            None => log.clone(),
        };
        out.insert(id.clone(), full);
    }
    out
}

fn all_requests(events: &[SessionEvent]) -> Vec<(StepId, ModelRequest)> {
    let mut out = Vec::new();
    for (id, full) in resolved(events) {
        for e in full.iter().filter(|e| e.session == id) {
            if let (Event::StepStart(_), Some(step)) = (&e.event, &e.step) {
                out.push((step.clone(), request_for(&full, step).expect("derives")));
            }
        }
    }
    out
}

fn text_of(req: &ModelRequest) -> String {
    String::from_utf8(req.bytes()).unwrap()
}

#[test]
fn generations_round_trip_every_fixture() {
    for f in fixtures::all() {
        let text = encode(&f.events);
        assert_eq!(text, f.jsonl(), "{}", f.name);
        let g = decode(&text).unwrap();
        assert_eq!(g.written_version, SCHEMA_VERSION);
        assert_eq!(g.events, f.events, "{}", f.name);
    }
}

#[test]
fn future_versions_and_other_formats_are_refused() {
    let future = "{\"format\":\"isonomic.session-log\",\"version\":2,\"revision\":0}\n";
    assert_eq!(
        decode(future),
        Err(LogError::FutureVersion {
            found: 2,
            supported: 1
        })
    );
    assert!(matches!(
        decode("{\"format\":\"other\",\"version\":1}\n"),
        Err(LogError::Header(_))
    ));
    assert_eq!(decode(""), Err(LogError::Empty));
}

#[test]
fn a_v0_generation_migrates_to_v1() {
    let text = include_str!("data/v0-chat.jsonl");
    let g = decode(text).unwrap();
    assert_eq!(g.written_version, 0);
    assert_eq!(g.events.len(), 8);
    let schema: serde_json::Value = serde_json::from_str(JSON_SCHEMA).unwrap();
    for e in &g.events {
        assert!(!matches!(e.event, Event::Unknown(_)), "{e:?}");
        let json = serde_json::to_value(e).unwrap();
        schema_check::check(&schema, &json).unwrap();
    }
    match &g.events[5].event {
        Event::Assistant(a) => assert_eq!(a.content, vec![Block::text("Hi! How can I help?")]),
        other => panic!("{other:?}"),
    }
    // Reading never rewrites: the input text is untouched and re-encoding
    // writes a new v1 generation.
    assert!(encode(&g.events).starts_with("{\"format\":\"isonomic.session-log\",\"version\":1"));
    let step = StepId::from("stp_v0");
    let req = request_for(&g.events, &step).unwrap();
    assert_eq!(req.messages.len(), 1);
}

#[test]
fn every_fixture_request_derives_and_is_deterministic() {
    for f in fixtures::all() {
        if f.name == "long-thread" {
            continue;
        }
        let a = all_requests(&f.events);
        let b = all_requests(&f.events);
        assert!(!a.is_empty(), "{}", f.name);
        for ((s1, r1), (s2, r2)) in a.iter().zip(&b) {
            assert_eq!(s1, s2);
            assert_eq!(fnv1a64(&r1.bytes()), fnv1a64(&r2.bytes()));
        }
    }
}

#[test]
fn refusals_and_attempts_never_reach_history() {
    for f in fixtures::all() {
        if f.name == "long-thread" {
            continue;
        }
        for (_, req) in all_requests(&f.events) {
            let text = text_of(&req);
            assert!(!text.contains("I can't help get into"), "{}", f.name);
            assert!(!text.contains("model.busy"), "{}", f.name);
            assert!(!text.contains("Short, warm and local"), "reasoning leaked");
        }
    }
}

#[test]
fn continue_is_a_prefill_and_extends_the_sealed_reply() {
    let f = fixtures::chat_basic();
    let reqs = all_requests(&f.events);
    let cont = reqs
        .iter()
        .find(|(_, r)| r.prefill)
        .expect("a continue request");
    let last = cont.1.messages.last().unwrap();
    assert_eq!(last.role, Role::Assistant);
    assert_eq!(
        last.content,
        vec![Part::Text {
            text:
                "Welcome, everyone! We're so glad you're here for our first meeting. Tonight we'll"
                    .into()
        }]
    );
    // After the continue, the refusal turn's request carries one merged reply.
    let refusal_turn = reqs.iter().find(|(_, r)| text_of(r).contains("wifi"));
    let merged = text_of(&refusal_turn.unwrap().1);
    assert!(merged.contains("Tonight we'll get to know each other"));
}

#[test]
fn forks_share_the_prefix_and_nothing_after() {
    let f = fixtures::chat_basic();
    let map = resolved(&f.events);
    let children: Vec<&SessionId> = map.keys().filter(|s| s.as_str() != "ses_0001").collect();
    assert_eq!(children.len(), 2);
    for child in children {
        let full = &map[child];
        let step = full
            .iter()
            .rev()
            .find_map(|e| {
                matches!(e.event, Event::StepStart(_))
                    .then(|| e.step.clone())
                    .flatten()
            })
            .unwrap();
        let text = text_of(&request_for(full, &step).unwrap());
        assert!(text.contains("three good names"), "prefix shared");
        assert!(!text.contains("wifi"), "parent's later turns not shared");
    }
}

#[test]
fn tool_results_follow_call_order_and_titles_use_their_prompt() {
    let f = fixtures::tool_heavy();
    let reqs = all_requests(&f.events);
    let second = &reqs[1].1;
    let tool_msg = second
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .unwrap();
    let calls: Vec<&str> = tool_msg
        .content
        .iter()
        .map(|p| match p {
            Part::ToolResult { call, .. } => call.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(calls, ["cal_0001", "cal_0002", "cal_0003"]);
    assert_eq!(
        heuristic_title(&f.events),
        "Pull together my notes on the garden project"
    );

    let chat = fixtures::chat_basic();
    let title = all_requests(&chat.events)
        .into_iter()
        .find(|(_, r)| r.system == history::TITLE_PROMPT)
        .unwrap()
        .1;
    assert_eq!(title.messages.len(), 2);
    assert!(title.tools.is_empty());
}

#[test]
fn compaction_trims_and_summarizes_from_its_position_on() {
    let f = fixtures::autonomous_run();
    let reqs = all_requests(&f.events);
    let summary_step = reqs
        .iter()
        .position(|(_, r)| r.system == history::SUMMARY_PROMPT)
        .unwrap();
    let before = text_of(&reqs[summary_step - 1].1);
    assert!(!before.contains("bytes trimmed"));
    let after = &reqs[summary_step + 1].1;
    let text = text_of(after);
    assert!(text.contains("bytes trimmed"));
    assert!(matches!(after.messages[0].content[0], Part::Summary { .. }));
    assert!(after.estimate_tokens() < reqs[summary_step - 1].1.estimate_tokens());
}

#[test]
fn memory_store_appends_in_order_and_purges() {
    let store = MemoryStore::new();
    let f = fixtures::client_gone();
    let s = f.events[0].session.clone();
    block_on(store.append(&s, &f.events[..3])).unwrap();
    assert_eq!(
        block_on(store.append(&s, &f.events[4..5])),
        Err(StoreError::Conflict {
            expected: 4,
            found: 5
        })
    );
    block_on(store.append(&s, &f.events[3..])).unwrap();
    let tail = block_on(store.read(&s, 10, 5)).unwrap();
    assert_eq!(tail.first().unwrap().seq, 11);
    assert_eq!(tail.len(), 5);
    assert_eq!(block_on(store.purge(std::slice::from_ref(&s))).unwrap(), 28);
    assert_eq!(block_on(store.read(&s, 0, 1)), Err(StoreError::NotFound));
}
