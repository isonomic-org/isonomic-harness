// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Kit calls: the resolving state between waiting and the verdict, and the
//! approval a verdict asks for (`docs/kits.md`). A resolve failure is the
//! call's result and never an approval request.

use crate::machine::Session;
use crate::tools::State;
use crate::*;
use harness_core::access::{Access, GrantAnswer};
use harness_policy::{rememberable, Resolved, Verdict};

impl Session {
    /// Ask the host to resolve a kit call.
    pub(crate) fn resolve(&mut self, id: CallId, rec: ToolCall, spec: ToolSpec) {
        if let Some(c) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) {
            c.state = State::Resolving;
        }
        self.out.push(Output::Resolve { call: rec, spec });
    }

    /// The host resolved a kit call: judge it on its access and grant.
    pub(crate) fn resolved(&mut self, id: CallId, result: Result<(Access, GrantAnswer), ToolDone>) {
        let open = self
            .calls_mut()
            .and_then(|c| c.calls.get(&id))
            .filter(|c| c.state == State::Resolving)
            .map(|c| (c.record.clone(), c.spec.clone()));
        let Some((rec, Some(spec))) = open else {
            self.out.push(Output::Refused(Refusal::NotOpen));
            return;
        };
        let (access, grant) = match result {
            Ok(pair) => pair,
            Err(done) => return self.finish(&id, done, false),
        };
        let policy = self.cfg.preset.policy.policy.clone();
        let resolved = Resolved {
            access: &access,
            grant: &grant,
        };
        let verdict = policy.judge(&spec, &self.flags, Some(resolved));
        if let Some(c) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) {
            c.access = Some(access);
            c.state = State::Waiting;
        }
        self.judged(id, &rec, &spec, verdict);
    }

    /// Act on a verdict: run, refuse, or ask the person.
    pub(crate) fn judged(&mut self, id: CallId, rec: &ToolCall, spec: &ToolSpec, verdict: Verdict) {
        let rule = match verdict {
            Verdict::Allow => return self.run(id),
            Verdict::Deny(reason) => return self.finish(&id, ToolDone::denied(&reason), false),
            Verdict::Ask(rule) => rule,
        };
        let access = self
            .calls_mut()
            .and_then(|c| c.calls.get(&id))
            .and_then(|c| c.access.clone());
        let approval = ApprovalId::new(self.next_id("apv"));
        let expires = self.now + self.cfg.preset.policy.approval_timeout_ms;
        let step = self.calls_mut().map(|c| c.step.clone());
        let effects = Access::narrow(access.as_ref(), &spec.effects);
        let remember = access.as_ref().map(|a| rememberable(&rule, Some(a)));
        self.emit(
            step,
            Event::ApprovalRequest(ApprovalRequest {
                approval: approval.clone(),
                call: Some(id.clone()),
                summary: summary(rec),
                rule,
                effects,
                expires_at: Some(expires),
                scope: access.as_ref().map(|a| a.scope.clone()),
                preview: access
                    .as_ref()
                    .map(|a| a.preview.clone())
                    .unwrap_or_default(),
                reversible: access.as_ref().map(|a| a.reversible),
                remember,
            }),
        );
        if let Some(c) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) {
            c.state = State::Approval(approval, Some(expires));
        }
    }
}

/// What the person sees on an approval: the tool and its input, short.
fn summary(rec: &ToolCall) -> String {
    let input = serde_json::to_string(&rec.input).unwrap_or_default();
    let mut s = format!("{} {}", rec.tool, input);
    if s.len() > 200 {
        let mut cut = 197;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}
