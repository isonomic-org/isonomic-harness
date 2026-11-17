// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! `todo` and `plan`: per-session lists. Their state is a pure function of
//! the session's own calls, so after a restart [`Todo::replay`] and
//! [`PlanTool::replay`] rebuild it from the log.

use crate::{engine, schema};
use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::collections::BTreeMap;
use std::sync::Mutex;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Successful calls of `tool` in `session`, in order.
fn calls_of<'a>(events: &'a [SessionEvent], tool: &str) -> Vec<&'a Value> {
    let mut inputs: BTreeMap<CallId, &Value> = BTreeMap::new();
    let mut out = Vec::new();
    for e in events {
        match &e.event {
            Event::ToolCall(c) if c.tool == tool => {
                inputs.insert(c.call.clone(), &c.input);
            }
            Event::ToolResult(r) if r.outcome == ToolOutcome::Ok => {
                if let Some(i) = inputs.remove(&r.call) {
                    out.push(i);
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    text: String,
    done: bool,
}

/// `todo`: add, check off, remove or list items for the session.
#[derive(Debug, Default)]
pub struct Todo {
    lists: Mutex<BTreeMap<SessionId, Vec<Item>>>,
}

impl Todo {
    /// Rebuild a session's list from its log.
    pub fn replay(&self, session: &SessionId, events: &[SessionEvent]) {
        let mut items = Vec::new();
        for input in calls_of(events, "todo") {
            let _ = apply(&mut items, input);
        }
        lock(&self.lists).insert(session.clone(), items);
    }

    /// A session's list as the model sees it.
    pub fn render(&self, session: &SessionId) -> String {
        render(lock(&self.lists).get(session).map_or(&[][..], |v| &v[..]))
    }
}

fn render(items: &[Item]) -> String {
    if items.is_empty() {
        return "The list is empty.".into();
    }
    items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            format!(
                "{}. [{}] {}",
                i + 1,
                if it.done { "x" } else { " " },
                it.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn find(items: &[Item], key: &str) -> Option<usize> {
    if let Ok(n) = key.trim().parse::<usize>() {
        return (1..=items.len()).contains(&n).then(|| n - 1);
    }
    let key = key.trim().to_lowercase();
    items.iter().position(|i| i.text.to_lowercase() == key)
}

fn apply(items: &mut Vec<Item>, input: &Value) -> Result<String, String> {
    let action = input
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("list");
    let item = input
        .get("item")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let need = || {
        if item.is_empty() {
            Err(format!("todo {action} needs an `item`."))
        } else {
            Ok(())
        }
    };
    match action {
        "add" => {
            need()?;
            items.push(Item {
                text: item.to_owned(),
                done: false,
            });
            Ok("Added.".into())
        }
        "done" | "check" | "undo" | "remove" => {
            need()?;
            let i = find(items, item).ok_or_else(|| format!("No item \"{item}\" on the list."))?;
            match action {
                "remove" => {
                    items.remove(i);
                    Ok("Removed.".into())
                }
                "undo" => {
                    items[i].done = false;
                    Ok("Marked not done.".into())
                }
                _ => {
                    items[i].done = true;
                    Ok("Checked off.".into())
                }
            }
        }
        "list" => Ok(render(items)),
        other => Err(format!(
            "Unknown todo action \"{other}\"; use add, done, undo, remove or list."
        )),
    }
}

impl Tool for Todo {
    fn spec(&self) -> ToolSpec {
        engine(
            "todo",
            "Add, check off or list to-do items. Actions: add, done, undo, remove, list; `item` is the text or its number.",
            schema(&[("action", "string"), ("item", "string")], &["action"]),
        )
    }

    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        let mut lists = lock(&self.lists);
        let items = lists.entry(cx.session.clone()).or_default();
        ready(ToolOutput::Done(match apply(items, input) {
            Ok(text) => ToolDone::ok(text),
            Err(msg) => ToolDone::error("tool.input", &msg),
        }))
    }
}

/// A plan's steps and the step underway.
type Plan = (Vec<String>, Option<usize>);

/// `plan`: record or update the plan for a multi-step task.
#[derive(Debug, Default)]
pub struct PlanTool {
    plans: Mutex<BTreeMap<SessionId, Plan>>,
}

fn plan_of(input: &Value) -> Result<Plan, String> {
    let Some(Value::Array(steps)) = input.get("steps") else {
        return Err("plan needs `steps`, a list of short steps.".into());
    };
    let steps: Vec<String> = steps
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if steps.is_empty() {
        return Err("plan needs at least one step.".into());
    }
    let current = match input.get("current") {
        Some(Value::Number(Number::Unsigned(n))) => Some(*n as usize),
        _ => None,
    };
    if current.is_some_and(|c| c == 0 || c > steps.len()) {
        return Err(format!("`current` must be between 1 and {}.", steps.len()));
    }
    Ok((steps, current))
}

impl PlanTool {
    /// Rebuild a session's plan from its log.
    pub fn replay(&self, session: &SessionId, events: &[SessionEvent]) {
        let last = calls_of(events, "plan")
            .into_iter()
            .rev()
            .find_map(|i| plan_of(i).ok());
        let mut plans = lock(&self.plans);
        match last {
            Some(p) => {
                plans.insert(session.clone(), p);
            }
            None => {
                plans.remove(session);
            }
        }
    }

    /// A session's plan as the model sees it.
    pub fn render(&self, session: &SessionId) -> Option<String> {
        lock(&self.plans).get(session).map(|(steps, current)| {
            steps
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let mark = if current == &Some(i + 1) { "→" } else { " " };
                    format!("{mark} {}. {s}", i + 1)
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
    }
}

impl Tool for PlanTool {
    fn spec(&self) -> ToolSpec {
        engine(
            "plan",
            "Record or update the plan for a multi-step task; `current` is the step underway (from 1).",
            schema(&[("steps", "array"), ("current", "integer")], &["steps"]),
        )
    }

    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        let out = match plan_of(input) {
            Ok(plan) => {
                let n = plan.0.len();
                lock(&self.plans).insert(cx.session.clone(), plan);
                ToolDone::ok(format!("Plan recorded: {n} steps."))
            }
            Err(msg) => ToolDone::error("tool.input", &msg),
        };
        ready(ToolOutput::Done(out))
    }
}
