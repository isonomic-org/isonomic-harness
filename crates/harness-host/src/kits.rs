// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Kit tools in the host (`docs/kits.md`): the three offer filters
//! (capability, grants, budget), resolving a call and its grant check, the
//! second check at run, remembered approvals, and the linked folders on the
//! context record.

use crate::exec::{lock, CatchUnwind};
use crate::host::{Inner, Slot};
use harness_core::access::{grant_effect, GrantAnswer, Scope};
use harness_core::offer::{admit, offer, KitCandidate};
use harness_core::*;
use harness_loop::Input as LoopInput;
use harness_tools::{Handler, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

fn no_grant(scope: &Scope) -> GrantAnswer {
    GrantAnswer::Deny(format!("No permission covers {}.", scope.label))
}

impl Inner {
    /// The kit tools a new session of `app` is offered, by name: the
    /// preset's kits, kept to what the app's grants could cover and the
    /// capability table's budget. Interactive sessions only: host backends
    /// are not confined.
    pub(crate) fn kit_tools(
        &self,
        preset: &harness_preset::Preset,
        app: &AppId,
        session: &SessionId,
        profile: Option<&Profile>,
        model: Option<&str>,
    ) -> Vec<String> {
        let Some(grants) = &self.deps.grants else {
            return Vec::new();
        };
        if preset.kits.is_empty() || preset.class != SessionClass::Interactive {
            return Vec::new();
        }
        let model = model.unwrap_or(&preset.model.id);
        let budget = offer(profile, model, self.deps.model.caps(model).tools).kit_tools;
        if budget == 0 {
            return Vec::new();
        }
        let tools: Vec<_> = self
            .cfg
            .tools
            .iter()
            .filter(|t| t.offered(app))
            .filter_map(|t| t.kit().map(|k| (t.spec(), k)))
            .filter(|(s, _)| s.scope.is_some())
            .collect();
        let candidates: Vec<KitCandidate<'_>> = tools
            .iter()
            .map(|(spec, k)| KitCandidate {
                spec,
                kit: k.kit,
                core: k.core,
            })
            .collect();
        admit(
            &preset.kits,
            &candidates,
            &grants.held(app, session),
            budget,
        )
    }

    /// The result cap for a session offering `tools`: the capability
    /// table's, when any kit tool is offered.
    pub(crate) fn result_cap(&self, cfg: &harness_loop::Config) -> Option<usize> {
        if !cfg.tools.iter().any(|t| t.scope.is_some()) {
            return None;
        }
        let model = &cfg.preset.model.id;
        let caps = self.deps.model.caps(model);
        offer(cfg.profile.as_ref(), model, caps.tools).result_cap
    }

    /// The tool's act on `scope`, checked with the grants.
    fn check(&self, slot: &Slot, spec: &ToolSpec, scope: &Scope) -> GrantAnswer {
        let effect = grant_effect(spec.scope.as_ref(), &spec.effects);
        match (&self.deps.grants, effect) {
            (Some(g), Some(e)) => g.check(&slot.app, &slot.id, &e, scope),
            _ => no_grant(scope),
        }
    }

    /// Resolve a kit call: the tool's access, then the grant check, back to
    /// the loop. A failure is the call's result.
    pub(crate) fn resolve_call(self: &Arc<Self>, slot: &Arc<Slot>, call: ToolCall, spec: ToolSpec) {
        let tool = match lock(&slot.registry)
            .get(&spec.name)
            .map(|e| e.handler.clone())
        {
            Some(Handler::Engine(t)) => t,
            _ => {
                let msg = format!("{} cannot run here.", spec.name);
                let result = Err(ToolDone::error("kit.unavailable", &msg));
                self.feed(
                    slot,
                    LoopInput::Resolved {
                        call: call.call,
                        result,
                    },
                );
                return;
            }
        };
        let ctx = self.tool_ctx(slot, &call);
        let (me, s) = (self.clone(), slot.clone());
        let task = async move {
            let resolved = match tool.access(&call.input, &ctx) {
                None => None,
                Some(f) => CatchUnwind(Box::pin(f)).await.ok(),
            };
            let result = match resolved {
                None => {
                    let msg = format!("{} could not work out what it would touch.", spec.name);
                    Err(ToolDone::error("kit.no_access", &msg))
                }
                Some(Err(done)) => Err(done),
                Some(Ok(access)) if Some(&access.scope.kind) != spec.scope.as_ref() => {
                    let msg = format!("{} resolved to the wrong kind of scope.", spec.name);
                    Err(ToolDone::error("kit.scope", &msg))
                }
                Some(Ok(access)) => {
                    let grant = me.check(&s, &spec, &access.scope);
                    // A denied call never runs, so its scope is not kept.
                    if !matches!(grant, GrantAnswer::Deny(_)) {
                        lock(&s.scopes).insert(call.call.clone(), access.scope.clone());
                    }
                    Ok((access, grant))
                }
            };
            let input = LoopInput::Resolved {
                call: call.call.clone(),
                result,
            };
            me.apply(&s, input).await;
        };
        let h = self.deps.executor.run(slot.id.clone(), Box::pin(task));
        lock(&self.state).handles.push(h);
    }

    /// The check again, as the call runs: a grant revoked since the verdict
    /// refuses it. Only a denial refuses: a grant switched to "ask first"
    /// after the verdict does not ask again, because the verdict already
    /// allowed the call or the person already answered it.
    pub(crate) fn recheck(&self, slot: &Slot, call: &CallId, spec: &ToolSpec) -> Option<ToolDone> {
        spec.scope.as_ref()?;
        let Some(scope) = lock(&slot.scopes).remove(call) else {
            let msg = format!("{} was not resolved before it ran.", spec.name);
            return Some(ToolDone::error("kit.unresolved", &msg));
        };
        match self.check(slot, spec, &scope) {
            GrantAnswer::Deny(reason) => Some(ToolDone::error("grant.revoked", &reason)),
            _ => None,
        }
    }

    /// What an engine tool knows about the call it serves.
    pub(crate) fn tool_ctx(&self, slot: &Slot, call: &ToolCall) -> ToolCtx {
        ToolCtx {
            session: slot.id.clone(),
            app: slot.app.clone(),
            call: call.call.clone(),
            now: self.now(),
            child: call.child.clone(),
            result_cap: slot.result_cap,
        }
    }

    /// A person allowed `request` for longer than once, with a verified
    /// token: the grants remember it for its effect and scope.
    pub(crate) fn remember(
        &self,
        session: &SessionId,
        owner: &AppId,
        request: &ApprovalRequest,
        how: &Remember,
    ) {
        let (Some(g), Some(scope)) = (&self.deps.grants, &request.scope) else {
            return;
        };
        if let Some(effect) = grant_effect(Some(&scope.kind), &request.effects) {
            g.remember(owner, session, &effect, scope, how);
        }
    }

    /// The linked folders' names, for the context record, when the session
    /// offers a folder tool.
    pub(crate) fn folders(&self, slot: &Slot) -> Vec<String> {
        let folder_tool = slot
            .cfg
            .tools
            .iter()
            .any(|t| t.scope == Some(ScopeKind::Folder));
        let Some(g) = self.deps.grants.as_ref().filter(|_| folder_tool) else {
            return Vec::new();
        };
        let mut names: Vec<String> = Vec::new();
        for h in g.held(&slot.app, &slot.id) {
            if h.scope.kind == ScopeKind::Folder && !names.contains(&h.scope.label) {
                names.push(h.scope.label);
            }
        }
        names
    }

    /// Purged sessions take their remembered approvals with them.
    pub(crate) fn forget_grants(&self, sessions: &[SessionId]) {
        if let Some(g) = &self.deps.grants {
            g.forget(sessions);
        }
    }
}

/// A refusal at run, as the call's output.
pub(crate) fn refused(done: ToolDone) -> ToolOutput {
    ToolOutput::Done(done)
}
