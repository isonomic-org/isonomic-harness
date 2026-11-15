//! Isonomic Harness tools.
//!
//! A tool has a name, a JSON Schema input, a concurrency class, declared
//! effects and a kind: **engine** (runs in the host), **client** (run by the
//! app that registered it) or **cell** (refused until Milestone 6).
//!
//! - [`Tool`]: what an engine tool implements.
//! - [`Registry`]: the tools one session may call.
//! - [`guard`]: checks a call's input before anything runs.
//! - [`plan_batches`]: which calls run together.
//! - [`ClientCalls`]: client calls whose app may go away.
//!
//! Failures are structured results the model can read ([`ToolDone`]).

mod batch;
mod client;
mod guard;
mod output;
mod registry;

pub use batch::plan_batches;
pub use client::ClientCalls;
pub use guard::{guard, GuardFailure};
pub use output::{limit_output, QuestionSpec, ToolDone, ToolOutput};
pub use registry::{Entry, Handler, Registry, RegistryError};

use harness_core::future::BoxFuture;
use harness_core::*;

/// What an engine tool knows about the call it serves.
#[derive(Clone, Debug)]
pub struct ToolCtx {
    /// The session.
    pub session: SessionId,
    /// The session's owner app.
    pub app: AppId,
    /// The call.
    pub call: CallId,
    /// The time the call started.
    pub now: Millis,
    /// For a delegating tool: the child session to create.
    pub child: Option<SessionId>,
}

/// An engine tool.
pub trait Tool: Send + Sync {
    /// How the tool is offered to the model.
    fn spec(&self) -> ToolSpec;

    /// Run the tool. Failures are results, never panics.
    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput>;
}
