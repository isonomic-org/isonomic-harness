//! The traits the engine implements. Each has an in-memory implementation
//! in `memory.rs`. `SessionStore` is harness-log's.

use crate::types::*;
use harness_core::future::{BoxFuture, BoxStream};
use harness_core::*;
use harness_log::{ModelEvent, ModelRequest, StoreError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use harness_log::SessionStore;

/// A streamed model event.
pub type ModelChunk = ModelEvent;
/// A model failure; `transient` errors are retried.
pub type ModelError = ErrorInfo;

/// What a model can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelCaps {
    /// It continues an assistant prefix.
    pub prefill: bool,
    /// It reasons.
    pub reasoning: bool,
    /// It calls tools.
    pub tools: bool,
    /// Its context size in tokens.
    pub context_tokens: u64,
}

/// Cancels a request when set.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh token.
    pub fn new() -> Self {
        Self::default()
    }
    /// Cancel.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    /// Whether it was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The models.
pub trait ModelClient: Send + Sync {
    /// What `model` can do.
    fn caps(&self, model: &str) -> ModelCaps;
    /// Stream a request.
    fn stream(
        &self,
        req: ModelRequest,
        cancel: CancelToken,
    ) -> BoxStream<'static, Result<ModelChunk, ModelError>>;
}

/// Why a client tool call could not be delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelError(pub String);

/// Delivers client tool calls to apps.
pub trait ClientToolChannel: Send + Sync {
    /// Hand `call` to `app`; the result comes back through
    /// `client_tool_result`.
    fn invoke(
        &self,
        app: &AppId,
        s: &SessionId,
        call: &ToolCall,
    ) -> BoxFuture<'static, Result<(), ChannelError>>;
    /// Whether the app is connected.
    fn is_connected(&self, app: &AppId) -> bool;
}

/// Turns capability tokens into callers and checks user actions.
pub trait ScopeResolver: Send + Sync {
    /// The caller a token stands for.
    fn resolve(&self, capability: &CapabilityToken) -> Result<Caller, HostError>;
    /// Whether `token` was minted by a person acting on `approval`.
    fn verify_user_action(
        &self,
        caller: &Caller,
        token: &UserActionToken,
        approval: &ApprovalId,
    ) -> bool;
}

/// A stored blob and who owns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    /// The owner app.
    pub owner: AppId,
    /// The session.
    pub session: SessionId,
    /// Its type.
    pub media_type: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// Attachments and artifact bodies.
pub trait BlobStore: Send + Sync {
    /// Store a blob.
    fn put(
        &self,
        owner: &AppId,
        s: &SessionId,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> BoxFuture<'static, Result<BlobId, StoreError>>;
    /// Read a blob.
    fn get(&self, id: &BlobId) -> BoxFuture<'static, Result<Blob, StoreError>>;
    /// Delete every blob of these sessions; returns how many.
    fn delete_for(&self, sessions: &[SessionId]) -> BoxFuture<'static, Result<u64, StoreError>>;
}

/// Runs background work.
pub trait Spawner: Send + Sync {
    /// Run `task` to completion in the background.
    fn spawn(&self, task: BoxFuture<'static, ()>);
    /// A future that completes after `ms` of the host's clock.
    fn sleep(&self, ms: u64) -> BoxFuture<'static, ()>;
}

/// The person's locale for a session, as the engine knows it: what the
/// context link adds. Addition to the published API: the engine owns the
/// time zone and language.
pub trait Locale: Send + Sync {
    /// The request context at `now`; `location` only when the person opted
    /// in for this session.
    fn context(
        &self,
        app: &AppId,
        s: &SessionId,
        now: Millis,
        location_opt_in: bool,
    ) -> RequestContext;
}

/// A fault in a session's work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A panic, with its message.
    Panic(String),
}

/// A task run for a session.
#[derive(Clone, Debug)]
pub struct RunHandle {
    /// The session.
    pub session: SessionId,
    fault: Arc<std::sync::Mutex<Option<Fault>>>,
    done: Arc<AtomicBool>,
}

impl RunHandle {
    pub(crate) fn new(session: SessionId) -> Self {
        Self {
            session,
            fault: Arc::default(),
            done: Arc::default(),
        }
    }
    /// The fault, if the task panicked.
    pub fn fault(&self) -> Option<Fault> {
        self.fault.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// Whether the task finished (or faulted).
    pub fn finished(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
    pub(crate) fn report(&self, fault: Option<Fault>) {
        *self.fault.lock().unwrap_or_else(|e| e.into_inner()) = fault;
        self.done.store(true, Ordering::SeqCst);
    }
}

/// Runs each session's work; panics and hangs are reported, never
/// propagated. Milestone 0 runs in process; the trait lets loops move into
/// a supervised worker process later.
pub trait Executor: Send + Sync {
    /// Run `task` for `session`.
    fn run(&self, session: SessionId, task: BoxFuture<'static, ()>) -> RunHandle;
}
