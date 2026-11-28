// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The schema's string enums. Every one is open: a value from a newer
//! revision deserializes as `Other` instead of failing.

use serde::{Deserialize, Serialize};

macro_rules! open_enum {
    ($(#[$doc:meta])* $name:ident { $($(#[$vdoc:meta])* $variant:ident),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$vdoc])* $variant,)+
            /// A value from a newer schema revision.
            #[serde(untagged)]
            Other(String),
        }
    };
}

open_enum!(
    /// Who a session acts for.
    PrincipalKind {
        /// The person using the app.
        User,
        /// An agent with its own key.
        Agent,
    }
);

open_enum!(
    /// The hardware profile the session runs under.
    Profile {
        /// The slowest supported profile: about 3 tokens a second locally.
        Intel,
        /// A low-end machine.
        Low,
        /// A typical machine.
        Standard,
        /// A high-end machine.
        High,
    }
);

open_enum!(
    /// A session's status.
    Status {
        /// Running or idle normally.
        Active,
        /// A client tool's app is gone.
        AwaitingClient,
        /// A panic or hang sealed a turn; the session takes no more input.
        Failed,
    }
);

open_enum!(
    /// Why a session was forked.
    ForkReason {
        /// The person edited a prompt.
        Edit,
        /// The person regenerated a reply.
        Regenerate,
        /// Any other branch.
        Branch,
    }
);

open_enum!(
    /// Where system content came from.
    SystemOrigin {
        /// The preset's composed prompt.
        Preset,
        /// The context link's additions for this turn.
        Context,
        /// A notice from the host.
        Notice,
    }
);

open_enum!(
    /// What opened a turn.
    Cause {
        /// The person sent input.
        User,
        /// The person asked to continue a sealed partial reply.
        Continue,
        /// A trigger woke the session (autonomous).
        Wake,
        /// A parent session delegated a task (delegated).
        Delegate,
    }
);

open_enum!(
    /// What woke an autonomous session.
    WakeSource {
        /// A schedule.
        Schedule,
        /// A heartbeat.
        Heartbeat,
        /// An outside channel.
        Channel,
        /// A webhook.
        Webhook,
    }
);

open_enum!(
    /// Why a step's model request was made.
    Purpose {
        /// The conversation reply. The only purpose in model history.
        Reply,
        /// The post-turn title step.
        Title,
        /// A compaction summary.
        Summary,
    }
);

open_enum!(
    /// How much the model reasons.
    Reasoning {
        /// No reasoning (the default on the slowest profile).
        Off,
        /// Brief.
        Low,
        /// Moderate.
        Medium,
        /// Extended.
        High,
    }
);

open_enum!(
    /// Where a tool runs.
    ToolKind {
        /// In the host.
        Engine,
        /// In the app that registered it.
        Client,
        /// In a Cell. Refused until Milestone 6.
        Cell,
        /// A display widget: no side effect; the host validates and
        /// records it, and the kit renders it (revision 1).
        Display,
    }
);

open_enum!(
    /// What became of a display call, on its `tool.result` (revision 1).
    DisplayOutcome {
        /// Valid: the kit renders the widget.
        Shown,
        /// Invalid the first time: the model may call again.
        Retry,
        /// Invalid again, or unparseable: the kit shows the input's
        /// `fallback_text`, or a generic block without one.
        Fallback,
    }
);

open_enum!(
    /// What a tool may do. Policy reads these.
    Effect {
        /// It reads the person's private data.
        ReadsPrivateData,
        /// It brings untrusted content into the session.
        IngestsUntrustedContent,
        /// It acts outside the device: sends, posts, buys.
        ActsOutward,
        /// It runs code.
        ExecutesCode,
    }
);

open_enum!(
    /// How a tool runs alongside other calls in the same step.
    Concurrency {
        /// With other parallel calls.
        Parallel,
        /// Alone, after the calls before it.
        Serial,
    }
);

open_enum!(
    /// Why a reply stopped.
    StopReason {
        /// The model finished.
        End,
        /// The model called tools.
        ToolUse,
        /// The output limit was reached; continue may extend it.
        MaxTokens,
        /// Cancelled mid-stream; the partial reply is sealed and kept.
        Cancelled,
    }
);

open_enum!(
    /// How a failed attempt ended.
    AttemptOutcome {
        /// A model or transport error.
        Error,
        /// Cancelled before any reply was kept.
        Cancelled,
        /// The model refused. A refusal cannot be regenerated.
        Refused,
    }
);

open_enum!(
    /// How a tool call ended.
    ToolOutcome {
        /// It succeeded.
        Ok,
        /// It failed; the content says why, readably.
        Error,
        /// Policy or the person denied it; the content gives the reason.
        Denied,
        /// It timed out, such as a client tool whose app is gone.
        Timeout,
        /// The step was cancelled before it finished.
        Cancelled,
    }
);

open_enum!(
    /// The policy rule that asked for approval.
    ApprovalRule {
        /// The tool acts outward (deny by default).
        ActsOutward,
        /// The tool executes code (deny by default).
        ExecutesCode,
        /// Private data and untrusted content have both entered the session.
        ThreeFlag,
        /// The session's autonomy level requires it.
        Autonomy,
    }
);

open_enum!(
    /// An approval decision.
    Decision {
        /// Allowed.
        Allow,
        /// Denied.
        Deny,
    }
);

open_enum!(
    /// What a decision rests on.
    Basis {
        /// A person chose explicitly.
        User,
        /// The answer was ambiguous, so it is a denial.
        Ambiguous,
        /// Nobody answered in time.
        Timeout,
        /// Policy decided without asking.
        Policy,
        /// The turn was cancelled first.
        Cancelled,
    }
);

open_enum!(
    /// Who decided.
    DeciderScope {
        /// The shell's approvals sheet (authoritative).
        Shell,
        /// The first-party app that owns the session.
        App,
        /// The host itself (timeouts, cancellation, policy).
        Host,
    }
);

open_enum!(
    /// An artifact operation.
    ArtifactOp {
        /// Version 1.
        Create,
        /// A new version.
        Update,
    }
);

open_enum!(
    /// Where a title came from.
    TitleSource {
        /// The post-turn title step.
        Model,
        /// The opening words of the first prompt.
        Heuristic,
        /// The person renamed the session.
        User,
    }
);

open_enum!(
    /// How a step ended.
    StepOutcome {
        /// A final reply.
        Complete,
        /// Tool calls ran; another step follows.
        ToolCalls,
        /// Cancelled.
        Cancelled,
        /// Failed after retries.
        Failed,
        /// The model refused.
        Refused,
        /// A budget stopped it.
        Budget,
    }
);

open_enum!(
    /// How a turn closed.
    TurnStop {
        /// Nothing more is owed.
        Done,
        /// Cancelled; partial work is sealed.
        Cancelled,
        /// Failed.
        Failed,
        /// A budget stopped it cleanly.
        Budget,
        /// The input was rejected before any step.
        Rejected,
        /// The model refused.
        Refused,
        /// Sealed on recovery after an interruption.
        Interrupted,
    }
);

open_enum!(
    /// A budget dimension.
    BudgetKind {
        /// Tokens.
        Tokens,
        /// Steps.
        Steps,
        /// Wall time in milliseconds.
        WallTime,
        /// Cost in micro-units.
        Cost,
    }
);

open_enum!(
    /// What a `memory` event records.
    MemoryOp {
        /// A new memory.
        Created,
        /// A new revision of an existing memory.
        Revised,
        /// The memory and all its revisions were deleted.
        Deleted,
    }
);

open_enum!(
    /// Whose memory: about the user everywhere, one app's, or one agent's.
    MemoryScope {
        /// About the user, read by every app granted it.
        User,
        /// Private to one app (optionally split into containers).
        App,
        /// Private to one agent.
        Agent,
    }
);

open_enum!(
    /// A privacy tag on a model request. The model gateway routes on it.
    PrivacyTag {
        /// The request contains memory; it goes to a remote provider only
        /// when the person allowed memory with remote models.
        ContainsMemory,
    }
);

open_enum!(
    /// Whether memory reached a request: recorded on the context record and
    /// on each request's record, so replay rebuilds it exactly.
    MemoryPolicy {
        /// Memory was included.
        Used,
        /// Memory was withheld (a remote model without the person's
        /// permission): references resolve to `[memory withheld]`.
        Withheld,
        /// Memory is off for this session, app or preset.
        Off,
    }
);
