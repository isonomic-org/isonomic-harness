// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files kit through the host: registered from `Deps.files`, offered
//! where a grant covers the folder, and run on the place access resolved.

use harness_core::access::Scope;
use harness_core::*;
use harness_host::*;
use harness_toolkit::files::MemoryFs;
use harness_toolkit::{Grants, Standing};
use std::sync::Arc;

fn design() -> Scope {
    Scope {
        kind: ScopeKind::Folder,
        id: "f1".into(),
        label: "Design".into(),
    }
}

fn setup() -> (MemoryHost, Arc<MemoryFs>, Caller, SessionId) {
    let fs = Arc::new(MemoryFs::new());
    fs.link("chat", "f1", "Design", false);
    fs.put("f1", "notes.md", "one\ntwo\n");
    let m = MemoryHost::with_files(memory_config(), Some(fs.clone()));
    for (effect, ask_first) in [
        (Effect::ReadsPrivateData, false),
        (Effect::ModifiesData, false),
    ] {
        m.grants.issue(Standing {
            app: AppId::from("chat"),
            effect,
            scope: design(),
            ask_first,
        });
    }
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = m
        .block_on(
            m.host
                .create_session(&a, &PresetId::from("test"), CreateOptions::default()),
        )
        .unwrap();
    (m, fs, a, s)
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

fn say(t: &str) -> Input {
    Input {
        content: vec![Block::text(t)],
        client_ref: None,
    }
}

#[test]
fn the_kit_is_offered_and_reads_and_edits_a_linked_folder() {
    let (m, fs, a, s) = setup();
    m.model.calls(&[
        ("read_file", "{\"path\":\"Design/notes.md\"}"),
        (
            "edit_file",
            "{\"path\":\"Design/notes.md\",\"find\":\"two\",\"replace\":\"2\"}",
        ),
        ("write_file", "{\"path\":\"/etc/hosts\",\"text\":\"x\"}"),
    ]);
    m.block_on(m.host.send(&a, &s, say("Fix the notes.")))
        .unwrap();
    let offered: Vec<String> = m.model.requests.lock().unwrap()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    for name in [
        "list_dir",
        "read_file",
        "search_files",
        "edit_file",
        "trash_path",
    ] {
        assert!(offered.contains(&name.to_string()), "{name}");
    }
    let r = results(&m, &s);
    assert_eq!(r[0].outcome, ToolOutcome::Ok);
    assert_eq!(r[1].outcome, ToolOutcome::Ok);
    assert_eq!(
        r[2].error.as_ref().map(|e| e.code.as_str()),
        Some("files.not_linked")
    );
    assert_eq!(fs.get("f1", "notes.md").as_deref(), Some("one\n2\n"));
    // The result's meta is recorded for the person's view; a failure to
    // resolve has none.
    let meta = r[1].meta.as_ref().expect("a files result carries meta");
    assert_eq!(meta.get("scope").and_then(Value::as_str), Some("f1"));
    assert_eq!(
        meta.get("path").and_then(Value::as_str),
        Some("Design/notes.md")
    );
    assert!(meta.get("pre_image").is_some());
    assert!(r[2].meta.is_none());
    let folders = m.log(&s).iter().find_map(|e| match &e.event {
        Event::System(sys) => sys.context.as_ref().map(|c| c.folders.clone()),
        _ => None,
    });
    assert_eq!(folders, Some(vec!["Design".to_string()]));
}

#[test]
fn a_change_that_cannot_be_undone_asks_despite_a_standing_allow() {
    let (m, fs, a, s) = setup();
    // The standing allow covers changes in Design without asking.
    m.model
        .calls(&[("write_file", "{\"path\":\"Design/a.md\",\"text\":\"a\"}")]);
    m.block_on(m.host.send(&a, &s, say("Write."))).unwrap();
    assert_eq!(results(&m, &s)[0].outcome, ToolOutcome::Ok);
    // Once the folder's changes cannot be undone, the same allow no longer
    // covers one: it asks, and offers "once" only.
    fs.irreversible("f1");
    m.model
        .calls(&[("write_file", "{\"path\":\"Design/b.md\",\"text\":\"b\"}")]);
    m.block_on(m.host.send(&a, &s, say("Write again.")))
        .unwrap();
    let asked: Vec<ApprovalRequest> = m
        .log(&s)
        .into_iter()
        .filter_map(|e| match e.event {
            Event::ApprovalRequest(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].rule, ApprovalRule::Grant);
    assert_eq!(asked[0].scope, Some(design()));
    assert_eq!(asked[0].reversible, Some(false));
    assert_eq!(asked[0].remember, Some(Remember::Once));
    assert_eq!(fs.get("f1", "b.md"), None);
}

#[test]
fn no_files_service_registers_no_files_tool() {
    let m = MemoryHost::new();
    assert!(m
        .grants
        .held(&AppId::from("chat"), &SessionId::from("x"))
        .is_empty());
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = m
        .block_on(
            m.host
                .create_session(&a, &PresetId::from("test"), CreateOptions::default()),
        )
        .unwrap();
    m.block_on(m.host.send(&a, &s, say("Hi"))).unwrap();
    let reqs = m.model.requests.lock().unwrap();
    assert!(!reqs[0].tools.iter().any(|t| t.name == "list_dir"));
}
