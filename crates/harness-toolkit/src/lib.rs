// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isonomic Harness standard tools, one wave at a time.
//!
//! Milestone 0 ships four engine tools: [`AskUser`], [`Todo`], [`PlanTool`]
//! and [`Now`] (time and context). The service traits for later waves are
//! defined now, each with a fake, in [`services`]: artifacts (wave 3), web
//! search and URL reading through [`services::Egress`] (wave 4), memory
//! (after its spec is approved) and connections (later). No tool for them
//! ships yet.
//!
//! Tools never panic: bad input is a readable error result.

mod now;
pub mod services;
mod state;

pub use now::{FixedZone, LocalZone, Now};
pub use state::{PlanTool, Todo};

use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use harness_tools::{QuestionSpec, Tool, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

pub(crate) fn schema(props: &[(&str, &str)], required: &[&str]) -> Value {
    Value::object([
        ("type", Value::from("object")),
        (
            "properties",
            Value::object(
                props
                    .iter()
                    .map(|(k, t)| (*k, Value::object([("type", Value::from(*t))]))),
            ),
        ),
        ("required", Value::from(required.to_vec())),
    ])
}

pub(crate) fn engine(name: &str, description: &str, input_schema: Value) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: description.into(),
        input_schema,
        kind: ToolKind::Engine,
        effects: Vec::new(),
        concurrency: Concurrency::Serial,
    }
}

/// `ask_user`: ask the person a question; the answer is the result.
#[derive(Debug, Default)]
pub struct AskUser;

impl AskUser {
    fn question(input: &Value) -> Result<QuestionSpec, String> {
        let prompt = input
            .get("prompt")
            .and_then(Value::as_str)
            .filter(|p| !p.trim().is_empty())
            .ok_or("ask_user needs a non-empty `prompt`.")?;
        let mut options = Vec::new();
        if let Some(Value::Array(items)) = input.get("options") {
            for (i, item) in items.iter().enumerate() {
                let (id, label) = match item {
                    Value::String(s) => (format!("{}", i + 1), s.clone()),
                    Value::Object(_) => {
                        let label = item.get("label").and_then(Value::as_str);
                        let id = item.get("id").and_then(Value::as_str);
                        match (id, label) {
                            (Some(id), Some(label)) => (id.to_owned(), label.to_owned()),
                            (None, Some(label)) => (format!("{}", i + 1), label.to_owned()),
                            _ => return Err(format!("Option {} needs a `label`.", i + 1)),
                        }
                    }
                    _ => return Err(format!("Option {} must be text or {{id, label}}.", i + 1)),
                };
                options.push(Choice { id, label });
            }
        }
        let flag = |k: &str| matches!(input.get(k), Some(Value::Bool(true)));
        let free_text =
            options.is_empty() || !matches!(input.get("free_text"), Some(Value::Bool(false)));
        Ok(QuestionSpec {
            prompt: prompt.to_owned(),
            options,
            multiple: flag("multiple"),
            free_text,
        })
    }
}

impl Tool for AskUser {
    fn spec(&self) -> ToolSpec {
        engine(
            "ask_user",
            "Ask the person a question, with options when there are clear choices.",
            schema(
                &[
                    ("prompt", "string"),
                    ("options", "array"),
                    ("multiple", "boolean"),
                    ("free_text", "boolean"),
                ],
                &["prompt"],
            ),
        )
    }

    fn call<'a>(&'a self, input: &'a Value, _cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        ready(match Self::question(input) {
            Ok(q) => ToolOutput::Ask(q),
            Err(msg) => ToolOutput::Done(ToolDone::error("tool.input", &msg)),
        })
    }
}

/// The Milestone 0 toolkit, ready to register.
pub fn toolkit(zone: Arc<dyn LocalZone>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(AskUser),
        Arc::new(Todo::default()),
        Arc::new(PlanTool::default()),
        Arc::new(Now::new(zone)),
    ]
}
