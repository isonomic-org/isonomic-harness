//! Session classes. The class sets defaults for policy, budgets and wake-up;
//! it never forks the loop.

use serde::{Deserialize, Serialize};

/// The class of a session.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionClass {
    /// A person drives it: Chat and in-app assistants. Built in Milestone 0.
    Interactive,
    /// Woken by triggers and channels; may have a Cell. Refused by the host
    /// in Milestone 0.
    Autonomous,
    /// A subagent of another session. Refused by the host in Milestone 0.
    Delegated,
    /// A class from a newer schema revision.
    #[serde(untagged)]
    Other(String),
}
