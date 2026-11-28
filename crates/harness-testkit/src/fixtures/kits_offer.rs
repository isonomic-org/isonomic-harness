// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! kits-offer: one session per profile, model and set of grants, each with
//! one turn whose `step.start.tools` is exactly what the offer table and the
//! grants admit from fake files, shell and web tools (`docs/kits.md`). No
//! grant, no tool; low, unknown and no profile, and the smallest model, get
//! none. The preset offers no display widgets; the display fixture covers
//! those.

use crate::recorder::Recorder;
use crate::tools::kit_fakes;
use harness_core::access::{HeldGrant, Scope};
use harness_core::offer::{admit, offer, KitCandidate};
use harness_core::*;

/// The preset's `[kits] use`.
const KITS: &[&str] = &["files", "shell", "web"];

fn held(effect: Effect, kind: ScopeKind, id: &str) -> HeldGrant {
    HeldGrant {
        effect,
        scope: Scope {
            kind,
            id: id.into(),
            label: if id == "*" {
                "any site".into()
            } else {
                id.into()
            },
        },
        ask_first: false,
    }
}

fn every_grant() -> Vec<HeldGrant> {
    vec![
        held(Effect::ReadsPrivateData, ScopeKind::Folder, "design"),
        held(Effect::ModifiesData, ScopeKind::Folder, "design"),
        held(Effect::ExecutesCode, ScopeKind::Folder, "design"),
        held(Effect::IngestsUntrustedContent, ScopeKind::Web, "*"),
    ]
}

/// The cases, in session order: profile, model, grants.
pub fn cases() -> Vec<(Option<Profile>, &'static str, Vec<HeldGrant>)> {
    vec![
        (Some(Profile::Standard), "local:small", Vec::new()),
        (
            Some(Profile::Standard),
            "local:small",
            vec![held(Effect::ReadsPrivateData, ScopeKind::Folder, "design")],
        ),
        (Some(Profile::Standard), "local:small", every_grant()),
        (Some(Profile::Intel), "local:small", every_grant()),
        (Some(Profile::High), "local:small", every_grant()),
        (Some(Profile::Low), "local:small", every_grant()),
        (None, "local:small", every_grant()),
        (Some(Profile::Standard), "local:smallest", every_grant()),
    ]
}

/// What the table and the grants admit, as specs in admission order.
pub fn admitted(profile: Option<&Profile>, model: &str, grants: &[HeldGrant]) -> Vec<ToolSpec> {
    let fakes = kit_fakes();
    let candidates: Vec<KitCandidate<'_>> = fakes
        .iter()
        .map(|(spec, kit, core)| KitCandidate {
            spec,
            kit,
            core: *core,
        })
        .collect();
    let kits: Vec<String> = KITS.iter().map(|k| (*k).to_owned()).collect();
    let budget = offer(profile, model, true).kit_tools;
    admit(&kits, &candidates, grants, budget)
        .iter()
        .filter_map(|n| fakes.iter().find(|(s, ..)| &s.name == n))
        .map(|(s, ..)| s.clone())
        .collect()
}

pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    for (profile, model, grants) in cases() {
        let start = SessionStart {
            class: SessionClass::Interactive,
            app: AppId::from("notes"),
            principal: Principal {
                kind: PrincipalKind::User,
                id: None,
            },
            preset: PresetRef {
                id: PresetId::from("notes"),
                version: 1,
            },
            parent: None,
            profile: profile.clone(),
            model: None,
        };
        let mut s = r.session(start, model);
        let tools = admitted(profile.as_ref(), model, &grants);
        let folder_tool = tools.iter().any(|t| t.scope == Some(ScopeKind::Folder));
        let mut folders: Vec<String> = Vec::new();
        for g in grants
            .iter()
            .filter(|g| folder_tool && g.scope.kind == ScopeKind::Folder)
        {
            if !folders.contains(&g.scope.label) {
                folders.push(g.scope.label.clone());
            }
        }
        r.turn(&mut s, Cause::User, None);
        r.user(&mut s, "What is in my design folder?");
        r.context_with(&mut s, None, folders);
        r.step(&mut s, Purpose::Reply, (!tools.is_empty()).then_some(tools));
        r.reply(
            &mut s,
            vec![Block::text("Here is what I can see.")],
            StopReason::End,
            None,
        );
        r.end_step(&mut s, StepOutcome::Complete);
        r.end_turn(&mut s, TurnStop::Done, None);
    }
    r.finish()
}
