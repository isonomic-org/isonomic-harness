// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The verdict table, row by row at each autonomy level, and the
//! invariants it keeps (`docs/kits.md`, "How a call is decided").

use super::*;
use harness_core::access::Scope;

const LEVELS: [Autonomy; 3] = [Autonomy::Strict, Autonomy::Supervised, Autonomy::Trusted];
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

fn spec(name: &str, effects: Vec<Effect>, scope: Option<ScopeKind>) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: String::new(),
        input_schema: Value::Null,
        kind: ToolKind::Engine,
        effects,
        concurrency: Concurrency::Serial,
        scope,
    }
}

fn access(reversible: bool, confined: bool) -> Access {
    Access {
        scope: Scope {
            kind: ScopeKind::Folder,
            id: "f1".into(),
            label: "Design".into(),
        },
        preview: vec![],
        reversible,
        remember: Remember::Always,
        confined,
        effects: None,
    }
}

fn policy(autonomy: Autonomy) -> Policy {
    Policy {
        autonomy,
        allow_outward: vec!["pre".into()],
    }
}

const BOTH: Flags = Flags {
    private: true,
    untrusted: true,
};

/// The rules as they stood before kits (cba5524), for the replay check.
fn before(p: &Policy, spec: &ToolSpec, flags: &Flags) -> Verdict {
    if spec.kind == ToolKind::Cell {
        return Verdict::Deny("Cell tools are not available yet.".into());
    }
    let has = |e: Effect| spec.effects.contains(&e);
    if p.autonomy == Autonomy::Strict && !spec.effects.is_empty() {
        if flags.three_flag() && has(Effect::ActsOutward) {
            return Verdict::Ask(ApprovalRule::ThreeFlag);
        }
        return Verdict::Ask(ApprovalRule::Autonomy);
    }
    if has(Effect::ActsOutward) {
        if flags.three_flag() {
            return Verdict::Ask(ApprovalRule::ThreeFlag);
        }
        let pre = p.allow_outward.iter().any(|n| n == &spec.name);
        if !pre && p.autonomy == Autonomy::Supervised {
            return Verdict::Ask(ApprovalRule::ActsOutward);
        }
    }
    if has(Effect::ExecutesCode) {
        return Verdict::Ask(ApprovalRule::ExecutesCode);
    }
    Verdict::Allow
}

/// Every existing tool gets the verdict it gets today: any mix of the four
/// old effects that no existing tool carries both outward and code.
#[test]
fn every_existing_tool_keeps_its_verdict() {
    for mask in 0..16 {
        let effects = subset(mask);
        if effects.contains(&Effect::ActsOutward) && effects.contains(&Effect::ExecutesCode) {
            continue;
        }
        for name in ["pre", "other"] {
            for kind in [ToolKind::Engine, ToolKind::Client, ToolKind::Cell] {
                let mut s = spec(name, effects.clone(), None);
                s.kind = kind;
                for level in LEVELS {
                    for (private, untrusted) in [(false, false), (true, false), (true, true)] {
                        let f = Flags { private, untrusted };
                        let p = policy(level);
                        assert_eq!(
                            p.evaluate(&s, &f),
                            before(&p, &s, &f),
                            "{s:?} {level:?} {f:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_table_row_by_row() {
    use ApprovalRule as R;
    use Effect::*;
    let ask = |r| Verdict::Ask(r);
    let none = Flags::default();
    let (rev, irr, cell) = (
        access(true, false),
        access(false, false),
        access(true, true),
    );
    let (allow, ask_first) = (GrantAnswer::Allow, GrantAnswer::Ask);
    let deny = GrantAnswer::Deny("Design is not a linked folder.".into());
    let r = |a: &'static Access, g: &'static GrantAnswer| {
        Some(Resolved {
            access: a,
            grant: g,
        })
    };
    let leak = |x: Access| -> &'static Access { Box::leak(Box::new(x)) };
    let leakg = |x: GrantAnswer| -> &'static GrantAnswer { Box::leak(Box::new(x)) };
    let (rev, irr, cell) = (leak(rev), leak(irr), leak(cell));
    let (allow, ask_first, deny) = (leakg(allow), leakg(ask_first), leakg(deny));
    let write = spec("write_file", vec![ModifiesData], Some(ScopeKind::Folder));
    let run = spec(
        "run_command",
        vec![ExecutesCode, ActsOutward],
        Some(ScopeKind::Folder),
    );
    let act = spec("page_act", vec![ActsOutward], Some(ScopeKind::Site));
    let read = spec("read_file", vec![ReadsPrivateData], Some(ScopeKind::Folder));
    let mut cell_tool = read.clone();
    cell_tool.kind = ToolKind::Cell;
    for level in LEVELS {
        let p = policy(level);
        let strict = level == Autonomy::Strict;
        // Row 1.
        assert!(matches!(
            p.judge(&cell_tool, &none, r(rev, allow)),
            Verdict::Deny(_)
        ));
        // Row 2: a grant deny is the result, never an approval.
        for s in [&write, &run, &act, &read] {
            assert_eq!(
                p.judge(s, &BOTH, r(rev, deny)),
                Verdict::Deny("Design is not a linked folder.".into())
            );
        }
        // Row 3, at every level.
        assert_eq!(p.judge(&act, &BOTH, r(rev, allow)), ask(R::ThreeFlag));
        assert_eq!(p.judge(&run, &BOTH, r(cell, allow)), ask(R::ThreeFlag));
        // Row 4.
        let want = if strict {
            ask(R::Autonomy)
        } else {
            Verdict::Allow
        };
        assert_eq!(p.judge(&read, &none, r(rev, allow)), want);
        // Row 5: code always asks on the host, and is the code card even
        // when it also acts outward; a confined access skips it.
        let code = if strict {
            ask(R::Autonomy)
        } else {
            ask(R::ExecutesCode)
        };
        assert_eq!(p.judge(&run, &none, r(rev, allow)), code);
        let in_cell = if strict {
            ask(R::Autonomy)
        } else {
            Verdict::Allow
        };
        assert_eq!(p.judge(&run, &none, r(cell, allow)), in_cell);
        // Row 6: with a scope, the grant answer decides at every level.
        let site = if strict {
            ask(R::Autonomy)
        } else {
            ask(R::ActsOutward)
        };
        assert_eq!(p.judge(&act, &none, r(rev, ask_first)), site);
        let site_ok = if strict {
            ask(R::Autonomy)
        } else {
            Verdict::Allow
        };
        assert_eq!(p.judge(&act, &none, r(rev, allow)), site_ok);
        // Row 7.
        let grant = if strict {
            ask(R::Autonomy)
        } else {
            ask(R::Grant)
        };
        assert_eq!(p.judge(&write, &none, r(rev, ask_first)), grant);
        assert_eq!(p.judge(&write, &none, r(irr, allow)), grant);
        assert_eq!(p.judge(&write, &none, None), grant);
        let covered = if strict {
            ask(R::Autonomy)
        } else {
            Verdict::Allow
        };
        assert_eq!(p.judge(&write, &none, r(rev, allow)), covered);
        assert_eq!(p.judge(&write, &none, r(cell, ask_first)), covered);
    }
}

/// Invariants 2 and 3, and the remember rule: code, the three-flag rule,
/// the autonomy level and a change that cannot be undone offer only once;
/// once both flags are set every outward call asks.
#[test]
fn what_may_be_remembered_and_the_flags() {
    let answers = [GrantAnswer::Allow, GrantAnswer::Ask];
    for mask in 0..32 {
        let effects = subset(mask);
        for level in LEVELS {
            for reversible in [false, true] {
                for confined in [false, true] {
                    for g in &answers {
                        let a = access(reversible, confined);
                        let s = spec("t", effects.clone(), Some(ScopeKind::Folder));
                        let res = Some(Resolved {
                            access: &a,
                            grant: g,
                        });
                        let v = policy(level).judge(&s, &BOTH, res);
                        if effects.contains(&Effect::ActsOutward) {
                            assert_eq!(v, Verdict::Ask(ApprovalRule::ThreeFlag));
                        }
                        let Verdict::Ask(rule) = v else { continue };
                        let most = rememberable(&rule, Some(&a));
                        let once = matches!(
                            rule,
                            ApprovalRule::ThreeFlag
                                | ApprovalRule::ExecutesCode
                                | ApprovalRule::Autonomy
                        ) || !reversible;
                        if once {
                            assert_eq!(most, Remember::Once);
                        }
                        if effects.contains(&Effect::ExecutesCode) && !confined {
                            assert_eq!(most, Remember::Once);
                        }
                    }
                }
            }
        }
    }
    assert_eq!(rememberable(&ApprovalRule::Grant, None), Remember::Once);
}

/// A confined access may narrow effects, an unconfined one may not, and
/// none may add one.
#[test]
fn only_a_confined_access_narrows() {
    let s = spec(
        "run_command",
        vec![Effect::ExecutesCode, Effect::ActsOutward],
        Some(ScopeKind::Folder),
    );
    let p = policy(Autonomy::Trusted);
    let mut a = access(true, false);
    a.effects = Some(vec![]);
    let res = Some(Resolved {
        access: &a,
        grant: &GrantAnswer::Allow,
    });
    assert_eq!(
        p.judge(&s, &BOTH, res),
        Verdict::Ask(ApprovalRule::ThreeFlag)
    );
    a.confined = true;
    let res = Some(Resolved {
        access: &a,
        grant: &GrantAnswer::Allow,
    });
    assert_eq!(p.judge(&s, &BOTH, res), Verdict::Allow);
    let read = spec(
        "read_file",
        vec![Effect::ReadsPrivateData],
        Some(ScopeKind::Folder),
    );
    a.effects = Some(vec![Effect::ExecutesCode, Effect::ActsOutward]);
    let res = Some(Resolved {
        access: &a,
        grant: &GrantAnswer::Allow,
    });
    assert_eq!(p.judge(&read, &BOTH, res), Verdict::Allow);
}
