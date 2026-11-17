// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Event payloads. Optional fields are left out when empty, so a log carries
//! only what happened.

use crate::block::{Block, MemoryRef};
use crate::class::SessionClass;
use crate::error::{is_false, ErrorInfo};
use crate::ids::*;
use crate::kinds::*;
use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn is_zero(n: &u64) -> bool {
    *n == 0
}

fn is_zero32(n: &u32) -> bool {
    *n == 0
}

/// `session.start`: what the session is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStart {
    /// Interactive, autonomous or delegated.
    pub class: SessionClass,
    /// The owner app.
    pub app: AppId,
    /// Who the session acts for.
    pub principal: Principal,
    /// The preset it was created from.
    pub preset: PresetRef,
    /// For a delegated session: the parent session and the call that made it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentRef>,
    /// The hardware profile, which picks the title and compaction paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// The model the person picked for this session, replacing the
    /// preset's. Absent: the preset's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Who a session acts for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// A person or an agent.
    pub kind: PrincipalKind,
    /// The agent's key id, for an agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// A preset and its version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetRef {
    /// The preset.
    pub id: PresetId,
    /// Its version.
    pub version: u32,
}

/// The parent of a delegated session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParentRef {
    /// The parent session.
    pub session: SessionId,
    /// The parent's tool call that created this session.
    pub call: CallId,
}

/// `session.status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStatus {
    /// The new status.
    pub status: Status,
    /// Why, in a short sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// For awaiting client: the calls waiting on the app.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calls: Vec<CallId>,
    /// For awaiting client: when those calls time out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<Millis>,
}

/// `forked`: the session's history starts with the parent's events up to
/// and including `at_seq`, which is always at a turn boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forked {
    /// The session forked from.
    pub from: SessionId,
    /// The last parent event shared.
    pub at_seq: Seq,
    /// The last parent turn shared, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_turn: Option<TurnId>,
    /// Edit, regenerate or another branch.
    pub reason: ForkReason,
}

/// `system`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct System {
    /// Preset, context or notice.
    pub origin: SystemOrigin,
    /// The exact text the model sees.
    pub text: String,
    /// For context: what the context link added, structured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<RequestContext>,
    /// For context: the session's memory block, by reference.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory: Vec<MemoryRef>,
    /// For context: whether memory was used, withheld or off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_policy: Option<MemoryPolicy>,
}

/// What the context link adds to a turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestContext {
    /// The person's local date, `YYYY-MM-DD`.
    pub date: String,
    /// Their time zone, such as `America/New_York`.
    pub time_zone: String,
    /// Their language, such as `en`.
    pub language: String,
    /// Their location, only when they opted in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

/// `turn.start`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStart {
    /// What opened the turn.
    pub cause: Cause,
    /// For a wake: what woke the session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake: Option<Wake>,
}

/// What woke a session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wake {
    /// Schedule, heartbeat, channel or webhook.
    pub source: WakeSource,
    /// The trigger's id.
    pub id: String,
}

/// `user`: the input that opened the turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    /// Text, attachments and mentions.
    pub content: Vec<Block>,
}

/// `step.start`: everything about the request besides history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StepStart {
    /// Reply, title or summary.
    pub purpose: Purpose,
    /// The model id.
    pub model: String,
    /// Request parameters.
    pub params: Params,
    /// The tool set, present when it differs from the last step that had one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSpec>>,
    /// Whether memory reached this request: used, withheld or off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_policy: Option<MemoryPolicy>,
}

/// Request parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Params {
    /// The output limit.
    pub max_output_tokens: u32,
    /// The reasoning level.
    pub reasoning: Reasoning,
    /// Provider options, passed through.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, Value>,
}

/// A tool as offered to the model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// The name the model calls.
    pub name: String,
    /// What it does, for the model.
    pub description: String,
    /// JSON Schema for its input.
    pub input_schema: Value,
    /// Engine, client or cell.
    pub kind: ToolKind,
    /// What it may do.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// How it runs alongside other calls.
    pub concurrency: Concurrency,
}

/// `assistant`: a reply the model completed or that was sealed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Assistant {
    /// Reasoning and text blocks. Tool calls follow as `tool.call` events.
    pub content: Vec<Block>,
    /// Why it stopped.
    pub stop: StopReason,
    /// The compact stream that produced it.
    pub stream: Vec<Chunk>,
    /// Tokens used by the request that produced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// For a continue: the `seq` of the sealed reply this one extends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continues: Option<Seq>,
}

/// One stretch of streamed output: `[block, bytes, at_ms]`. `block` indexes
/// `content`; `bytes` of UTF-8 were appended to it (always ending on a
/// character boundary); `at_ms` is the time since the step started.
/// Playing the chunks in order rebuilds the reply at its original pace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk(pub u32, pub u32, pub u32);

/// `attempt`: a request that did not produce a kept reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    /// Error, cancelled or refused.
    pub outcome: AttemptOutcome,
    /// The error, for an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    /// What streamed before it ended, or the refusal text. Display only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    /// Tokens used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Its index in the request series (0 is the first try).
    #[serde(default, skip_serializing_if = "is_zero32")]
    pub retry: u32,
    /// True when the host retries the same request next.
    #[serde(default, skip_serializing_if = "is_false")]
    pub will_retry: bool,
}

/// `tool.call`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// The call.
    pub call: CallId,
    /// The tool's name.
    pub tool: String,
    /// Engine, client or cell.
    pub kind: ToolKind,
    /// The input, parsed.
    pub input: Value,
    /// The model's raw input, only when it did not parse.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_raw: Option<String>,
    /// The tool's declared effects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// Calls with the same batch number ran concurrently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<u32>,
    /// A delegated session this call created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<SessionId>,
}

/// `tool.result`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// The call.
    pub call: CallId,
    /// How it ended.
    pub outcome: ToolOutcome,
    /// What the model sees: output, or a readable failure or denial reason.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    /// The error, for an error or timeout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    /// The tool's output limit cut the middle out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<Truncation>,
    /// How long it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Output kept from a long result: the first and last bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Truncation {
    /// The full size.
    pub original_bytes: u64,
    /// Bytes kept from the start.
    pub head_bytes: u64,
    /// Bytes kept from the end.
    pub tail_bytes: u64,
}

/// `approval.request`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// The approval.
    pub approval: ApprovalId,
    /// The tool call it gates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<CallId>,
    /// What will happen, for the person.
    pub summary: String,
    /// The rule that asked.
    pub rule: ApprovalRule,
    /// The effects at stake.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// When it times out as a denial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Millis>,
}

/// `approval.decision`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    /// The approval.
    pub approval: ApprovalId,
    /// Allow or deny.
    pub decision: Decision,
    /// What the decision rests on.
    pub basis: Basis,
    /// Who decided.
    pub by: Decider,
    /// The answer as given, when it was ambiguous.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// A reason, for the log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Who decided an approval or answered a question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decider {
    /// Shell, app or host.
    pub scope: DeciderScope,
    /// The app, for an app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<AppId>,
}

/// `question`: from the ask-user tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// The question.
    pub question: QuestionId,
    /// The ask-user call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<CallId>,
    /// What is asked.
    pub prompt: String,
    /// Choices, if any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    /// More than one choice allowed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub multiple: bool,
    /// Free text allowed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub free_text: bool,
}

/// A choice in a question.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// Its id.
    pub id: String,
    /// Its label.
    pub label: String,
}

/// `answer`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    /// The question.
    pub question: QuestionId,
    /// Chosen option ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected: Vec<String>,
    /// Free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The person declined to answer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub skipped: bool,
    /// Who answered.
    pub by: Decider,
}

/// `artifact`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEvent {
    /// The artifact.
    pub artifact: ArtifactId,
    /// This version, from 1.
    pub version: u32,
    /// Create or update.
    pub op: ArtifactOp,
    /// Its title.
    pub title: String,
    /// The body's media type.
    pub media_type: String,
    /// The body in the blob store.
    pub blob: BlobId,
    /// The body's size.
    pub bytes: u64,
    /// The tool call that wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<CallId>,
}

/// `title`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Title {
    /// The title.
    pub title: String,
    /// Model, heuristic or user.
    pub source: TitleSource,
}

/// `step.end`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepEnd {
    /// How it ended.
    pub outcome: StepOutcome,
    /// From `step.start` to now.
    pub duration_ms: u64,
    /// Tokens for every attempt in the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// `turn.end`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnEnd {
    /// How it closed.
    pub stop: TurnStop,
    /// Steps it ran (post-turn steps not counted).
    pub steps: u32,
    /// From `turn.start` to now.
    pub duration_ms: u64,
    /// Tokens for the whole turn.
    pub usage: Usage,
    /// A short sentence, for rejected, failed and budget stops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The error, for a failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
    /// The budget hit, for a budget stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget: Option<BudgetHit>,
}

/// A budget that stopped a turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetHit {
    /// Which budget.
    pub kind: BudgetKind,
    /// Its limit.
    pub limit: u64,
    /// What was used.
    pub used: u64,
}

/// `compacted`: applies to model history from here on; replay applies it at
/// the same point, so rebuilt requests stay exact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compacted {
    /// The last event the compaction covers.
    pub through_seq: Seq,
    /// Tool results trimmed to head and tail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trimmed: Vec<Trim>,
    /// Older turns replaced by a summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    /// Estimated history tokens before.
    pub tokens_before: u64,
    /// Estimated history tokens after.
    pub tokens_after: u64,
}

/// A trimmed tool result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trim {
    /// The call whose result was trimmed.
    pub call: CallId,
    /// The result's size.
    pub original_bytes: u64,
    /// Bytes kept from the start.
    pub head_bytes: u64,
    /// Bytes kept from the end.
    pub tail_bytes: u64,
}

/// A summary that replaces whole turns in model history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// The summary the model sees.
    pub text: String,
    /// The turns it replaces.
    pub turns: Vec<TurnId>,
}

/// Token use, and cost where a provider charges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt tokens.
    pub input_tokens: u64,
    /// Output tokens, reasoning included.
    pub output_tokens: u64,
    /// Of the output, reasoning tokens.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub reasoning_tokens: u64,
    /// Of the input, tokens served from cache.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cached_tokens: u64,
    /// Cost in millionths of the account currency; absent for local models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros: Option<u64>,
}

impl Usage {
    /// The sum of two usages.
    pub fn plus(self, other: Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens + other.input_tokens,
            output_tokens: self.output_tokens + other.output_tokens,
            reasoning_tokens: self.reasoning_tokens + other.reasoning_tokens,
            cached_tokens: self.cached_tokens + other.cached_tokens,
            cost_micros: match (self.cost_micros, other.cost_micros) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            },
        }
    }
}
