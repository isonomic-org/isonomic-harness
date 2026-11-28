// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

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

use harness_core::access::Access;
use harness_core::offer::KitMember;
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
    /// The largest result a kit tool should return, in bytes, from the
    /// capability table; `None` when the session has no kit tools. The
    /// output limit's cut stays the backstop.
    pub result_cap: Option<usize>,
}

/// An engine tool.
pub trait Tool: Send + Sync {
    /// How the tool is offered to the model.
    fn spec(&self) -> ToolSpec;

    /// Run the tool. Failures are results, never panics.
    fn call<'a>(&'a self, input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput>;

    /// Whether a new session of `app` is offered this tool at all. A tool
    /// with nothing to work on for that app (no connected apps, say) says
    /// no, so the model is not handed a tool it cannot use. Yes by default.
    fn offered(&self, _app: &AppId) -> bool {
        true
    }

    /// The kit this tool belongs to, for a kit tool. Kit tools are offered
    /// by the preset's `[kits]`, the grants and the capability table, never
    /// by `tools.allow`. `None`, the default, for every other tool.
    fn kit(&self) -> Option<KitMember> {
        None
    }

    /// For a kit tool (a spec with a scope): resolve what the call will
    /// touch before policy judges it, opening it and keeping it open for
    /// the run. A failure is the call's result. `None`, the default, means
    /// the tool has nothing to resolve; a tool with a scope that returns
    /// `None` is refused.
    fn access<'a>(
        &'a self,
        _input: &'a Value,
        _cx: &'a ToolCtx,
    ) -> Option<BoxFuture<'a, Result<Access, ToolDone>>> {
        None
    }
}
