// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Model history, derived from the log alone.
//!
//! [`request_for`] rebuilds the request a step made from the events before
//! it. The loop builds every request with the same function, so replaying a
//! log reproduces each request byte for byte.
//!
//! The rules (see `docs/schema.md`):
//! - the system prompt is the latest `system(preset)`; a turn's
//!   `system(context)` goes in front of that turn's user message;
//! - only `reply` steps contribute; reasoning never does;
//! - attempts, and turns that ended `rejected` or `refused`, are left out;
//! - a reply with `continues` extends the last assistant message;
//! - tool results follow their assistant message in call order;
//! - `compacted` trims results to head and tail and replaces summarized
//!   turns with one summary message.

use crate::model::*;
use harness_core::*;
use std::collections::{BTreeMap, BTreeSet};

/// The system prompt of a title step.
pub const TITLE_PROMPT: &str =
    "Write a short title, at most six words, for the conversation below. Reply with the title only.";
/// The system prompt of a summary step.
pub const SUMMARY_PROMPT: &str =
    "Summarize the conversation so far in a few sentences. Keep names, decisions and open questions.";
/// The closing instruction of a summary step.
pub const SUMMARY_ASK: &str = "Summarize the conversation above.";

/// Why a request could not be derived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryError {
    /// No `step.start` with this id.
    NoSuchStep(StepId),
}

impl core::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HistoryError::NoSuchStep(s) => write!(f, "no step {s}"),
        }
    }
}

impl std::error::Error for HistoryError {}

/// A fork's full history: the parent's events through the fork point, then
/// the child's own. `parent` must itself be resolved if it is a fork.
pub fn resolve_fork(parent: &[SessionEvent], child: &[SessionEvent]) -> Vec<SessionEvent> {
    let point = child.iter().find_map(|e| match &e.event {
        Event::Forked(f) => Some((f.from.clone(), f.at_seq)),
        _ => None,
    });
    let Some((from, at)) = point else {
        return child.to_vec();
    };
    let end = parent
        .iter()
        .position(|e| e.session == from && e.seq == at)
        .map_or(parent.len(), |i| i + 1);
    parent[..end].iter().chain(child).cloned().collect()
}

/// The request `step` made, rebuilt from `events` (a resolved history).
pub fn request_for(events: &[SessionEvent], step: &StepId) -> Result<ModelRequest, HistoryError> {
    let idx = events
        .iter()
        .position(|e| e.step.as_ref() == Some(step) && matches!(e.event, Event::StepStart(_)))
        .ok_or_else(|| HistoryError::NoSuchStep(step.clone()))?;
    let Event::StepStart(ss) = &events[idx].event else {
        unreachable!("position matched a step.start")
    };
    let prefix = &events[..idx];
    let turn = events[idx].turn.as_ref();
    let base = |system: String, messages: Vec<Message>| ModelRequest {
        model: ss.model.clone(),
        params: ss.params.clone(),
        system,
        messages,
        tools: Vec::new(),
        prefill: false,
        privacy: Vec::new(),
    };
    let mut req = match ss.purpose {
        Purpose::Title => base(TITLE_PROMPT.into(), title_messages(prefix)),
        Purpose::Summary => {
            let mut messages = history(prefix, turn);
            messages.push(Message {
                role: Role::User,
                content: vec![Part::Text {
                    text: SUMMARY_ASK.into(),
                }],
            });
            base(SUMMARY_PROMPT.into(), messages)
        }
        _ => {
            let mut req = base(system_prompt(prefix), history(prefix, None));
            req.tools = events[..=idx]
                .iter()
                .rev()
                .find_map(|e| match &e.event {
                    Event::StepStart(s) if s.purpose == Purpose::Reply => s.tools.clone(),
                    _ => None,
                })
                .unwrap_or_default();
            req.prefill = is_continue_start(prefix, turn);
            req
        }
    };
    let withheld = matches!(
        ss.memory_policy,
        Some(MemoryPolicy::Withheld | MemoryPolicy::Off)
    );
    if !withheld && req.messages.iter().any(|m| contains_memory(&m.content)) {
        req.privacy.push(PrivacyTag::ContainsMemory);
    }
    Ok(req)
}

fn contains_memory(parts: &[Part]) -> bool {
    parts.iter().any(|p| match p {
        Part::MemoryRef { .. } => true,
        Part::ToolResult { content, .. } => contains_memory(content),
        _ => false,
    })
}

/// The heuristic title: the first prompt's opening words (at most eight).
pub fn heuristic_title(events: &[SessionEvent]) -> String {
    let first = events.iter().find_map(|e| match &e.event {
        Event::User(u) => Some(texts(&u.content)),
        _ => None,
    });
    let words: Vec<&str> = first
        .as_deref()
        .unwrap_or("")
        .split_whitespace()
        .take(8)
        .collect();
    words
        .join(" ")
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
        .to_owned()
}

fn system_prompt(prefix: &[SessionEvent]) -> String {
    prefix
        .iter()
        .rev()
        .find_map(|e| match &e.event {
            Event::System(s) if s.origin == SystemOrigin::Preset => Some(s.text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn is_continue_start(prefix: &[SessionEvent], turn: Option<&TurnId>) -> bool {
    let Some(turn) = turn else { return false };
    let mut cause = None;
    for e in prefix.iter().filter(|e| e.turn.as_ref() == Some(turn)) {
        match &e.event {
            Event::TurnStart(t) => cause = Some(t.cause.clone()),
            Event::StepStart(s) if s.purpose == Purpose::Reply => return false,
            _ => {}
        }
    }
    cause == Some(Cause::Continue)
}

fn texts(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn title_messages(prefix: &[SessionEvent]) -> Vec<Message> {
    let purposes = purposes(prefix);
    let first_user = prefix.iter().find_map(|e| match &e.event {
        Event::User(u) => Some(texts(&u.content)),
        _ => None,
    });
    let first_reply = prefix.iter().find_map(|e| match &e.event {
        Event::Assistant(a) if in_reply(e, &purposes) => Some(texts(&a.content)),
        _ => None,
    });
    let mut out = Vec::new();
    for (role, text) in [(Role::User, first_user), (Role::Assistant, first_reply)] {
        if let Some(text) = text {
            out.push(Message {
                role,
                content: vec![Part::Text { text }],
            });
        }
    }
    out
}

fn purposes(events: &[SessionEvent]) -> BTreeMap<StepId, Purpose> {
    events
        .iter()
        .filter_map(|e| match (&e.event, &e.step) {
            (Event::StepStart(s), Some(id)) => Some((id.clone(), s.purpose.clone())),
            _ => None,
        })
        .collect()
}

fn in_reply(e: &SessionEvent, purposes: &BTreeMap<StepId, Purpose>) -> bool {
    e.step
        .as_ref()
        .is_none_or(|s| purposes.get(s) == Some(&Purpose::Reply))
}

/// The conversation as model messages. `skip` leaves out one turn (the
/// current turn of a summary step).
pub fn history(events: &[SessionEvent], skip: Option<&TurnId>) -> Vec<Message> {
    let purposes = purposes(events);
    let mut dropped: BTreeSet<TurnId> = BTreeSet::new();
    let mut summarized: BTreeSet<TurnId> = BTreeSet::new();
    let mut summary = None;
    let mut trims: BTreeMap<CallId, (u64, u64)> = BTreeMap::new();
    for e in events {
        match &e.event {
            Event::TurnEnd(t) if matches!(t.stop, TurnStop::Rejected | TurnStop::Refused) => {
                dropped.extend(e.turn.clone());
            }
            Event::Compacted(c) => {
                for t in &c.trimmed {
                    trims.insert(t.call.clone(), (t.head_bytes, t.tail_bytes));
                }
                if let Some(s) = &c.summary {
                    summarized.extend(s.turns.iter().cloned());
                    summary = Some(s.text.clone());
                }
            }
            _ => {}
        }
    }
    if let Some(t) = skip {
        dropped.insert(t.clone());
    }

    let mut msgs: Vec<Message> = Vec::new();
    let mut turn_user: BTreeMap<TurnId, usize> = BTreeMap::new();
    let mut step_msg: BTreeMap<StepId, usize> = BTreeMap::new();
    let mut call_order: BTreeMap<CallId, usize> = BTreeMap::new();
    let mut results: Vec<(StepId, Vec<(usize, Part)>)> = Vec::new();
    let mut summary_placed = false;
    let flush = |msgs: &mut Vec<Message>,
                 results: &mut Vec<(StepId, Vec<(usize, Part)>)>,
                 step: Option<&StepId>| {
        let mut keep = Vec::new();
        for (s, mut parts) in results.drain(..) {
            if step.is_none_or(|x| x == &s) {
                parts.sort_by_key(|(n, _)| *n);
                msgs.push(Message {
                    role: Role::Tool,
                    content: parts.into_iter().map(|(_, p)| p).collect(),
                });
            } else {
                keep.push((s, parts));
            }
        }
        *results = keep;
    };

    for e in events {
        if let Some(t) = &e.turn {
            if dropped.contains(t) {
                continue;
            }
            if summarized.contains(t) {
                if !summary_placed {
                    if let Some(text) = &summary {
                        msgs.push(Message {
                            role: Role::User,
                            content: vec![Part::Summary { text: text.clone() }],
                        });
                    }
                    summary_placed = true;
                }
                continue;
            }
        }
        if !in_reply(e, &purposes) {
            continue;
        }
        match &e.event {
            Event::User(u) => {
                msgs.push(Message {
                    role: Role::User,
                    content: parts(&u.content),
                });
                if let Some(t) = &e.turn {
                    turn_user.insert(t.clone(), msgs.len() - 1);
                }
            }
            Event::System(s) if s.origin == SystemOrigin::Context => {
                let part = Part::Context {
                    text: s.text.clone(),
                };
                match e.turn.as_ref().and_then(|t| turn_user.get(t)) {
                    Some(&i) => msgs[i].content.insert(0, part),
                    None => {
                        msgs.push(Message {
                            role: Role::User,
                            content: vec![part],
                        });
                        if let Some(t) = &e.turn {
                            turn_user.insert(t.clone(), msgs.len() - 1);
                        }
                    }
                }
            }
            Event::Assistant(a) => {
                let new = parts(&a.content);
                let last = msgs.iter().rposition(|m| m.role == Role::Assistant);
                let target = match (a.continues, last) {
                    (Some(_), Some(i)) => {
                        for p in new {
                            match (msgs[i].content.last_mut(), p) {
                                (Some(Part::Text { text }), Part::Text { text: more }) => {
                                    text.push_str(&more)
                                }
                                (_, p) => msgs[i].content.push(p),
                            }
                        }
                        i
                    }
                    _ => {
                        msgs.push(Message {
                            role: Role::Assistant,
                            content: new,
                        });
                        msgs.len() - 1
                    }
                };
                if let Some(s) = &e.step {
                    step_msg.insert(s.clone(), target);
                }
            }
            Event::ToolCall(c) => {
                let i = match e.step.as_ref().and_then(|s| step_msg.get(s)) {
                    Some(&i) => i,
                    None => {
                        msgs.push(Message {
                            role: Role::Assistant,
                            content: vec![],
                        });
                        msgs.len() - 1
                    }
                };
                msgs[i].content.push(Part::ToolCall {
                    call: c.call.clone(),
                    tool: c.tool.clone(),
                    input: c.input.clone(),
                });
                let n = call_order.len();
                call_order.insert(c.call.clone(), n);
            }
            Event::ToolResult(r) => {
                let mut content = parts(&r.content);
                if let Some(&(head, tail)) = trims.get(&r.call) {
                    content = vec![Part::Text {
                        text: trim_text(&texts(&r.content), head, tail),
                    }];
                }
                let part = Part::ToolResult {
                    call: r.call.clone(),
                    outcome: r.outcome.clone(),
                    content,
                };
                let n = call_order.get(&r.call).copied().unwrap_or(usize::MAX);
                let step = e.step.clone().unwrap_or_else(|| StepId::from(""));
                match results.iter_mut().find(|(s, _)| *s == step) {
                    Some((_, list)) => list.push((n, part)),
                    None => results.push((step, vec![(n, part)])),
                }
            }
            Event::StepEnd(_) => flush(&mut msgs, &mut results, e.step.as_ref()),
            _ => {}
        }
    }
    flush(&mut msgs, &mut results, None);
    msgs
}

fn parts(blocks: &[Block]) -> Vec<Part> {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(Part::Text { text: text.clone() }),
            Block::Attachment(a) => Some(Part::Attachment {
                name: a.name.clone(),
                media_type: a.media_type.clone(),
                text: a.text.clone().unwrap_or_default(),
            }),
            Block::Mention { target, id, label } => Some(Part::Mention {
                target: target.clone(),
                id: id.clone(),
                label: label.clone(),
            }),
            Block::Json { value } => Some(Part::Json {
                value: value.clone(),
            }),
            Block::MemoryRef(m) => Some(Part::MemoryRef {
                id: m.id.clone(),
                revision: m.revision,
                hash: m.hash.clone(),
            }),
            Block::Reasoning { .. } | Block::Unknown(_) => None,
        })
        .collect()
}

/// Keep the first `head` and last `tail` bytes of `text` (on character
/// boundaries) with a marker between.
pub fn trim_text(text: &str, head: u64, tail: u64) -> String {
    let len = text.len();
    let (head, tail) = (head as usize, tail as usize);
    if head + tail >= len {
        return text.to_owned();
    }
    let mut h = head;
    while !text.is_char_boundary(h) {
        h -= 1;
    }
    let mut t = len - tail;
    while !text.is_char_boundary(t) {
        t += 1;
    }
    format!(
        "{}\n[… {} bytes trimmed …]\n{}",
        &text[..h],
        t - h,
        &text[t..]
    )
}
