//! Policy: deny by default, the three-flag rule, deciders, budgets, the approvals book.

use harness_core::*;
use harness_policy::*;

fn spec(name: &str, effects: &[Effect]) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: String::new(),
        input_schema: Value::Object(Default::default()),
        kind: ToolKind::Engine,
        effects: effects.to_vec(),
        concurrency: Concurrency::Parallel,
    }
}

#[test]
fn deny_by_default_for_outward_and_code() {
    let p = Policy::default();
    let f = Flags::default();
    assert_eq!(p.evaluate(&spec("now", &[]), &f), Verdict::Allow);
    assert_eq!(
        p.evaluate(&spec("search", &[Effect::IngestsUntrustedContent]), &f),
        Verdict::Allow
    );
    assert_eq!(
        p.evaluate(&spec("send", &[Effect::ActsOutward]), &f),
        Verdict::Ask(ApprovalRule::ActsOutward)
    );
    assert_eq!(
        p.evaluate(&spec("run", &[Effect::ExecutesCode]), &f),
        Verdict::Ask(ApprovalRule::ExecutesCode)
    );
    let mut cell = spec("cell", &[]);
    cell.kind = ToolKind::Cell;
    assert!(matches!(p.evaluate(&cell, &f), Verdict::Deny(_)));
}

#[test]
fn three_flag_rule_overrides_preapproval_and_is_sticky() {
    let p = Policy {
        autonomy: Autonomy::Trusted,
        allow_outward: vec!["send".into()],
    };
    let send = spec("send", &[Effect::ActsOutward]);
    let mut f = Flags::default();
    assert_eq!(p.evaluate(&send, &f), Verdict::Allow);
    f.note(&[Effect::ReadsPrivateData]);
    assert_eq!(p.evaluate(&send, &f), Verdict::Allow);
    f.note(&[Effect::IngestsUntrustedContent]);
    assert!(f.three_flag());
    assert_eq!(p.evaluate(&send, &f), Verdict::Ask(ApprovalRule::ThreeFlag));
    // Nothing clears a flag.
    f.note(&[]);
    f.note(&[Effect::ActsOutward]);
    assert!(f.private && f.untrusted);
    assert_eq!(p.evaluate(&send, &f), Verdict::Ask(ApprovalRule::ThreeFlag));
    // Code still asks under trusted autonomy.
    assert_eq!(
        p.evaluate(&spec("run", &[Effect::ExecutesCode]), &Flags::default()),
        Verdict::Ask(ApprovalRule::ExecutesCode)
    );
}

#[test]
fn strict_autonomy_asks_for_any_effect() {
    let p = Policy {
        autonomy: Autonomy::Strict,
        allow_outward: vec![],
    };
    let f = Flags::default();
    assert_eq!(p.evaluate(&spec("now", &[]), &f), Verdict::Allow);
    assert_eq!(
        p.evaluate(&spec("read", &[Effect::ReadsPrivateData]), &f),
        Verdict::Ask(ApprovalRule::Autonomy)
    );
}

#[test]
fn who_may_decide() {
    let mail = AppId::from("mail");
    let tok = UserActionToken("t".into());
    assert_eq!(may_decide(&Caller::Shell, &mail, Some(&tok), true), Ok(()));
    assert_eq!(
        may_decide(&Caller::Shell, &mail, None, true),
        Err(DecideError::MissingUserAction)
    );
    assert_eq!(
        may_decide(&Caller::Shell, &mail, Some(&tok), false),
        Err(DecideError::MissingUserAction)
    );
    assert_eq!(
        may_decide(&Caller::first_party("mail"), &mail, Some(&tok), true),
        Ok(())
    );
    assert_eq!(
        may_decide(&Caller::first_party("notes"), &mail, Some(&tok), true),
        Err(DecideError::NotFound)
    );
    for kind in [AppKind::View, AppKind::ThirdParty] {
        let c = Caller::App {
            app: mail.clone(),
            kind,
        };
        assert_eq!(
            may_decide(&c, &mail, Some(&tok), true),
            Err(DecideError::NotAllowedToDecide)
        );
    }
    assert!(Caller::Shell.sees(&mail));
    assert!(!Caller::first_party("notes").sees(&mail));
}

#[test]
fn ambiguous_answers_are_denials() {
    use DecisionInput::*;
    assert_eq!(interpret(&Allow), (Decision::Allow, Basis::User, None));
    assert_eq!(interpret(&Deny), (Decision::Deny, Basis::User, None));
    assert_eq!(
        interpret(&Text(" Yes. ".into())),
        (Decision::Allow, Basis::User, None)
    );
    assert_eq!(
        interpret(&Text("No!".into())),
        (Decision::Deny, Basis::User, None)
    );
    assert_eq!(
        interpret(&Text("maybe later".into())),
        (Decision::Deny, Basis::Ambiguous, Some("maybe later".into()))
    );
    assert_eq!(interpret(&Text("yes but no".into())).1, Basis::Ambiguous);
}

#[test]
fn limits_tighten_and_meter_stops_between_steps() {
    let preset = Limits {
        tokens: Some(1000),
        steps: Some(8),
        wall_ms: None,
        cost_micros: None,
    };
    let app = Limits {
        tokens: Some(2000),
        steps: Some(3),
        wall_ms: Some(60_000),
        cost_micros: None,
    };
    let l = preset.tighten(&app);
    assert_eq!(l.tokens, Some(1000));
    assert_eq!(l.steps, Some(3));
    assert_eq!(l.wall_ms, Some(60_000));
    assert_eq!(l.cost_micros, None);

    let mut m = Meter::start(1_000);
    for _ in 0..3 {
        assert_eq!(m.check(&l, 1_000), None);
        m.step();
    }
    let hit = m.check(&l, 1_000).unwrap();
    assert_eq!((hit.kind, hit.limit, hit.used), (BudgetKind::Steps, 3, 3));

    let mut m = Meter::start(0);
    m.add(&Usage {
        input_tokens: 900,
        output_tokens: 100,
        ..Usage::default()
    });
    assert_eq!(m.check(&l, 0).unwrap().kind, BudgetKind::Tokens);
    let m = Meter::start(0);
    assert_eq!(m.check(&l, 60_000).unwrap().kind, BudgetKind::WallTime);
}

#[test]
fn approvals_book() {
    let mut book = Approvals::new();
    let pending = |s: &str, owner: &str, id: &str, exp: Option<u64>| PendingApproval {
        session: SessionId::from(s),
        owner: AppId::from(owner),
        request: ApprovalRequest {
            approval: ApprovalId::from(id),
            call: None,
            summary: "send".into(),
            rule: ApprovalRule::ActsOutward,
            effects: vec![Effect::ActsOutward],
            expires_at: exp,
        },
        requested_at: 0,
    };
    book.add(pending("s1", "mail", "a1", Some(10)));
    book.add(pending("s2", "notes", "a2", None));
    book.add(pending("s3", "mail", "a3", None));
    assert_eq!(book.list(None).len(), 3);
    assert_eq!(book.list(Some(&AppId::from("mail"))).len(), 2);
    let gone = book.expired(10);
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].request.approval, ApprovalId::from("a1"));
    assert_eq!(book.drop_sessions(&[SessionId::from("s3")]).len(), 1);
    let (s2, a2) = (SessionId::from("s2"), ApprovalId::from("a2"));
    assert!(book.get(&SessionId::from("s9"), &a2).is_none());
    assert!(book.take(&s2, &a2).is_some());
    assert!(book.list(None).is_empty());

    // Ids are minted per session: the same id in two sessions is two
    // approvals.
    book.add(pending("s1", "mail", "apv_0001", None));
    book.add(pending("s2", "notes", "apv_0001", None));
    assert_eq!(book.list(None).len(), 2);
    let id = ApprovalId::from("apv_0001");
    let taken = book.take(&SessionId::from("s1"), &id).unwrap();
    assert_eq!(taken.owner, AppId::from("mail"));
    assert_eq!(book.get(&s2, &id).unwrap().owner, AppId::from("notes"));
}
