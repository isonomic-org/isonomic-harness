//! Keeping host state in step with the log (approvals, questions, audit,
//! budgets), and the watchdog: faults, hangs and deadlines.

use crate::exec::lock;
use crate::host::{Inner, Slot};
use crate::traits::*;
use crate::types::*;
use harness_core::*;
use harness_loop::Input as LoopInput;
use harness_policy::{Meter, PendingApproval};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;

impl Inner {
    /// Keep the registry, approvals, questions, audit and meters in step
    /// with the log. Returns a budget hit, if one ran out.
    pub(crate) fn observe(&self, slot: &Arc<Slot>, e: &SessionEvent) -> Option<BudgetHit> {
        {
            let mut m = lock(&slot.meta);
            m.updated_at = e.at;
            match &e.event {
                Event::Title(t) => m.title = Some(t.title.clone()),
                Event::SessionStatus(s) => m.status = s.status.clone(),
                _ => {}
            }
        }
        let audit = |kind: &str, call: Option<CallId>, detail: String| AuditRecord {
            session: slot.id.clone(),
            app: slot.app.clone(),
            seq: e.seq,
            at: e.at,
            kind: kind.into(),
            call,
            detail,
        };
        let mut st = lock(&self.state);
        match &e.event {
            Event::ApprovalRequest(r) => {
                st.approvals.add(PendingApproval {
                    session: slot.id.clone(),
                    owner: slot.app.clone(),
                    request: r.clone(),
                    requested_at: e.at,
                });
                st.audit
                    .push(audit("approval.request", r.call.clone(), r.summary.clone()));
            }
            Event::ApprovalDecision(d) => {
                st.approvals.take(&d.approval);
                st.decided.insert(d.approval.clone());
                let detail = format!("{:?} ({:?})", d.decision, d.basis).to_lowercase();
                st.audit.push(audit("approval.decision", None, detail));
            }
            Event::Question(q) => {
                st.questions.insert(q.question.clone(), slot.id.clone());
            }
            Event::Answer(a) => {
                st.questions.remove(&a.question);
            }
            Event::ToolCall(c) => {
                st.audit
                    .push(audit("tool.call", Some(c.call.clone()), c.tool.clone()))
            }
            Event::ToolResult(r) => {
                st.client_calls.remove(&r.call);
                let detail = format!("{:?}", r.outcome).to_lowercase();
                st.audit
                    .push(audit("tool.result", Some(r.call.clone()), detail));
            }
            Event::StepEnd(s) => {
                let usage = s.usage?;
                let now = e.at;
                let app = st
                    .app_meters
                    .entry(slot.app.clone())
                    .or_insert_with(|| Meter::start(now));
                app.add(&usage);
                let app_hit = app.check(&self.cfg.app_limits, now);
                drop(st);
                let mut m = lock(&slot.meter);
                m.add(&usage);
                return m.check(&self.cfg.session_limits, now).or(app_hit);
            }
            _ => {}
        }
        None
    }

    /// Faults, hangs and deadlines.
    pub(crate) fn tick(self: &Arc<Self>) {
        let now = self.now();
        let (faults, slots) = {
            let mut st = lock(&self.state);
            let faults: Vec<(SessionId, Fault)> = st
                .handles
                .iter()
                .filter(|h| h.finished())
                .filter_map(|h| h.fault().map(|f| (h.session.clone(), f)))
                .collect();
            st.handles.retain(|h| !h.finished());
            let slots: Vec<Arc<Slot>> = st.sessions.values().cloned().collect();
            (faults, slots)
        };
        for (id, Fault::Panic(m)) in faults {
            if let Some(s) = self.slot(&id) {
                let err = ErrorInfo::new("task.panic", format!("Session work failed: {m}"));
                self.feed(&s, LoopInput::Fail(err));
            }
        }
        let waiting: std::collections::BTreeSet<SessionId> = {
            let st = lock(&self.state);
            st.approvals
                .list(None)
                .into_iter()
                .map(|p| p.session)
                .chain(st.questions.values().cloned())
                .chain(st.client_calls.values().cloned())
                .collect()
        };
        for s in &slots {
            if s.failed.load(SeqCst) || !lock(&s.lp).running() {
                continue;
            }
            if waiting.contains(&s.id) {
                let last = s.last_progress.load(SeqCst);
                self.feed(s, LoopInput::Tick);
                s.last_progress.store(last.max(now), SeqCst);
                continue;
            }
            let hang = s.cfg.preset.policy.hang_ms;
            if now.saturating_sub(s.last_progress.load(SeqCst)) > hang {
                let err = ErrorInfo::new(
                    "loop.hang",
                    "The session stopped responding and was stopped.",
                );
                self.feed(s, LoopInput::Fail(err));
                s.failed.store(true, SeqCst);
            } else {
                // Deadlines do not count as progress.
                let last = s.last_progress.load(SeqCst);
                self.feed(s, LoopInput::Tick);
                s.last_progress.store(last, SeqCst);
            }
        }
        for s in slots {
            let me = self.clone();
            self.deps
                .spawner
                .spawn(Box::pin(async move { me.flush(&s).await }));
        }
    }
}
