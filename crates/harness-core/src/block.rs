//! Content blocks: the parts of user input, model replies and tool results.

use crate::ids::BlobId;
use crate::value::Value;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// One block of content. Unknown block types from a newer revision
/// deserialize as [`Block::Unknown`] and render generically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    /// Plain text (Markdown for model replies).
    Text {
        /// The text.
        text: String,
    },
    /// Model reasoning, shown apart from the answer and never sent back to
    /// the model.
    Reasoning {
        /// The reasoning text.
        text: String,
        /// How long the model reasoned.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elapsed_ms: Option<u64>,
    },
    /// A file the person attached.
    Attachment(Attachment),
    /// A mention of a file, connection, tool or other item in scope.
    Mention {
        /// What kind of item, such as `file` or `connection`.
        target: String,
        /// The item's id.
        id: String,
        /// The label shown.
        label: String,
    },
    /// Structured data, typically in a tool result.
    Json {
        /// The data.
        value: Value,
    },
    /// A block type from a newer schema revision.
    #[serde(untagged)]
    Unknown(UnknownBlock),
}

impl Block {
    /// A text block.
    pub fn text(text: impl Into<String>) -> Self {
        Block::Text { text: text.into() }
    }
}

/// An attachment. Its extracted text is in the log, so a request that
/// included it can be rebuilt from the log alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// The file in the blob store.
    pub blob: BlobId,
    /// The file name.
    pub name: String,
    /// The media type, such as `text/markdown`.
    pub media_type: String,
    /// The file size.
    pub bytes: u64,
    /// The text extracted from it, when the model sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// The block type names schema v1 defines.
pub const BLOCK_TYPES: &[&str] = &["text", "reasoning", "attachment", "mention", "json"];

/// A block whose type this build does not know.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnknownBlock {
    /// The block's type name.
    #[serde(rename = "type")]
    pub kind: UnknownBlockType,
    /// Its other fields.
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

/// The type name of an unknown block. Deserializing one fails for a known
/// name, so a malformed known block is an error, never "unknown".
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct UnknownBlockType(pub String);

impl<'de> Deserialize<'de> for UnknownBlockType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        if BLOCK_TYPES.contains(&name.as_str()) {
            return Err(serde::de::Error::custom(format!(
                "malformed {name:?} block"
            )));
        }
        Ok(Self(name))
    }
}
