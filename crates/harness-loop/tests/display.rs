// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display calls in the chat preset: offered by profile and model, never
//! an approval, validated in full, one retry per widget per turn, then the
//! fallback; the turn goes on in every case.

mod common;
use common::*;
use harness_core::*;
use harness_loop::*;

const TABLE: &str = r#"{"columns": [{"title": "City"}, {"title": "Rain", "numeric": true}], "rows": [["Lisbon", "774"], ["Porto", "1,254"]], "fallback_text": "Lisbon 774, Porto 1,254."}"#;
const BAD_TABLE: &str = r#"{"columns": [{"title": "City"}, {"title": "Rain", "numeric": true}], "rows": [["Lisbon", "774"], ["Porto"]], "fallback_text": "Lisbon 774, Porto 1,254."}"#;
const BAD_CHART: &str = r#"{"kind": "pie", "points": [{"label": "a", "value": 1}, {"label": "b", "value": 2}], "fallback_text": "a 1, b 2."}"#;

const STARTERS: [&str; 4] = ["show_table", "show_steps", "show_metrics", "show_chart"];

fn chat(profile: Profile) -> Driver {
    Driver::with_profile(Config::new(preset("chat"), None, vec![]), profile)
}

fn display_names(tools: &[ToolSpec]) -> Vec<String> {
    tools
        .iter()
        .filter(|t| t.kind == ToolKind::Display)
        .map(|t| t.name.clone())
        .collect()
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

fn text_of(r: &ToolResult) -> String {
    r.content
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_valid_call_is_shown_with_no_approval_and_the_turn_goes_on() {
    let mut d = chat(Profile::Standard);
    d.user("Compare the rain.");
    assert_eq!(display_names(&d.last_request().tools), STARTERS);
    d.call_tools(&[("show_table", TABLE)]);
    let r = &results(&d)[0];
    assert_eq!(r.outcome, ToolOutcome::Ok);
    assert_eq!(text_of(r), SHOWN);
    assert_eq!(r.display, Some(DisplayOutcome::Shown));
    assert_eq!(r.duration_ms, None);
    assert!(
        d.runs.is_empty(),
        "the host runs nothing for a display call"
    );
    assert!(!d.types().iter().any(|t| t == "approval.request"));
    d.reply("Porto is wetter.");
    assert!(d.types().iter().any(|t| t == "turn.end"));
    d.check();
}

#[test]
fn a_bad_call_is_retried_once_then_falls_back() {
    let mut d = chat(Profile::Standard);
    d.user("Compare the rain.");
    d.call_tools(&[("show_table", BAD_TABLE)]);
    let first = &results(&d)[0];
    assert_eq!(first.outcome, ToolOutcome::Error);
    assert_eq!(first.display, Some(DisplayOutcome::Retry));
    assert_eq!(
        text_of(first),
        "show_table: rows[1] has 1 cells; there are 2 columns."
    );
    // The retry, valid: shown. A chart's budget is its own.
    d.call_tools(&[("show_table", TABLE), ("show_chart", BAD_CHART)]);
    let rs = results(&d);
    assert_eq!(rs[1].display, Some(DisplayOutcome::Shown));
    assert_eq!(rs[2].display, Some(DisplayOutcome::Retry));
    // The chart again, still bad, and unparseable input: both fall back.
    d.call_tools(&[("show_chart", BAD_CHART), ("show_table", "{not json")]);
    let rs = results(&d);
    assert_eq!(rs[3].outcome, ToolOutcome::Ok);
    assert_eq!(rs[3].display, Some(DisplayOutcome::Fallback));
    assert_eq!(text_of(&rs[3]), SHOWN_AS_TEXT);
    assert_eq!(
        rs[4].display,
        Some(DisplayOutcome::Fallback),
        "the table used its retry"
    );
    d.reply("Here it is in words.");
    d.reply("Rain compared");

    // The budget is per turn: a new turn retries again.
    d.user("Again.");
    d.call_tools(&[("show_chart", BAD_CHART)]);
    assert_eq!(results(&d)[5].display, Some(DisplayOutcome::Retry));
    d.reply("Done.");
    assert!(!d.types().iter().any(|t| t == "approval.request"));
    d.check();
}

#[test]
fn display_tools_follow_the_profile_the_model_and_the_switch() {
    for profile in [Profile::Standard, Profile::Intel, Profile::High] {
        let mut d = chat(profile);
        d.user("Hi.");
        assert_eq!(display_names(&d.last_request().tools), STARTERS);
    }
    let mut low = chat(Profile::Low);
    low.user("Hi.");
    assert!(display_names(&low.last_request().tools).is_empty());

    let small = Config::new(preset("chat"), Some(Profile::Standard), vec![])
        .with_model(Some(harness_core::display::SMALLEST_MODEL));
    assert!(display_names(&small.tools).is_empty());

    let mut off = preset("chat");
    off.display.enabled = false;
    let mut d = Driver::with_profile(Config::new(off, None, vec![]), Profile::Standard);
    d.user("Hi.");
    assert!(display_names(&d.last_request().tools).is_empty());
    let steps = d.own().into_iter().filter_map(|e| match &e.event {
        Event::StepStart(s) => s.tools.clone(),
        _ => None,
    });
    for tools in steps {
        assert!(display_names(&tools).is_empty());
    }
}

#[test]
fn client_tools_coming_and_going_keep_the_display_tools() {
    let mut d = chat(Profile::Standard);
    let note = spec("insert_note", ToolKind::Client, &[], Concurrency::Serial);
    d.send(Input::Tools(vec![note]));
    d.user("Hi.");
    let tools = &d.last_request().tools;
    assert_eq!(tools[0].name, "insert_note");
    assert_eq!(display_names(tools), STARTERS);
    d.send(Input::Tools(vec![]));
    d.reply("Hello.");
    d.reply("Greeting");
    d.user("Again.");
    assert_eq!(display_names(&d.last_request().tools), STARTERS);
    d.reply("Hello again.");
    d.check();
}
