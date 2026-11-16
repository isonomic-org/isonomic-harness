//! Isonomic Harness loop: one session's turn and step driver.
//!
//! The loop is **sans-IO**. [`Session::handle`] takes the time and one
//! [`Input`] and returns [`Output`]s: events to persist (then publish),
//! live deltas, model requests to make, tools to run. The host does every
//! await; the loop never names a runtime, so it compiles anywhere,
//! WebAssembly included.
//!
//! Guarantees:
//!
//! - Every model request is built from the log with
//!   `harness_log::history::request_for` right after its `step.start` is
//!   written, so replaying the log rebuilds it byte for byte.
//! - Cancel works at every await point: a cancelled reply is sealed with its
//!   partial text, the attempt is recorded, and every outstanding call gets
//!   a result.
//! - Refusals, errors and failed attempts are recorded as `attempt`s and
//!   never reach history.

mod context;
mod machine;
mod steps;
mod stop;
mod subagent;
mod tools;

pub use context::{context_text, long_date};
pub use machine::Session;
pub use subagent::{
    check_delegation, Activation, Delegation, DelegationError, FakeActivation, MAX_DEPTH,
};

use harness_core::*;
use harness_log::ModelEvent;
use harness_policy::Limits;
use harness_preset::Preset;
use harness_tools::ToolOutput;

/// What one session runs with.
#[derive(Clone, Debug)]
pub struct Config {
    /// The preset.
    pub preset: Preset,
    /// The hardware profile.
    pub profile: Option<Profile>,
    /// Tools offered: the preset's engine tools and the owner app's client
    /// tools, as the host's registry resolved them.
    pub tools: Vec<ToolSpec>,
    /// Per-turn limits (the preset's, tightened by the app).
    pub limits: Limits,
    /// Retries for transient model errors, inside one step.
    pub max_retries: u32,
    /// Tool output over this many bytes keeps head and tail.
    pub max_tool_output: usize,
    /// The context size, in tokens, of the model the session runs on, when
    /// the host knows it, picked or the preset's own. The preset's
    /// compaction budget is an upper bound; the model's real window may be
    /// smaller.
    pub context_tokens: Option<u64>,
}

/// A picked model's name as it is recorded: trimmed, and `None` when blank.
pub fn picked_model(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_owned)
}

impl Config {
    /// A config from a preset with its own limits and the toolkit defaults.
    pub fn new(preset: Preset, profile: Option<Profile>, tools: Vec<ToolSpec>) -> Self {
        let limits = preset.limits;
        Self {
            preset,
            profile,
            tools,
            limits,
            max_retries: 2,
            max_tool_output: 65_536,
            context_tokens: None,
        }
    }

    /// The config with the model's context size (0 means unknown).
    pub fn with_context(mut self, tokens: u64) -> Self {
        self.context_tokens = (tokens > 0).then_some(tokens);
        self
    }

    /// What a request may hold before compaction: the preset's budget, and
    /// no more than the model's context less room for the reply (the output
    /// limit, or half the context when the limit is larger than that).
    pub fn compaction_budget(&self) -> u64 {
        let preset = self.preset.compaction.budget_tokens;
        match self.context_tokens {
            None => preset,
            Some(context) => {
                let reply = u64::from(self.preset.model.max_output_tokens).min(context / 2);
                preset.min(context - reply)
            }
        }
    }

    /// The config with the session's picked model, if any, in place of the
    /// preset's reply model (`session.start`'s `model`). A blank name is no
    /// pick.
    pub fn with_model(mut self, model: Option<&str>) -> Self {
        if let Some(m) = picked_model(model) {
            self.preset.model.id = m;
        }
        self
    }
}

/// One input to the loop.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// The person sent input. `context` is what the engine knows (local
    /// date, time zone, language, and a location only when the person opted
    /// in); the context link applies the preset's settings to it.
    User {
        /// The message.
        content: Vec<Block>,
        /// The request context.
        context: Option<RequestContext>,
    },
    /// Continue the sealed partial reply of the last turn.
    Continue,
    /// Stop the current turn.
    Cancel,
    /// A budget kept by the host (per session or per app) ran out.
    BudgetStop(BudgetHit),
    /// The host's watchdog or executor found a fault: seal the turn as
    /// failed, write `session.status(failed)`, take no more input.
    Fail(ErrorInfo),
    /// A streamed event for the step's request.
    Model(ModelEvent),
    /// The step's request failed.
    ModelError(ErrorInfo),
    /// A tool finished, or asks the person.
    Tool {
        /// The call.
        call: CallId,
        /// Its output.
        output: ToolOutput,
    },
    /// An approval was decided. The host has checked who decided and their
    /// user-action token.
    Decision(ApprovalDecision),
    /// A question was answered.
    Answer(Answer),
    /// The owner app disconnected.
    ClientGone,
    /// The owner app reconnected.
    ClientBack,
    /// The tool set changed (a client registered tools).
    Tools(Vec<ToolSpec>),
    /// The person renamed the session.
    Rename(String),
    /// Check deadlines: approvals and client calls.
    Tick,
}

/// One output of the loop.
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// Persist, then publish.
    Event(SessionEvent),
    /// Publish on the lossy live channel; never stored.
    Delta(LiveDelta),
    /// Make this request; stream its events back as [`Input::Model`].
    Request {
        /// The step.
        step: StepId,
        /// The request, exactly as replay rebuilds it.
        request: harness_log::ModelRequest,
    },
    /// Abandon the step's request.
    CancelRequest(StepId),
    /// Run this call: an engine tool in the host, a client tool in the app.
    RunTool {
        /// The call as recorded.
        call: ToolCall,
        /// Its tool.
        spec: ToolSpec,
    },
    /// Abandon a running call.
    CancelTool(CallId),
    /// An input could not be taken; nothing was written.
    Refused(Refusal),
    /// The turn closed and nothing more is owed.
    Idle,
}

/// Why an input was not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A turn is running.
    Busy,
    /// Nothing is running.
    NotRunning,
    /// Continue is not possible: the reason, for the person.
    ContinueRefused(String),
    /// The session failed and takes no more input.
    SessionFailed,
    /// No such approval, question or call is open.
    NotOpen,
}

/// What a link sees before a step.
#[derive(Clone, Debug, PartialEq)]
pub struct StepInfo {
    /// The turn.
    pub turn: TurnId,
    /// Reply, title or summary.
    pub purpose: Purpose,
    /// The first step of its turn.
    pub first: bool,
    /// The turn's user input, for the first step.
    pub input: Vec<Block>,
}

/// A link's verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Go on.
    Pass,
    /// Stop here, with a reason the model or person can read.
    Deny(String),
}

/// A link's verdict on a finished call.
#[derive(Clone, Debug, PartialEq)]
pub enum PostTool {
    /// Record the result as is.
    Pass,
    /// Record this result instead (a redaction, say).
    Replace(harness_tools::ToolDone),
}

/// One link of the interception chain. Every method has a passing default.
/// The chain runs in order; the first link that decides wins.
///
/// - `pre_step` denying the first step rejects the input: the turn closes
///   with no step. Denying a later step fails the turn.
/// - `request` sees the exact request and may deny it.
/// - `pre_tool` denying a call gives the model a readable reason.
/// - `post_tool` may replace a result before it is recorded.
/// - `turn_stopping` may fail a turn that is about to close.
pub trait Interceptor: Send {
    /// Before a step starts.
    fn pre_step(&mut self, _step: &StepInfo) -> Link {
        Link::Pass
    }
    /// Before a request is made.
    fn request(&mut self, _request: &harness_log::ModelRequest) -> Link {
        Link::Pass
    }
    /// Before a call runs (after policy allowed it).
    fn pre_tool(&mut self, _call: &ToolCall) -> Link {
        Link::Pass
    }
    /// After a call finished, before its result is recorded.
    fn post_tool(&mut self, _call: &ToolCall, _done: &harness_tools::ToolDone) -> PostTool {
        PostTool::Pass
    }
    /// Before a turn closes as done.
    fn turn_stopping(&mut self, _turn: &TurnId) -> Link {
        Link::Pass
    }
}
