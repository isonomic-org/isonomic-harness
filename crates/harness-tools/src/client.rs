//! The client-tool lifecycle: calls run by an app that may go away.
//!
//! When the app is gone its calls get a deadline (the session is *awaiting
//! client*); a call still open at its deadline times out as a readable
//! failure; when the app comes back, deadlines clear.

use crate::ToolDone;
use harness_core::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    app: AppId,
    tool: String,
    deadline: Option<Millis>,
}

/// Open client calls.
#[derive(Clone, Debug, Default)]
pub struct ClientCalls {
    pending: BTreeMap<CallId, Pending>,
}

impl ClientCalls {
    /// No open calls.
    pub fn new() -> Self {
        Self::default()
    }

    /// A call was sent to `app`.
    pub fn start(&mut self, call: CallId, app: AppId, tool: String) {
        self.pending.insert(
            call,
            Pending {
                app,
                tool,
                deadline: None,
            },
        );
    }

    /// `app` went away: its open calls time out at `now + timeout_ms`.
    /// Returns them.
    pub fn gone(&mut self, app: &AppId, now: Millis, timeout_ms: u64) -> Vec<CallId> {
        let mut out = Vec::new();
        for (call, p) in self.pending.iter_mut().filter(|(_, p)| &p.app == app) {
            p.deadline.get_or_insert(now + timeout_ms);
            out.push(call.clone());
        }
        out
    }

    /// `app` came back: deadlines clear. Returns its open calls.
    pub fn back(&mut self, app: &AppId) -> Vec<CallId> {
        let mut out = Vec::new();
        for (call, p) in self.pending.iter_mut().filter(|(_, p)| &p.app == app) {
            p.deadline = None;
            out.push(call.clone());
        }
        out
    }

    /// The call finished. Returns false if it was not open (a late result).
    pub fn finish(&mut self, call: &CallId) -> bool {
        self.pending.remove(call).is_some()
    }

    /// Calls past their deadline, removed, each with its timeout result.
    pub fn expired(&mut self, now: Millis) -> Vec<(CallId, ToolDone)> {
        let due: Vec<CallId> = self
            .pending
            .iter()
            .filter(|(_, p)| p.deadline.is_some_and(|d| d <= now))
            .map(|(c, _)| c.clone())
            .collect();
        due.into_iter()
            .filter_map(|c| self.pending.remove(&c).map(|p| (c, p)))
            .map(|(c, p)| {
                let mut done = ToolDone::timeout(&format!(
                    "{} did not run: the app that runs it is not connected. Nothing was changed.",
                    p.tool
                ));
                done.error = Some(ErrorInfo::new("client.gone", "The app is not connected."));
                (c, done)
            })
            .collect()
    }

    /// The earliest deadline, if any.
    pub fn next_deadline(&self) -> Option<Millis> {
        self.pending.values().filter_map(|p| p.deadline).min()
    }

    /// Whether any call waits on an app that is gone.
    pub fn awaiting(&self) -> bool {
        self.pending.values().any(|p| p.deadline.is_some())
    }

    /// Every open call.
    pub fn open(&self) -> Vec<CallId> {
        self.pending.keys().cloned().collect()
    }
}
