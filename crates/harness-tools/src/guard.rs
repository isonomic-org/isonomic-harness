//! Input guards: a call whose input does not fit its tool never runs; the
//! model gets a readable reason instead.

use harness_core::*;

/// Why a call's input was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuardFailure {
    /// A sentence the model can act on.
    pub message: String,
}

/// Check a call's input against its tool's schema: the input parsed, it is
/// an object, required fields are present and top-level types match.
pub fn guard(spec: &ToolSpec, input: &Value, input_raw: Option<&str>) -> Result<(), GuardFailure> {
    let fail = |message: String| Err(GuardFailure { message });
    if let Some(raw) = input_raw {
        return fail(format!(
            "The input to {} was not valid JSON: {}",
            spec.name,
            shorten(raw)
        ));
    }
    let schema = &spec.input_schema;
    let Value::Object(fields) = input else {
        return fail(format!("The input to {} must be a JSON object.", spec.name));
    };
    if let Some(Value::Array(required)) = schema.get("required") {
        for name in required.iter().filter_map(Value::as_str) {
            if !fields.contains_key(name) {
                return fail(format!("{} needs the field `{name}`.", spec.name));
            }
        }
    }
    if let Some(Value::Object(props)) = schema.get("properties") {
        for (name, value) in fields {
            let Some(want) = props
                .get(name)
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !has_type(value, want) {
                return fail(format!(
                    "The field `{name}` of {} must be of type {want}.",
                    spec.name
                ));
            }
        }
    }
    Ok(())
}

fn has_type(v: &Value, t: &str) -> bool {
    match (t, v) {
        ("string", Value::String(_))
        | ("boolean", Value::Bool(_))
        | ("array", Value::Array(_))
        | ("object", Value::Object(_))
        | ("null", Value::Null)
        | ("number", Value::Number(_)) => true,
        ("integer", Value::Number(n)) => !matches!(n, Number::Float(_)),
        _ => false,
    }
}

fn shorten(raw: &str) -> String {
    let mut end = raw.len().min(80);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    raw[..end].to_owned()
}
