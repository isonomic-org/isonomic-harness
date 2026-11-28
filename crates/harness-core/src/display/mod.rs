// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display widgets: tools the model calls to show a component inline (a
//! table, a step list, headline figures, a chart). A call has no side
//! effect: the host validates it, records it, and tells the model "shown";
//! the conversation kit renders it, or its `fallback_text`, or a generic
//! block. The reference is `docs/display.md`.
//!
//! Specs are versioned with the event schema: each records the revision
//! that added it. After the v1 freeze a spec changes only additively; a
//! breaking change is a new widget name.

mod json;
mod validate;

use crate::value::Value;
use crate::{Concurrency, Profile, ToolKind, ToolSpec};
use std::sync::OnceLock;

pub use validate::QUOTE_MAX;

/// One display widget: its tool name, its line for the model, and its
/// strict JSON schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplaySpec {
    /// The tool name the model calls, such as `show_table`.
    pub name: &'static str,
    /// One line, for the model.
    pub description: &'static str,
    /// The input's JSON schema, as JSON text.
    pub schema: &'static str,
    /// The schema revision that added it.
    pub since_revision: u32,
}

/// Every display widget this build knows, in a fixed order.
pub const DISPLAY_SPECS: &[DisplaySpec] = &[
    DisplaySpec {
        name: "show_table",
        description: "Show a table of rows and columns the person can sort. Use for comparisons of 3 or more items.",
        schema: include_str!("../../schema/display/show_table.v1.json"),
        since_revision: 1,
    },
    DisplaySpec {
        name: "show_steps",
        description: "Show a short list of steps and where each one stands. Use for plans, procedures and progress.",
        schema: include_str!("../../schema/display/show_steps.v1.json"),
        since_revision: 1,
    },
    DisplaySpec {
        name: "show_metrics",
        description: "Show 1 to 6 headline figures side by side, each with an optional change. Use for summaries of numbers.",
        schema: include_str!("../../schema/display/show_metrics.v1.json"),
        since_revision: 1,
    },
    DisplaySpec {
        name: "show_chart",
        description: "Show one series of labelled values as a bar, line or area chart. Use when the shape of the numbers matters.",
        schema: include_str!("../../schema/display/show_chart.v1.json"),
        since_revision: 1,
    },
];

/// The widget named `name`, if this build knows it.
pub fn display_spec(name: &str) -> Option<&'static DisplaySpec> {
    DISPLAY_SPECS.iter().find(|s| s.name == name)
}

/// Each spec's schema, read once.
fn schemas() -> &'static [Value] {
    static SCHEMAS: OnceLock<Vec<Value>> = OnceLock::new();
    SCHEMAS.get_or_init(|| {
        DISPLAY_SPECS
            .iter()
            .map(|s| json::parse(s.schema).unwrap_or_else(|e| panic!("{}: {e}", s.name)))
            .collect()
    })
}

fn schema_of(name: &str) -> Option<&'static Value> {
    let i = DISPLAY_SPECS.iter().position(|s| s.name == name)?;
    schemas().get(i)
}

impl DisplaySpec {
    /// The schema as a value.
    pub fn schema_value(&self) -> Value {
        schema_of(self.name).cloned().unwrap_or_default()
    }

    /// The tool as offered to the model: kind `display`, no effects,
    /// parallel.
    pub fn tool_spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.to_owned(),
            description: self.description.to_owned(),
            input_schema: self.schema_value(),
            kind: ToolKind::Display,
            effects: Vec::new(),
            concurrency: Concurrency::Parallel,
            scope: None,
        }
    }
}

/// The tool for the widget named `name`, as offered to the model.
pub fn display_tool_spec(name: &str) -> Option<ToolSpec> {
    display_spec(name).map(DisplaySpec::tool_spec)
}

/// Why a display call's input was refused: a readable reason that names the
/// widget, the field and the rule, for the model to correct. It quotes at
/// most [`QUOTE_MAX`] characters of the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayError {
    /// The reason.
    pub reason: String,
}

impl DisplayError {
    fn new(reason: String) -> Self {
        Self { reason }
    }
}

impl std::fmt::Display for DisplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for DisplayError {}

/// Check a display call's input against its widget's schema, then the
/// widget's cross-field rules. Pure and deterministic.
pub fn validate(name: &str, input: &Value) -> Result<(), DisplayError> {
    let Some(schema) = schema_of(name) else {
        let quoted = validate::quote(name);
        return Err(DisplayError::new(format!(
            "There is no display widget named {quoted}."
        )));
    };
    validate::check(schema, input, "")
        .and_then(|()| validate::cross(name, input))
        .map_err(|e| DisplayError::new(format!("{name}: {}", e.reason)))
}

/// The smallest local model, which is never offered display tools.
pub const SMALLEST_MODEL: &str = "local:smallest";

/// Whether the session's model is offered display tools on this hardware
/// profile: yes on `high`, `standard` and `intel` except with
/// [`SMALLEST_MODEL`]; no on `low`, on a profile this build does not know,
/// and with no profile. A thin wrapper over the capability table
/// ([`offer`](crate::offer::offer)).
pub fn display_capable(profile: Option<&Profile>, model: &str) -> bool {
    crate::offer::offer(profile, model, true).display_tools > 0
}

#[cfg(test)]
mod tests;
