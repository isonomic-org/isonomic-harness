// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Grants: which app may apply which effect to which scope, asking first or
//! not. The identity library issues and checks them in the engine; the
//! harness sees them through [`Grants`], with [`MemoryGrants`] as the fake
//! and [`contract`] as the suite every implementation passes
//! (`docs/kits.md`).
//!
//! The host checks to decide whether to ask; the engine's service checks
//! again at the moment of the operation, and that second check is the one
//! that protects the person.

use harness_core::access::{GrantAnswer, HeldGrant, Scope};
use harness_core::*;
use std::sync::Mutex;

/// Grants as the harness sees them.
pub trait Grants: Send + Sync {
    /// Which effects on which scopes the session can use at all: feeds the
    /// offer rule and the context line.
    fn held(&self, app: &AppId, session: &SessionId) -> Vec<HeldGrant>;

    /// Allow, ask, or deny with a reason, for `effect` (the tool's act) on
    /// `scope`.
    fn check(
        &self,
        app: &AppId,
        session: &SessionId,
        effect: &Effect,
        scope: &Scope,
    ) -> GrantAnswer;

    /// A person approved with "this session" or "always". The host calls it
    /// only after a decision that carried a verified user-action token.
    /// `Once` changes nothing.
    fn remember(
        &self,
        app: &AppId,
        session: &SessionId,
        effect: &Effect,
        scope: &Scope,
        how: &Remember,
    );

    /// These sessions were purged: their session approvals go with them.
    fn forget(&self, sessions: &[SessionId]);
}

/// A standing grant, as issued: `scope.id` of `*` covers every scope of its
/// kind (the web grant's "act on any site").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    /// The app it is for.
    pub app: AppId,
    /// The act it allows, as an effect.
    pub effect: Effect,
    /// What it covers.
    pub scope: Scope,
    /// Marked "ask first".
    pub ask_first: bool,
}

/// In-memory grants: standing grants and per-session approvals.
#[derive(Debug, Default)]
pub struct MemoryGrants {
    standing: Mutex<Vec<Standing>>,
    approvals: Mutex<Vec<(SessionId, AppId, Effect, Scope)>>,
}

fn covers(grant: &Scope, scope: &Scope) -> bool {
    grant.kind == scope.kind && (grant.id == "*" || grant.id == scope.id)
}

impl MemoryGrants {
    /// With these standing grants.
    pub fn new(standing: Vec<Standing>) -> Self {
        Self {
            standing: Mutex::new(standing),
            approvals: Mutex::default(),
        }
    }

    /// Issue a standing grant.
    pub fn issue(&self, grant: Standing) {
        lock(&self.standing).push(grant);
    }

    /// Revoke every standing grant of `app` on `scope`.
    pub fn revoke(&self, app: &AppId, scope: &Scope) {
        lock(&self.standing).retain(|g| !(&g.app == app && &g.scope == scope));
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Grants for MemoryGrants {
    fn held(&self, app: &AppId, session: &SessionId) -> Vec<HeldGrant> {
        let mut out: Vec<HeldGrant> = lock(&self.standing)
            .iter()
            .filter(|g| &g.app == app)
            .map(|g| HeldGrant {
                effect: g.effect.clone(),
                scope: g.scope.clone(),
                ask_first: g.ask_first,
            })
            .collect();
        for (s, a, effect, scope) in lock(&self.approvals).iter() {
            if s == session && a == app {
                out.push(HeldGrant {
                    effect: effect.clone(),
                    scope: scope.clone(),
                    ask_first: false,
                });
            }
        }
        out
    }

    fn check(
        &self,
        app: &AppId,
        session: &SessionId,
        effect: &Effect,
        scope: &Scope,
    ) -> GrantAnswer {
        let approved = lock(&self.approvals)
            .iter()
            .any(|(s, a, e, sc)| s == session && a == app && e == effect && sc == scope);
        if approved {
            return GrantAnswer::Allow;
        }
        let standing = lock(&self.standing);
        let matching: Vec<&Standing> = standing
            .iter()
            .filter(|g| &g.app == app && &g.effect == effect && covers(&g.scope, scope))
            .collect();
        if matching.iter().any(|g| !g.ask_first) {
            GrantAnswer::Allow
        } else if !matching.is_empty() {
            GrantAnswer::Ask
        } else {
            GrantAnswer::Deny(format!("No permission covers {}.", scope.label))
        }
    }

    fn remember(
        &self,
        app: &AppId,
        session: &SessionId,
        effect: &Effect,
        scope: &Scope,
        how: &Remember,
    ) {
        match how {
            Remember::Session => lock(&self.approvals).push((
                session.clone(),
                app.clone(),
                effect.clone(),
                scope.clone(),
            )),
            Remember::Always => self.issue(Standing {
                app: app.clone(),
                effect: effect.clone(),
                scope: scope.clone(),
                ask_first: false,
            }),
            _ => {}
        }
    }

    fn forget(&self, sessions: &[SessionId]) {
        lock(&self.approvals).retain(|(s, ..)| !sessions.contains(s));
    }
}

/// The grants contract: every [`Grants`] implementation passes it, the fake
/// here and the engine's. `make` builds one holding exactly these standing
/// grants.
pub mod contract {
    use super::*;

    fn folder(id: &str) -> Scope {
        Scope {
            kind: ScopeKind::Folder,
            id: id.into(),
            label: id.into(),
        }
    }

    /// Run the suite; panics on the first broken rule.
    pub fn run<G: Grants>(make: impl Fn(Vec<Standing>) -> G) {
        let (app, other) = (AppId::from("chat"), AppId::from("notes"));
        let (s1, s2) = (SessionId::from("s1"), SessionId::from("s2"));
        let (read, write) = (Effect::ReadsPrivateData, Effect::ModifiesData);
        let (design, notes) = (folder("design"), folder("notes"));
        let issued = |effect: &Effect, scope: &Scope, ask_first| Standing {
            app: app.clone(),
            effect: effect.clone(),
            scope: scope.clone(),
            ask_first,
        };

        // Nothing issued: deny, readably, and nothing held.
        let g = make(vec![]);
        assert!(
            matches!(g.check(&app, &s1, &read, &design), GrantAnswer::Deny(r) if !r.is_empty())
        );
        assert!(g.held(&app, &s1).is_empty());

        // A read-and-write folder: read without asking, write asking first,
        // for that app and that folder only.
        let g = make(vec![
            issued(&read, &design, false),
            issued(&write, &design, true),
        ]);
        assert_eq!(g.check(&app, &s1, &read, &design), GrantAnswer::Allow);
        assert_eq!(g.check(&app, &s1, &write, &design), GrantAnswer::Ask);
        assert!(matches!(
            g.check(&app, &s1, &read, &notes),
            GrantAnswer::Deny(_)
        ));
        assert!(matches!(
            g.check(&other, &s1, &read, &design),
            GrantAnswer::Deny(_)
        ));
        assert_eq!(g.held(&app, &s1).len(), 2);
        assert!(g.held(&other, &s1).is_empty());

        // Once changes nothing; this session covers the same effect and
        // scope in that session only (a fork has its own id).
        g.remember(&app, &s1, &write, &design, &Remember::Once);
        assert_eq!(g.check(&app, &s1, &write, &design), GrantAnswer::Ask);
        g.remember(&app, &s1, &write, &design, &Remember::Session);
        assert_eq!(g.check(&app, &s1, &write, &design), GrantAnswer::Allow);
        assert_eq!(g.check(&app, &s2, &write, &design), GrantAnswer::Ask);
        assert!(matches!(
            g.check(&app, &s1, &write, &notes),
            GrantAnswer::Deny(_)
        ));
        assert!(matches!(
            g.check(&app, &s1, &read, &notes),
            GrantAnswer::Deny(_)
        ));

        // Purge takes the session's approvals with it.
        g.forget(std::slice::from_ref(&s1));
        assert_eq!(g.check(&app, &s1, &write, &design), GrantAnswer::Ask);

        // Always issues the grant again without "ask first".
        g.remember(&app, &s1, &write, &design, &Remember::Always);
        assert_eq!(g.check(&app, &s2, &write, &design), GrantAnswer::Allow);

        // A grant on every site covers each site, asking first.
        let any = Scope {
            kind: ScopeKind::Site,
            id: "*".into(),
            label: "any site".into(),
        };
        let site = Scope {
            kind: ScopeKind::Site,
            id: "example.org".into(),
            label: "example.org".into(),
        };
        let g = make(vec![issued(&Effect::ActsOutward, &any, true)]);
        assert_eq!(
            g.check(&app, &s1, &Effect::ActsOutward, &site),
            GrantAnswer::Ask
        );
        g.remember(&app, &s1, &Effect::ActsOutward, &site, &Remember::Session);
        assert_eq!(
            g.check(&app, &s1, &Effect::ActsOutward, &site),
            GrantAnswer::Allow
        );
        let elsewhere = Scope {
            id: "example.net".into(),
            label: "example.net".into(),
            ..site.clone()
        };
        assert_eq!(
            g.check(&app, &s1, &Effect::ActsOutward, &elsewhere),
            GrantAnswer::Ask
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_fake_passes_the_contract() {
        super::contract::run(super::MemoryGrants::new);
    }
}
