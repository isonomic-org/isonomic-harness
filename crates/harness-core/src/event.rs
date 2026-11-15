//! The session event: the platform's main contract.
//!
//! An append-only log of these events is the only record of what the model
//! saw. One JSON object per event: the envelope (`session`, `seq`, `at`,
//! `turn`, `step`), then `type`, then the payload fields.

use crate::ids::{Millis, Seq, SessionId, StepId, TurnId};
use crate::memory::MemoryEvent;
use crate::value::Value;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

pub use crate::kinds::*;
pub use crate::payload::*;

/// One durable session event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionEvent {
    /// The session this event belongs to.
    pub session: SessionId,
    /// Its position in the session: 1, 2, 3, … with no gaps.
    pub seq: Seq,
    /// When it was persisted.
    pub at: Millis,
    /// The turn it belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<TurnId>,
    /// The step it belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<StepId>,
    /// The event itself.
    #[serde(flatten)]
    pub event: Event,
}

/// Every event type in schema v1, tagged by `type`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Event {
    /// The first event of every session.
    #[serde(rename = "session.start")]
    SessionStart(SessionStart),
    /// The session's status changed (awaiting client, active, failed).
    #[serde(rename = "session.status")]
    SessionStatus(SessionStatus),
    /// This session was forked from another; it inherits that log's prefix.
    #[serde(rename = "forked")]
    Forked(Forked),
    /// System content the model sees: the preset prompt or request context.
    #[serde(rename = "system")]
    System(System),
    /// A turn opened: input was claimed.
    #[serde(rename = "turn.start")]
    TurnStart(TurnStart),
    /// The person's input.
    #[serde(rename = "user")]
    User(User),
    /// A model request began.
    #[serde(rename = "step.start")]
    StepStart(StepStart),
    /// The model's reply, with the compact stream that produced it.
    #[serde(rename = "assistant")]
    Assistant(Assistant),
    /// A failed, cancelled or refused request. Never model history.
    #[serde(rename = "attempt")]
    Attempt(Attempt),
    /// The model called a tool.
    #[serde(rename = "tool.call")]
    ToolCall(ToolCall),
    /// A tool call's result. Every call gets exactly one.
    #[serde(rename = "tool.result")]
    ToolResult(ToolResult),
    /// An action needs approval.
    #[serde(rename = "approval.request")]
    ApprovalRequest(ApprovalRequest),
    /// An approval was decided.
    #[serde(rename = "approval.decision")]
    ApprovalDecision(ApprovalDecision),
    /// The model asked the person a question.
    #[serde(rename = "question")]
    Question(Question),
    /// The person answered.
    #[serde(rename = "answer")]
    Answer(Answer),
    /// An artifact was created or updated.
    #[serde(rename = "artifact")]
    Artifact(ArtifactEvent),
    /// The session's title was set; renaming is another title event.
    #[serde(rename = "title")]
    Title(Title),
    /// A model request and its tool calls finished.
    #[serde(rename = "step.end")]
    StepEnd(StepEnd),
    /// A turn closed: nothing more is owed.
    #[serde(rename = "turn.end")]
    TurnEnd(TurnEnd),
    /// Older content was trimmed or summarized.
    #[serde(rename = "compacted")]
    Compacted(Compacted),
    /// The session created, revised or deleted a memory (by reference).
    #[serde(rename = "memory")]
    Memory(MemoryEvent),
    /// An event type from a newer schema revision. Render it generically.
    #[serde(untagged)]
    Unknown(UnknownEvent),
}

/// The event type names schema v1 defines, in schema order.
pub const EVENT_TYPES: &[&str] = &[
    "session.start",
    "session.status",
    "forked",
    "system",
    "turn.start",
    "user",
    "step.start",
    "assistant",
    "attempt",
    "tool.call",
    "tool.result",
    "approval.request",
    "approval.decision",
    "question",
    "answer",
    "artifact",
    "title",
    "step.end",
    "turn.end",
    "compacted",
    "memory",
];

impl Event {
    /// The event's `type` name.
    pub fn type_name(&self) -> &str {
        match self {
            Event::SessionStart(_) => "session.start",
            Event::SessionStatus(_) => "session.status",
            Event::Forked(_) => "forked",
            Event::System(_) => "system",
            Event::TurnStart(_) => "turn.start",
            Event::User(_) => "user",
            Event::StepStart(_) => "step.start",
            Event::Assistant(_) => "assistant",
            Event::Attempt(_) => "attempt",
            Event::ToolCall(_) => "tool.call",
            Event::ToolResult(_) => "tool.result",
            Event::ApprovalRequest(_) => "approval.request",
            Event::ApprovalDecision(_) => "approval.decision",
            Event::Question(_) => "question",
            Event::Answer(_) => "answer",
            Event::Artifact(_) => "artifact",
            Event::Title(_) => "title",
            Event::StepEnd(_) => "step.end",
            Event::TurnEnd(_) => "turn.end",
            Event::Compacted(_) => "compacted",
            Event::Memory(_) => "memory",
            Event::Unknown(u) => &u.kind.0,
        }
    }
}

/// An event whose type this build does not know.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnknownEvent {
    /// The event's type name.
    #[serde(rename = "type")]
    pub kind: UnknownEventType,
    /// Its payload fields.
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

/// The type name of an unknown event. Deserializing one fails for a known
/// name, so a malformed known event is an error, never "unknown".
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct UnknownEventType(pub String);

impl<'de> Deserialize<'de> for UnknownEventType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        if EVENT_TYPES.contains(&name.as_str()) {
            return Err(serde::de::Error::custom(format!(
                "malformed {name:?} event"
            )));
        }
        Ok(Self(name))
    }
}
