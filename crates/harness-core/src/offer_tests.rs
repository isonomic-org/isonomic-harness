// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::access::Scope;
use crate::display::display_capable;
use crate::kinds::{Concurrency, Effect, ScopeKind, ToolKind};
use crate::value::Value;

fn spec(name: &str, scope: ScopeKind, effects: Vec<Effect>) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: String::new(),
        input_schema: Value::Null,
        kind: ToolKind::Engine,
        effects,
        concurrency: Concurrency::Parallel,
        scope: Some(scope),
    }
}

fn held(effect: Effect, kind: ScopeKind) -> HeldGrant {
    HeldGrant {
        effect,
        scope: Scope {
            kind,
            id: "x".into(),
            label: "X".into(),
        },
        ask_first: false,
    }
}

const PROFILES: [Option<Profile>; 5] = [
    None,
    Some(Profile::Low),
    Some(Profile::Intel),
    Some(Profile::Standard),
    Some(Profile::High),
];

#[test]
fn the_table_reads_as_the_design() {
    let row = |p: Option<Profile>| {
        let o = offer(p.as_ref(), "local:default", true);
        (o.display_tools, o.kit_tools, o.result_cap)
    };
    assert_eq!(row(None), (0, 0, None));
    assert_eq!(row(Some(Profile::Low)), (0, 0, None));
    assert_eq!(row(Some(Profile::Other("tiny".into()))), (0, 0, None));
    assert_eq!(row(Some(Profile::Intel)), (5, 4, Some(2048)));
    assert_eq!(row(Some(Profile::Standard)), (5, 12, Some(8192)));
    assert_eq!(row(Some(Profile::High)), (5, 20, Some(16384)));
    for p in PROFILES {
        assert_eq!(offer(p.as_ref(), SMALLEST_MODEL, true), Offer::NONE);
        assert_eq!(offer(p.as_ref(), "local:default", false), Offer::NONE);
    }
}

#[test]
fn display_capable_is_the_table() {
    for p in PROFILES {
        for model in ["local:default", SMALLEST_MODEL, "local:large"] {
            let want = offer(p.as_ref(), model, true).display_tools > 0;
            assert_eq!(display_capable(p.as_ref(), model), want);
        }
    }
}

fn files() -> Vec<ToolSpec> {
    use Effect::*;
    vec![
        spec("list_dir", ScopeKind::Folder, vec![ReadsPrivateData]),
        spec("write_file", ScopeKind::Folder, vec![ModifiesData]),
        spec("read_url", ScopeKind::Web, vec![IngestsUntrustedContent]),
        spec("page_act", ScopeKind::Site, vec![ActsOutward]),
        spec("run_command", ScopeKind::Folder, vec![ExecutesCode]),
    ]
}

fn cands(specs: &[ToolSpec]) -> Vec<KitCandidate<'_>> {
    let kit = |n: &str| match n {
        "read_url" | "page_act" => "web",
        "run_command" => "shell",
        _ => "files",
    };
    specs
        .iter()
        .map(|s| KitCandidate {
            spec: s,
            kit: kit(&s.name),
            core: matches!(s.name.as_str(), "list_dir" | "read_url" | "run_command"),
        })
        .collect()
}

#[test]
fn no_grant_no_tool_and_core_first() {
    let specs = files();
    let c = cands(&specs);
    let kits: Vec<String> = ["files", "web"].map(String::from).to_vec();
    assert!(admit(&kits, &c, &[], 20).is_empty());
    let read = [held(Effect::ReadsPrivateData, ScopeKind::Folder)];
    assert_eq!(admit(&kits, &c, &read, 20), ["list_dir"]);
    let all = [
        held(Effect::ReadsPrivateData, ScopeKind::Folder),
        held(Effect::ModifiesData, ScopeKind::Folder),
        held(Effect::IngestsUntrustedContent, ScopeKind::Web),
        held(Effect::ActsOutward, ScopeKind::Site),
        held(Effect::ExecutesCode, ScopeKind::Folder),
    ];
    assert_eq!(
        admit(&kits, &c, &all, 20),
        ["list_dir", "read_url", "write_file", "page_act"]
    );
    assert_eq!(admit(&kits, &c, &all, 2), ["list_dir", "read_url"]);
    let web_first: Vec<String> = ["web", "files"].map(String::from).to_vec();
    assert_eq!(
        admit(&web_first, &c, &all, 3),
        ["read_url", "list_dir", "page_act"]
    );
}

/// Invariant 5: the offered tools are always inside the preset's kits, the
/// app's grants and the budget.
#[test]
fn the_offer_stays_inside_kits_grants_and_budget() {
    let specs = files();
    let c = cands(&specs);
    let pool = [
        held(Effect::ReadsPrivateData, ScopeKind::Folder),
        held(Effect::ModifiesData, ScopeKind::Folder),
        held(Effect::IngestsUntrustedContent, ScopeKind::Web),
        held(Effect::ActsOutward, ScopeKind::Site),
        held(Effect::ExecutesCode, ScopeKind::Folder),
    ];
    let kit_sets: [&[&str]; 4] = [
        &[],
        &["files"],
        &["web", "files"],
        &["shell", "web", "files"],
    ];
    for mask in 0u32..32 {
        let h: Vec<HeldGrant> = (0..5)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| pool[i].clone())
            .collect();
        for ks in kit_sets {
            let kits: Vec<String> = ks.iter().map(|s| s.to_string()).collect();
            for budget in 0..6 {
                let got = admit(&kits, &c, &h, budget);
                assert!(got.len() <= budget);
                for name in &got {
                    let cand = c.iter().find(|x| &x.spec.name == name).unwrap();
                    assert!(kits.iter().any(|k| k == cand.kit));
                    let e = grant_effect(cand.spec.scope.as_ref(), &cand.spec.effects).unwrap();
                    assert!(h.iter().any(|g| g.effect == e));
                }
            }
        }
    }
}
