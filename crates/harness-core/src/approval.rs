// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Approval and question payloads: what the person is asked and what they
//! answered.

use crate::access::{PreviewItem, Scope};
use crate::error::is_false;
use crate::ids::*;
use crate::kinds::*;
use serde::{Deserialize, Serialize};

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
    /// What the call touches, for a kit tool (revision 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// Labelled values the engine worked out, never cut short (revision 2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview: Vec<PreviewItem>,
    /// Whether the engine can undo it (revision 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reversible: Option<bool>,
    /// The longest the person may be offered to remember the answer
    /// (revision 2). Absent means once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember: Option<Remember>,
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
    /// How long the person chose to remember an allow, within what the
    /// request offered (revision 2). Absent means once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember: Option<Remember>,
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
