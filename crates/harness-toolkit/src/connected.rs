// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The connected-app tools, over [`services::Connections`]:
//!
//! - `connections` lists the owner's connections and their operations;
//! - `connection_read` runs an operation that only reads, without asking;
//! - `connection_act` runs any other operation, outward, so policy asks.
//!
//! Every call names a connection by its handle, never by a secret, and is
//! scoped to the session's owner app. A result is capped at
//! [`RESULT_CAP`] characters so one call cannot fill the window.

use crate::services::{Connections, Operation};
use crate::{engine, schema};
use harness_core::future::BoxFuture;
use harness_core::*;
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

/// The most characters of one result handed back to the model.
pub const RESULT_CAP: usize = 16_000;

/// `connections`: what the owner has connected, and what each offers.
pub struct ListConnections {
    service: Arc<dyn Connections>,
}

/// `connection_read` (reads only) or `connection_act` (everything else).
pub struct CallConnection {
    service: Arc<dyn Connections>,
    reads: bool,
}

/// The three tools over one service, ready to register.
pub fn connected(service: Arc<dyn Connections>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ListConnections {
            service: Arc::clone(&service),
        }),
        Arc::new(CallConnection {
            service: Arc::clone(&service),
            reads: true,
        }),
        Arc::new(CallConnection {
            service,
            reads: false,
        }),
    ]
}

fn operation_value(op: &Operation) -> Value {
    Value::object([
        ("name", Value::from(op.name.as_str())),
        ("description", Value::from(op.description.as_str())),
        ("input_schema", op.input_schema.clone()),
        ("reads_only", Value::Bool(op.reads_only)),
    ])
}

impl Tool for ListConnections {
    fn spec(&self) -> ToolSpec {
        let mut spec = engine(
            "connections",
            "List the person's connected apps and the operations each offers, \
             with their inputs. Call it before using connection_read or connection_act.",
            schema(&[], &[]),
        );
        spec.effects = vec![Effect::ReadsPrivateData];
        spec
    }

    fn call<'a>(&'a self, _input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let mut out = Vec::new();
            for c in self.service.list(&cx.app).await {
                let ops = self.service.operations(&cx.app, &c.handle).await;
                let ops: Vec<Value> = if ops.is_empty() {
                    c.operations
                        .iter()
                        .map(|name| Value::object([("name", Value::from(name.as_str()))]))
                        .collect()
                } else {
                    ops.iter().map(operation_value).collect()
                };
                out.push(Value::object([
                    ("handle", Value::from(c.handle.as_str())),
                    ("label", Value::from(c.label.as_str())),
                    ("operations", Value::Array(ops)),
                ]));
            }
            ToolOutput::Done(ToolDone::json(Value::Array(out)))
        })
    }
}

impl CallConnection {
    fn name(&self) -> &'static str {
        if self.reads {
            "connection_read"
        } else {
            "connection_act"
        }
    }

    /// Whether `operation` on `handle` may go through this tool: it must
    /// be one the connection offers, and read-only exactly when this is
    /// the read tool. Unknown read-only marks count as acting.
    async fn admits(&self, cx: &ToolCtx, handle: &str, operation: &str) -> Result<(), String> {
        let listed = self.service.list(&cx.app).await;
        let Some(conn) = listed.iter().find(|c| c.handle == handle) else {
            return Err(format!("No connected app has the handle {handle:?}."));
        };
        if !conn.operations.iter().any(|o| o == operation) {
            return Err(format!("{} has no operation {operation:?}.", conn.label));
        }
        let reads_only = self
            .service
            .operations(&cx.app, handle)
            .await
            .iter()
            .any(|op| op.name == operation && op.reads_only);
        match (self.reads, reads_only) {
            (true, false) => Err(format!(
                "{operation:?} can change something in {}: use connection_act.",
                conn.label
            )),
            (false, true) => Err(format!(
                "{operation:?} only reads from {}: use connection_read.",
                conn.label
            )),
            _ => Ok(()),
        }
    }
}

fn capped(text: String) -> String {
    if text.chars().count() <= RESULT_CAP {
        return text;
    }
    let mut cut: String = text.chars().take(RESULT_CAP).collect();
    cut.push_str("\n[cut short]");
    cut
}

impl Tool for CallConnection {
    fn spec(&self) -> ToolSpec {
        let description = if self.reads {
            "Run an operation of a connected app that only reads. Name the \
             connection's handle, the operation and its input."
        } else {
            "Run an operation of a connected app that can change something \
             (send, create, update, delete). The person is asked first."
        };
        let mut spec = engine(
            self.name(),
            description,
            schema(
                &[
                    ("connection", "string"),
                    ("operation", "string"),
                    ("input", "object"),
                ],
                &["connection", "operation"],
            ),
        );
        spec.effects = vec![Effect::ReadsPrivateData, Effect::IngestsUntrustedContent];
        if !self.reads {
            spec.effects.push(Effect::ActsOutward);
        }
        spec
    }

    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            let text = |k: &str| {
                input
                    .get(k)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            };
            let (Some(handle), Some(operation)) = (text("connection"), text("operation")) else {
                let msg = format!("{} needs `connection` and `operation`.", self.name());
                return ToolOutput::Done(ToolDone::error("tool.input", &msg));
            };
            if let Err(msg) = self.admits(cx, handle, operation).await {
                return ToolOutput::Done(ToolDone::error("connection.denied", &msg));
            }
            let args = input.get("input").cloned().unwrap_or(Value::Null);
            match self.service.call(&cx.app, handle, operation, args).await {
                Ok(Value::String(s)) => ToolOutput::Done(ToolDone::ok(capped(s))),
                Ok(value) => {
                    let text = serde_json::to_string(&value).unwrap_or_default();
                    if text.chars().count() <= RESULT_CAP {
                        ToolOutput::Done(ToolDone::json(value))
                    } else {
                        ToolOutput::Done(ToolDone::ok(capped(text)))
                    }
                }
                Err(e) => ToolOutput::Done(ToolDone::error(&e.code, &e.message)),
            }
        })
    }
}
