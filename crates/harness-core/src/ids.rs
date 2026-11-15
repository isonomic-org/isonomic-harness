//! Ids. Every id is an opaque string chosen by the host; consumers compare
//! ids and never parse them.

use serde::{Deserialize, Serialize};

/// A per-session sequence number. The first event of a session is 1; each
/// next event is one more. A subscriber that has seen nothing resumes from 0.
pub type Seq = u64;

/// Milliseconds since the Unix epoch, UTC.
pub type Millis = u64;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Wrap a string as this id.
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// The id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_owned())
            }
        }
    };
}

id!(
    /// A session.
    SessionId
);
id!(
    /// A turn within a session.
    TurnId
);
id!(
    /// A step (one model request plus the tool calls it makes).
    StepId
);
id!(
    /// A tool call.
    CallId
);
id!(
    /// An approval request.
    ApprovalId
);
id!(
    /// An ask-user question.
    QuestionId
);
id!(
    /// An artifact; its versions are numbered from 1.
    ArtifactId
);
id!(
    /// A blob in the engine's content store (attachments, artifact bodies).
    BlobId
);
id!(
    /// An app, as resolved from its capability token.
    AppId
);
id!(
    /// A preset.
    PresetId
);
