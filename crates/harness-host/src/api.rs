//! Driving sessions: send, continue, cancel, approvals, questions, client
//! tools, subscriptions and purge (`docs/host-api.md`).

use crate::events::Subscription;
use crate::exec::lock;
use crate::host::{Host, Slot};
use crate::types::*;
use harness_core::future::BoxFuture;
use harness_core::*;
use harness_loop::Input as LoopInput;
use harness_policy::{interpret, may_decide, DecideError, PendingApproval};
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;

type Fut<'a, T> = BoxFuture<'a, Result<T, HostError>>;

use crate::sessions::refused;

impl Host {
    /// Send input; a turn opens when it is claimed.
    pub fn send(&self, caller: &Caller, id: &SessionId, input: Input) -> Fut<'_, Accepted> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.owned(&caller, &id).await?;
            for b in &input.content {
                if let Block::Attachment(a) = b {
                    let blob = me
                        .deps
                        .blobs
                        .get(&a.blob)
                        .await
                        .map_err(|_| HostError::NotFound)?;
                    if blob.owner != slot.app {
                        return Err(HostError::NotFound);
                    }
                }
            }
            let queued = lock(&slot.lp).running();
            let context = me
                .deps
                .locale
                .context(&slot.app, &id, me.now(), slot.location_opt_in);
            let outs = me
                .apply(
                    &slot,
                    LoopInput::User {
                        content: input.content,
                        context: Some(context),
                    },
                )
                .await;
            match refused(&outs) {
                Some(e) => Err(e),
                None => Ok(Accepted { queued }),
            }
        })
    }

    /// Continue the last sealed partial reply.
    pub fn continue_reply(&self, caller: &Caller, id: &SessionId) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.owned(&caller, &id).await?;
            if !me.deps.model.caps(&slot.cfg.preset.model.id).prefill {
                return Err(HostError::ContinueRefused(
                    "The model cannot continue a reply.".into(),
                ));
            }
            let outs = me.apply(&slot, LoopInput::Continue).await;
            refused(&outs).map_or(Ok(()), Err)
        })
    }

    /// Stop the running turn. Idempotent.
    pub fn cancel(&self, caller: &Caller, id: &SessionId) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.load(&caller, &id).await?;
            me.apply(&slot, LoopInput::Cancel).await;
            Ok(())
        })
    }

    /// Pending approvals the caller may see.
    pub fn list_approvals(&self, caller: &Caller) -> Fut<'_, Vec<PendingApproval>> {
        let (me, caller) = (self.0.clone(), caller.clone());
        Box::pin(async move { Ok(lock(&me.state).approvals.list(caller.scope.app())) })
    }

    /// Decide an approval. Needs a verified user-action token; Views and
    /// third-party code never decide; an ambiguous answer is a denial.
    pub fn decide_approval(
        &self,
        caller: &Caller,
        id: &ApprovalId,
        decision: DecisionInput,
        token: Option<UserActionToken>,
    ) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let pending = {
                let st = lock(&me.state);
                match st.approvals.get(&id) {
                    Some(p) => p.clone(),
                    None if st.decided.contains(&id) => return Err(HostError::AlreadyDecided),
                    None => return Err(HostError::NotFound),
                }
            };
            let verified = token
                .as_ref()
                .is_some_and(|t| me.deps.scopes.verify_user_action(&caller, t, &id));
            may_decide(&caller.scope, &pending.owner, token.as_ref(), verified).map_err(
                |e| match e {
                    DecideError::NotFound => HostError::NotFound,
                    DecideError::NotAllowedToDecide => HostError::NotAllowedToDecide,
                    DecideError::MissingUserAction => HostError::MissingUserAction,
                },
            )?;
            let slot = me.slot(&pending.session).ok_or(HostError::NotFound)?;
            let (decision, basis, answer) = interpret(&decision);
            let d = ApprovalDecision {
                approval: id,
                decision,
                basis,
                by: caller.decider(),
                answer,
                reason: None,
            };
            me.apply(&slot, LoopInput::Decision(d)).await;
            Ok(())
        })
    }

    /// Answer an ask-user question.
    pub fn answer_question(
        &self,
        caller: &Caller,
        id: &QuestionId,
        answer: AnswerInput,
    ) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let session = lock(&me.state)
                .questions
                .get(&id)
                .cloned()
                .ok_or(HostError::NotFound)?;
            let slot = me.load(&caller, &session).await?;
            let a = Answer {
                question: id,
                selected: answer.selected,
                text: answer.text,
                skipped: answer.skipped,
                by: caller.decider(),
            };
            me.apply(&slot, LoopInput::Answer(a)).await;
            Ok(())
        })
    }

    /// Register the owner app's client tools for a session.
    pub fn register_client_tools(
        &self,
        caller: &Caller,
        id: &SessionId,
        specs: Vec<ToolSpec>,
    ) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.owned(&caller, &id).await?;
            let specs = {
                let mut r = lock(&slot.registry);
                r.register_client(&slot.app, specs)
                    .map_err(|e| HostError::InvalidTools(format!("{e:?}")))?;
                let mut s = r.specs();
                if !slot.cfg.preset.tools.client {
                    s.retain(|t| t.kind != ToolKind::Client);
                }
                s
            };
            me.apply(&slot, LoopInput::Tools(specs)).await;
            Ok(())
        })
    }

    /// A client tool's result, from the app that registered it.
    pub fn client_tool_result(
        &self,
        caller: &Caller,
        call: &CallId,
        outcome: ClientOutcome,
    ) -> Fut<'_, ()> {
        let (me, caller, call) = (self.0.clone(), caller.clone(), call.clone());
        Box::pin(async move {
            let session = lock(&me.state).client_calls.get(&call).cloned();
            let Some(session) = session else {
                let slots: Vec<Arc<Slot>> = lock(&me.state).sessions.values().cloned().collect();
                if let Some(s) = slots.iter().find(|s| caller.scope.app() == Some(&s.app)) {
                    lock(&me.state).audit.push(AuditRecord {
                        session: s.id.clone(),
                        app: s.app.clone(),
                        seq: 0,
                        at: me.now(),
                        kind: "tool.result.late".into(),
                        call: Some(call),
                        detail: "A result after the call closed was dropped.".into(),
                    });
                }
                return Ok(());
            };
            let slot = me.owned(&caller, &session).await?;
            let done = match outcome {
                ClientOutcome::Ok(content) => harness_tools::ToolDone {
                    outcome: ToolOutcome::Ok,
                    content,
                    error: None,
                    truncated: None,
                    artifacts: Vec::new(),
                },
                ClientOutcome::Error(err, content) => harness_tools::ToolDone {
                    outcome: ToolOutcome::Error,
                    content,
                    error: Some(err),
                    truncated: None,
                    artifacts: Vec::new(),
                },
            };
            me.apply(
                &slot,
                LoopInput::Tool {
                    call,
                    output: harness_tools::ToolOutput::Done(done),
                },
            )
            .await;
            Ok(())
        })
    }

    /// Subscribe to a session's events from `from_seq` (exclusive).
    pub fn subscribe(
        &self,
        caller: &Caller,
        id: &SessionId,
        from_seq: Seq,
    ) -> Fut<'_, Subscription> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.load(&caller, &id).await?;
            let feed = slot.hub.join(me.cfg.subscriber_queue);
            Ok(Subscription::new(me.deps.store.clone(), id, from_seq, feed))
        })
    }

    /// An app deletes all of its own sessions: in-flight turns stop first,
    /// then logs and blobs go. Nothing else is touched.
    pub fn purge(&self, caller: &Caller) -> Fut<'_, PurgeReport> {
        let (me, caller) = (self.0.clone(), caller.clone());
        Box::pin(async move {
            let Scope::App { app, .. } = &caller.scope else {
                return Err(HostError::Unauthorized);
            };
            let metas = me.deps.store.list_meta(Some(app)).await?;
            let ids: Vec<SessionId> = metas.iter().map(|m| m.id.clone()).collect();
            for id in &ids {
                if let Some(slot) = me.slot(id) {
                    if lock(&slot.lp).running() {
                        me.apply(&slot, LoopInput::Cancel).await;
                    }
                    slot.failed.store(true, SeqCst);
                    for t in lock(&slot.tokens).values() {
                        t.cancel();
                    }
                }
            }
            let events = me.deps.store.purge(&ids).await?;
            let blobs = me.deps.blobs.delete_for(&ids).await?;
            let mut st = lock(&me.state);
            for id in &ids {
                if let Some(slot) = st.sessions.remove(id) {
                    slot.hub.close();
                }
            }
            st.approvals.drop_sessions(&ids);
            st.questions.retain(|_, s| !ids.contains(s));
            st.client_calls.retain(|_, s| !ids.contains(s));
            Ok(PurgeReport {
                sessions: ids.len() as u64,
                events,
                blobs,
            })
        })
    }
}
