//! One-step migrations between schema versions.
//!
//! Version 0 is the pre-release draft: `kind` instead of `type`, underscores
//! instead of dots in type names, `ts` instead of `at`, and plain `text`
//! where v1 has content blocks. Each function moves one event exactly one
//! version forward.

use serde_json::{json, Map, Value};

/// Move one event from `from` to `from + 1`. `None` when there is no
/// migration from that version.
pub fn step(from: u32, event: Value) -> Option<Value> {
    match from {
        0 => Some(v0_to_v1(event)),
        _ => None,
    }
}

fn v0_to_v1(event: Value) -> Value {
    let Value::Object(mut map) = event else {
        return event;
    };
    let mut out = Map::new();
    for key in ["session", "seq"] {
        if let Some(v) = map.remove(key) {
            out.insert(key.into(), v);
        }
    }
    out.insert("at".into(), map.remove("ts").unwrap_or(json!(0)));
    for key in ["turn", "step"] {
        if let Some(v) = map.remove(key) {
            out.insert(key.into(), v);
        }
    }
    let kind = map
        .remove("kind")
        .and_then(|k| k.as_str().map(str::to_owned))
        .unwrap_or_default();
    let kind = match kind.as_str() {
        "assistant" | "user" | "system" | "title" | "attempt" | "question" | "answer"
        | "artifact" | "compacted" | "forked" => kind,
        other => other.replacen('_', ".", 1),
    };
    out.insert("type".into(), Value::String(kind.clone()));
    match kind.as_str() {
        "user" => {
            let text = map.remove("text").unwrap_or(json!(""));
            out.insert("content".into(), json!([{ "type": "text", "text": text }]));
        }
        "assistant" => {
            let text = map.remove("text").unwrap_or(json!(""));
            out.insert("content".into(), json!([{ "type": "text", "text": text }]));
            let stop = map.remove("stop").unwrap_or(json!("end"));
            out.insert("stop".into(), stop);
            out.insert("stream".into(), json!([]));
        }
        _ => {}
    }
    out.extend(map);
    Value::Object(out)
}
