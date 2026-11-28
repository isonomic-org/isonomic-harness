// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a session is offered: one capability table for a hardware profile
//! and a model, and the rule that admits kit tools (`docs/kits.md`).
//! [`display_capable`](crate::display::display_capable) is a thin wrapper
//! over the table.

use crate::access::{grant_effect, HeldGrant};
use crate::display::SMALLEST_MODEL;
use crate::kinds::Profile;
use crate::payload::ToolSpec;

/// The tool kits a preset may name in `[kits] use`, in no order.
pub const KITS: &[&str] = &["files", "shell", "web", "browser", "computer"];

/// The most display tools an interactive preset offers.
pub const MAX_DISPLAY_TOOLS: usize = 5;

/// What the capability table allows a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    /// How many display tools; 0 offers none.
    pub display_tools: usize,
    /// How many kit tools, core tools first; 0 offers none.
    pub kit_tools: usize,
    /// The largest result per kit call, in bytes; `None` when no kit tool is
    /// offered.
    pub result_cap: Option<usize>,
}

impl Offer {
    /// Nothing.
    pub const NONE: Offer = Offer {
        display_tools: 0,
        kit_tools: 0,
        result_cap: None,
    };
}

/// The capability table. Low, unknown or no profile, the smallest local
/// model, and a model that cannot call tools get nothing. Otherwise:
/// intel 4 kit tools and 2 KiB, standard 12 and 8 KiB, high 20 and 16 KiB,
/// each with the display spec's display tools. Starting values, to be
/// measured on the Intel machine.
pub fn offer(profile: Option<&Profile>, model: &str, calls_tools: bool) -> Offer {
    if model == SMALLEST_MODEL || !calls_tools {
        return Offer::NONE;
    }
    let (kit_tools, cap) = match profile {
        Some(Profile::Intel) => (4, 2 * 1024),
        Some(Profile::Standard) => (12, 8 * 1024),
        Some(Profile::High) => (20, 16 * 1024),
        _ => return Offer::NONE,
    };
    Offer {
        display_tools: MAX_DISPLAY_TOOLS,
        kit_tools,
        result_cap: Some(cap),
    }
}

/// One kit tool as the offer rule sees it.
#[derive(Clone, Copy, Debug)]
pub struct KitCandidate<'a> {
    /// Its spec; `scope` must be set.
    pub spec: &'a ToolSpec,
    /// Its kit, one of [`KITS`].
    pub kit: &'a str,
    /// Admitted before the kit's other tools.
    pub core: bool,
}

/// The kit tools a session is offered, by name, in admission order. A tool
/// passes when its kit is in `kits` (the preset's order) and a held grant
/// could cover its act on its scope kind. Core tools are admitted first, kit
/// by kit in the preset's order, then the rest in the same order, up to
/// `budget`.
pub fn admit(
    kits: &[String],
    candidates: &[KitCandidate<'_>],
    held: &[HeldGrant],
    budget: usize,
) -> Vec<String> {
    let usable = |c: &KitCandidate<'_>| {
        let effect = grant_effect(c.spec.scope.as_ref(), &c.spec.effects);
        effect.is_some_and(|e| {
            held.iter()
                .any(|h| h.effect == e && Some(&h.scope.kind) == c.spec.scope.as_ref())
        })
    };
    let mut out: Vec<String> = Vec::new();
    for core in [true, false] {
        for kit in kits {
            for c in candidates {
                let fits = out.len() < budget && !out.contains(&c.spec.name);
                if fits && c.core == core && c.kit == kit && usable(c) {
                    out.push(c.spec.name.clone());
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "offer_tests.rs"]
mod tests;
