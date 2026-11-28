// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn v(text: &str) -> Value {
    serde_json::from_str(text).expect("test JSON")
}

fn reason(name: &str, text: &str) -> String {
    validate(name, &v(text)).expect_err("refused").reason
}

const TABLE: &str = r#"{"title": "Laptops under 1,500", "columns": [{"title": "Model"}, {"title": "Price", "numeric": true}, {"title": "Battery (h)", "numeric": true}],
 "rows": [["Aria 14", "1,199", "14"], ["Kite 13", "999", "11"], ["Orbit 16", "1,449", "9"]],
 "sort": {"column": 1, "ascending": true},
 "fallback_text": "| Model | Price | Battery (h) |\n|---|---|---|\n| Kite 13 | 999 | 11 |"}"#;

const STEPS: &str = r#"{"title": "Move your notes to the new laptop",
 "steps": [{"title": "Export notes", "state": "done"}, {"title": "Copy the export", "state": "current", "detail": "Use a USB drive or the local network"},
           {"title": "Import on the new laptop"}, {"title": "Check every notebook opened"}],
 "fallback_text": "1. Export notes (done)\n2. Copy the export (now)"}"#;

const METRICS: &str = r#"{"title": "October spending",
 "metrics": [{"label": "Total", "value": "2,340", "change_percent": -8.5, "lower_is_better": true},
             {"label": "Groceries", "value": "610", "change_percent": 4.1, "lower_is_better": true},
             {"label": "Saved", "value": "450", "change_percent": 12.0}],
 "fallback_text": "October spending: total 2,340 (down 8.5%)."}"#;

const CHART: &str = r#"{"title": "Rainfall in Lisbon", "kind": "bar", "series": "Rain", "unit": "mm",
 "points": [{"label": "Oct", "value": 100}, {"label": "Nov", "value": 128}, {"label": "Dec", "value": 127}, {"label": "Jan", "value": 110}],
 "fallback_text": "Rainfall in Lisbon (mm): Oct 100, Nov 128, Dec 127, Jan 110."}"#;

#[test]
fn every_schema_reads_as_its_json_does() {
    for s in DISPLAY_SPECS {
        assert_eq!(json::parse(s.schema).unwrap(), v(s.schema), "{}", s.name);
        let schema = s.schema_value();
        assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&Value::Bool(false))
        );
        let req = schema.get("required").expect("required");
        assert!(matches!(req, Value::Array(r) if r.contains(&Value::from("fallback_text"))));
        assert!(s.description.chars().count() <= 140, "{}", s.name);
        assert_eq!(s.since_revision, 1);
    }
}

#[test]
fn the_reader_keeps_escapes_and_numbers() {
    let read = json::parse(r#"{"a": "x\n\"é", "b": [-1, 2.5, 1e2, true, null]}"#).unwrap();
    assert_eq!(
        read,
        v(r#"{"a": "x\n\"é", "b": [-1, 2.5, 1e2, true, null]}"#)
    );
    assert!(json::parse("{\"a\": 1} x").is_err());
    assert!(json::parse(&"[".repeat(64)).is_err());
}

/// Every array has maxItems and every string maxLength (spec 3.2).
fn bounded(schema: &Value, at: &str) {
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => {
            assert!(schema.get("maxItems").is_some(), "{at} has no maxItems");
            bounded(schema.get("items").expect("items"), &format!("{at}[]"));
        }
        Some("string") if schema.get("enum").is_none() => {
            assert!(schema.get("maxLength").is_some(), "{at} has no maxLength");
        }
        Some("object") => {
            if let Some(Value::Object(props)) = schema.get("properties") {
                for (k, sub) in props {
                    bounded(sub, &format!("{at}.{k}"));
                }
            }
        }
        _ => {}
    }
}

#[test]
fn every_array_and_string_is_bounded() {
    for s in DISPLAY_SPECS {
        bounded(&s.schema_value(), s.name);
    }
}

#[test]
fn each_example_call_is_valid() {
    for (name, text) in [
        ("show_table", TABLE),
        ("show_steps", STEPS),
        ("show_metrics", METRICS),
        ("show_chart", CHART),
    ] {
        assert_eq!(validate(name, &v(text)), Ok(()), "{name}");
    }
}

#[test]
fn a_bad_call_gets_a_reason_naming_the_field() {
    let missing = reason(
        "show_chart",
        r#"{"kind": "bar", "points": [{"label": "a", "value": 1}, {"label": "b", "value": 2}]}"#,
    );
    assert_eq!(missing, "show_chart: fallback_text is required");
    let extra = reason(
        "show_metrics",
        r#"{"metrics": [{"label": "a", "value": "1", "colour": "red"}], "fallback_text": "a"}"#,
    );
    assert_eq!(
        extra,
        "show_metrics: metrics[0] has a field \"colour\" that is not allowed"
    );
    let long = format!(
        r#"{{"kind": "line", "points": [{}], "fallback_text": "x"}}"#,
        (0..61)
            .map(|i| format!(r#"{{"label": "p{i}", "value": {i}}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    assert_eq!(
        reason("show_chart", &long),
        "show_chart: points has 61 items; at most 60"
    );
    let nested = reason(
        "show_table",
        r#"{"columns": [{"title": "A"}], "rows": [[1]], "fallback_text": "x"}"#,
    );
    assert_eq!(
        nested,
        "show_table: rows[0][0] must be a string, not a number"
    );
    let cells = reason(
        "show_table",
        r#"{"columns": [{"title": "A"}, {"title": "B"}, {"title": "C"}, {"title": "D"}], "rows": [["1","2","3","4"],["1","2","3","4"],["1","2","3"]], "fallback_text": "x"}"#,
    );
    assert_eq!(
        cells,
        "show_table: rows[2] has 3 cells; there are 4 columns."
    );
    let state = reason(
        "show_steps",
        r#"{"steps": [{"title": "a", "state": "late"}], "fallback_text": "x"}"#,
    );
    assert_eq!(
        state,
        "show_steps: steps[0].state must be one of done, current, pending, failed"
    );
}

#[test]
fn the_cross_field_rules_hold() {
    let sort = reason(
        "show_table",
        r#"{"columns": [{"title": "A"}], "rows": [["1"]], "sort": {"column": 3}, "fallback_text": "x"}"#,
    );
    assert_eq!(sort, "show_table: sort.column is 3; there are 1 columns.");
    let total = reason(
        "show_table",
        r#"{"columns": [{"title": "A", "total": true}], "rows": [["1"]], "fallback_text": "x"}"#,
    );
    assert_eq!(
        total,
        "show_table: columns[0] has total but is not numeric."
    );
    let two = reason(
        "show_steps",
        r#"{"steps": [{"title": "a", "state": "current"}, {"title": "b", "state": "current"}], "fallback_text": "x"}"#,
    );
    assert_eq!(two, "show_steps: steps has 2 current steps; at most 1.");
    let dup = reason(
        "show_chart",
        r#"{"kind": "bar", "points": [{"label": "a", "value": 1}, {"label": "a", "value": 2}], "fallback_text": "x"}"#,
    );
    assert_eq!(dup, "show_chart: points[1] repeats the label \"a\".");
}

#[test]
fn a_reason_quotes_little_of_the_input() {
    let key = "k".repeat(500);
    let text = format!(
        r#"{{"metrics": [{{"label": "a", "value": "1"}}], "fallback_text": "a", "{key}": 1}}"#
    );
    let r = reason("show_metrics", &text);
    assert!(r.len() < 200, "{r}");
    assert_eq!(
        reason("show_map", "{}"),
        "There is no display widget named \"show_map\"."
    );
    assert_eq!(
        validate("show_table", &Value::Null).unwrap_err().reason,
        "show_table: the input must be an object, not null"
    );
}

#[test]
fn the_tool_is_a_parallel_display_tool_with_no_effects() {
    let spec = display_tool_spec("show_steps").unwrap();
    assert_eq!(spec.kind, ToolKind::Display);
    assert!(spec.effects.is_empty());
    assert_eq!(spec.concurrency, Concurrency::Parallel);
    assert_eq!(
        spec.input_schema,
        v(display_spec("show_steps").unwrap().schema)
    );
    assert!(display_tool_spec("show_map").is_none());
}

#[test]
fn the_gate_follows_the_profile_and_the_model() {
    for p in [Profile::High, Profile::Standard, Profile::Intel] {
        assert!(display_capable(Some(&p), "local:default"));
        assert!(!display_capable(Some(&p), SMALLEST_MODEL));
    }
    assert!(!display_capable(Some(&Profile::Low), "local:default"));
    assert!(!display_capable(
        Some(&Profile::Other("tiny".into())),
        "local:default"
    ));
    assert!(!display_capable(None, "local:default"));
}
