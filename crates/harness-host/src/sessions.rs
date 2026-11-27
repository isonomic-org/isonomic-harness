// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Session calls: create, list, open, fork, archive, rename; and loading
//! sessions from the store. Every call checks the caller's scope first;
//! another app's ids answer `NotFound`.

use crate::exec::lock;
use crate::host::{Host, Inner, Slot};
use crate::types::*;
use harness_core::future::BoxFuture;
use harness_core::*;
use harness_log::SessionMeta;
use harness_loop::{Input as LoopInput, Output, Refusal, Session};
use harness_policy::Meter;
use harness_preset::Preset;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

type Fut<'a, T> = BoxFuture<'a, Result<T, HostError>>;

fn summary(m: &SessionMeta, last_seq: Seq) -> SessionSummary {
    SessionSummary {
        id: m.id.clone(),
        app: m.app.clone(),
        class: m.class.clone(),
        preset: m.preset.clone(),
        title: m.title.clone(),
        status: m.status.clone(),
        archived: m.archived,
        forked_from: m.forked_from.clone(),
        created_at: m.created_at,
        updated_at: m.updated_at,
        last_seq,
        model: m.model.clone(),
    }
}

fn last_seq(slot: &Slot) -> Seq {
    let lp = lock(&slot.lp);
    lp.events()
        .iter()
        .rev()
        .find(|e| e.session == slot.id)
        .map_or(0, |e| e.seq)
}

impl Inner {
    pub(crate) fn preset(
        &self,
        owner: &AppId,
        id: &PresetId,
        version: Option<u32>,
    ) -> Option<StoredPreset> {
        let st = lock(&self.state);
        let pick = |list: &Vec<StoredPreset>| match version {
            Some(v) => list.iter().find(|p| p.reference.version == v).cloned(),
            None => list.last().cloned(),
        };
        st.presets
            .get(&(Some(owner.clone()), id.clone()))
            .and_then(pick)
            .or_else(|| st.presets.get(&(None, id.clone())).and_then(pick))
    }

    pub(crate) fn new_slot(
        &self,
        meta: SessionMeta,
        lp: Session,
        cfg: harness_loop::Config,
        opt_in: bool,
        preset: &Preset,
    ) -> Arc<Slot> {
        let now = self.now();
        let mut registry = self.registry_for(preset, &meta.app);
        registry.retain_named(&preset.tools.allow);
        Arc::new(Slot {
            id: meta.id.clone(),
            app: meta.app.clone(),
            meta: Mutex::new(meta),
            lp: Mutex::new(lp),
            cfg,
            location_opt_in: opt_in,
            outbox: Mutex::new(VecDeque::new()),
            flushing: AtomicBool::new(false),
            hub: Default::default(),
            gen: AtomicU64::new(0),
            tokens: Mutex::default(),
            last_progress: AtomicU64::new(now),
            failed: AtomicBool::new(false),
            inject_panic: AtomicBool::new(false),
            inject_hang: AtomicBool::new(false),
            registry: Mutex::new(registry),
            meter: Mutex::new(Meter::start(now)),
        })
    }

    /// `cfg` for the model the session runs on: the picked model's name,
    /// if there is one, and the context size of whichever model it is, so
    /// compaction fits the model's real window and not only the preset's
    /// budget.
    pub(crate) fn model_config(
        &self,
        cfg: harness_loop::Config,
        model: Option<&str>,
    ) -> harness_loop::Config {
        let cfg = cfg.with_model(model);
        let context = self.deps.model.caps(&cfg.preset.model.id).context_tokens;
        cfg.with_context(context)
    }

    pub(crate) fn loop_config(
        &self,
        preset: &Preset,
        app: &AppId,
        profile: Option<Profile>,
        limits: Option<harness_policy::Limits>,
    ) -> harness_loop::Config {
        let specs = self.registry_for(preset, app).specs();
        let mut cfg = harness_loop::Config::new(preset.clone(), profile, specs);
        if let Some(l) = limits {
            cfg.limits = cfg.limits.tighten(&l);
        }
        cfg
    }

    /// A session the caller may see, loading it from the store if needed.
    pub(crate) async fn load(
        self: &Arc<Self>,
        caller: &Caller,
        id: &SessionId,
    ) -> Result<Arc<Slot>, HostError> {
        let slot = match self.slot(id) {
            Some(s) => s,
            None => {
                let metas = self.deps.store.list_meta(None).await?;
                let meta = metas
                    .into_iter()
                    .find(|m| &m.id == id)
                    .ok_or(HostError::NotFound)?;
                if !caller.scope.sees(&meta.app) {
                    return Err(HostError::NotFound);
                }
                let events = self.history(&meta).await?;
                // The log says what the session runs on: profile and model.
                let (profile, model) = events
                    .iter()
                    .filter(|e| &e.session == id)
                    .find_map(|e| match &e.event {
                        Event::SessionStart(s) => Some((s.profile.clone(), s.model.clone())),
                        _ => None,
                    })
                    .unwrap_or_default();
                let stored = self
                    .preset(&meta.app, &meta.preset.id, Some(meta.preset.version))
                    .or_else(|| self.preset(&meta.app, &PresetId::from("chat"), None));
                let preset = stored.and_then(|p| p.preset).ok_or(HostError::NotFound)?;
                let model = harness_loop::picked_model(model.as_deref());
                let cfg = self.model_config(
                    self.loop_config(&preset, &meta.app, profile, None),
                    model.as_deref(),
                );
                // The log is the source for the model; a registry row that
                // disagrees (written before the field, or edited) follows it.
                let meta = if meta.model == model {
                    meta
                } else {
                    let meta = SessionMeta { model, ..meta };
                    self.deps.store.put_meta(&meta).await?;
                    meta
                };
                let (lp, outs) = Session::resume(cfg.clone(), id.clone(), events, self.now());
                let failed = lp.failed();
                let slot = self.new_slot(meta, lp, cfg, false, &preset);
                slot.failed.store(failed, SeqCst);
                lock(&slot.outbox).extend(outs);
                let slot = lock(&self.state)
                    .sessions
                    .entry(id.clone())
                    .or_insert(slot)
                    .clone();
                self.flush(&slot).await;
                slot
            }
        };
        if !caller.scope.sees(&slot.app) {
            return Err(HostError::NotFound);
        }
        Ok(slot)
    }

    /// Like `load`, but only the owner app may drive.
    pub(crate) async fn owned(
        self: &Arc<Self>,
        caller: &Caller,
        id: &SessionId,
    ) -> Result<Arc<Slot>, HostError> {
        let slot = self.load(caller, id).await?;
        match &caller.scope {
            Scope::App { app, .. } if app == &slot.app => Ok(slot),
            Scope::App { .. } => Err(HostError::NotFound),
            Scope::Shell => Err(HostError::Unauthorized),
        }
    }
}

/// No turn is open at `seq`.
pub(crate) fn boundary(own: &[&SessionEvent], seq: Seq) -> bool {
    let mut open = 0i64;
    for e in own.iter().filter(|e| e.seq <= seq) {
        match e.event {
            Event::TurnStart(_) => open += 1,
            Event::TurnEnd(_) => open -= 1,
            _ => {}
        }
    }
    open == 0
}

/// The longest model name the host records, in bytes.
const MAX_MODEL_NAME: usize = 200;

/// A picked model's name as the host records it: trimmed; `None` when
/// blank; refused when it is too long or holds a control character.
fn picked(name: Option<&str>) -> Result<Option<String>, HostError> {
    let model = harness_loop::picked_model(name);
    match &model {
        Some(m) if m.len() > MAX_MODEL_NAME || m.chars().any(char::is_control) => {
            Err(HostError::Model(ErrorInfo::new(
                "model.name",
                "The model name is too long or not plain text.",
            )))
        }
        _ => Ok(model),
    }
}

pub(crate) fn refused(outs: &[Output]) -> Option<HostError> {
    outs.iter().find_map(|o| match o {
        Output::Refused(Refusal::SessionFailed) => Some(HostError::SessionFailed),
        Output::Refused(Refusal::ContinueRefused(r)) => Some(HostError::ContinueRefused(r.clone())),
        Output::Refused(Refusal::Busy) => {
            Some(HostError::ContinueRefused("A turn is running.".into()))
        }
        _ => None,
    })
}

impl Host {
    /// Create a session from a preset (the caller's own, or a built-in).
    pub fn create_session(
        &self,
        caller: &Caller,
        preset: &PresetId,
        opts: CreateOptions,
    ) -> Fut<'_, SessionId> {
        let (me, caller, preset) = (self.0.clone(), caller.clone(), preset.clone());
        Box::pin(async move {
            let owner = caller.owner();
            let stored = me
                .preset(&owner, &preset, None)
                .ok_or(HostError::NotFound)?;
            let p = stored
                .preset
                .ok_or(HostError::InvalidPreset(stored.diagnostics))?;
            if p.class != SessionClass::Interactive {
                return Err(HostError::ClassNotSupported(p.class.clone()));
            }
            let model = picked(opts.model.as_deref())?;
            let id = me.new_id().await?;
            let now = me.now();
            let start = SessionStart {
                class: p.class.clone(),
                app: owner.clone(),
                principal: caller.principal.clone(),
                preset: p.reference.clone(),
                parent: None,
                profile: me.profile(),
                model,
            };
            let cfg = me.model_config(
                me.loop_config(&p, &start.app, start.profile.clone(), opts.limits),
                start.model.as_deref(),
            );
            let model = start.model.clone();
            let (lp, outs) = Session::create(cfg.clone(), id.clone(), start, now);
            let meta = SessionMeta {
                id: id.clone(),
                app: owner,
                class: p.class.clone(),
                preset: p.reference.clone(),
                title: None,
                status: Status::Active,
                archived: false,
                forked_from: None,
                created_at: now,
                updated_at: now,
                model,
            };
            let slot = me.new_slot(meta, lp, cfg, opts.location_opt_in, &p);
            lock(&slot.outbox).extend(outs);
            lock(&me.state).sessions.insert(id.clone(), slot.clone());
            me.flush(&slot).await;
            if let Some(t) = opts.title {
                me.apply(&slot, LoopInput::Rename(t)).await;
            }
            Ok(id)
        })
    }

    /// Registry rows the caller may see.
    pub fn list_sessions(
        &self,
        caller: &Caller,
        filter: SessionFilter,
    ) -> Fut<'_, Page<SessionSummary>> {
        let (me, caller) = (self.0.clone(), caller.clone());
        Box::pin(async move {
            let owner = caller.scope.app().cloned();
            let mut metas = me.deps.store.list_meta(owner.as_ref()).await?;
            metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
            let text = filter.text.as_ref().map(|t| t.to_lowercase());
            let rows: Vec<SessionMeta> = metas
                .into_iter()
                .filter(|m| filter.archived.is_none_or(|a| a == m.archived))
                .filter(|m| filter.preset.as_ref().is_none_or(|p| p == &m.preset.id))
                .filter(|m| {
                    text.as_ref()
                        .is_none_or(|t| m.title.as_deref().unwrap_or("").to_lowercase().contains(t))
                })
                .collect();
            let start: usize = filter
                .cursor
                .as_deref()
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let limit = if filter.limit == 0 {
                50
            } else {
                filter.limit as usize
            };
            let items = rows
                .iter()
                .skip(start)
                .take(limit)
                .map(|m| summary(m, me.slot(&m.id).map_or(0, |s| last_seq(&s))))
                .collect();
            let next = (start + limit < rows.len()).then(|| (start + limit).to_string());
            Ok(Page { items, next })
        })
    }

    /// A session's summary and what is pending.
    pub fn open_session(&self, caller: &Caller, id: &SessionId) -> Fut<'_, SessionSnapshot> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.load(&caller, &id).await?;
            let meta = lock(&slot.meta).clone();
            let (turn_open, seq) = {
                let lp = lock(&slot.lp);
                let mut open = None;
                for e in lp.events().iter().filter(|e| e.session == id) {
                    match &e.event {
                        Event::TurnStart(_) => open = e.turn.clone(),
                        Event::TurnEnd(_) => open = None,
                        _ => {}
                    }
                }
                (
                    open,
                    lp.events()
                        .iter()
                        .rev()
                        .find(|e| e.session == id)
                        .map_or(0, |e| e.seq),
                )
            };
            let st = lock(&me.state);
            let approvals = st
                .approvals
                .list(None)
                .into_iter()
                .filter(|p| p.session == id)
                .map(|p| p.request.approval)
                .collect();
            let questions = st
                .questions
                .iter()
                .filter(|(s, _)| *s == id)
                .map(|(_, q)| q.clone())
                .collect();
            let awaiting_client = if meta.status == Status::AwaitingClient {
                st.client_calls
                    .iter()
                    .filter(|(s, _)| *s == id)
                    .map(|(_, c)| c.clone())
                    .collect()
            } else {
                Vec::new()
            };
            Ok(SessionSnapshot {
                summary: summary(&meta, seq),
                turn_open,
                approvals,
                questions,
                awaiting_client,
            })
        })
    }

    /// Fork at a turn boundary into an independent session.
    pub fn fork_session(
        &self,
        caller: &Caller,
        id: &SessionId,
        at: ForkPoint,
        reason: ForkReason,
    ) -> Fut<'_, SessionId> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.owned(&caller, &id).await?;
            let events = lock(&slot.lp).events().to_vec();
            let own: Vec<&SessionEvent> = events.iter().filter(|e| e.session == id).collect();
            let seq = match &at {
                ForkPoint::AfterTurn(t) => own
                    .iter()
                    .find(|e| e.turn.as_ref() == Some(t) && matches!(e.event, Event::TurnEnd(_)))
                    .map(|e| e.seq),
                ForkPoint::BeforeTurn(t) => own
                    .iter()
                    .find(|e| e.turn.as_ref() == Some(t) && matches!(e.event, Event::TurnStart(_)))
                    .map(|e| e.seq - 1)
                    .filter(|s| boundary(&own, *s)),
            }
            .ok_or(HostError::NotAtTurnBoundary)?;
            let end = events
                .iter()
                .position(|e| e.session == id && e.seq == seq)
                .ok_or(HostError::NotAtTurnBoundary)?;
            let prefix = &events[..=end];
            let parent_meta = lock(&slot.meta).clone();
            let start = events
                .iter()
                .find_map(|e| match (&e.event, e.session == id) {
                    (Event::SessionStart(s), true) => Some(s.clone()),
                    _ => None,
                })
                .ok_or(HostError::NotFound)?;
            let child = me.new_id().await?;
            let now = me.now();
            let forked = Forked {
                from: id.clone(),
                at_seq: seq,
                after_turn: match &at {
                    ForkPoint::AfterTurn(t) => Some(t.clone()),
                    ForkPoint::BeforeTurn(_) => None,
                },
                reason,
            };
            let (lp, outs) =
                Session::fork(slot.cfg.clone(), child.clone(), start, prefix, forked, now);
            let meta = SessionMeta {
                id: child.clone(),
                forked_from: Some((id.clone(), seq)),
                created_at: now,
                updated_at: now,
                title: parent_meta.title.clone(),
                ..parent_meta
            };
            let preset = slot.cfg.preset.clone();
            let new = me.new_slot(meta, lp, slot.cfg.clone(), slot.location_opt_in, &preset);
            lock(&new.outbox).extend(outs);
            lock(&me.state).sessions.insert(child.clone(), new.clone());
            me.flush(&new).await;
            Ok(child)
        })
    }

    /// Archive or unarchive.
    pub fn archive_session(&self, caller: &Caller, id: &SessionId, archived: bool) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.load(&caller, &id).await?;
            let meta = {
                let mut m = lock(&slot.meta);
                m.archived = archived;
                m.clone()
            };
            me.deps.store.put_meta(&meta).await?;
            Ok(())
        })
    }

    /// Rename: a `title` event with source `user`.
    pub fn rename_session(&self, caller: &Caller, id: &SessionId, title: String) -> Fut<'_, ()> {
        let (me, caller, id) = (self.0.clone(), caller.clone(), id.clone());
        Box::pin(async move {
            let slot = me.load(&caller, &id).await?;
            me.apply(&slot, LoopInput::Rename(title)).await;
            Ok(())
        })
    }
}
