//! Presets: the reference presets, diagnostics, profile paths and ceilings.

use harness_core::*;
use harness_policy::{Autonomy, Limits};
use harness_preset::*;

fn errors(text: &str) -> Vec<(String, String)> {
    parse(text)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.path, d.message))
        .collect()
}

#[test]
fn reference_presets_validate_cleanly() {
    let all = builtins();
    assert_eq!(all.len(), BUILTIN_IDS.len());
    for (id, (_, parsed)) in BUILTIN_IDS.iter().zip(&all) {
        assert!(
            parsed.diagnostics.is_empty(),
            "{id}: {:?}",
            parsed.diagnostics
        );
        let p = parsed.preset.as_ref().unwrap();
        assert_eq!(p.reference.id, PresetId::from(*id));
        assert_eq!(p.reference.version, 1);
        assert_eq!(p.class, SessionClass::Interactive);
        assert!(!p.system_prompt().is_empty());
    }
    let (_, chat) = builtin("chat").unwrap();
    let chat = chat.preset.unwrap();
    assert_eq!(chat.prompt.len(), 2);
    assert_eq!(chat.prompt[0].id, "persona");
    assert!(chat
        .system_prompt()
        .contains("\n\nUse a tool only when it helps."));
    assert!(!chat.system_prompt().contains('\\'));
    assert_eq!(chat.tools.allow, ["ask_user", "todo", "plan", "now"]);
    assert_eq!(chat.limits.steps, Some(24));
    assert_eq!(chat.policy.policy.autonomy, Autonomy::Supervised);
    assert_eq!(chat.model.params().reasoning, Reasoning::Low);
    assert!(builtin("nope").is_none());
}

#[test]
fn title_path_by_profile() {
    let chat = builtin("chat").unwrap().1.preset.unwrap();
    assert_eq!(chat.title_path(Some(&Profile::Intel)), TitlePath::Heuristic);
    assert_eq!(
        chat.title_path(Some(&Profile::Low)),
        TitlePath::Model("local:smallest".into())
    );
    assert_eq!(
        chat.title_path(Some(&Profile::Standard)),
        TitlePath::Model("local:default".into())
    );
    assert_eq!(
        chat.title_path(None),
        TitlePath::Model("local:default".into())
    );
    let panel = builtin("assistant-panel").unwrap().1.preset.unwrap();
    assert_eq!(panel.title_path(Some(&Profile::High)), TitlePath::Off);
    let mut low = chat.clone();
    low.model.title_model = None;
    assert_eq!(low.title_path(Some(&Profile::Low)), TitlePath::Heuristic);
}

#[test]
fn summaries_by_profile() {
    let chat = builtin("chat").unwrap().1.preset.unwrap();
    assert!(!chat.compaction.summaries_allowed(Some(&Profile::Intel)));
    assert!(chat.compaction.summaries_allowed(Some(&Profile::Low)));
    assert!(chat.compaction.summaries_allowed(None));
    let panel = builtin("assistant-panel").unwrap().1.preset.unwrap();
    assert!(!panel.compaction.summaries_allowed(Some(&Profile::High)));
}

#[test]
fn a_minimal_preset_gets_defaults() {
    let p = parse("id = \"mine\"\nversion = 3\n[persona]\nprompt = \"Hi.\"\n");
    assert!(!p.has_errors(), "{:?}", p.diagnostics);
    let p = p.preset.unwrap();
    assert_eq!(p.name, "mine");
    assert_eq!(p.reference.version, 3);
    assert_eq!(p.title, TitleMode::Auto);
    assert_eq!(p.context.location, LocationMode::OptIn);
    assert!(p.tools.client);
    assert_eq!(p.limits, Limits::default());
}

#[test]
fn invalid_presets_keep_their_diagnostics() {
    // Syntax errors point at a line and column.
    let p = parse("id = \"x\"\nversion = \n");
    assert!(p.preset.is_none());
    assert_eq!(p.diagnostics.len(), 1);
    assert_eq!(p.diagnostics[0].path, "2:11");

    // Unknown keys are errors.
    let e = errors("id = \"x\"\nversion = 1\ncolour = \"red\"\n");
    assert_eq!(e.len(), 1);
    assert!(e[0].1.contains("colour"), "{e:?}");

    let e = errors(
        r#"
id = "Bad Id"
version = 0
class = "sometimes"
[[prompt]]
id = "a"
text = "x"
[[prompt]]
id = "a"
text = " "
[model]
reasoning = "maximum"
[tools]
allow = ["send"]
[limits]
steps = 0
[policy]
autonomy = "wild"
allow_outward = ["post"]
[context]
location = "always"
[post_turn]
title = "sometimes"
[compaction]
trim_over_bytes = 100
head_bytes = 80
tail_bytes = 40
[[wake]]
on = "schedule"
spec = "hourly"
"#,
    );
    let paths: Vec<&str> = e.iter().map(|(p, _)| p.as_str()).collect();
    for want in [
        "id",
        "version",
        "class",
        "prompt[1].id",
        "prompt[1].text",
        "model.reasoning",
        "limits.steps",
        "policy.autonomy",
        "policy.allow_outward[0]",
        "context.location",
        "post_turn.title",
        "compaction",
        "wake",
    ] {
        assert!(paths.contains(&want), "missing {want} in {paths:?}");
    }
    let (_, autonomy) = e.iter().find(|(p, _)| p == "policy.autonomy").unwrap();
    assert!(autonomy.contains("strict, supervised, trusted"));
}

#[test]
fn warnings_do_not_block() {
    let p = parse("id = \"auto\"\nversion = 1\nclass = \"autonomous\"\n");
    assert!(!p.has_errors());
    assert!(p.preset.is_some());
    let paths: Vec<&str> = p.diagnostics.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, ["prompt", "wake"]);
}

#[test]
fn app_presets_may_only_tighten() {
    let ceiling = Ceiling {
        limits: Limits {
            tokens: Some(100_000),
            steps: Some(16),
            wall_ms: None,
            cost_micros: None,
        },
        autonomy: Autonomy::Supervised,
    };
    let chat = builtin("chat").unwrap().1.preset.unwrap();
    let d = chat.check_ceiling(&ceiling);
    let paths: Vec<&str> = d.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(paths, ["limits.tokens", "limits.steps"]);

    let panel = builtin("assistant-panel").unwrap().1.preset.unwrap();
    assert!(panel.check_ceiling(&ceiling).is_empty());
    let mut trusted = panel.clone();
    trusted.policy.policy.autonomy = Autonomy::Trusted;
    assert_eq!(trusted.check_ceiling(&ceiling)[0].path, "policy.autonomy");
    // No limit at all is not a tightening.
    let mut open = panel.clone();
    open.limits.steps = None;
    assert_eq!(open.check_ceiling(&ceiling)[0].path, "limits.steps");
    assert_eq!(chat.effective_limits(&ceiling.limits).tokens, Some(100_000));
}
