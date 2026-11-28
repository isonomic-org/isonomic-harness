// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The seven recorded session fixtures.
//!
//! | Fixture | Contents |
//! | --- | --- |
//! | chat-basic | Three prompts, streaming, reasoning, a model title, edit and regenerate forks, a stop then continue, a refusal |
//! | tool-heavy | Parallel tool calls, a transient retry, a failed tool, a long result, an artifact created then updated, a heuristic title |
//! | approval | An outward action approved, one denied, an ambiguous answer denied, an ask-user question answered |
//! | autonomous-run | Two scheduled wakes, compaction (trim and summary), a delegated child, a budget stop |
//! | long-thread | 10,000 events |
//! | client-gone | A client tool whose app disconnects: awaiting client, timeout, resume |
//! | display | Each starter display widget shown, a table retried, a chart that falls back, an unknown widget |

mod approval;
mod autonomous_run;
mod chat_basic;
mod client_gone;
mod display;
mod long_thread;
mod tool_heavy;

use harness_core::*;

/// A recorded fixture.
#[derive(Clone, Debug)]
pub struct Fixture {
    /// Its name, which is also its file stem.
    pub name: &'static str,
    /// Whether its JSONL is committed (long-thread is generated on demand).
    pub committed: bool,
    /// Its events, in persistence order.
    pub events: Vec<SessionEvent>,
}

impl Fixture {
    /// The fixture as a JSONL generation: header line, then one event per line.
    pub fn jsonl(&self) -> String {
        to_jsonl(&self.events)
    }

    /// How many sessions it holds.
    pub fn sessions(&self) -> usize {
        let mut ids: Vec<&SessionId> = self.events.iter().map(|e| &e.session).collect();
        ids.sort();
        ids.dedup();
        ids.len()
    }
}

/// Every fixture, in manifest order.
pub fn all() -> Vec<Fixture> {
    vec![
        chat_basic(),
        tool_heavy(),
        approval(),
        autonomous_run(),
        long_thread(),
        client_gone(),
        display(),
    ]
}

/// One fixture by name.
pub fn by_name(name: &str) -> Option<Fixture> {
    all().into_iter().find(|f| f.name == name)
}

/// chat-basic.
pub fn chat_basic() -> Fixture {
    Fixture {
        name: "chat-basic",
        committed: true,
        events: chat_basic::record(),
    }
}

/// tool-heavy.
pub fn tool_heavy() -> Fixture {
    Fixture {
        name: "tool-heavy",
        committed: true,
        events: tool_heavy::record(),
    }
}

/// approval.
pub fn approval() -> Fixture {
    Fixture {
        name: "approval",
        committed: true,
        events: approval::record(),
    }
}

/// autonomous-run.
pub fn autonomous_run() -> Fixture {
    Fixture {
        name: "autonomous-run",
        committed: true,
        events: autonomous_run::record(),
    }
}

/// long-thread: exactly 10,000 events.
pub fn long_thread() -> Fixture {
    Fixture {
        name: "long-thread",
        committed: false,
        events: long_thread::record(),
    }
}

/// display.
pub fn display() -> Fixture {
    Fixture {
        name: "display",
        committed: true,
        events: display::record(),
    }
}

/// client-gone.
pub fn client_gone() -> Fixture {
    Fixture {
        name: "client-gone",
        committed: true,
        events: client_gone::record(),
    }
}

/// Events as a JSONL generation.
pub fn to_jsonl(events: &[SessionEvent]) -> String {
    let mut out = serde_json::to_string(&LogHeader::current()).expect("header serializes");
    out.push('\n');
    for event in events {
        out.push_str(&serde_json::to_string(event).expect("event serializes"));
        out.push('\n');
    }
    out
}

/// `fixtures/MANIFEST.json` for these fixtures.
pub fn manifest(fixtures: &[Fixture]) -> String {
    let entries: Vec<serde_json::Value> = fixtures
        .iter()
        .map(|f| {
            let text = f.jsonl();
            serde_json::json!({
                "name": f.name,
                "file": format!("{}.jsonl", f.name),
                "committed": f.committed,
                "sessions": f.sessions(),
                "events": f.events.len(),
                "bytes": text.len(),
                "fnv1a64": crate::digest(text.as_bytes()),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "format": LOG_FORMAT,
        "version": SCHEMA_VERSION,
        "revision": SCHEMA_REVISION,
        "generator": "cargo xtask fixtures",
        "fixtures": entries,
    });
    let mut out = serde_json::to_string_pretty(&doc).expect("manifest serializes");
    out.push('\n');
    out
}

/// Shorthands for fixture scripts.
pub(crate) mod build {
    use crate::recorder::{Ctx, Recorder};
    use harness_core::*;

    /// An interactive session for `app` on `preset`.
    pub fn interactive(r: &mut Recorder, app: &str, preset: &str, profile: Profile) -> Ctx {
        r.session(
            SessionStart {
                class: SessionClass::Interactive,
                app: AppId::from(app),
                principal: Principal {
                    kind: PrincipalKind::User,
                    id: None,
                },
                preset: PresetRef {
                    id: PresetId::from(preset),
                    version: 1,
                },
                parent: None,
                profile: Some(profile),
                model: None,
            },
            "local:small",
        )
    }

    /// A tool call.
    pub fn call(id: &str, tool: &ToolSpec, input: Value, batch: Option<u32>) -> ToolCall {
        ToolCall {
            call: CallId::from(id),
            tool: tool.name.clone(),
            kind: tool.kind.clone(),
            input,
            input_raw: None,
            effects: tool.effects.clone(),
            batch,
            child: None,
        }
    }

    /// A tool result.
    pub fn result(call: &str, outcome: ToolOutcome, content: Vec<Block>) -> ToolResult {
        ToolResult {
            meta: None,
            call: CallId::from(call),
            outcome,
            content,
            error: None,
            truncated: None,
            duration_ms: None,
            display: None,
        }
    }

    /// One text block.
    pub fn text(s: &str) -> Vec<Block> {
        vec![Block::text(s)]
    }

    /// A decider.
    pub fn by(scope: DeciderScope, app: Option<&str>) -> Decider {
        Decider {
            scope,
            app: app.map(AppId::from),
        }
    }
}
