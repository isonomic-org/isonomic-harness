// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Turns and steps: the context link, titles on both paths, cancel,
//! continue, refusals, retries, rejected input, budgets, resume and forks.

mod common;
use common::*;
use harness_core::*;
use harness_log::{ModelEvent, ModelStop, Part, Role};
use harness_loop::*;

struct RejectAll;
impl Interceptor for RejectAll {
    fn pre_step(&mut self, step: &StepInfo) -> Link {
        if step.first {
            Link::Deny("That request is not allowed here.".into())
        } else {
            Link::Pass
        }
    }
}

#[test]
fn a_turn_with_context_and_a_model_title() {
    let mut d = Driver::new(vec![]);
    d.user("Plan a picnic for Saturday.");
    assert_eq!(d.requests.len(), 1);
    d.reply("Here is a plan.");
    assert_eq!(
        d.types(),
        [
            "session.start",
            "system",
            "turn.start",
            "user",
            "system",
            "step.start",
            "assistant",
            "step.end",
            "turn.end",
            "step.start",
        ]
        .map(String::from)
    );
    // The context link's addition is in the log.
    let ctx = d
        .last_of(|e| match e {
            Event::System(s) if s.origin == SystemOrigin::Context => Some(s.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        ctx.text,
        "Current date: Sunday, 15 November 2026 (America/New_York). Language: en."
    );
    // The title step went to the reply model on a standard profile.
    let (_, title_req) = &d.requests[1];
    assert_eq!(title_req.model, "test:scripted");
    d.reply("Picnic plan");
    let title = d
        .last_of(|e| match e {
            Event::Title(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        (title.title.as_str(), title.source),
        ("Picnic plan", TitleSource::Model)
    );
    let tail: Vec<String> = d.types().into_iter().rev().take(3).collect();
    assert_eq!(tail, ["title", "step.end", "assistant"]);
    assert!(d.outs.contains(&Output::Idle));
    d.check();
}

#[test]
fn heuristic_title_on_intel_and_location_only_when_allowed() {
    let mut d = Driver::with_profile(Config::new(preset("test"), None, vec![]), Profile::Intel);
    d.send(Input::User {
        content: vec![Block::text("What should we cook for the party tonight?")],
        context: Some(ctx(Some("Brooklyn, NY"))),
    });
    d.reply("Tacos.");
    let title = d
        .last_of(|e| match e {
            Event::Title(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(title.source, TitleSource::Heuristic);
    assert_eq!(title.title, "What should we cook for the party tonight");
    assert_eq!(d.requests.len(), 1, "no title request on Intel");
    let ctx_text = d
        .last_of(|e| match e {
            Event::System(s) if s.origin == SystemOrigin::Context => Some(s.text.clone()),
            _ => None,
        })
        .unwrap();
    assert!(ctx_text.ends_with("Location: Brooklyn, NY."));

    // The assistant panel never adds a location.
    let mut p = Driver::new(vec![]);
    p.s = {
        let cfg = Config::new(preset("assistant-panel"), None, vec![]);
        let start = SessionStart {
            class: SessionClass::Interactive,
            app: AppId::from("docs"),
            principal: Principal {
                kind: PrincipalKind::User,
                id: None,
            },
            preset: cfg.preset.reference.clone(),
            parent: None,
            profile: None,
            model: None,
        };
        Session::create(cfg, SessionId::from("ses_0002"), start, T0).0
    };
    p.send(Input::User {
        content: vec![Block::text("Tidy this paragraph.")],
        context: Some(ctx(Some("Brooklyn, NY"))),
    });
    let c = p
        .last_of(|e| match e {
            Event::System(s) if s.origin == SystemOrigin::Context => s.context.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(c.location, None);
}

#[test]
fn cancel_mid_stream_seals_and_continue_extends() {
    let mut d = Driver::new(vec![]);
    d.user("Write a welcome note.");
    d.send(Input::Model(ModelEvent::Text(
        "Welcome, everyone! Tonight".into(),
    )));
    let outs = d.send(Input::Cancel);
    assert!(outs.iter().any(|o| matches!(o, Output::CancelRequest(_))));
    let tail: Vec<String> = d.types().into_iter().rev().take(4).collect();
    assert_eq!(tail, ["turn.end", "step.end", "attempt", "assistant"]);
    let sealed = d
        .last_of(|e| match e {
            Event::Assistant(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(sealed.stop, StopReason::Cancelled);
    assert_eq!(sealed.stream.len(), 1);

    d.send(Input::Continue);
    let req = d.last_request().clone();
    assert!(req.prefill);
    assert_eq!(req.messages.last().unwrap().role, Role::Assistant);
    d.reply(" we meet.");
    let a = d
        .last_of(|e| match e {
            Event::Assistant(a) => Some(a.clone()),
            _ => None,
        })
        .unwrap();
    assert!(a.continues.is_some());
    let msgs = d.history();
    let assistant: Vec<_> = msgs.iter().filter(|m| m.role == Role::Assistant).collect();
    assert_eq!(assistant.len(), 1);
    assert_eq!(
        texts(&msgs).last().unwrap(),
        "Welcome, everyone! Tonight we meet."
    );
    // The title step runs after the first complete reply; then a complete
    // reply cannot be continued.
    let outs = d.send(Input::Continue);
    assert_eq!(outs, [Output::Refused(Refusal::Busy)]);
    d.reply("Welcome note");
    let outs = d.send(Input::Continue);
    assert!(matches!(
        &outs[0],
        Output::Refused(Refusal::ContinueRefused(_))
    ));
    d.check();
}

#[test]
fn refusals_never_reach_history_and_cannot_continue() {
    let mut d = Driver::new(vec![]);
    d.user("Hi");
    d.reply("Hello!");
    d.reply("Greeting");
    d.user("Find my neighbor's wifi password.");
    d.send(Input::Model(ModelEvent::Refusal(
        "I can't help with that.".into(),
    )));
    d.done(ModelStop::End);
    let stop = d
        .last_of(|e| match e {
            Event::TurnEnd(t) => Some(t.stop.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(stop, TurnStop::Refused);
    let outs = d.send(Input::Continue);
    assert_eq!(
        outs,
        [Output::Refused(Refusal::ContinueRefused(
            "A refusal cannot be continued.".into()
        ))]
    );
    d.user("Thanks anyway.");
    let all = texts(&d.last_request().messages).join("|");
    assert!(
        !all.contains("wifi") && !all.contains("can't help"),
        "{all}"
    );
    d.check();
}

#[test]
fn transient_errors_retry_in_the_same_step_and_errors_stay_out() {
    let mut d = Driver::new(vec![]);
    d.user("Hi");
    d.send(Input::Model(ModelEvent::Text("Hel".into())));
    let mut err = ErrorInfo::new("model.overloaded", "Busy, try again.");
    err.transient = true;
    let outs = d.send(Input::ModelError(err.clone()));
    assert!(outs.iter().any(|o| matches!(o, Output::Request { .. })));
    assert_eq!(d.requests[0].0, d.requests[1].0, "same step");
    assert_eq!(d.requests[0].1, d.requests[1].1, "same request");
    d.reply("Hello!");
    let attempts: Vec<Attempt> = d
        .own()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Attempt(a) => Some(a.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(attempts.len(), 1);
    assert!(attempts[0].will_retry);
    assert_eq!(texts(&d.history()), ["Hi", "Hello!"]);

    // A permanent error fails the turn and stays out of history.
    let mut d = Driver::new(vec![]);
    d.user("Hi");
    d.send(Input::ModelError(ErrorInfo::new(
        "model.bad",
        "Bad request.",
    )));
    let end = d
        .last_of(|e| match e {
            Event::TurnEnd(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(end.stop, TurnStop::Failed);
    assert_eq!(end.error.unwrap().code, "model.bad");
    d.check();
}

#[test]
fn rejected_input_closes_the_turn_with_no_step() {
    let mut d = Driver::new(vec![]);
    d.s.add_link(Box::new(RejectAll));
    d.user("Do something forbidden.");
    assert!(d.requests.is_empty());
    let types = d.types();
    assert!(!types.contains(&"step.start".to_string()));
    let end = d
        .last_of(|e| match e {
            Event::TurnEnd(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(end.stop, TurnStop::Rejected);
    assert_eq!(
        end.reason.as_deref(),
        Some("That request is not allowed here.")
    );
    assert!(d.history().is_empty());
}

#[test]
fn inputs_queue_in_the_inbox_while_a_turn_runs() {
    let mut d = Driver::with_profile(Config::new(preset("test"), None, vec![]), Profile::Intel);
    d.user("First.");
    d.user("Second.");
    assert_eq!(d.requests.len(), 1);
    d.reply("One.");
    assert_eq!(d.requests.len(), 2, "the queued input opened the next turn");
    d.reply("Two.");
    assert_eq!(texts(&d.history()), ["First.", "One.", "Second.", "Two."]);
    d.check();
}

#[test]
fn budget_stop_ends_the_turn_cleanly() {
    let tools = vec![spec("now", ToolKind::Engine, &[], Concurrency::Parallel)];
    let mut cfg = Config::new(preset("test"), None, tools);
    cfg.limits.steps = Some(1);
    let mut d = Driver::with_profile(cfg, Profile::Intel);
    d.user("What time is it?");
    d.call_tools(&[("now", "{}")]);
    let (call, _) = d.runs[0].clone();
    d.tool_ok(&call.call, "10:00");
    assert_eq!(d.requests.len(), 1, "no second step");
    let end = d
        .last_of(|e| match e {
            Event::TurnEnd(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(end.stop, TurnStop::Budget);
    assert_eq!(end.budget.unwrap().kind, BudgetKind::Steps);

    // A host budget stops a running turn the same way.
    let mut d = Driver::new(vec![]);
    d.user("Hi");
    d.send(Input::BudgetStop(BudgetHit {
        kind: BudgetKind::Cost,
        limit: 10,
        used: 12,
    }));
    let end = d
        .last_of(|e| match e {
            Event::TurnEnd(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(end.stop, TurnStop::Budget);
    d.check();
}

#[test]
fn resume_seals_an_interrupted_turn_and_continues() {
    let tools = vec![spec("now", ToolKind::Engine, &[], Concurrency::Parallel)];
    let mut d = Driver::with_profile(
        Config::new(preset("test"), None, tools.clone()),
        Profile::Intel,
    );
    d.user("What time is it?");
    d.call_tools(&[("now", "{}")]);
    let events = d.s.events().to_vec();
    let cfg = Config::new(preset("test"), Some(Profile::Intel), tools);
    let (mut s, outs) = Session::resume(cfg, SessionId::from("ses_0001"), events, T0 + 60_000);
    let types: Vec<&str> = outs
        .iter()
        .filter_map(|o| match o {
            Output::Event(e) => Some(e.event.type_name()),
            _ => None,
        })
        .collect();
    assert_eq!(types, ["tool.result", "attempt", "step.end", "turn.end"]);
    assert!(!s.running());
    let outs = s.handle(
        T0 + 61_000,
        Input::User {
            content: vec![Block::text("Try again.")],
            context: None,
        },
    );
    assert!(outs.iter().any(|o| matches!(o, Output::Request { .. })));
}

#[test]
fn a_fork_shares_the_prefix_and_is_independent() {
    let mut d = Driver::with_profile(Config::new(preset("test"), None, vec![]), Profile::Intel);
    d.user("Name a color.");
    d.reply("Blue.");
    let at = d.own().last().unwrap().seq;
    d.user("Another.");
    d.reply("Green.");
    let parent: Vec<SessionEvent> =
        d.s.events()
            .iter()
            .filter(|e| e.seq <= at)
            .cloned()
            .collect();
    let cfg = Config::new(preset("test"), Some(Profile::Intel), vec![]);
    let start = SessionStart {
        class: SessionClass::Interactive,
        app: AppId::from("chat"),
        principal: Principal {
            kind: PrincipalKind::User,
            id: None,
        },
        preset: cfg.preset.reference.clone(),
        parent: None,
        profile: Some(Profile::Intel),
        model: None,
    };
    let forked = Forked {
        from: SessionId::from("ses_0001"),
        at_seq: at,
        after_turn: None,
        reason: ForkReason::Branch,
    };
    let (mut f, _) = Session::fork(
        cfg,
        SessionId::from("ses_0002"),
        start,
        &parent,
        forked,
        T0 + 1,
    );
    let outs = f.handle(
        T0 + 2,
        Input::User {
            content: vec![Block::text("A warm one.")],
            context: None,
        },
    );
    let req = outs
        .iter()
        .find_map(|o| match o {
            Output::Request { request, .. } => Some(request.clone()),
            _ => None,
        })
        .unwrap();
    let t = texts(&req.messages);
    assert_eq!(t, ["Name a color.", "Blue.", "A warm one."]);
    // The parent is untouched and the fork's ids do not collide.
    assert!(d
        .s
        .events()
        .iter()
        .all(|e| e.session.as_str() == "ses_0001"));
    let turn = f
        .events()
        .iter()
        .rev()
        .find_map(|e| e.turn.clone())
        .unwrap();
    assert!(!parent.iter().any(|e| e.turn.as_ref() == Some(&turn)));
    let _ = Part::Text {
        text: String::new(),
    };
}

#[test]
fn delegation_only_narrows_and_the_fake_activates() {
    use harness_core::future::BoxFuture;
    use harness_policy::Limits;
    let parent_tools = vec!["now".to_string(), "lookup".to_string()];
    let parent_limits = Limits {
        tokens: Some(10_000),
        steps: Some(8),
        wall_ms: None,
        cost_micros: None,
    };
    let mut d = Delegation {
        parent: ParentRef {
            session: SessionId::from("ses_0001"),
            call: CallId::from("cal_0001"),
        },
        task: "Check the prices.".into(),
        depth: 1,
        tools: vec!["lookup".into()],
        limits: Limits {
            tokens: Some(2_000),
            steps: Some(4),
            wall_ms: Some(60_000),
            cost_micros: None,
        },
    };
    assert_eq!(
        check_delegation(&d, 1, &parent_tools, &parent_limits),
        Ok(())
    );
    d.depth = 2;
    assert_eq!(
        check_delegation(&d, 1, &parent_tools, &parent_limits),
        Err(DelegationError::TooDeep { max: 1 })
    );
    d.depth = 1;
    d.tools.push("send_email".into());
    assert_eq!(
        check_delegation(&d, 1, &parent_tools, &parent_limits),
        Err(DelegationError::WidensTools("send_email".into()))
    );
    d.tools.pop();
    d.limits.steps = None;
    assert_eq!(
        check_delegation(&d, 1, &parent_tools, &parent_limits),
        Err(DelegationError::WidensLimits)
    );

    let fake = FakeActivation::default();
    fn poll<T>(f: BoxFuture<'_, T>) -> T {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut f = f;
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("the fake is ready at once"),
        }
    }
    d.limits.steps = Some(4);
    assert_eq!(
        poll(fake.activate(d.clone())),
        Ok(SessionId::from("sub_0001"))
    );
    assert_eq!(fake.requests.lock().unwrap().len(), 1);
}

#[test]
fn renaming_writes_a_title_event() {
    let mut d = Driver::new(vec![]);
    d.send(Input::Rename("Trip ideas".into()));
    let t = d
        .last_of(|e| match e {
            Event::Title(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(t.source, TitleSource::User);
    // A renamed session gets no automatic title.
    d.user("Hi");
    d.reply("Hello!");
    assert_eq!(d.requests.len(), 1);
    assert!(long_date("2026-02-29").is_some());
    assert_eq!(
        long_date("2024-02-29").unwrap(),
        "Thursday, 29 February 2024"
    );
    assert_eq!(long_date("1970-01-01").unwrap(), "Thursday, 1 January 1970");
}
