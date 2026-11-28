// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Approval, question and call ids are minted per session, so two sessions
//! hold the same ids. The host keeps them apart by (session, id).

use harness_core::*;
use harness_host::*;

fn text(t: &str) -> Input {
    Input {
        content: vec![Block::text(t)],
        client_ref: None,
    }
}

fn create(m: &MemoryHost, c: &Caller) -> SessionId {
    m.block_on(
        m.host
            .create_session(c, &PresetId::from("test"), CreateOptions::default()),
    )
    .unwrap()
}

fn client_tool(name: &str, effects: Vec<Effect>) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("The {name} tool."),
        input_schema: Value::object([("type", Value::from("object"))]),
        kind: ToolKind::Client,
        effects,
        concurrency: Concurrency::Serial,
    }
}

/// One turn that calls `tool`, leaving the call open.
fn call(m: &MemoryHost, c: &Caller, s: &SessionId, tool: ToolSpec) {
    let name = tool.name.clone();
    m.block_on(m.host.register_client_tools(c, s, vec![tool]))
        .unwrap();
    m.model.calls(&[(name.as_str(), "{}")]);
    m.block_on(m.host.send(c, s, text("Go."))).unwrap();
}

fn decisions(m: &MemoryHost, s: &SessionId) -> usize {
    m.log(s)
        .iter()
        .filter(|e| matches!(e.event, Event::ApprovalDecision(_)))
        .count()
}

#[test]
fn two_sessions_with_the_same_approval_id_stay_apart() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let b = m.caller(&m.app_token("docs", AppKind::FirstParty));
    let shell = m.caller(&m.shell_token());
    let (sa, sb) = (create(&m, &a), create(&m, &b));
    call(&m, &a, &sa, client_tool("post", vec![Effect::ActsOutward]));
    call(&m, &b, &sb, client_tool("post", vec![Effect::ActsOutward]));

    // Both are pending, under the same id.
    let all = m.block_on(m.host.list_approvals(&shell)).unwrap();
    assert_eq!(all.len(), 2);
    let id = all[0].request.approval.clone();
    assert_eq!(all[1].request.approval, id);
    assert_eq!(m.block_on(m.host.list_approvals(&a)).unwrap().len(), 1);
    assert_eq!(m.block_on(m.host.list_approvals(&b)).unwrap().len(), 1);

    // A user action on one session's approval does not decide the other's.
    let for_a = m.user_action(&shell, &sa, &id);
    assert_eq!(
        m.block_on(m.host.decide_approval(
            &shell,
            &sb,
            &id,
            DecisionInput::Allow,
            Some(for_a.clone())
        )),
        Err(HostError::MissingUserAction)
    );
    // An app cannot reach the other app's approval through its session.
    let own = m.user_action(&a, &sb, &id);
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&a, &sb, &id, DecisionInput::Allow, Some(own))
        ),
        Err(HostError::NotFound)
    );

    // Deciding A's leaves B's pending.
    m.model.text("Posted.");
    m.block_on(
        m.host
            .decide_approval(&shell, &sa, &id, DecisionInput::Deny, Some(for_a.clone())),
    )
    .unwrap();
    assert_eq!((decisions(&m, &sa), decisions(&m, &sb)), (1, 0));
    let left = m.block_on(m.host.list_approvals(&shell)).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].session, sb);
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&shell, &sa, &id, DecisionInput::Allow, Some(for_a))
        ),
        Err(HostError::AlreadyDecided)
    );
    // A's decided approval says nothing to B.
    let own = m.user_action(&b, &sa, &id);
    assert_eq!(
        m.block_on(
            m.host
                .decide_approval(&b, &sa, &id, DecisionInput::Allow, Some(own))
        ),
        Err(HostError::NotFound)
    );

    // B's is still decidable.
    let for_b = m.user_action(&shell, &sb, &id);
    m.model.text("Not posted.");
    m.block_on(
        m.host
            .decide_approval(&shell, &sb, &id, DecisionInput::Deny, Some(for_b)),
    )
    .unwrap();
    assert_eq!(decisions(&m, &sb), 1);
}

#[test]
fn two_sessions_with_the_same_call_id_stay_apart() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let (s1, s2) = (create(&m, &a), create(&m, &a));
    call(&m, &a, &s1, client_tool("insert_block", vec![]));
    call(&m, &a, &s2, client_tool("insert_block", vec![]));
    let invoked = m.clients.invoked.lock().unwrap().clone();
    assert_eq!(invoked.len(), 2);
    let id = invoked[0].2.call.clone();
    assert_eq!(invoked[1].2.call, id);

    let result = |s: &SessionId| {
        m.log(s)
            .iter()
            .filter(|e| matches!(e.event, Event::ToolResult(_)))
            .count()
    };
    m.model.text("Done in the second.");
    m.block_on(m.host.client_tool_result(
        &a,
        &s2,
        &id,
        ClientOutcome::Ok(vec![Block::text("Inserted.")]),
    ))
    .unwrap();
    assert_eq!((result(&s1), result(&s2)), (0, 1));
    let open = |s: &SessionId| {
        m.block_on(m.host.open_session(&a, s))
            .unwrap()
            .turn_open
            .is_some()
    };
    assert!(open(&s1));
    assert!(!open(&s2));

    // A second result for the closed call is dropped and audited there.
    m.block_on(m.host.client_tool_result(
        &a,
        &s2,
        &id,
        ClientOutcome::Ok(vec![Block::text("Again.")]),
    ))
    .unwrap();
    assert_eq!((result(&s1), result(&s2)), (0, 1));
    let shell = m.caller(&m.shell_token());
    let late: Vec<_> = m
        .block_on(m.host.activity(&shell, None))
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == "tool.result.late")
        .collect();
    assert_eq!(late.len(), 1);
    assert_eq!(late[0].session, s2);

    // The first session's call is still open and takes its own result.
    m.model.text("Done in the first.");
    m.block_on(m.host.client_tool_result(
        &a,
        &s1,
        &id,
        ClientOutcome::Ok(vec![Block::text("Inserted.")]),
    ))
    .unwrap();
    assert_eq!(result(&s1), 1);
}

#[test]
fn results_and_answers_name_a_session_the_caller_owns() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let b = m.caller(&m.app_token("docs", AppKind::FirstParty));
    let sa = create(&m, &a);
    call(&m, &a, &sa, client_tool("insert_block", vec![]));
    let id = m.clients.invoked.lock().unwrap()[0].2.call.clone();
    assert_eq!(
        m.block_on(m.host.client_tool_result(
            &b,
            &sa,
            &id,
            ClientOutcome::Ok(vec![Block::text("Hijack.")]),
        )),
        Err(HostError::NotFound)
    );
    let answer = AnswerInput {
        selected: Vec::new(),
        text: Some("Yes".into()),
        skipped: false,
    };
    let q = QuestionId::from("qst_0001");
    assert_eq!(
        m.block_on(m.host.answer_question(&b, &sa, &q, answer.clone())),
        Err(HostError::NotFound)
    );
    // No such question in the caller's own session either.
    assert_eq!(
        m.block_on(m.host.answer_question(&a, &sa, &q, answer)),
        Err(HostError::NotFound)
    );
}

/// One turn that asks the person a question, leaving it open.
fn ask(m: &MemoryHost, c: &Caller, s: &SessionId) {
    m.model
        .calls(&[("ask_user", "{\"prompt\":\"Which one?\"}")]);
    m.block_on(m.host.send(c, s, text("Ask me."))).unwrap();
}

fn count(m: &MemoryHost, s: &SessionId, kind: &str) -> usize {
    m.log(s)
        .iter()
        .filter(|e| e.event.type_name() == kind)
        .count()
}

#[test]
fn two_sessions_with_the_same_question_id_stay_apart() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let (s1, s2) = (create(&m, &a), create(&m, &a));
    ask(&m, &a, &s1);
    ask(&m, &a, &s2);
    let open = |s: &SessionId| m.block_on(m.host.open_session(&a, s)).unwrap().questions;
    let id = open(&s1)[0].clone();
    assert_eq!(open(&s2), std::slice::from_ref(&id));

    let answer = AnswerInput {
        selected: Vec::new(),
        text: Some("The first.".into()),
        skipped: false,
    };
    m.model.text("Noted.");
    m.block_on(m.host.answer_question(&a, &s2, &id, answer.clone()))
        .unwrap();
    assert_eq!((count(&m, &s1, "answer"), count(&m, &s2, "answer")), (0, 1));
    // The other session's question is still open, and takes its own answer.
    assert_eq!(open(&s1), std::slice::from_ref(&id));
    assert!(open(&s2).is_empty());
    assert_eq!(
        m.block_on(m.host.answer_question(&a, &s2, &id, answer.clone())),
        Err(HostError::NotFound)
    );
    m.model.text("Noted here too.");
    m.block_on(m.host.answer_question(&a, &s1, &id, answer))
        .unwrap();
    assert_eq!(count(&m, &s1, "answer"), 1);
}

/// A session with one finished turn, and a fork of it after that turn.
fn parent_and_fork(m: &MemoryHost, c: &Caller) -> (SessionId, SessionId) {
    let parent = create(m, c);
    m.model.text("Blue.");
    m.model.text("Colors");
    m.block_on(m.host.send(c, &parent, text("A color?")))
        .unwrap();
    let turn = m.log(&parent).iter().find_map(|e| e.turn.clone()).unwrap();
    let at = ForkPoint::AfterTurn(turn);
    let child = m
        .block_on(m.host.fork_session(c, &parent, at, ForkReason::Branch))
        .unwrap();
    (parent, child)
}

#[test]
fn a_parent_and_its_fork_with_the_same_approval_id_stay_apart() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let shell = m.caller(&m.shell_token());
    let (parent, child) = parent_and_fork(&m, &a);
    // The fork continues the parent's counters, so both mint the same id.
    call(
        &m,
        &a,
        &parent,
        client_tool("post", vec![Effect::ActsOutward]),
    );
    call(
        &m,
        &a,
        &child,
        client_tool("post", vec![Effect::ActsOutward]),
    );
    let all = m.block_on(m.host.list_approvals(&a)).unwrap();
    assert_eq!(all.len(), 2);
    let id = all[0].request.approval.clone();
    assert_eq!(all[1].request.approval, id);

    let token = m.user_action(&shell, &child, &id);
    m.model.text("Not posted.");
    m.block_on(
        m.host
            .decide_approval(&shell, &child, &id, DecisionInput::Deny, Some(token)),
    )
    .unwrap();
    assert_eq!((decisions(&m, &parent), decisions(&m, &child)), (0, 1));
    let left = m.block_on(m.host.list_approvals(&a)).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].session, parent);
}

#[test]
fn a_parent_and_its_fork_with_the_same_call_id_stay_apart() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let (parent, child) = parent_and_fork(&m, &a);
    call(&m, &a, &parent, client_tool("insert_block", vec![]));
    call(&m, &a, &child, client_tool("insert_block", vec![]));
    let invoked = m.clients.invoked.lock().unwrap().clone();
    assert_eq!(invoked.len(), 2);
    let id = invoked[0].2.call.clone();
    assert_eq!(invoked[1].2.call, id);

    m.model.text("Done in the fork.");
    m.block_on(m.host.client_tool_result(
        &a,
        &child,
        &id,
        ClientOutcome::Ok(vec![Block::text("Inserted.")]),
    ))
    .unwrap();
    assert_eq!(
        (
            count(&m, &parent, "tool.result"),
            count(&m, &child, "tool.result")
        ),
        (0, 1)
    );
    let waiting = |s: &SessionId| {
        m.block_on(m.host.open_session(&a, s))
            .unwrap()
            .turn_open
            .is_some()
    };
    assert!(waiting(&parent));
    assert!(!waiting(&child));
}

#[test]
fn a_result_for_a_call_never_issued_is_dropped_and_audited() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = create(&m, &a);
    let before = m.log(&s).len();
    let bogus = CallId::from("cal_9999");
    let outcome = ClientOutcome::Ok(vec![Block::text("Nothing asked for this.")]);
    assert_eq!(
        m.block_on(m.host.client_tool_result(&a, &s, &bogus, outcome)),
        Ok(())
    );
    assert_eq!(m.log(&s).len(), before);
    let shell = m.caller(&m.shell_token());
    let late: Vec<_> = m
        .block_on(m.host.activity(&shell, None))
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == "tool.result.late")
        .collect();
    assert_eq!(late.len(), 1);
    assert_eq!((&late[0].session, &late[0].call), (&s, &Some(bogus)));
}

#[test]
fn an_app_cannot_answer_a_display_call() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let opts = CreateOptions::default();
    let s = m
        .block_on(m.host.create_session(&a, &PresetId::from("chat"), opts))
        .unwrap();
    let input =
        r#"{"metrics": [{"label": "Total", "value": "2,340"}], "fallback_text": "Total 2,340."}"#;
    m.model.calls(&[("show_metrics", input)]);
    m.model.text("Shown.");
    m.block_on(m.host.send(&a, &s, text("Show the total.")))
        .unwrap();
    let id = m
        .log(&s)
        .iter()
        .find_map(|e| match &e.event {
            Event::ToolCall(c) if c.kind == ToolKind::Display => Some(c.call.clone()),
            _ => None,
        })
        .expect("a display call");
    let late = ClientOutcome::Ok(vec![Block::text("Shown.")]);
    let err = m.block_on(m.host.client_tool_result(&a, &s, &id, late));
    assert_eq!(err, Err(HostError::NotFound));
}

#[test]
fn a_model_that_does_not_call_tools_gets_no_display_tools() {
    let display = |m: &MemoryHost, s: &SessionId| {
        m.log(s).iter().any(|e| match &e.event {
            Event::StepStart(st) => st
                .tools
                .iter()
                .flatten()
                .any(|t| t.kind == ToolKind::Display),
            _ => false,
        })
    };
    for tools in [true, false] {
        let m = MemoryHost::new();
        if !tools {
            m.model.without_tools("local:default");
        }
        let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
        let opts = CreateOptions::default();
        let s = m
            .block_on(m.host.create_session(&a, &PresetId::from("chat"), opts))
            .unwrap();
        m.block_on(m.host.send(&a, &s, text("Hi."))).unwrap();
        assert_eq!(display(&m, &s), tools);
    }
}
