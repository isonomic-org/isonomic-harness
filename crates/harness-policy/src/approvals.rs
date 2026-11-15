//! Pending approvals, across sessions.

use harness_core::*;
use std::collections::BTreeMap;

/// An approval waiting for a decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingApproval {
    /// The session.
    pub session: SessionId,
    /// The session's owner app.
    pub owner: AppId,
    /// The request as recorded.
    pub request: ApprovalRequest,
    /// When it was requested.
    pub requested_at: Millis,
}

/// Every pending approval of a host.
#[derive(Clone, Debug, Default)]
pub struct Approvals {
    pending: BTreeMap<ApprovalId, PendingApproval>,
}

impl Approvals {
    /// None pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// An approval was requested.
    pub fn add(&mut self, p: PendingApproval) {
        self.pending.insert(p.request.approval.clone(), p);
    }

    /// One pending approval.
    pub fn get(&self, id: &ApprovalId) -> Option<&PendingApproval> {
        self.pending.get(id)
    }

    /// Remove an approval once decided.
    pub fn take(&mut self, id: &ApprovalId) -> Option<PendingApproval> {
        self.pending.remove(id)
    }

    /// Pending approvals visible to an owner (`None` for all).
    pub fn list(&self, owner: Option<&AppId>) -> Vec<PendingApproval> {
        self.pending
            .values()
            .filter(|p| owner.is_none_or(|o| &p.owner == o))
            .cloned()
            .collect()
    }

    /// Approvals past their expiry at `now`, removed.
    pub fn expired(&mut self, now: Millis) -> Vec<PendingApproval> {
        let due: Vec<ApprovalId> = self
            .pending
            .iter()
            .filter(|(_, p)| p.request.expires_at.is_some_and(|t| t <= now))
            .map(|(id, _)| id.clone())
            .collect();
        due.iter()
            .filter_map(|id| self.pending.remove(id))
            .collect()
    }

    /// Drop every approval of these sessions (cancel, purge).
    pub fn drop_sessions(&mut self, sessions: &[SessionId]) -> Vec<PendingApproval> {
        let ids: Vec<ApprovalId> = self
            .pending
            .iter()
            .filter(|(_, p)| sessions.contains(&p.session))
            .map(|(id, _)| id.clone())
            .collect();
        ids.iter()
            .filter_map(|id| self.pending.remove(id))
            .collect()
    }
}
