// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host core: session slots, feeding the loop, persist-then-publish,
//! dispatch of model requests and tools, faults, the watchdog and budgets.

use crate::events::Hub;
use crate::exec::{lock, CatchUnwind};
use crate::traits::*;
use crate::types::*;
use harness_core::future::{next, BoxFuture};
use harness_core::*;
use harness_log::{ModelEvent, SessionMeta};
use harness_loop::{Input as LoopInput, Output, Session};
use harness_policy::{Approvals, Limits, Meter};
use harness_preset::Ceiling;
use harness_tools::{Handler, Registry, Tool};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

/// Everything the engine supplies.
#[derive(Clone)]
pub struct Deps {
    /// The event log and registry rows.
    pub store: Arc<dyn SessionStore>,
    /// Models.
    pub model: Arc<dyn ModelClient>,
    /// Client tool delivery.
    pub clients: Arc<dyn ClientToolChannel>,
    /// Tokens to callers.
    pub scopes: Arc<dyn ScopeResolver>,
    /// Attachments and artifacts.
    pub blobs: Arc<dyn BlobStore>,
    /// Background work.
    pub spawner: Arc<dyn Spawner>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Session work, with faults reported.
    pub executor: Arc<dyn Executor>,
    /// The person's locale.
    pub locale: Arc<dyn Locale>,
    /// Grants, for the tool kits (`docs/kits.md`); `None` offers no kit
    /// tool.
    pub grants: Option<Arc<dyn harness_toolkit::Grants>>,
}

/// Host settings.
#[derive(Clone)]
pub struct HostConfig {
    /// The most an app's preset may allow.
    pub ceiling: Ceiling,
    /// Limits per session, across its turns.
    pub session_limits: Limits,
    /// Limits per app, across its sessions.
    pub app_limits: Limits,
    /// Engine tools; a preset offers those it names.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Each subscriber's live queue.
    pub subscriber_queue: usize,
    /// How often the watchdog looks, in milliseconds.
    pub watchdog_ms: u64,
    /// The hardware profile sessions run under.
    pub profile: Option<Profile>,
}

pub(crate) struct State {
    pub(crate) sessions: BTreeMap<SessionId, Arc<Slot>>,
    pub(crate) presets: BTreeMap<(Option<AppId>, PresetId), Vec<StoredPreset>>,
    pub(crate) approvals: Approvals,
    // Approval, question and call ids are minted per session, so each
    // index is keyed by (session, id).
    pub(crate) decided: BTreeSet<(SessionId, ApprovalId)>,
    pub(crate) questions: BTreeSet<(SessionId, QuestionId)>,
    pub(crate) client_calls: BTreeSet<(SessionId, CallId)>,
    pub(crate) audit: Vec<AuditRecord>,
    pub(crate) handles: Vec<RunHandle>,
    pub(crate) app_meters: BTreeMap<AppId, Meter>,
}

pub(crate) struct Slot {
    pub(crate) id: SessionId,
    pub(crate) app: AppId,
    pub(crate) meta: Mutex<SessionMeta>,
    pub(crate) lp: Mutex<Session>,
    pub(crate) cfg: harness_loop::Config,
    pub(crate) location_opt_in: bool,
    pub(crate) outbox: Mutex<VecDeque<Output>>,
    pub(crate) flushing: AtomicBool,
    pub(crate) hub: Hub,
    pub(crate) gen: AtomicU64,
    pub(crate) tokens: Mutex<BTreeMap<StepId, CancelToken>>,
    pub(crate) last_progress: AtomicU64,
    pub(crate) failed: AtomicBool,
    pub(crate) inject_panic: AtomicBool,
    pub(crate) inject_hang: AtomicBool,
    pub(crate) registry: Mutex<Registry>,
    pub(crate) meter: Mutex<Meter>,
    /// The scope each resolved kit call touches, until it runs.
    pub(crate) scopes: Mutex<BTreeMap<CallId, harness_core::access::Scope>>,
    /// The largest kit result, from the capability table.
    pub(crate) result_cap: Option<usize>,
}

pub(crate) struct Inner {
    pub(crate) deps: Deps,
    pub(crate) cfg: HostConfig,
    pub(crate) state: Mutex<State>,
    pub(crate) ids: AtomicU64,
    /// Every session id in the store at first mint, and each one minted
    /// since; never reused, even after a purge.
    pub(crate) minted: Mutex<Option<BTreeSet<SessionId>>>,
}

/// The host. Cheap to clone; every clone is the same host.
#[derive(Clone)]
pub struct Host(pub(crate) Arc<Inner>);

impl Host {
    /// Build the host over the engine's traits, with the built-in presets,
    /// and start the watchdog.
    pub fn new(deps: Deps, cfg: HostConfig) -> Self {
        let mut presets = BTreeMap::new();
        for (text, parsed) in harness_preset::builtins() {
            if let Some(p) = parsed.preset.clone() {
                presets.insert(
                    (None, p.reference.id.clone()),
                    vec![StoredPreset {
                        reference: p.reference.clone(),
                        owner: None,
                        text: text.to_owned(),
                        preset: Some(p),
                        diagnostics: parsed.diagnostics,
                    }],
                );
            }
        }
        let inner = Arc::new(Inner {
            deps,
            cfg,
            state: Mutex::new(State {
                sessions: BTreeMap::new(),
                presets,
                approvals: Approvals::new(),
                decided: BTreeSet::new(),
                questions: BTreeSet::new(),
                client_calls: BTreeSet::new(),
                audit: Vec::new(),
                handles: Vec::new(),
                app_meters: BTreeMap::new(),
            }),
            ids: AtomicU64::new(0),
            minted: Mutex::new(None),
        });
        let watch = Arc::downgrade(&inner);
        let sleep_ms = inner.cfg.watchdog_ms.max(1);
        let spawner = inner.deps.spawner.clone();
        inner.deps.spawner.spawn(Box::pin(async move {
            loop {
                spawner.sleep(sleep_ms).await;
                let Some(me) = watch.upgrade() else { return };
                me.tick();
            }
        }));
        Host(inner)
    }

    /// The schema version and revision, for the SDK handshake.
    pub fn schema(&self) -> (u32, u32) {
        (SCHEMA_VERSION, SCHEMA_REVISION)
    }

    /// Run the watchdog and deadlines once (the background task does this
    /// every `watchdog_ms`).
    pub fn tick(&self) {
        self.0.tick();
    }
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

impl Inner {
    pub(crate) fn now(&self) -> Millis {
        self.deps.clock.now_ms()
    }

    pub(crate) fn profile(&self) -> Option<Profile> {
        self.cfg.profile.clone()
    }

    pub(crate) fn slot(&self, id: &SessionId) -> Option<Arc<Slot>> {
        lock(&self.state).sessions.get(id).cloned()
    }

    /// Hand one input to a session's loop; its outputs go to the outbox.
    pub(crate) fn feed(self: &Arc<Self>, slot: &Arc<Slot>, input: LoopInput) -> Vec<Output> {
        if slot.failed.load(SeqCst) && !matches!(input, LoopInput::Tick | LoopInput::Rename(_)) {
            return vec![Output::Refused(harness_loop::Refusal::SessionFailed)];
        }
        let now = self.now();
        slot.last_progress.store(now, SeqCst);
        let boom = slot.inject_panic.swap(false, SeqCst);
        let res = {
            let mut lp = lock(&slot.lp);
            catch_unwind(AssertUnwindSafe(|| {
                if boom {
                    panic!("injected panic");
                }
                lp.handle(now, input)
            }))
        };
        match res {
            Ok(outs) => {
                lock(&slot.outbox).extend(outs.iter().cloned());
                outs
            }
            Err(p) => {
                self.panicked(slot, panic_text(p));
                Vec::new()
            }
        }
    }

    /// Feed, then persist and dispatch.
    pub(crate) async fn apply(self: &Arc<Self>, slot: &Arc<Slot>, input: LoopInput) -> Vec<Output> {
        let outs = self.feed(slot, input);
        self.flush(slot).await;
        outs
    }

    /// A loop panicked: rebuild it from the log with its turn sealed as
    /// failed, and mark the session failed. Other sessions never notice.
    fn panicked(self: &Arc<Self>, slot: &Arc<Slot>, msg: String) {
        slot.failed.store(true, SeqCst);
        let me = self.clone();
        let s = slot.clone();
        self.deps.spawner.spawn(Box::pin(async move {
            me.flush(&s).await;
            let meta = lock(&s.meta).clone();
            let Ok(events) = me.history(&meta).await else {
                return;
            };
            let err = ErrorInfo::new("loop.panic", format!("The session's loop failed: {msg}"));
            let (lp, outs) = Session::fail(s.cfg.clone(), s.id.clone(), events, me.now(), err);
            *lock(&s.lp) = lp;
            lock(&s.outbox).extend(outs);
            me.flush(&s).await;
        }));
    }

    /// The resolved history of a session: a fork's parent prefix, then its own.
    pub(crate) fn history<'a>(
        self: &'a Arc<Self>,
        meta: &'a SessionMeta,
    ) -> BoxFuture<'a, Result<Vec<SessionEvent>, HostError>> {
        Box::pin(async move {
            let mut own = Vec::new();
            loop {
                let after = own.last().map_or(0, |e: &SessionEvent| e.seq);
                let page = self.deps.store.read(&meta.id, after, 1024).await?;
                if page.is_empty() {
                    break;
                }
                own.extend(page);
            }
            let Some((parent, _)) = &meta.forked_from else {
                return Ok(own);
            };
            let metas = self.deps.store.list_meta(None).await?;
            let Some(pm) = metas.into_iter().find(|m| &m.id == parent) else {
                return Ok(own);
            };
            let parent_events = self.history(&pm).await?;
            Ok(harness_log::resolve_fork(&parent_events, &own))
        })
    }

    /// Persist the outbox in order, publishing each event after the store
    /// accepts it, and run side effects after the events before them.
    pub(crate) fn flush<'a>(self: &'a Arc<Self>, slot: &'a Arc<Slot>) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if slot.flushing.swap(true, SeqCst) {
                return;
            }
            loop {
                let batch: Vec<Output> = lock(&slot.outbox).drain(..).collect();
                if batch.is_empty() {
                    slot.flushing.store(false, SeqCst);
                    if lock(&slot.outbox).is_empty() || slot.flushing.swap(true, SeqCst) {
                        return;
                    }
                    continue;
                }
                let mut events = Vec::new();
                for o in batch {
                    match o {
                        Output::Event(e) => events.push(e),
                        other => {
                            self.persist(slot, std::mem::take(&mut events)).await;
                            self.side_effect(slot, other);
                        }
                    }
                }
                self.persist(slot, events).await;
            }
        })
    }

    async fn persist(self: &Arc<Self>, slot: &Arc<Slot>, events: Vec<SessionEvent>) {
        if events.is_empty() {
            return;
        }
        let own: Vec<SessionEvent> = events
            .into_iter()
            .filter(|e| e.session == slot.id)
            .collect();
        if self.deps.store.append(&slot.id, &own).await.is_err() {
            slot.failed.store(true, SeqCst);
            return;
        }
        let mut hits = Vec::new();
        for e in &own {
            slot.hub.event(e);
            if let Some(hit) = self.observe(slot, e) {
                hits.push(hit);
            }
        }
        let meta = lock(&slot.meta).clone();
        let _ = self.deps.store.put_meta(&meta).await;
        if let Some(hit) = hits.into_iter().next() {
            self.feed(slot, LoopInput::BudgetStop(hit));
        }
    }

    fn side_effect(self: &Arc<Self>, slot: &Arc<Slot>, o: Output) {
        match o {
            Output::Delta(d) => slot.hub.delta(&d),
            Output::Request { step, request } => self.start_model(slot, step, request),
            Output::CancelRequest(step) => {
                if let Some(t) = lock(&slot.tokens).remove(&step) {
                    t.cancel();
                }
            }
            Output::RunTool { call, spec } => self.run_tool(slot, call, spec),
            Output::Resolve { call, spec } => self.resolve_call(slot, call, spec),
            Output::CancelTool(call) => {
                lock(&self.state)
                    .client_calls
                    .remove(&(slot.id.clone(), call));
            }
            Output::Event(_) | Output::Refused(_) | Output::Idle => {}
        }
    }

    fn start_model(
        self: &Arc<Self>,
        slot: &Arc<Slot>,
        step: StepId,
        request: harness_log::ModelRequest,
    ) {
        let token = CancelToken::new();
        lock(&slot.tokens).insert(step, token.clone());
        let gen = slot.gen.fetch_add(1, SeqCst) + 1;
        let hang = slot.inject_hang.swap(false, SeqCst);
        let (me, s, model) = (self.clone(), slot.clone(), self.deps.model.clone());
        let task = async move {
            if hang {
                std::future::pending::<()>().await;
            }
            let mut stream = model.stream(request, token.clone());
            while let Some(item) = next(&mut stream).await {
                if token.is_cancelled() || s.gen.load(SeqCst) != gen {
                    return;
                }
                let (input, last) = match item {
                    Ok(ev) => {
                        let last = matches!(ev, ModelEvent::Done { .. });
                        (LoopInput::Model(ev), last)
                    }
                    Err(e) => (LoopInput::ModelError(e), true),
                };
                me.apply(&s, input).await;
                if last {
                    return;
                }
            }
            if !token.is_cancelled() && s.gen.load(SeqCst) == gen {
                let mut err = ErrorInfo::new("model.ended", "The model stream ended early.");
                err.transient = true;
                me.apply(&s, LoopInput::ModelError(err)).await;
            }
        };
        let h = self.deps.executor.run(slot.id.clone(), Box::pin(task));
        lock(&self.state).handles.push(h);
    }

    fn run_tool(self: &Arc<Self>, slot: &Arc<Slot>, call: ToolCall, spec: ToolSpec) {
        match spec.kind {
            ToolKind::Client => {
                lock(&self.state)
                    .client_calls
                    .insert((slot.id.clone(), call.call.clone()));
                if !self.deps.clients.is_connected(&slot.app) {
                    self.feed(slot, LoopInput::ClientGone);
                    return;
                }
                let (me, s) = (self.clone(), slot.clone());
                let fut = self.deps.clients.invoke(&slot.app, &slot.id, &call);
                self.deps.spawner.spawn(Box::pin(async move {
                    if fut.await.is_err() {
                        me.apply(&s, LoopInput::ClientGone).await;
                    }
                }));
            }
            _ => {
                let tool = match lock(&slot.registry)
                    .get(&spec.name)
                    .map(|e| e.handler.clone())
                {
                    Some(Handler::Engine(t)) => t,
                    _ => return,
                };
                if let Some(done) = self.recheck(slot, &call.call, &spec) {
                    let output = crate::kits::refused(done);
                    self.feed(slot, LoopInput::Tool { call: call.call, output });
                    return;
                }
                let ctx = self.tool_ctx(slot, &call);
                let (me, s) = (self.clone(), slot.clone());
                let task = async move {
                    let out = CatchUnwind(Box::pin(tool.call(&call.input, &ctx))).await;
                    let output = match out {
                        Ok(o) => o,
                        Err(m) => {
                            let msg =
                                format!("{} failed inside the host and did not finish.", call.tool);
                            let mut d = harness_tools::ToolDone::error("tool.panic", &msg);
                            d.content.push(Block::text(m));
                            harness_tools::ToolOutput::Done(d)
                        }
                    };
                    me.apply(
                        &s,
                        LoopInput::Tool {
                            call: call.call.clone(),
                            output,
                        },
                    )
                    .await;
                };
                let h = self.deps.executor.run(slot.id.clone(), Box::pin(task));
                lock(&self.state).handles.push(h);
            }
        }
    }

    /// A fresh session id: the clock and a counter, skipping every id the
    /// store already holds. The counter restarts with the host, so without
    /// that check a restarted host could mint an id a second time.
    pub(crate) async fn new_id(&self) -> Result<SessionId, HostError> {
        if lock(&self.minted).is_none() {
            let metas = self.deps.store.list_meta(None).await?;
            let known: BTreeSet<SessionId> = metas.into_iter().map(|m| m.id).collect();
            lock(&self.minted).get_or_insert(known);
        }
        let mut minted = lock(&self.minted);
        let seen = minted.get_or_insert_with(BTreeSet::new);
        loop {
            let n = self.ids.fetch_add(1, SeqCst) + 1;
            let id = SessionId::new(format!("ses_{}_{n:04}", self.now()));
            if seen.insert(id.clone()) {
                return Ok(id);
            }
        }
    }

    /// The tools a preset offers in a session, and the registry behind them.
    /// The engine tools a session of `app` may call: the preset's
    /// allow-list, and the kit tools admitted for it (`kits`).
    pub(crate) fn registry_for(
        &self,
        preset: &harness_preset::Preset,
        app: &AppId,
        kits: &[String],
    ) -> Registry {
        let mut r = Registry::new();
        for t in &self.cfg.tools {
            let name = t.spec().name;
            let named = preset.tools.allow.contains(&name) || kits.contains(&name);
            if named && t.offered(app) {
                r.add_engine(t.clone());
            }
        }
        r
    }
}
