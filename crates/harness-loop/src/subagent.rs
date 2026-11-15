//! Subagent activation. A delegating tool asks an [`Activation`] to start a
//! child session for a task: a local child now, external execution in an
//! agent machine later. Milestone 0 ships the trait and a fake; the real
//! local child arrives with Agents.
//!
//! Delegation only narrows authority: the child gets a subset of the
//! parent's tools and limits no looser than the parent's, and depth is
//! limited.

use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use harness_policy::Limits;
use std::sync::Mutex;

/// The deepest delegation allowed anywhere, whatever a preset says.
pub const MAX_DEPTH: u32 = 4;

/// A request to start a child session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegation {
    /// The parent session and the call that delegates.
    pub parent: ParentRef,
    /// The task, as the child's first input.
    pub task: String,
    /// The child's depth (the parent's plus one).
    pub depth: u32,
    /// Tools the child may use, by name.
    pub tools: Vec<String>,
    /// The child's limits.
    pub limits: Limits,
}

/// Why a delegation is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DelegationError {
    /// Deeper than allowed.
    TooDeep {
        /// The limit.
        max: u32,
    },
    /// A tool the parent does not have.
    WidensTools(String),
    /// A limit looser than the parent's.
    WidensLimits,
    /// The activation failed.
    Failed(String),
}

/// Check that a delegation only narrows the parent's authority.
pub fn check_delegation(
    d: &Delegation,
    max_depth: u32,
    parent_tools: &[String],
    parent_limits: &Limits,
) -> Result<(), DelegationError> {
    let max = max_depth.min(MAX_DEPTH);
    if d.depth > max {
        return Err(DelegationError::TooDeep { max });
    }
    if let Some(t) = d.tools.iter().find(|t| !parent_tools.contains(t)) {
        return Err(DelegationError::WidensTools(t.clone()));
    }
    if parent_limits.tighten(&d.limits) != d.limits {
        return Err(DelegationError::WidensLimits);
    }
    Ok(())
}

/// Starts child sessions. The host implements it; the loop never awaits it.
pub trait Activation: Send + Sync {
    /// Start a child for `d`; returns the child session's id. The child's
    /// result reaches the parent as the delegating call's result.
    fn activate(&self, d: Delegation) -> BoxFuture<'_, Result<SessionId, DelegationError>>;
}

/// A fake activation: records requests and hands out ids `sub_0001`, …
#[derive(Debug, Default)]
pub struct FakeActivation {
    /// Every delegation it was asked for.
    pub requests: Mutex<Vec<Delegation>>,
}

impl Activation for FakeActivation {
    fn activate(&self, d: Delegation) -> BoxFuture<'_, Result<SessionId, DelegationError>> {
        let mut reqs = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        reqs.push(d);
        let id = SessionId::new(format!("sub_{:04}", reqs.len()));
        ready(Ok(id))
    }
}
