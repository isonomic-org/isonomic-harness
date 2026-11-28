// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! display: Chat on a standard machine shows each starter widget once; a
//! malformed table is retried and shown; a malformed chart fails twice and
//! falls back to its text; and a call to `show_map`, as a newer host would
//! record it, has no `fallback_text`, so readers can test an unknown widget.

use super::build::*;
use crate::recorder::{Ctx, Recorder};
use harness_core::display::{display_tool_spec, DISPLAY_SPECS};
use harness_core::*;

const PROMPT: &str = "You are Isonomic, an assistant that runs on the person's own device.";

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("fixture JSON")
}

fn shown(r: &mut Recorder, s: &mut Ctx, call: &str, outcome: DisplayOutcome) {
    let (tool_outcome, content) = match outcome {
        DisplayOutcome::Shown => (ToolOutcome::Ok, "shown".to_owned()),
        _ => (ToolOutcome::Ok, "shown as text".to_owned()),
    };
    let mut res = result(call, tool_outcome, text(&content));
    res.display = Some(outcome);
    r.display_result(s, res);
}

fn retry(r: &mut Recorder, s: &mut Ctx, call: &str, reason: &str) {
    let mut res = result(call, ToolOutcome::Error, text(reason));
    res.error = Some(ErrorInfo::new("display.invalid", reason));
    res.display = Some(DisplayOutcome::Retry);
    r.display_result(s, res);
}

const TABLE: &str = r#"{"title": "Laptops under 1,500", "columns": [{"title": "Model"}, {"title": "Price", "numeric": true}, {"title": "Battery (h)", "numeric": true}], "rows": [["Aria 14", "1,199", "14"], ["Kite 13", "999", "11"], ["Orbit 16", "1,449", "9"]], "sort": {"column": 1, "ascending": true}, "fallback_text": "| Model | Price | Battery (h) |\n|---|---|---|\n| Kite 13 | 999 | 11 |\n| Aria 14 | 1,199 | 14 |\n| Orbit 16 | 1,449 | 9 |"}"#;
const STEPS: &str = r#"{"title": "Move your notes to the new laptop", "steps": [{"title": "Export notes", "state": "done"}, {"title": "Copy the export", "state": "current", "detail": "Use a USB drive or the local network"}, {"title": "Import on the new laptop"}, {"title": "Check every notebook opened"}], "fallback_text": "1. Export notes (done)\n2. Copy the export (now): use a USB drive or the local network\n3. Import on the new laptop\n4. Check every notebook opened"}"#;
const METRICS: &str = r#"{"title": "October spending", "metrics": [{"label": "Total", "value": "2,340", "change_percent": -8.5, "lower_is_better": true}, {"label": "Groceries", "value": "610", "change_percent": 4.1, "lower_is_better": true}, {"label": "Saved", "value": "450", "change_percent": 12.0}], "fallback_text": "October spending: total 2,340 (down 8.5%), groceries 610 (up 4.1%), saved 450 (up 12%)."}"#;
const CHART: &str = r#"{"title": "Rainfall in Lisbon", "kind": "bar", "series": "Rain", "unit": "mm", "points": [{"label": "Oct", "value": 100}, {"label": "Nov", "value": 128}, {"label": "Dec", "value": 127}, {"label": "Jan", "value": 110}], "fallback_text": "Rainfall in Lisbon (mm): Oct 100, Nov 128, Dec 127, Jan 110."}"#;
const BAD_TABLE: &str = r#"{"columns": [{"title": "City"}, {"title": "Rain (mm)", "numeric": true}], "rows": [["Lisbon", "774"], ["Porto"]], "fallback_text": "Lisbon 774 mm, Porto 1,254 mm."}"#;
const GOOD_TABLE: &str = r#"{"columns": [{"title": "City"}, {"title": "Rain (mm)", "numeric": true}], "rows": [["Lisbon", "774"], ["Porto", "1,254"]], "fallback_text": "Lisbon 774 mm, Porto 1,254 mm."}"#;
const BAD_CHART: &str = r#"{"kind": "pie", "points": [{"label": "Lisbon", "value": 774}, {"label": "Porto", "value": 1254}], "fallback_text": "Yearly rain: Lisbon 774 mm, Porto 1,254 mm."}"#;
const MAP: &str = r#"{"places": [{"name": "Lisbon", "lat": 38.72, "lon": -9.14}, {"name": "Porto", "lat": 41.15, "lon": -8.61}]}"#;

const TABLE_REASON: &str = "show_table: rows[1] has 1 cells; there are 2 columns.";
const CHART_REASON: &str = "show_chart: kind must be one of bar, line, area";

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);
    r.preset_prompt(&mut s, PROMPT);
    // Offered once: a later step records tools only when they change.
    let offered: Vec<ToolSpec> = DISPLAY_SPECS.iter().map(|d| d.tool_spec()).collect();
    let spec = |name: &str| display_tool_spec(name).expect("a starter widget");

    // Turn 1: each starter widget, valid, in one parallel batch.
    r.turn(&mut s, Cause::User, None);
    r.user(
        &mut s,
        "Show me what you can draw: a table, steps, figures and a chart.",
    );
    r.context(&mut s, None);
    r.step(&mut s, Purpose::Reply, Some(offered.clone()));
    r.reply(
        &mut s,
        text("Here are four examples."),
        StopReason::ToolUse,
        None,
    );
    let valid = [
        ("cal_0001", "show_table", TABLE),
        ("cal_0002", "show_steps", STEPS),
        ("cal_0003", "show_metrics", METRICS),
        ("cal_0004", "show_chart", CHART),
    ];
    for (id, name, input) in valid {
        r.call(&mut s, call(id, &spec(name), json(input), Some(0)));
    }
    for (id, _, _) in valid {
        shown(&mut r, &mut s, id, DisplayOutcome::Shown);
    }
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("The table sorts by any column; the chart shows Lisbon's wettest months."),
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);

    // Turn 2: a table retried, a chart that falls back, an unknown widget.
    r.advance(20_000);
    r.turn(&mut s, Cause::User, None);
    r.user(&mut s, "Compare yearly rain in Lisbon and Porto.");
    r.context(&mut s, None);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Here is the comparison."),
        StopReason::ToolUse,
        None,
    );
    r.call(
        &mut s,
        call("cal_0005", &spec("show_table"), json(BAD_TABLE), None),
    );
    retry(&mut r, &mut s, "cal_0005", TABLE_REASON);
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(&mut s, Vec::new(), StopReason::ToolUse, None);
    r.call(
        &mut s,
        call("cal_0006", &spec("show_table"), json(GOOD_TABLE), None),
    );
    shown(&mut r, &mut s, "cal_0006", DisplayOutcome::Shown);
    r.call(
        &mut s,
        call("cal_0007", &spec("show_chart"), json(BAD_CHART), None),
    );
    retry(&mut r, &mut s, "cal_0007", CHART_REASON);
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(&mut s, Vec::new(), StopReason::ToolUse, None);
    r.call(
        &mut s,
        call("cal_0008", &spec("show_chart"), json(BAD_CHART), None),
    );
    shown(&mut r, &mut s, "cal_0008", DisplayOutcome::Fallback);
    // As a newer host records a widget this build does not know.
    let mut map = spec("show_table");
    map.name = "show_map".into();
    r.call(&mut s, call("cal_0009", &map, json(MAP), None));
    shown(&mut r, &mut s, "cal_0009", DisplayOutcome::Shown);
    r.end_step(&mut s, StepOutcome::ToolCalls);
    r.step(&mut s, Purpose::Reply, None);
    r.reply(
        &mut s,
        text("Porto gets about 60% more rain than Lisbon in a year."),
        StopReason::End,
        None,
    );
    r.end_step(&mut s, StepOutcome::Complete);
    r.end_turn(&mut s, TurnStop::Done, None);
    r.finish()
}
