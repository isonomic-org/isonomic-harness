// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a kit call touches, and what may cover it: scopes, the access a
//! tool's service resolves before policy judges the call, and the answers
//! of a grant check (`docs/kits.md`). Pure types.

use crate::kinds::{Effect, Remember, ScopeKind};
use serde::{Deserialize, Serialize};

/// What a call touches: a kind, an id and a label for the person, such as
/// folder "Design" or site "example.org".
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope {
    /// Folder, web, site or skills.
    pub kind: ScopeKind,
    /// Which one: the linked folder's id, `pages` or `search`, the site's
    /// registered name.
    pub id: String,
    /// What the person sees.
    pub label: String,
}

/// One labelled value of an approval's preview: the real path, the size
/// change, the full command. Never cut short.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewItem {
    /// What it is.
    pub label: String,
    /// Its value, whole.
    pub value: String,
}

/// What a kit call will touch, resolved by the tool's service before policy
/// judges it. The run uses what resolving opened; it never looks a name up
/// a second time. Not part of the event schema: what the person needs from
/// it travels in the approval fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Access {
    /// What the call touches.
    pub scope: Scope,
    /// Labelled values for the approval.
    pub preview: Vec<PreviewItem>,
    /// Whether the engine can undo it.
    pub reversible: bool,
    /// The longest the person may be offered.
    pub remember: Remember,
    /// Whether the backend enforces the grants mechanically (a Cell).
    pub confined: bool,
    /// The effects the call can really have; `None` means the tool's own.
    /// Honoured only when confined, and only to drop effects.
    pub effects: Option<Vec<Effect>>,
}

impl Access {
    /// The effects a call to a tool with `declared` effects is judged on:
    /// the access's, when it is confined and reports any, kept only where
    /// the tool declares them. An access never adds an effect.
    pub fn narrow(access: Option<&Access>, declared: &[Effect]) -> Vec<Effect> {
        match access {
            Some(Access {
                confined: true,
                effects: Some(reported),
                ..
            }) => declared
                .iter()
                .filter(|e| reported.contains(e))
                .cloned()
                .collect(),
            _ => declared.to_vec(),
        }
    }
}

/// A grant check's answer for one effect on one scope: the same three
/// answers the identity check gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantAnswer {
    /// A grant covers it without asking.
    Allow,
    /// A grant covers it, marked "ask first".
    Ask,
    /// Nothing covers it; the reason is readable.
    Deny(String),
}

/// One grant a session can use, as `held` lists it: an effect on a scope.
/// Feeds the offer rule and the context line.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HeldGrant {
    /// The act it allows: read, write, fetch, act or execute, as an effect.
    pub effect: Effect,
    /// What it covers.
    pub scope: Scope,
    /// Marked "ask first".
    pub ask_first: bool,
}

/// The act a tool's grant check is made for, as an effect: read or write
/// for a folder tool (execute for a command), fetch for the web, act for a
/// site, write for skills. `None` for a tool with no scope.
pub fn grant_effect(scope: Option<&ScopeKind>, effects: &[Effect]) -> Option<Effect> {
    let has = |e: Effect| effects.contains(&e);
    Some(match scope? {
        ScopeKind::Folder if has(Effect::ExecutesCode) => Effect::ExecutesCode,
        ScopeKind::Folder if has(Effect::ModifiesData) => Effect::ModifiesData,
        ScopeKind::Folder => Effect::ReadsPrivateData,
        ScopeKind::Web => Effect::IngestsUntrustedContent,
        ScopeKind::Site => Effect::ActsOutward,
        ScopeKind::Skills => Effect::ModifiesData,
        ScopeKind::Other(_) => return None,
    })
}

impl Remember {
    /// Its length: once 0, session 1, always 2. A value from a newer
    /// revision counts as once.
    pub fn rank(&self) -> u8 {
        match self {
            Remember::Session => 1,
            Remember::Always => 2,
            _ => 0,
        }
    }

    /// The shorter of two.
    pub fn at_most(self, limit: &Remember) -> Remember {
        let rank = self.rank().min(limit.rank());
        [Remember::Once, Remember::Session, Remember::Always][rank as usize].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Effect; 5] = [
        Effect::ReadsPrivateData,
        Effect::IngestsUntrustedContent,
        Effect::ActsOutward,
        Effect::ExecutesCode,
        Effect::ModifiesData,
    ];

    fn subset(mask: u32) -> Vec<Effect> {
        (0..5)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| ALL[i].clone())
            .collect()
    }

    /// Invariant 12: narrowed effects are a subset of the tool's, and are
    /// narrowed only when the access is confined.
    #[test]
    fn an_access_only_narrows_and_only_when_confined() {
        let scope = Scope {
            kind: ScopeKind::Folder,
            id: "f".into(),
            label: "F".into(),
        };
        for declared in 0..32 {
            for reported in 0..32 {
                for confined in [false, true] {
                    let a = Access {
                        scope: scope.clone(),
                        preview: vec![],
                        reversible: true,
                        remember: Remember::Session,
                        confined,
                        effects: Some(subset(reported)),
                    };
                    let tool = subset(declared);
                    let got = Access::narrow(Some(&a), &tool);
                    assert!(got.iter().all(|e| tool.contains(e)));
                    if !confined {
                        assert_eq!(got, tool);
                    }
                }
            }
            assert_eq!(Access::narrow(None, &subset(declared)), subset(declared));
        }
    }

    #[test]
    fn the_act_follows_the_scope() {
        use Effect::*;
        let f = Some(&ScopeKind::Folder);
        assert_eq!(
            grant_effect(f, &[ReadsPrivateData, IngestsUntrustedContent]),
            Some(ReadsPrivateData)
        );
        assert_eq!(grant_effect(f, &[ModifiesData]), Some(ModifiesData));
        assert_eq!(
            grant_effect(f, &[ExecutesCode, ActsOutward]),
            Some(ExecutesCode)
        );
        assert_eq!(
            grant_effect(Some(&ScopeKind::Web), &[ActsOutward]),
            Some(IngestsUntrustedContent)
        );
        assert_eq!(grant_effect(Some(&ScopeKind::Site), &[]), Some(ActsOutward));
        assert_eq!(grant_effect(None, &[ModifiesData]), None);
        assert_eq!(grant_effect(Some(&ScopeKind::Other("x".into())), &[]), None);
    }

    #[test]
    fn remember_never_grows() {
        let all = [
            Remember::Once,
            Remember::Session,
            Remember::Always,
            Remember::Other("x".into()),
        ];
        for a in &all {
            for b in &all {
                let got = a.clone().at_most(b);
                assert!(got.rank() <= a.rank().min(b.rank()));
                assert!(!matches!(got, Remember::Other(_)));
            }
        }
    }
}
