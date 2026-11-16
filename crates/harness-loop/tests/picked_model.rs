//! A picked model: its name as recorded, and compaction that fits its
//! context instead of the preset's budget.

mod common;
use common::*;
use harness_core::*;
use harness_loop::*;

const PICKED: &str = "local:picked";

fn start(model: Option<&str>) -> SessionStart {
    SessionStart {
        class: SessionClass::Interactive,
        app: AppId::from("chat"),
        principal: Principal {
            kind: PrincipalKind::User,
            id: None,
        },
        preset: preset("test").reference,
        parent: None,
        profile: Some(Profile::Standard),
        model: model.map(str::to_owned),
    }
}

fn recorded(s: &Session) -> Option<String> {
    s.events().iter().find_map(|e| match &e.event {
        Event::SessionStart(st) => st.model.clone(),
        _ => None,
    })
}

#[test]
fn a_blank_or_padded_name_is_never_recorded_as_given() {
    let cfg = || Config::new(preset("test"), Some(Profile::Standard), vec![]);
    for blank in ["", "  ", "\n"] {
        let (s, _) = Session::create(cfg(), SessionId::from("ses_1"), start(Some(blank)), T0);
        assert_eq!(recorded(&s), None, "{blank:?}");
        let json = serde_json::to_string(&s.events()[0]).unwrap();
        assert!(!json.contains("\"model\""), "{json}");
    }
    let padded = "  local:picked \n";
    let (s, _) = Session::create(cfg(), SessionId::from("ses_1"), start(Some(padded)), T0);
    assert_eq!(recorded(&s).as_deref(), Some(PICKED));
}

#[test]
fn the_compaction_budget_fits_the_models_context() {
    let cfg = Config::new(preset("test"), None, vec![]);
    let preset_budget = cfg.preset.compaction.budget_tokens;
    let out = u64::from(cfg.preset.model.max_output_tokens);
    // No context known: the preset's budget, as before.
    assert_eq!(cfg.compaction_budget(), preset_budget);
    assert_eq!(
        cfg.clone().with_context(0).compaction_budget(),
        preset_budget
    );
    // A roomy model: still the preset's budget.
    let roomy = cfg.clone().with_context(preset_budget + out + 1);
    assert_eq!(roomy.compaction_budget(), preset_budget);
    // A small one: its context less room for the reply.
    let small = cfg.clone().with_context(preset_budget + out - 100);
    assert_eq!(small.compaction_budget(), preset_budget - 100);
    // A tiny one: never less than half its context.
    assert_eq!(
        cfg.with_context(600).compaction_budget(),
        600 - out.min(300)
    );
}

/// Three long turns, then a fourth; the summary steps the session ran.
fn summary_models(context: Option<u64>) -> Vec<String> {
    let mut cfg = Config::new(preset("test"), None, vec![]).with_model(Some(PICKED));
    if let Some(c) = context {
        cfg = cfg.with_context(c);
    }
    let mut d = Driver::with(cfg);
    let long = "A sentence about gardens and weather. ".repeat(12);
    for i in 0..4 {
        d.user(&format!("Question {i}. {long}"));
        // A summary step, when compaction needs one, comes before the reply.
        while d.s.events().iter().rev().find_map(|e| match &e.event {
            Event::StepStart(s) => Some(s.purpose.clone()),
            _ => None,
        }) == Some(Purpose::Summary)
            && !matches!(d.s.events().last().unwrap().event, Event::StepEnd(_))
        {
            d.reply("The person asked about gardens; the assistant answered.");
        }
        d.reply(&format!("Answer {i}. {long}"));
        if i == 0 {
            d.reply("Gardens");
        }
    }
    d.check();
    d.own()
        .iter()
        .filter_map(|e| match &e.event {
            Event::StepStart(s) if s.purpose == Purpose::Summary => Some(s.model.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_small_picked_model_is_summarized_for_and_by_itself() {
    // Under the preset's budget of 2000 tokens, these turns need nothing.
    assert!(summary_models(None).is_empty());
    // On a model with a 700-token context they do, and the summary step
    // runs on the picked model.
    let models = summary_models(Some(700));
    assert!(!models.is_empty());
    assert!(models.iter().all(|m| m == PICKED), "{models:?}");
}
