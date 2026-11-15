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
