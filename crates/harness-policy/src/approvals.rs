// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pending approvals, across sessions. Approval ids are minted per session,
//! so an approval is named by its session and its id together.

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
    pending: BTreeMap<(SessionId, ApprovalId), PendingApproval>,
}

impl Approvals {
    /// None pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// An approval was requested.
    pub fn add(&mut self, p: PendingApproval) {
        self.pending
            .insert((p.session.clone(), p.request.approval.clone()), p);
    }

    /// One pending approval of a session.
    pub fn get(&self, session: &SessionId, id: &ApprovalId) -> Option<&PendingApproval> {
        self.pending.get(&(session.clone(), id.clone()))
    }

    /// Remove an approval once decided.
    pub fn take(&mut self, session: &SessionId, id: &ApprovalId) -> Option<PendingApproval> {
        self.pending.remove(&(session.clone(), id.clone()))
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
        self.remove_where(|p| p.request.expires_at.is_some_and(|t| t <= now))
    }

    /// Drop every approval of these sessions (cancel, purge).
    pub fn drop_sessions(&mut self, sessions: &[SessionId]) -> Vec<PendingApproval> {
        self.remove_where(|p| sessions.contains(&p.session))
    }

    fn remove_where(&mut self, gone: impl Fn(&PendingApproval) -> bool) -> Vec<PendingApproval> {
        let keys: Vec<(SessionId, ApprovalId)> = self
            .pending
            .iter()
            .filter(|(_, p)| gone(p))
            .map(|(k, _)| k.clone())
            .collect();
        keys.iter().filter_map(|k| self.pending.remove(k)).collect()
    }
}
