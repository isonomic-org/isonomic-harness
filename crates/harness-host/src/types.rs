//! The host's arguments, results and errors (`docs/host-api.md`).

use harness_core::*;
use harness_log::StoreError;
use harness_policy::Limits;
use harness_preset::Diagnostic;

pub use harness_policy::{
    AppKind, Caller as Scope, DecisionInput, PendingApproval, UserActionToken,
};

/// Who is calling: the scope from the capability token, and who it acts for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    /// The shell, or an app and its kind.
    pub scope: Scope,
    /// The person or agent.
    pub principal: Principal,
}

impl Caller {
    /// The shell, acting for the person.
    pub fn shell() -> Self {
        Caller {
            scope: Scope::Shell,
            principal: Principal {
                kind: PrincipalKind::User,
                id: None,
            },
        }
    }

    /// An app, acting for the person.
    pub fn app(app: &str, kind: AppKind) -> Self {
        Caller {
            scope: Scope::App {
                app: AppId::from(app),
                kind,
            },
            principal: Principal {
                kind: PrincipalKind::User,
                id: None,
            },
        }
    }

    /// The app sessions created by this caller belong to.
    pub fn owner(&self) -> AppId {
        self.scope
            .app()
            .cloned()
            .unwrap_or_else(|| AppId::from("shell"))
    }

    /// Who decided, for the log.
    pub fn decider(&self) -> Decider {
        match &self.scope {
            Scope::Shell => Decider {
                scope: DeciderScope::Shell,
                app: None,
            },
            Scope::App { app, .. } => Decider {
                scope: DeciderScope::App,
                app: Some(app.clone()),
            },
        }
    }
}

/// A capability token, as the broker hands it over.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityToken(pub String);

/// Why a call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum HostError {
    /// No such thing in the caller's scope (also another app's ids).
    NotFound,
    /// The caller may not do this.
    Unauthorized,
    /// Milestone 0 runs interactive sessions only.
    ClassNotSupported(SessionClass),
    /// The preset did not validate.
    InvalidPreset(Vec<Diagnostic>),
    /// A fork point that is not a turn boundary.
    NotAtTurnBoundary,
    /// Continue is not possible, and why.
    ContinueRefused(String),
    /// Views and third-party code may not decide approvals.
    NotAllowedToDecide,
    /// No verified user-action token.
    MissingUserAction,
    /// The approval was already decided.
    AlreadyDecided,
    /// The session failed and takes no more input.
    SessionFailed,
    /// Client tool registrations must be client tools with free names.
    InvalidTools(String),
    /// The store failed.
    Store(StoreError),
    /// The model failed outside a turn.
    Model(ErrorInfo),
}

impl From<StoreError> for HostError {
    fn from(e: StoreError) -> Self {
        HostError::Store(e)
    }
}

/// `create_session` options.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CreateOptions {
    /// A title to start with.
    pub title: Option<String>,
    /// The person opted in to sharing a location.
    pub location_opt_in: bool,
    /// Limits that may only tighten the preset's.
    pub limits: Option<Limits>,
    /// The model the person picked: it replaces the preset's reply model
    /// for this session and is recorded in `session.start`. `None`, or a
    /// blank name, runs the preset's model. The host only carries the name;
    /// whether the caller may use that model is the engine's check.
    pub model: Option<String>,
}

/// `list_sessions` filter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionFilter {
    /// Archived or not; `None` for both.
    pub archived: Option<bool>,
    /// One preset only.
    pub preset: Option<PresetId>,
    /// Title contains this (case-insensitive).
    pub text: Option<String>,
    /// From a previous page.
    pub cursor: Option<String>,
    /// Page size; 0 means 50.
    pub limit: u32,
}

/// A page of results.
#[derive(Clone, Debug, PartialEq)]
pub struct Page<T> {
    /// The items.
    pub items: Vec<T>,
    /// The next page's cursor.
    pub next: Option<String>,
}

/// A registry row as callers see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSummary {
    /// The session.
    pub id: SessionId,
    /// Its owner app.
    pub app: AppId,
    /// Its class.
    pub class: SessionClass,
    /// Its preset.
    pub preset: PresetRef,
    /// Its title.
    pub title: Option<String>,
    /// Active, awaiting client or failed.
    pub status: Status,
    /// Archived.
    pub archived: bool,
    /// For a fork: the parent and the seq it forked at.
    pub forked_from: Option<(SessionId, Seq)>,
    /// Created.
    pub created_at: Millis,
    /// Last written.
    pub updated_at: Millis,
    /// Its last seq (0 when not loaded since the host started).
    pub last_seq: Seq,
    /// The model picked for it at creation; `None` when it runs on its
    /// preset's model.
    pub model: Option<String>,
}

/// `open_session`: the summary and what is pending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    /// The summary.
    pub summary: SessionSummary,
    /// The open turn.
    pub turn_open: Option<TurnId>,
    /// Pending approvals.
    pub approvals: Vec<ApprovalId>,
    /// Open questions.
    pub questions: Vec<QuestionId>,
    /// Client calls waiting for their app.
    pub awaiting_client: Vec<CallId>,
}

/// Where to fork.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForkPoint {
    /// Just before this turn started.
    BeforeTurn(TurnId),
    /// Right after this turn ended.
    AfterTurn(TurnId),
}

/// What `send` carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    /// The message.
    pub content: Vec<Block>,
    /// The app's own reference, echoed in audit records.
    pub client_ref: Option<String>,
}

/// `send`'s answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accepted {
    /// The input waits in the inbox behind a running turn.
    pub queued: bool,
}

/// `answer_question`'s input.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerInput {
    /// Chosen option ids.
    pub selected: Vec<String>,
    /// Free text.
    pub text: Option<String>,
    /// The person skipped the question.
    pub skipped: bool,
}

/// What an app reports for its client tool call.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientOutcome {
    /// It worked.
    Ok(Vec<Block>),
    /// It failed, readably.
    Error(ErrorInfo, Vec<Block>),
}

/// `purge`'s report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PurgeReport {
    /// Sessions deleted.
    pub sessions: u64,
    /// Events deleted.
    pub events: u64,
    /// Blobs deleted.
    pub blobs: u64,
}

/// A preset as listed.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetSummary {
    /// Id and latest version.
    pub reference: PresetRef,
    /// The app that owns it; `None` for built-ins.
    pub owner: Option<AppId>,
    /// Its name.
    pub name: String,
    /// What validation found; errors mean it cannot be used.
    pub diagnostics: Vec<Diagnostic>,
}

/// A preset as stored.
#[derive(Clone, Debug)]
pub struct StoredPreset {
    /// Id and version.
    pub reference: PresetRef,
    /// The owner app; `None` for built-ins.
    pub owner: Option<AppId>,
    /// The TOML text.
    pub text: String,
    /// The parsed form, when valid.
    pub preset: Option<harness_preset::Preset>,
    /// Diagnostics.
    pub diagnostics: Vec<Diagnostic>,
}

/// An artifact version: metadata from the log, body from the blob store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactVersion {
    /// The `artifact` event.
    pub meta: ArtifactEvent,
    /// The body.
    pub bytes: Vec<u8>,
}

/// One audit record: every tool call, result and approval, for Activity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRecord {
    /// The session.
    pub session: SessionId,
    /// Its owner app.
    pub app: AppId,
    /// The event's seq (0 for a dropped late result).
    pub seq: Seq,
    /// When.
    pub at: Millis,
    /// The event type, or `tool.result.late`.
    pub kind: String,
    /// The call, if any.
    pub call: Option<CallId>,
    /// The tool, outcome or decision, in words.
    pub detail: String,
}
