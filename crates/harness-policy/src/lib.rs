// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isonomic Harness policy.
//!
//! Pure rules; harness-host enforces them, and nothing else does.
//!
//! - [`Caller`]: the shell or an app, from its capability token.
//! - [`Policy::evaluate`]: deny by default for outward actions and code,
//!   and the three-flag rule.
//! - [`may_decide`] and [`interpret`]: who may decide an approval, and what
//!   an answer means (an ambiguous answer is a denial).
//! - [`Limits`] and [`Meter`]: budgets that stop a turn cleanly.
//! - [`Approvals`]: the pending approvals of a host.

mod approvals;
mod budget;

pub use approvals::{Approvals, PendingApproval};
pub use budget::{Limits, Meter};

#[cfg(test)]
#[path = "verdict_tests.rs"]
mod verdict_tests;

use harness_core::access::{Access, GrantAnswer};
use harness_core::*;

/// What kind of app a caller is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppKind {
    /// Isonomic's own apps.
    FirstParty,
    /// A View (rendered content).
    View,
    /// Third-party code.
    ThirdParty,
}

/// Who is calling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    /// The desktop shell: sees every app.
    Shell,
    /// An app.
    App {
        /// Which.
        app: AppId,
        /// What kind.
        kind: AppKind,
    },
}

impl Caller {
    /// A first-party app.
    pub fn first_party(app: &str) -> Self {
        Caller::App {
            app: AppId::from(app),
            kind: AppKind::FirstParty,
        }
    }

    /// The caller's app, if it is one.
    pub fn app(&self) -> Option<&AppId> {
        match self {
            Caller::Shell => None,
            Caller::App { app, .. } => Some(app),
        }
    }

    /// Whether the caller may see things owned by `owner`.
    pub fn sees(&self, owner: &AppId) -> bool {
        match self {
            Caller::Shell => true,
            Caller::App { app, .. } => app == owner,
        }
    }
}

/// A token minted when a person acts in the UI; required to decide.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UserActionToken(pub String);

/// How much a session may do without asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Autonomy {
    /// Every call with any effect asks first.
    Strict,
    /// Outward actions and code ask first (the interactive default).
    #[default]
    Supervised,
    /// Outward actions run without asking unless the three-flag rule
    /// applies; code still asks.
    Trusted,
}

/// The session's data flags. Sticky: once set, they stay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// Private data has entered the session.
    pub private: bool,
    /// Untrusted content has entered the session.
    pub untrusted: bool,
}

impl Flags {
    /// Note the effects of a call that ran.
    pub fn note(&mut self, effects: &[Effect]) {
        for e in effects {
            match e {
                Effect::ReadsPrivateData => self.private = true,
                Effect::IngestsUntrustedContent => self.untrusted = true,
                _ => {}
            }
        }
    }

    /// Both flags are set: every outward action asks.
    pub fn three_flag(&self) -> bool {
        self.private && self.untrusted
    }
}

/// What policy says about a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Run it.
    Allow,
    /// Ask first, under this rule.
    Ask(ApprovalRule),
    /// Refuse, with a reason the model can read.
    Deny(String),
}

/// A session's policy profile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// The autonomy level.
    pub autonomy: Autonomy,
    /// Tools pre-approved for outward action (never for the three-flag rule).
    pub allow_outward: Vec<String>,
}

/// What resolving and the grant check said about a kit call (a tool with
/// a scope). Resolving failures never get here: they are the call's result.
#[derive(Clone, Copy, Debug)]
pub struct Resolved<'a> {
    /// What the call touches.
    pub access: &'a Access,
    /// The grant check's answer for the tool's act on that scope.
    pub grant: &'a GrantAnswer,
}

impl Policy {
    /// Judge a call to `spec` given the session's flags: a tool without a
    /// scope, as [`judge`](Self::judge) with nothing resolved.
    pub fn evaluate(&self, spec: &ToolSpec, flags: &Flags) -> Verdict {
        self.judge(spec, flags, None)
    }

    /// Judge a call: the first row of "How a call is decided" that applies
    /// (`docs/kits.md`). The call is judged on the effects its access
    /// reports, when a confined access reports any.
    pub fn judge(&self, spec: &ToolSpec, flags: &Flags, resolved: Option<Resolved<'_>>) -> Verdict {
        if spec.kind == ToolKind::Cell {
            return Verdict::Deny("Cell tools are not available yet.".into());
        }
        if let Some(GrantAnswer::Deny(reason)) = resolved.map(|r| r.grant) {
            return Verdict::Deny(reason.clone());
        }
        let access = resolved.map(|r| r.access);
        let effects = Access::narrow(access, &spec.effects);
        let has = |e: Effect| effects.contains(&e);
        let confined = access.is_some_and(|a| a.confined);
        if has(Effect::ActsOutward) && flags.three_flag() {
            return Verdict::Ask(ApprovalRule::ThreeFlag);
        }
        if self.autonomy == Autonomy::Strict && !effects.is_empty() {
            return Verdict::Ask(ApprovalRule::Autonomy);
        }
        if has(Effect::ExecutesCode) && !confined {
            return Verdict::Ask(ApprovalRule::ExecutesCode);
        }
        if has(Effect::ActsOutward) {
            let asks = match resolved {
                Some(r) => r.grant == &GrantAnswer::Ask,
                None => {
                    let pre = self.allow_outward.iter().any(|n| n == &spec.name);
                    !pre && self.autonomy == Autonomy::Supervised
                }
            };
            if asks {
                return Verdict::Ask(ApprovalRule::ActsOutward);
            }
        }
        if has(Effect::ModifiesData) && !confined {
            let covered =
                resolved.is_some_and(|r| r.grant == &GrantAnswer::Allow && r.access.reversible);
            if !covered {
                return Verdict::Ask(ApprovalRule::Grant);
            }
        }
        Verdict::Allow
    }
}

/// The longest an approval under `rule` may be remembered: only once for
/// the three-flag rule, code, the autonomy level, a call with no access, or
/// one that cannot be undone; otherwise what the access says.
pub fn rememberable(rule: &ApprovalRule, access: Option<&Access>) -> Remember {
    let once_rule = matches!(
        rule,
        ApprovalRule::ThreeFlag | ApprovalRule::ExecutesCode | ApprovalRule::Autonomy
    );
    match access {
        Some(a) if !once_rule && a.reversible => a.remember.clone().at_most(&Remember::Always),
        _ => Remember::Once,
    }
}

/// Why a caller may not decide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecideError {
    /// No user-action token, or one that does not verify.
    MissingUserAction,
    /// Views and third-party code may request approvals, never decide.
    NotAllowedToDecide,
    /// Another app's approval: answered as not found.
    NotFound,
}

/// Whether `caller` may decide an approval in a session owned by `owner`.
/// `verified` says whether the token checked out with the engine.
pub fn may_decide(
    caller: &Caller,
    owner: &AppId,
    token: Option<&UserActionToken>,
    verified: bool,
) -> Result<(), DecideError> {
    match caller {
        Caller::App { app, .. } if app != owner => return Err(DecideError::NotFound),
        Caller::App {
            kind: AppKind::View | AppKind::ThirdParty,
            ..
        } => return Err(DecideError::NotAllowedToDecide),
        _ => {}
    }
    if token.is_none() || !verified {
        return Err(DecideError::MissingUserAction);
    }
    Ok(())
}

/// A decision as given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionInput {
    /// The allow button.
    Allow,
    /// The deny button.
    Deny,
    /// Words; anything but a clear yes or no is ambiguous.
    Text(String),
}

const YES: &[&str] = &[
    "yes", "y", "allow", "approve", "approved", "ok", "okay", "go ahead", "do it",
];
const NO: &[&str] = &[
    "no", "n", "deny", "denied", "don't", "do not", "stop", "cancel",
];

/// What a decision means: an ambiguous answer is a denial.
pub fn interpret(input: &DecisionInput) -> (Decision, Basis, Option<String>) {
    match input {
        DecisionInput::Allow => (Decision::Allow, Basis::User, None),
        DecisionInput::Deny => (Decision::Deny, Basis::User, None),
        DecisionInput::Text(raw) => {
            let norm = raw
                .trim()
                .trim_end_matches(|c: char| c.is_ascii_punctuation())
                .to_lowercase();
            if YES.contains(&norm.as_str()) {
                (Decision::Allow, Basis::User, None)
            } else if NO.contains(&norm.as_str()) {
                (Decision::Deny, Basis::User, None)
            } else {
                (Decision::Deny, Basis::Ambiguous, Some(raw.clone()))
            }
        }
    }
}
