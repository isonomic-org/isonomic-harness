// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The schema subset display specs use, checked recursively, then each
//! widget's cross-field rules. Pure and deterministic.
//!
//! The subset: `type`, `properties`, `required`, `additionalProperties:
//! false`, `items`, `enum`, `minItems`, `maxItems`, `minLength`,
//! `maxLength`, `minimum`, `maximum`.

use super::DisplayError;
use crate::value::{Number, Value};
use std::collections::BTreeSet;

/// At most this many characters of the input appear in a reason.
pub const QUOTE_MAX: usize = 80;

fn num(n: &Number) -> f64 {
    match *n {
        Number::Unsigned(u) => u as f64,
        Number::Signed(i) => i as f64,
        Number::Float(f) => f,
    }
}

fn int(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => Some(num(n)),
        _ => None,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// `text` cut to [`QUOTE_MAX`] characters, quoted.
pub(crate) fn quote(text: &str) -> String {
    let mut cut: String = text.chars().take(QUOTE_MAX).collect();
    if cut.len() < text.len() {
        cut.push_str("...");
    }
    format!("\"{cut}\"")
}

fn fail(path: &str, rule: String) -> Result<(), DisplayError> {
    let at = if path.is_empty() { "the input" } else { path };
    Err(DisplayError::new(format!("{at} {rule}")))
}

fn child(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

/// Check `v` against `schema` at `path`.
pub(crate) fn check(schema: &Value, v: &Value, path: &str) -> Result<(), DisplayError> {
    if let Some(Value::Array(allowed)) = schema.get("enum") {
        if !allowed.contains(v) {
            let names: Vec<&str> = allowed.iter().filter_map(Value::as_str).collect();
            return fail(path, format!("must be one of {}", names.join(", ")));
        }
    }
    let want = schema.get("type").and_then(Value::as_str).unwrap_or("");
    match (want, v) {
        ("object", Value::Object(map)) => {
            let props = schema.get("properties");
            if let Some(Value::Array(req)) = schema.get("required") {
                for key in req.iter().filter_map(Value::as_str) {
                    if !map.contains_key(key) {
                        return fail(&child(path, key), "is required".into());
                    }
                }
            }
            for (key, value) in map {
                match props.and_then(|p| p.get(key)) {
                    Some(sub) => check(sub, value, &child(path, key))?,
                    None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                        let name = quote(key);
                        return fail(path, format!("has a field {name} that is not allowed"));
                    }
                    None => {}
                }
            }
            Ok(())
        }
        ("array", Value::Array(items)) => {
            if let Some(min) = int(schema.get("minItems")) {
                if (items.len() as f64) < min {
                    return fail(path, format!("needs at least {min} items"));
                }
            }
            if let Some(max) = int(schema.get("maxItems")) {
                if (items.len() as f64) > max {
                    return fail(path, format!("has {} items; at most {max}", items.len()));
                }
            }
            if let Some(sub) = schema.get("items") {
                for (i, item) in items.iter().enumerate() {
                    check(sub, item, &format!("{path}[{i}]"))?;
                }
            }
            Ok(())
        }
        ("string", Value::String(s)) => {
            let len = s.chars().count() as f64;
            if let Some(min) = int(schema.get("minLength")) {
                if len < min {
                    return fail(path, format!("must be at least {min} characters"));
                }
            }
            if let Some(max) = int(schema.get("maxLength")) {
                if len > max {
                    return fail(path, format!("is {len} characters; at most {max}"));
                }
            }
            Ok(())
        }
        ("number" | "integer", Value::Number(n)) => {
            let x = num(n);
            if want == "integer" && x.fract() != 0.0 {
                return fail(path, "must be a whole number".into());
            }
            if let Some(min) = int(schema.get("minimum")) {
                if x < min {
                    return fail(path, format!("must be at least {min}"));
                }
            }
            if let Some(max) = int(schema.get("maximum")) {
                if x > max {
                    return fail(path, format!("must be at most {max}"));
                }
            }
            Ok(())
        }
        ("boolean", Value::Bool(_)) | ("", _) => Ok(()),
        (want, v) => {
            let article = if want.starts_with(['a', 'e', 'i', 'o']) {
                "an"
            } else {
                "a"
            };
            fail(
                path,
                format!("must be {article} {want}, not {}", type_name(v)),
            )
        }
    }
}

fn len(v: &Value, key: &str) -> usize {
    match v.get(key) {
        Some(Value::Array(a)) => a.len(),
        _ => 0,
    }
}

fn items<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    match v.get(key) {
        Some(Value::Array(a)) => a,
        _ => &[],
    }
}

/// The widget's cross-field rules, after the schema passed.
pub(crate) fn cross(name: &str, v: &Value) -> Result<(), DisplayError> {
    match name {
        "show_table" => {
            let cols = len(v, "columns");
            for (i, row) in items(v, "rows").iter().enumerate() {
                let cells = match row {
                    Value::Array(c) => c.len(),
                    _ => 0,
                };
                if cells != cols {
                    let rule = format!("has {cells} cells; there are {cols} columns.");
                    return fail(&format!("rows[{i}]"), rule);
                }
            }
            if let Some(col) = int(v.get("sort").and_then(|s| s.get("column"))) {
                if col >= cols as f64 {
                    return fail(
                        "sort.column",
                        format!("is {col}; there are {cols} columns."),
                    );
                }
            }
            for (i, c) in items(v, "columns").iter().enumerate() {
                let flag = |k: &str| c.get(k) == Some(&Value::Bool(true));
                if flag("total") && !flag("numeric") {
                    let rule = "has total but is not numeric.".to_owned();
                    return fail(&format!("columns[{i}]"), rule);
                }
            }
            Ok(())
        }
        "show_steps" => {
            let current = items(v, "steps")
                .iter()
                .filter(|s| s.get("state").and_then(Value::as_str) == Some("current"))
                .count();
            if current > 1 {
                return fail("steps", format!("has {current} current steps; at most 1."));
            }
            Ok(())
        }
        "show_chart" => {
            let mut seen = BTreeSet::new();
            for (i, p) in items(v, "points").iter().enumerate() {
                let label = p.get("label").and_then(Value::as_str).unwrap_or("");
                if !seen.insert(label) {
                    let rule = format!("repeats the label {}.", quote(label));
                    return fail(&format!("points[{i}]"), rule);
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
