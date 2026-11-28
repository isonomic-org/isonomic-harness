// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tool-kit frame in the host (`docs/kits.md`): the offer filters, the
//! grant check at resolve and again at run, remembered approvals, the
//! folders on the context record and purge, with fake kit tools.

use harness_core::access::{Access, Scope};
use harness_core::future::BoxFuture;
use harness_core::offer::KitMember;
use harness_core::*;
use harness_host::*;
use harness_toolkit::Standing;
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

/// A files-kit tool on the folder its input names.
struct FolderTool {
    name: &'static str,
    effect: Effect,
    core: bool,
    resolves: bool,
}

fn folder(id: &str) -> Scope {
    Scope {
        kind: ScopeKind::Folder,
        id: id.into(),
        label: id.into(),
    }
}

impl Tool for FolderTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.into(),
            description: "Work in a linked folder.".into(),
            input_schema: Value::object([("type", Value::from("object"))]),
            kind: ToolKind::Engine,
            effects: vec![self.effect.clone()],
            concurrency: Concurrency::Serial,
            scope: Some(ScopeKind::Folder),
        }
    }

    fn call<'a>(&'a self, _input: &'a Value, _cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async { ToolOutput::Done(ToolDone::ok("done")) })
    }

    fn kit(&self) -> Option<KitMember> {
        Some(KitMember {
            kit: "files",
            core: self.core,
        })
    }

    fn access<'a>(
        &'a self,
        input: &'a Value,
        _cx: &'a ToolCtx,
    ) -> Option<BoxFuture<'a, Result<Access, ToolDone>>> {
        if !self.resolves {
            return None;
        }
        let id = input
            .get("folder")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        Some(Box::pin(async move {
            if id.is_empty() {
                return Err(ToolDone::error("files.no_folder", "Name a folder."));
            }
            Ok(Access {
                scope: folder(&id),
                preview: Vec::new(),
                reversible: true,
                remember: Remember::Session,
                confined: false,
                effects: None,
            })
        }))
    }
}

fn host() -> MemoryHost {
    let mut cfg = memory_config();
    for (name, effect, core, resolves) in [
        ("read_file", Effect::ReadsPrivateData, true, true),
        ("write_file", Effect::ModifiesData, false, true),
        ("lost_file", Effect::ReadsPrivateData, false, false),
    ] {
        cfg.tools.push(Arc::new(FolderTool {
            name,
            effect,
            core,
            resolves,
        }));
    }
    MemoryHost::with(cfg)
}

fn grant(m: &MemoryHost, effect: Effect, id: &str, ask_first: bool) {
    m.grants.issue(Standing {
        app: AppId::from("chat"),
        effect,
        scope: folder(id),
        ask_first,
    });
}

fn text(t: &str) -> Input {
    Input {
        content: vec![Block::text(t)],
        client_ref: None,
    }
}

fn create(m: &MemoryHost, c: &Caller) -> SessionId {
    m.block_on(
        m.host
            .create_session(c, &PresetId::from("test"), CreateOptions::default()),
    )
    .unwrap()
}

/// The tools of the turn's first model request (the title request after
/// it offers none).
fn offered(m: &MemoryHost, a: &Caller, s: &SessionId) -> Vec<String> {
    m.model.requests.lock().unwrap().clear();
    m.block_on(m.host.send(a, s, text("Hi"))).unwrap();
    let reqs = m.model.requests.lock().unwrap();
    reqs[0].tools.iter().map(|t| t.name.clone()).collect()
}

fn results(m: &MemoryHost, s: &SessionId) -> Vec<ToolResult> {
    m.log(s)
        .into_iter()
        .filter_map(|e| match e.event {
            Event::ToolResult(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn requests(m: &MemoryHost, s: &SessionId) -> Vec<ApprovalRequest> {
    m.log(s)
        .into_iter()
        .filter_map(|e| match e.event {
            Event::ApprovalRequest(r) => Some(r),
            _ => None,
        })
        .collect()
}

fn code(r: &ToolResult) -> Option<&str> {
    r.error.as_ref().map(|e| e.code.as_str())
}

#[test]
fn kit_tools_are_offered_only_where_a_grant_could_cover_them() {
    let m = host();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = create(&m, &a);
    let none = offered(&m, &a, &s);
    assert!(none.contains(&"ask_user".to_string()));
    assert!(!none.iter().any(|n| n.ends_with("_file")));

    grant(&m, Effect::ReadsPrivateData, "design", false);
    let s = create(&m, &a);
    let names = offered(&m, &a, &s);
    assert!(names.contains(&"read_file".to_string()));
    assert!(names.contains(&"lost_file".to_string()));
    assert!(!names.contains(&"write_file".to_string()));
    // The context record names the linked folder.
    let folders = m.log(&s).iter().find_map(|e| match &e.event {
        Event::System(sys) => sys.context.as_ref().map(|c| c.folders.clone()),
        _ => None,
    });
    assert_eq!(folders, Some(vec!["design".to_string()]));
}

#[test]
fn a_covered_read_runs_and_an_uncovered_one_is_denied_without_asking() {
    let m = host();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    grant(&m, Effect::ReadsPrivateData, "design", false);
    let s = create(&m, &a);
    m.model.calls(&[
        ("read_file", "{\"folder\":\"design\"}"),
        ("read_file", "{\"folder\":\"taxes\"}"),
    ]);
    m.block_on(m.host.send(&a, &s, text("Read both."))).unwrap();
    let r = results(&m, &s);
    assert_eq!(r[0].outcome, ToolOutcome::Ok);
    assert_eq!(r[1].outcome, ToolOutcome::Denied);
    assert!(requests(&m, &s).is_empty());
}

#[test]
fn a_resolve_failure_is_the_result_and_never_an_approval() {
    let m = host();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    grant(&m, Effect::ReadsPrivateData, "design", false);
    let s = create(&m, &a);
    m.model.calls(&[("read_file", "{}"), ("lost_file", "{}")]);
    m.block_on(m.host.send(&a, &s, text("Read."))).unwrap();
    let r = results(&m, &s);
    assert_eq!(code(&r[0]), Some("files.no_folder"));
    assert_eq!(code(&r[1]), Some("kit.no_access"));
    assert!(requests(&m, &s).is_empty());
}

#[test]
fn this_session_remembers_a_write_for_that_folder_and_session_only() {
    let m = host();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let shell = m.caller(&m.shell_token());
    grant(&m, Effect::ModifiesData, "design", true);
    let s = create(&m, &a);
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s, text("Write."))).unwrap();
    let req = requests(&m, &s).remove(0);
    assert_eq!(req.rule, ApprovalRule::Grant);
    assert_eq!(req.scope, Some(folder("design")));
    assert_eq!(req.remember, Some(Remember::Session));
    assert_eq!(req.reversible, Some(true));
    let id = req.approval.clone();
    let t = m.user_action(&shell, &s, &id);
    // Asking for longer than the request offers is held to the request.
    m.block_on(m.host.decide_approval(
        &shell,
        &s,
        &id,
        DecisionInput::Allow,
        Some(Remember::Always),
        Some(t),
    ))
    .unwrap();
    let d = m.log(&s).iter().find_map(|e| match &e.event {
        Event::ApprovalDecision(d) => Some(d.clone()),
        _ => None,
    });
    assert_eq!(d.unwrap().remember, Some(Remember::Session));

    // The same write in the same session runs without asking.
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s, text("Again."))).unwrap();
    assert_eq!(requests(&m, &s).len(), 1);
    assert!(results(&m, &s).iter().all(|r| r.outcome == ToolOutcome::Ok));

    // A fork is another session: it asks again.
    let turn = m.log(&s).iter().rev().find_map(|e| e.turn.clone()).unwrap();
    let fork = m
        .block_on(
            m.host
                .fork_session(&a, &s, ForkPoint::AfterTurn(turn), ForkReason::Branch),
        )
        .unwrap();
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &fork, text("Write."))).unwrap();
    let pending = m.block_on(m.host.list_approvals(&shell)).unwrap();
    assert!(pending.iter().any(|p| p.session == fork));

    // Another session asks again.
    let s2 = create(&m, &a);
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s2, text("Write."))).unwrap();
    assert_eq!(requests(&m, &s2).len(), 1);

    // Purge takes the remembered approval with it.
    let gone = m.block_on(m.host.purge(&a, &AppId::from("chat"))).unwrap();
    assert!(gone.sessions >= 2);
    let s3 = create(&m, &a);
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s3, text("Write."))).unwrap();
    assert_eq!(requests(&m, &s3).len(), 1);
}

#[test]
fn once_remembers_nothing() {
    let m = host();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let shell = m.caller(&m.shell_token());
    grant(&m, Effect::ModifiesData, "design", true);
    let s = create(&m, &a);
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s, text("Write."))).unwrap();
    let id = requests(&m, &s)[0].approval.clone();
    let t = m.user_action(&shell, &s, &id);
    m.block_on(m.host.decide_approval(
        &shell,
        &s,
        &id,
        DecisionInput::Allow,
        Some(Remember::Once),
        Some(t),
    ))
    .unwrap();
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s, text("Again."))).unwrap();
    assert_eq!(requests(&m, &s).len(), 2);
}

#[test]
fn a_grant_revoked_after_the_verdict_refuses_the_run() {
    let m = host();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let shell = m.caller(&m.shell_token());
    grant(&m, Effect::ModifiesData, "design", true);
    let s = create(&m, &a);
    m.model.calls(&[("write_file", "{\"folder\":\"design\"}")]);
    m.block_on(m.host.send(&a, &s, text("Write."))).unwrap();
    let id = requests(&m, &s)[0].approval.clone();
    m.grants.revoke(&AppId::from("chat"), &folder("design"));
    let t = m.user_action(&shell, &s, &id);
    m.block_on(
        m.host
            .decide_approval(&shell, &s, &id, DecisionInput::Allow, None, Some(t)),
    )
    .unwrap();
    let r = results(&m, &s);
    assert_eq!(code(&r[0]), Some("grant.revoked"));
}
