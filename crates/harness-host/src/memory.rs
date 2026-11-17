// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! `MemoryHost`: a `Host` over in-memory implementations of every trait,
//! with a runtime-free spawner and a manual clock. The SDK's testkit
//! exposes it as its in-memory host.

use crate::exec::{lock, InProcessExecutor, ManualClock, MemorySpawner};
use crate::host::{Deps, Host, HostConfig, Inner};
use crate::traits::*;
use crate::types::*;
use harness_core::future::{iter, ready, BoxFuture, BoxStream};
use harness_core::*;
use harness_log::{MemoryStore, ModelEvent, ModelRequest, ModelStop, StoreError};
use harness_policy::{Autonomy, Limits};
use harness_preset::Ceiling;
use harness_toolkit::{toolkit, FixedZone};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};

/// In-memory blobs.
#[derive(Debug, Default)]
pub struct MemoryBlobs {
    blobs: Mutex<BTreeMap<BlobId, Blob>>,
}

impl BlobStore for MemoryBlobs {
    fn put(
        &self,
        owner: &AppId,
        s: &SessionId,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> BoxFuture<'static, Result<BlobId, StoreError>> {
        let mut b = lock(&self.blobs);
        let id = BlobId::new(format!("blb_{:04}", b.len() + 1));
        b.insert(
            id.clone(),
            Blob {
                owner: owner.clone(),
                session: s.clone(),
                media_type: media_type.into(),
                bytes,
            },
        );
        ready(Ok(id))
    }

    fn get(&self, id: &BlobId) -> BoxFuture<'static, Result<Blob, StoreError>> {
        ready(
            lock(&self.blobs)
                .get(id)
                .cloned()
                .ok_or(StoreError::NotFound),
        )
    }

    fn delete_for(&self, sessions: &[SessionId]) -> BoxFuture<'static, Result<u64, StoreError>> {
        let mut b = lock(&self.blobs);
        let before = b.len();
        b.retain(|_, v| !sessions.contains(&v.session));
        ready(Ok((before - b.len()) as u64))
    }
}

/// In-memory client channel: records invocations; apps connect and
/// disconnect by hand.
#[derive(Debug, Default)]
pub struct MemoryClients {
    connected: Mutex<BTreeSet<AppId>>,
    /// Calls delivered, in order.
    pub invoked: Mutex<Vec<(AppId, SessionId, ToolCall)>>,
}

impl ClientToolChannel for MemoryClients {
    fn invoke(
        &self,
        app: &AppId,
        s: &SessionId,
        call: &ToolCall,
    ) -> BoxFuture<'static, Result<(), ChannelError>> {
        if !self.is_connected(app) {
            return ready(Err(ChannelError("not connected".into())));
        }
        lock(&self.invoked).push((app.clone(), s.clone(), call.clone()));
        ready(Ok(()))
    }

    fn is_connected(&self, app: &AppId) -> bool {
        lock(&self.connected).contains(app)
    }
}

/// In-memory scopes: tokens map to callers; user-action tokens are minted
/// per (caller, session, approval).
#[derive(Debug, Default)]
pub struct MemoryScopes {
    tokens: Mutex<BTreeMap<CapabilityToken, Caller>>,
    actions: Mutex<BTreeSet<(String, SessionId, ApprovalId, UserActionToken)>>,
}

impl MemoryScopes {
    /// Map a token the caller issued (an SDK testkit's `apps.grant`, say)
    /// to a caller.
    pub fn insert(&self, token: CapabilityToken, caller: Caller) {
        lock(&self.tokens).insert(token, caller);
    }
}

fn caller_key(c: &Caller) -> String {
    format!("{:?}", c.scope)
}

impl ScopeResolver for MemoryScopes {
    fn resolve(&self, capability: &CapabilityToken) -> Result<Caller, HostError> {
        lock(&self.tokens)
            .get(capability)
            .cloned()
            .ok_or(HostError::Unauthorized)
    }

    fn verify_user_action(
        &self,
        caller: &Caller,
        token: &UserActionToken,
        session: &SessionId,
        approval: &ApprovalId,
    ) -> bool {
        lock(&self.actions).contains(&(
            caller_key(caller),
            session.clone(),
            approval.clone(),
            token.clone(),
        ))
    }
}

/// The locale every in-memory session gets: `2026-11-15`-style dates from
/// the clock, `America/New_York` at UTC-5, English, and a fixed place for
/// sessions that opted in.
#[derive(Debug)]
pub struct FixedLocale;

impl Locale for FixedLocale {
    fn context(&self, _app: &AppId, _s: &SessionId, now: Millis, opt_in: bool) -> RequestContext {
        let days = (now as i64 / 1000 - 5 * 3600).div_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        RequestContext {
            date: format!("{y:04}-{m:02}-{d:02}"),
            time_zone: "America/New_York".into(),
            language: "en".into(),
            location: opt_in.then(|| "Brooklyn, NY".into()),
        }
    }
}

/// A scripted model: each request takes the next scripted reply; with
/// nothing scripted it answers "OK."
#[derive(Debug, Default)]
pub struct ScriptedModel {
    replies: Mutex<VecDeque<Vec<Result<ModelEvent, ErrorInfo>>>>,
    /// The context size it reports; 0 stands for the default, 32,768.
    context: AtomicU64,
    /// Every request received, in order.
    pub requests: Mutex<Vec<ModelRequest>>,
}

impl ScriptedModel {
    /// Queue a reply.
    pub fn push(&self, events: Vec<Result<ModelEvent, ErrorInfo>>) {
        lock(&self.replies).push_back(events);
    }

    /// The context size, in tokens, it reports for every model from now on.
    pub fn set_context(&self, tokens: u64) {
        self.context.store(tokens, SeqCst);
    }

    /// Queue a text reply.
    pub fn text(&self, text: &str) {
        self.push(vec![
            Ok(ModelEvent::Text(text.into())),
            Ok(done(ModelStop::End)),
        ]);
    }

    /// Queue tool calls.
    pub fn calls(&self, calls: &[(&str, &str)]) {
        let mut v: Vec<Result<ModelEvent, ErrorInfo>> = calls
            .iter()
            .map(|(t, i)| {
                Ok(ModelEvent::ToolCall {
                    tool: (*t).into(),
                    input: (*i).into(),
                })
            })
            .collect();
        v.push(Ok(done(ModelStop::ToolUse)));
        self.push(v);
    }
}

fn done(stop: ModelStop) -> ModelEvent {
    ModelEvent::Done {
        stop,
        usage: Usage {
            input_tokens: 100,
            output_tokens: 10,
            ..Usage::default()
        },
    }
}

impl ModelClient for ScriptedModel {
    fn caps(&self, _model: &str) -> ModelCaps {
        ModelCaps {
            prefill: true,
            reasoning: true,
            tools: true,
            context_tokens: match self.context.load(SeqCst) {
                0 => 32_768,
                n => n,
            },
        }
    }

    fn stream(
        &self,
        req: ModelRequest,
        _cancel: CancelToken,
    ) -> BoxStream<'static, Result<ModelEvent, ErrorInfo>> {
        lock(&self.requests).push(req);
        let reply = lock(&self.replies)
            .pop_front()
            .unwrap_or_else(|| vec![Ok(ModelEvent::Text("OK.".into())), Ok(done(ModelStop::End))]);
        iter(reply)
    }
}

/// A host over in-memory everything. See `docs/host-api.md`.
pub struct MemoryHost {
    /// The host.
    pub host: Host,
    /// The store.
    pub store: Arc<MemoryStore>,
    /// The model (scripted by default).
    pub model: Arc<ScriptedModel>,
    /// Client tool delivery.
    pub clients: Arc<MemoryClients>,
    /// Scopes and user actions.
    pub scopes: Arc<MemoryScopes>,
    /// Blobs.
    pub blobs: Arc<MemoryBlobs>,
    /// The spawner.
    pub spawner: Arc<MemorySpawner>,
    /// The clock.
    pub clock: Arc<ManualClock>,
}

/// The configuration MemoryHost uses unless told otherwise.
pub fn memory_config() -> HostConfig {
    HostConfig {
        ceiling: Ceiling {
            limits: Limits {
                tokens: Some(1_000_000),
                steps: Some(64),
                wall_ms: Some(3_600_000),
                cost_micros: None,
            },
            autonomy: Autonomy::Supervised,
        },
        session_limits: Limits::default(),
        app_limits: Limits::default(),
        tools: toolkit(Arc::new(FixedZone {
            name: "America/New_York".into(),
            offset_minutes: -300,
        })),
        subscriber_queue: 256,
        watchdog_ms: 1_000,
        profile: Some(Profile::Standard),
    }
}

impl MemoryHost {
    /// A host at `2026-11-14T09:00Z`, with the toolkit and default limits.
    pub fn new() -> Self {
        Self::with(memory_config())
    }

    /// A host with a configuration.
    pub fn with(cfg: HostConfig) -> Self {
        let clock = Arc::new(ManualClock::new(1_794_646_800_000));
        let spawner = Arc::new(MemorySpawner::new(clock.clone()));
        let store = Arc::new(MemoryStore::new());
        let model = Arc::new(ScriptedModel::default());
        let clients = Arc::new(MemoryClients::default());
        let scopes = Arc::new(MemoryScopes::default());
        let blobs = Arc::new(MemoryBlobs::default());
        let deps = Deps {
            store: store.clone(),
            model: model.clone(),
            clients: clients.clone(),
            scopes: scopes.clone(),
            blobs: blobs.clone(),
            spawner: spawner.clone(),
            clock: clock.clone(),
            executor: Arc::new(InProcessExecutor::new(spawner.clone())),
            locale: Arc::new(FixedLocale),
        };
        Self {
            host: Host::new(deps, cfg),
            store,
            model,
            clients,
            scopes,
            blobs,
            spawner,
            clock,
        }
    }

    /// A capability token for an app (connected from the start).
    pub fn app_token(&self, app: &str, kind: AppKind) -> CapabilityToken {
        let t = CapabilityToken(format!("cap:{app}"));
        lock(&self.scopes.tokens).insert(t.clone(), Caller::app(app, kind));
        lock(&self.clients.connected).insert(AppId::from(app));
        t
    }

    /// Use a token issued elsewhere for `caller`; an app caller's app is
    /// connected from the start.
    pub fn insert_token(&self, token: CapabilityToken, caller: Caller) {
        if let Some(app) = caller.scope.app() {
            lock(&self.clients.connected).insert(app.clone());
        }
        self.scopes.insert(token, caller);
    }

    /// The shell's capability token.
    pub fn shell_token(&self) -> CapabilityToken {
        let t = CapabilityToken("cap:shell".into());
        lock(&self.scopes.tokens).insert(t.clone(), Caller::shell());
        t
    }

    /// The caller a token stands for.
    pub fn caller(&self, token: &CapabilityToken) -> Caller {
        self.scopes
            .resolve(token)
            .expect("a token this host minted")
    }

    /// Mint a user-action token, as the UI broker does when a person acts.
    pub fn user_action(
        &self,
        caller: &Caller,
        session: &SessionId,
        approval: &ApprovalId,
    ) -> UserActionToken {
        let t = UserActionToken(format!("ua:{}:{session}:{approval}", caller_key(caller)));
        lock(&self.scopes.actions).insert((
            caller_key(caller),
            session.clone(),
            approval.clone(),
            t.clone(),
        ));
        t
    }

    /// An app disconnects.
    pub fn disconnect(&self, app: &str) {
        let a = AppId::from(app);
        lock(&self.clients.connected).remove(&a);
        self.block_on(self.host.client_disconnected(&a));
    }

    /// An app reconnects.
    pub fn reconnect(&self, app: &str) {
        let a = AppId::from(app);
        lock(&self.clients.connected).insert(a.clone());
        self.block_on(self.host.client_connected(&a));
    }

    /// Move the clock, then run timers, the watchdog and pending work.
    pub fn advance(&self, ms: u64) {
        self.clock.advance(ms);
        self.spawner.wake_timers();
        self.run();
    }

    /// Run pending work until nothing is left to do.
    pub fn run(&self) {
        self.spawner.run();
    }

    /// The next input to `session` panics inside its loop.
    pub fn inject_panic(&self, session: &SessionId) {
        if let Some(s) = self.inner().slot(session) {
            s.inject_panic.store(true, SeqCst);
        }
    }

    /// The next model request of `session` never answers.
    pub fn inject_hang(&self, session: &SessionId) {
        if let Some(s) = self.inner().slot(session) {
            s.inject_hang.store(true, SeqCst);
        }
    }

    /// A session's own events, read from the store.
    pub fn log(&self, session: &SessionId) -> Vec<SessionEvent> {
        self.store.events(session)
    }

    /// Drive a future to completion, running background work meanwhile.
    pub fn block_on<T>(&self, fut: impl std::future::Future<Output = T>) -> T {
        let out = self.spawner.block_on(fut);
        self.run();
        out
    }

    fn inner(&self) -> &Arc<Inner> {
        &self.host.0
    }
}

impl Default for MemoryHost {
    fn default() -> Self {
        Self::new()
    }
}
