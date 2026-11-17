// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! A small JSON Schema checker for the event schema.
//!
//! It supports exactly the keywords `session-event.v1.json` uses and fails on
//! any other, so the schema can never quietly outgrow the checker.

use serde_json::Value;

const ANNOTATIONS: &[&str] = &[
    "$schema",
    "$id",
    "$defs",
    "title",
    "description",
    "$comment",
];

/// Check `instance` against `schema`. Errors name the failing path.
pub fn check(schema: &Value, instance: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    node(schema, schema, instance, "$", &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn node(root: &Value, schema: &Value, v: &Value, path: &str, errors: &mut Vec<String>) {
    let Some(map) = schema.as_object() else {
        errors.push(format!("{path}: schema is not an object"));
        return;
    };
    for (key, rule) in map {
        if ANNOTATIONS.contains(&key.as_str()) {
            continue;
        }
        match key.as_str() {
            "$ref" => {
                let target = rule.as_str().and_then(|r| r.strip_prefix("#/$defs/"));
                match target.and_then(|name| root["$defs"].get(name)) {
                    Some(def) => node(root, def, v, path, errors),
                    None => errors.push(format!("{path}: unresolved $ref {rule}")),
                }
            }
            "type" => {
                let ok = match rule {
                    Value::String(t) => is_type(v, t),
                    Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| is_type(v, t)),
                    _ => false,
                };
                if !ok {
                    errors.push(format!("{path}: expected type {rule}"));
                }
            }
            "properties" => {
                if let (Some(props), Some(obj)) = (rule.as_object(), v.as_object()) {
                    for (name, sub) in props {
                        if let Some(child) = obj.get(name) {
                            node(root, sub, child, &format!("{path}.{name}"), errors);
                        }
                    }
                }
            }
            "required" => {
                if let Some(obj) = v.as_object() {
                    for name in rule
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        if !obj.contains_key(name) {
                            errors.push(format!("{path}: missing {name}"));
                        }
                    }
                }
            }
            "enum" => {
                if !rule.as_array().is_some_and(|opts| opts.contains(v)) {
                    errors.push(format!("{path}: {v} is not one of {rule}"));
                }
            }
            "const" => {
                if rule != v {
                    errors.push(format!("{path}: expected {rule}"));
                }
            }
            "items" => {
                for (i, item) in v.as_array().into_iter().flatten().enumerate() {
                    node(root, rule, item, &format!("{path}[{i}]"), errors);
                }
            }
            "minItems" | "maxItems" => {
                if let (Some(items), Some(n)) = (v.as_array(), rule.as_u64()) {
                    let len = items.len() as u64;
                    if (key == "minItems" && len < n) || (key == "maxItems" && len > n) {
                        errors.push(format!("{path}: {key} {n}, found {len}"));
                    }
                }
            }
            "minLength" => {
                if let (Some(s), Some(n)) = (v.as_str(), rule.as_u64()) {
                    if (s.chars().count() as u64) < n {
                        errors.push(format!("{path}: shorter than {n}"));
                    }
                }
            }
            "pattern" => {
                if let (Some(s), Some(p)) = (v.as_str(), rule.as_str()) {
                    match hex_pattern(p) {
                        Some(n)
                            if s.len() == n
                                && s.bytes()
                                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) => {}
                        Some(_) => errors.push(format!("{path}: does not match {p}")),
                        None => errors.push(format!("{path}: unsupported pattern {p}")),
                    }
                }
            }
            "minimum" => {
                if let (Some(x), Some(min)) = (v.as_f64(), rule.as_f64()) {
                    if x < min {
                        errors.push(format!("{path}: below {min}"));
                    }
                }
            }
            "oneOf" => {
                let options = rule.as_array().map(Vec::as_slice).unwrap_or_default();
                let passing = options
                    .iter()
                    .filter(|sub| {
                        let mut e = Vec::new();
                        node(root, sub, v, path, &mut e);
                        e.is_empty()
                    })
                    .count();
                if passing != 1 {
                    errors.push(format!("{path}: matches {passing} of oneOf, expected 1"));
                }
            }
            other => errors.push(format!("{path}: unsupported keyword {other}")),
        }
    }
}

/// The one pattern form the schema uses: `^[0-9a-f]{N}$` (lowercase hex of
/// length N). Returns N.
fn hex_pattern(p: &str) -> Option<usize> {
    p.strip_prefix("^[0-9a-f]{")?
        .strip_suffix("}$")?
        .parse()
        .ok()
}

fn is_type(v: &Value, t: &str) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "number" => v.is_number(),
        "integer" => v.is_u64() || v.is_i64(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn checks_and_refuses_unknown_keywords() {
        let schema =
            json!({"type": "object", "required": ["a"], "properties": {"a": {"enum": [1, 2]}}});
        assert!(check(&schema, &json!({"a": 1})).is_ok());
        assert!(check(&schema, &json!({"a": 3})).is_err());
        assert!(check(&schema, &json!({})).is_err());
        let odd = json!({"pattern": "x"});
        assert!(check(&odd, &json!("x")).is_err());
    }
}
