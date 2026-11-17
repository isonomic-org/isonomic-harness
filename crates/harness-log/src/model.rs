// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the harness sends to a model and what comes back.
//!
//! A [`ModelRequest`] is derived from the log alone and has one canonical
//! byte encoding ([`ModelRequest::bytes`]). Replay compares those bytes.

use harness_core::future::fnv1a64;
use harness_core::*;
use serde::{Deserialize, Serialize};

/// One request to a model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    /// The model id.
    pub model: String,
    /// Request parameters.
    pub params: Params,
    /// The system prompt.
    pub system: String,
    /// The conversation.
    pub messages: Vec<Message>,
    /// Tools offered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    /// The last message is an assistant prefix the model continues.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prefill: bool,
    /// Privacy tags the model gateway routes on (`contains_memory`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub privacy: Vec<PrivacyTag>,
}

impl ModelRequest {
    /// The canonical encoding.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("request serializes")
    }

    /// A digest of the canonical encoding.
    pub fn digest(&self) -> String {
        fnv1a64(&self.bytes())
    }

    /// A rough token estimate (four bytes a token).
    pub fn estimate_tokens(&self) -> u64 {
        (self.bytes().len() as u64).div_ceil(4)
    }
}

/// One message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Who speaks.
    pub role: Role,
    /// What they say.
    pub content: Vec<Part>,
}

/// A message's speaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The person (or a parent session, or a wake-up).
    User,
    /// The model.
    Assistant,
    /// Tool results.
    Tool,
}

/// A part of a message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// Request context the context link added to this turn.
    Context {
        /// The text.
        text: String,
    },
    /// A summary standing in for compacted turns.
    Summary {
        /// The text.
        text: String,
    },
    /// An attachment's extracted text.
    Attachment {
        /// File name.
        name: String,
        /// Media type.
        media_type: String,
        /// Extracted text.
        text: String,
    },
    /// A mention.
    Mention {
        /// What kind of item.
        target: String,
        /// Its id.
        id: String,
        /// Its label.
        label: String,
    },
    /// Structured data.
    Json {
        /// The data.
        value: Value,
    },
    /// A memory revision, by reference. A resolver turns it into the
    /// memory's text; until then the reference itself is sent.
    MemoryRef {
        /// The memory.
        id: MemoryId,
        /// Its revision.
        revision: u32,
        /// Its salted hash.
        hash: String,
    },
    /// A tool call the model made.
    ToolCall {
        /// The call.
        call: CallId,
        /// The tool.
        tool: String,
        /// Its input.
        input: Value,
    },
    /// A tool call's result.
    ToolResult {
        /// The call.
        call: CallId,
        /// How it ended.
        outcome: ToolOutcome,
        /// What the model sees.
        content: Vec<Part>,
    },
}

/// What a model streams back.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelEvent {
    /// Answer text.
    Text(String),
    /// Reasoning text.
    Reasoning(String),
    /// A tool call, input as raw JSON text.
    ToolCall {
        /// The tool.
        tool: String,
        /// Raw JSON input.
        input: String,
    },
    /// The model refused; the text explains.
    Refusal(String),
    /// The reply is complete.
    Done {
        /// Why it stopped.
        stop: ModelStop,
        /// Tokens used.
        usage: Usage,
    },
}

/// Why a model stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelStop {
    /// It finished.
    End,
    /// It called tools.
    ToolUse,
    /// It hit the output limit.
    MaxTokens,
}
