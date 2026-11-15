//! Errors.

use serde::{Deserialize, Serialize};

/// A structured error recorded in the log (attempts, tool results, turn ends).
/// Readable by the model and by people.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    /// A stable, machine-readable code, such as `model.overloaded`.
    pub code: String,
    /// A short sentence.
    pub message: String,
    /// True when retrying the same request may succeed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub transient: bool,
}

impl ErrorInfo {
    /// A permanent error.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            transient: false,
        }
    }

    /// A transient error.
    pub fn transient(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            transient: true,
            ..Self::new(code, message)
        }
    }
}

/// Errors from harness-core itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A log header names another format.
    Format(String),
    /// A log was written by a newer schema version.
    FutureVersion {
        /// The version found.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Format(found) => write!(f, "not a session log: format {found:?}"),
            Error::FutureVersion { found, supported } => write!(
                f,
                "session log version {found} is newer than this build reads ({supported})"
            ),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn is_false(b: &bool) -> bool {
    !*b
}
