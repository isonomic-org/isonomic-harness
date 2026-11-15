//! The live channel: token deltas while a reply streams. Not stored, lossy,
//! separate from durable events. The durable `assistant` event that follows
//! is the truth; a consumer drops its deltas when that event arrives.

use crate::ids::{Seq, SessionId, StepId, TurnId};
use serde::{Deserialize, Serialize};

/// One streamed piece of a reply in progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveDelta {
    /// The session.
    pub session: SessionId,
    /// The turn.
    pub turn: TurnId,
    /// The step whose reply is streaming.
    pub step: StepId,
    /// The last durable event before this delta.
    pub after_seq: Seq,
    /// The index of the block being written, as in the final `assistant`.
    pub block: u32,
    /// What is streaming.
    pub kind: DeltaKind,
    /// The appended text.
    pub text: String,
}

/// What a delta appends to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeltaKind {
    /// Answer text.
    Text,
    /// Reasoning text.
    Reasoning,
    /// A tool call's input, as raw JSON text.
    ToolInput,
    /// A kind from a newer revision.
    #[serde(untagged)]
    Other(String),
}
