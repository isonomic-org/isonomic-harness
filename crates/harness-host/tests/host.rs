//! Gate 11 and the host calls, against MemoryHost.

use harness_core::*;
use harness_host::*;

fn setup() -> (MemoryHost, Caller, Caller) {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let b = m.caller(&m.app_token("docs", AppKind::FirstParty));
    (m, a, b)
}

#[test]
fn tokens_issued_elsewhere_resolve() {
    let m = MemoryHost::new();
    let t = CapabilityToken("sdk-grant-1".into());
    m.insert_token(t.clone(), Caller::app("notes", AppKind::FirstParty));
    let c = m.caller(&t);
    assert_eq!(c.owner(), AppId::from("notes"));
    let s = create(&m, &c, "test");
    m.block_on(m.host.send(&c, &s, text("Hi"))).unwrap();
    assert_eq!(m.host.schema(), (SCHEMA_VERSION, SCHEMA_REVISION));
}

fn text(t: &str) -> Input {
    Input {
        content: vec![Block::text(t)],
        client_ref: None,
    }
}

fn types(m: &MemoryHost, s: &SessionId) -> Vec<String> {
    m.log(s)
        .iter()
        .map(|e| e.event.type_name().to_owned())
        .collect()
}

fn create(m: &MemoryHost, c: &Caller, preset: &str) -> SessionId {
    m.block_on(
        m.host
            .create_session(c, &PresetId::from(preset), CreateOptions::default()),
    )
    .unwrap()
}

#[test]
fn a_turn_runs_end_to_end_with_persist_then_publish() {
    let (m, a, _) = setup();
    let s = create(&m, &a, "test");
    let mut sub = m.block_on(m.host.subscribe(&a, &s, 0)).unwrap();
    m.model.text("Hello there.");
    m.model.text("Greeting");
    m.block_on(m.host.send(&a, &s, text("Hi"))).unwrap();
    let t = types(&m, &s);
    assert_eq!(t.last().unwrap(), "title");
    assert!(t.contains(&"turn.end".to_string()));
    // Everything published was persisted first, in order, once.
    let mut seen = Vec::new();
    for _ in 0..t.len() {
        seen.push(m.block_on(sub.next()).unwrap().seq);
    }
    assert_eq!(seen, (1..=t.len() as u64).collect::<Vec<_>>());
    let snap = m.block_on(m.host.open_session(&a, &s)).unwrap();
    assert_eq!(snap.summary.title.as_deref(), Some("Greeting"));
    assert_eq!(snap.turn_open, None);
}

#[test]
fn apps_cannot_reach_each_others_sessions_presets_or_artifacts() {
    let (m, a, b) = setup();
    let s = create(&m, &a, "test");
    m.block_on(m.host.send(&a, &s, text("Hi"))).unwrap();
    let nf = Err(HostError::NotFound);
    assert_eq!(m.block_on(m.host.open_session(&b, &s)).map(|_| ()), nf);
    assert_eq!(
        m.block_on(m.host.send(&b, &s, text("Hijack"))).map(|_| ()),
        nf
    );
    assert_eq!(m.block_on(m.host.cancel(&b, &s)), nf);
    assert_eq!(m.block_on(m.host.subscribe(&b, &s, 0)).map(|_| ()), nf);
    assert_eq!(m.block_on(m.host.list_artifacts(&b, &s)).map(|_| ()), nf);
    assert!(m
        .block_on(m.host.list_sessions(&b, SessionFilter::default()))
        .unwrap()
        .items
        .is_empty());
    let shell = m.caller(&m.shell_token());
    assert_eq!(
        m.block_on(m.host.list_sessions(&shell, SessionFilter::default()))
            .unwrap()
            .items
            .len(),
        1
    );

    // Presets: an app's own preset is invisible to the other.
    let toml = "id = \"mine\"\nversion = 1\n[persona]\nprompt = \"Hi.\"\n[limits]\ntokens = 1000\nsteps = 4\nwall_ms = 1000\n".to_string();
    m.block_on(m.host.put_preset(&a, toml)).unwrap();
    let names = |c: &Caller| -> Vec<String> {
        m.block_on(m.host.list_presets(c))
            .unwrap()
            .iter()
            .map(|p| p.reference.id.to_string())
            .collect()
    };
    assert!(names(&a).contains(&"mine".to_string()));
    assert!(!names(&b).contains(&"mine".to_string()));
    assert_eq!(
        m.block_on(m.host.get_preset(&b, &PresetId::from("mine")))
            .map(|_| ()),
        nf
    );
    assert!(m
        .block_on(
            m.host
                .create_session(&b, &PresetId::from("mine"), CreateOptions::default())
        )
        .is_err());

    // B's purge leaves A's session alone.
    let r = m.block_on(m.host.purge(&b, &b.owner())).unwrap();
    assert_eq!(r.sessions, 0);
    assert!(!m.log(&s).is_empty());
}

#[test]
fn presets_validate_tighten_and_stay_listed_when_invalid() {
    let (m, a, _) = setup();
    let loose = "id = \"wide\"\nversion = 1\n[persona]\nprompt = \"Hi.\"\n".to_string();
    let Err(HostError::InvalidPreset(d)) = m.block_on(m.host.put_preset(&a, loose)) else {
        panic!("no limits is looser than the ceiling")
    };
    assert!(d.iter().any(|d| d.path == "limits.tokens"));
    let listed = m.block_on(m.host.list_presets(&a)).unwrap();
    let wide = listed
        .iter()
        .find(|p| p.reference.id.as_str() == "wide")
        .unwrap();
    assert!(!wide.diagnostics.is_empty());
    let auto = "id = \"auto\"\nversion = 1\nclass = \"autonomous\"\n[persona]\nprompt = \"Hi.\"\n[limits]\ntokens = 10\nsteps = 1\nwall_ms = 10\n[[wake]]\non = \"schedule\"\nspec = \"daily\"\n".to_string();
    m.block_on(m.host.put_preset(&a, auto)).unwrap();
    assert_eq!(
        m.block_on(
            m.host
                .create_session(&a, &PresetId::from("auto"), CreateOptions::default())
        ),
        Err(HostError::ClassNotSupported(SessionClass::Autonomous))
    );
}

fn outward(m: &MemoryHost, c: &Caller, s: &SessionId) -> ApprovalId {
    m.block_on(m.host.register_client_tools(
        c,
        s,
        vec![ToolSpec {
            name: "post".into(),
            description: "Post a message.".into(),
            input_schema: Value::object([("type", Value::from("object"))]),
            kind: ToolKind::Client,
            effects: vec![Effect::ActsOutward],
            concurrency: Concurrency::Serial,
        }],
    ))
    .unwrap();
    m.model.calls(&[("post", "{\"text\":\"hi\"}")]);
    m.block_on(m.host.send(c, s, text("Post hi."))).unwrap();
    m.block_on(m.host.list_approvals(c)).unwrap()[0]
        .request
        .approval
        .clone()
}

#[test]
fn approvals_need_a_user_action_and_the_right_decider() {
    let (m, a, _) = setup();
    let s = create(&m, &a, "test");
    let id = outward(&m, &a, &s);
    // No token: refused, nothing written.
    let before = m.log(&s).len();
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&a, &s, &id, DecisionInput::Allow, None)
        ),
        Err(HostError::MissingUserAction)
    );
    let forged = UserActionToken("ua:forged".into());
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&a, &s, &id, DecisionInput::Allow, Some(forged))
        ),
        Err(HostError::MissingUserAction)
    );
    assert_eq!(m.log(&s).len(), before);
    // A View of the same app may not decide.
    let view = Caller::app("chat", AppKind::View);
    let t = m.user_action(&view, &s, &id);
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&view, &s, &id, DecisionInput::Allow, Some(t))
        ),
        Err(HostError::NotAllowedToDecide)
    );
    // The shell decides, with an ambiguous answer: a denial.
    let shell = m.caller(&m.shell_token());
    let t = m.user_action(&shell, &s, &id);
    m.model.text("I didn't post it.");
    m.block_on(m.host.decide_approval(
        &shell,
        &s,
        &id,
        DecisionInput::Text("hmm, later".into()),
        Some(t.clone()),
    ))
    .unwrap();
    let d = m.log(&s).iter().find_map(|e| match &e.event {
        Event::ApprovalDecision(d) => Some(d.clone()),
        _ => None,
    });
    let d = d.unwrap();
    assert_eq!((d.decision, d.basis), (Decision::Deny, Basis::Ambiguous));
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&shell, &s, &id, DecisionInput::Allow, Some(t))
        ),
        Err(HostError::AlreadyDecided)
    );
    let act = m.block_on(m.host.activity(&shell, None)).unwrap();
    assert!(act.iter().any(|r| r.kind == "approval.decision"));
    assert_eq!(
        m.block_on(m.host.activity(&a, None)),
        Err(HostError::Unauthorized)
    );
}

#[test]
fn client_gone_waits_times_out_and_resumes() {
    let (m, a, _) = setup();
    let s = create(&m, &a, "test");
    m.block_on(m.host.register_client_tools(
        &a,
        &s,
        vec![ToolSpec {
            name: "insert_block".into(),
            description: "Insert a block.".into(),
            input_schema: Value::object([("type", Value::from("object"))]),
            kind: ToolKind::Client,
            effects: vec![],
            concurrency: Concurrency::Serial,
        }],
    ))
    .unwrap();
    m.disconnect("chat");
    m.model.calls(&[("insert_block", "{}")]);
    m.model.text("The editor is closed.");
    m.block_on(m.host.send(&a, &s, text("Add a heading.")))
        .unwrap();
    let snap = m.block_on(m.host.open_session(&a, &s)).unwrap();
    assert_eq!(snap.summary.status, Status::AwaitingClient);
    assert_eq!(snap.awaiting_client.len(), 1);
    m.advance(6_000);
    let timed_out = m
        .log(&s)
        .iter()
        .any(|e| matches!(&e.event, Event::ToolResult(r) if r.outcome == ToolOutcome::Timeout));
    assert!(timed_out);
    m.reconnect("chat");
    let snap = m.block_on(m.host.open_session(&a, &s)).unwrap();
    assert_eq!(snap.summary.status, Status::Active);
    // Now the call reaches the app and its result closes the step.
    m.model.calls(&[("insert_block", "{}")]);
    m.model.text("Added.");
    m.block_on(m.host.send(&a, &s, text("Try now."))).unwrap();
    let call = m
        .clients
        .invoked
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .2
        .call
        .clone();
    m.block_on(m.host.client_tool_result(
        &a,
        &s,
        &call,
        ClientOutcome::Ok(vec![Block::text("Inserted.")]),
    ))
    .unwrap();
    assert!(m.log(&s).iter().any(|e| matches!(&e.event, Event::ToolResult(r) if r.call == call && r.outcome == ToolOutcome::Ok)));
}

#[test]
fn a_panic_and_a_hang_in_one_session_leave_the_others_running() {
    let (m, a, b) = setup();
    let s1 = create(&m, &a, "test");
    let s2 = create(&m, &a, "test");
    let s3 = create(&m, &b, "test");
    m.inject_panic(&s1);
    let _ = m.block_on(m.host.send(&a, &s1, text("Boom")));
    m.run();
    let status = |s: &SessionId| {
        m.block_on(m.host.open_session(&m.caller(&m.shell_token()), s))
            .unwrap()
            .summary
            .status
    };
    assert_eq!(status(&s1), Status::Failed);
    assert_eq!(
        m.block_on(m.host.send(&a, &s1, text("Again"))).map(|_| ()),
        Err(HostError::SessionFailed)
    );

    m.inject_hang(&s2);
    m.block_on(m.host.send(&a, &s2, text("Hang"))).unwrap();
    m.advance(5_000);
    m.model.text("Fine.");
    m.block_on(m.host.send(&b, &s3, text("Still here?")))
        .unwrap();
    assert!(types(&m, &s3).contains(&"turn.end".to_string()));
    m.advance(10_000);
    assert_eq!(status(&s2), Status::Failed);
    let end = m.log(&s2).iter().rev().find_map(|e| match &e.event {
        Event::TurnEnd(t) => Some(t.clone()),
        _ => None,
    });
    assert_eq!(end.unwrap().stop, TurnStop::Failed);
    // The others keep running.
    m.model.text("Yes.");
    m.block_on(m.host.send(&b, &s3, text("And now?"))).unwrap();
    let ends = types(&m, &s3).iter().filter(|t| *t == "turn.end").count();
    assert_eq!(ends, 2);
}

#[test]
fn purge_stops_an_in_flight_turn_and_removes_only_the_callers_data() {
    let (m, a, b) = setup();
    let s1 = create(&m, &a, "test");
    let s2 = create(&m, &b, "test");
    m.inject_hang(&s1);
    m.block_on(m.host.send(&a, &s1, text("Long task"))).unwrap();
    m.block_on(m.host.send(&b, &s2, text("Hi"))).unwrap();
    // An app may name only itself.
    assert_eq!(
        m.block_on(m.host.purge(&a, &b.owner())),
        Err(HostError::NotFound)
    );
    let r = m.block_on(m.host.purge(&a, &a.owner())).unwrap();
    assert_eq!(r.sessions, 1);
    assert!(r.events > 0);
    assert!(m.log(&s1).is_empty());
    assert!(!m.log(&s2).is_empty());
    assert_eq!(
        m.block_on(m.host.open_session(&a, &s1)).map(|_| ()),
        Err(HostError::NotFound)
    );
    // The shell purges the app it names, and only that app.
    let shell = m.caller(&m.shell_token());
    let r = m.block_on(m.host.purge(&shell, &b.owner())).unwrap();
    assert_eq!(r.sessions, 1);
    assert!(m.log(&s2).is_empty());
}

#[test]
fn a_resuming_subscriber_sees_no_gaps_or_duplicates() {
    let (m, a, _) = setup();
    let s = create(&m, &a, "test");
    for i in 0..3 {
        m.model.text(&format!("Reply {i}"));
        m.block_on(m.host.send(&a, &s, text(&format!("Turn {i}"))))
            .unwrap();
    }
    let total = m.log(&s).len() as u64;
    let mut sub = m.block_on(m.host.subscribe(&a, &s, 0)).unwrap();
    let mut seen = Vec::new();
    for _ in 0..5 {
        seen.push(m.block_on(sub.next()).unwrap().seq);
    }
    let resume_at = sub.last_seq();
    drop(sub);
    m.model.text("Reply 3");
    m.block_on(m.host.send(&a, &s, text("Turn 3"))).unwrap();
    let mut sub = m.block_on(m.host.subscribe(&a, &s, resume_at)).unwrap();
    let now_total = m.log(&s).len() as u64;
    assert!(now_total > total);
    for _ in resume_at..now_total {
        seen.push(m.block_on(sub.next()).unwrap().seq);
    }
    assert_eq!(seen, (1..=now_total).collect::<Vec<_>>());
}

#[test]
fn a_slow_subscriber_does_not_delay_the_loop() {
    fn drive(subscribed: bool) -> (std::time::Duration, u64) {
        let mut cfg = memory_config();
        cfg.subscriber_queue = 8;
        let m = MemoryHost::with(cfg);
        let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
        let s = create(&m, &a, "test");
        let sub = subscribed.then(|| m.block_on(m.host.subscribe(&a, &s, 0)).unwrap());
        let t0 = std::time::Instant::now();
        for i in 0..100 {
            m.model.text(&format!("Reply {i}"));
            m.block_on(m.host.send(&a, &s, text("Go"))).unwrap();
        }
        let took = t0.elapsed();
        // The stalled subscriber still catches up from the store.
        if let Some(mut sub) = sub {
            let total = m.log(&s).len() as u64;
            let mut last = 0;
            for _ in 0..total {
                last = m.block_on(sub.next()).unwrap().seq;
            }
            assert_eq!(last, total);
        }
        (took, m.log(&s).len() as u64)
    }
    let (base, n1) = drive(false);
    let (slow, n2) = drive(true);
    assert_eq!(n1, n2);
    eprintln!("100 turns: {base:?} without a subscriber, {slow:?} with a stalled one");
    assert!(
        slow < base * 3 + std::time::Duration::from_millis(50),
        "{slow:?} vs {base:?}"
    );
}

#[test]
fn recover_seals_interrupted_turns_and_forks_are_independent() {
    let (m, a, _) = setup();
    let s = create(&m, &a, "test");
    m.model.text("Blue.");
    m.block_on(m.host.send(&a, &s, text("A color?"))).unwrap();
    let turn = m.log(&s).iter().find_map(|e| e.turn.clone()).unwrap();
    let child = m
        .block_on(m.host.fork_session(
            &a,
            &s,
            ForkPoint::AfterTurn(turn.clone()),
            ForkReason::Branch,
        ))
        .unwrap();
    m.model.text("Red.");
    m.block_on(m.host.send(&a, &child, text("Another?")))
        .unwrap();
    let req = m
        .model
        .requests
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|r| r.system != harness_log::history::TITLE_PROMPT)
        .unwrap()
        .clone();
    let all: Vec<String> = req
        .messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|p| match p {
            harness_log::Part::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(all, ["A color?", "Blue.", "Another?"]);
    assert_eq!(
        m.block_on(m.host.fork_session(
            &a,
            &s,
            ForkPoint::BeforeTurn(TurnId::from("nope")),
            ForkReason::Edit
        )),
        Err(HostError::NotAtTurnBoundary)
    );

    // A second host over the same store recovers an interrupted turn.
    m.inject_hang(&s);
    m.block_on(m.host.send(&a, &s, text("Interrupted?")))
        .unwrap();
    let store = m.store.clone();
    let mut cfg = memory_config();
    cfg.watchdog_ms = 1_000_000;
    let m2 = MemoryHost::with(cfg);
    for id in store.sessions() {
        let events = store.events(&id);
        m2.block_on(harness_log::SessionStore::append(&*m2.store, &id, &events))
            .unwrap();
    }
    for meta in m
        .block_on(harness_log::SessionStore::list_meta(&*m.store, None))
        .unwrap()
    {
        m2.block_on(harness_log::SessionStore::put_meta(&*m2.store, &meta))
            .unwrap();
    }
    assert_eq!(m2.block_on(m2.host.recover()).unwrap(), 2);
    let end = m2.log(&s).iter().rev().find_map(|e| match &e.event {
        Event::TurnEnd(t) => Some(t.stop.clone()),
        _ => None,
    });
    assert_eq!(end, Some(TurnStop::Interrupted));
}
