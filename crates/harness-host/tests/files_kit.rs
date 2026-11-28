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
    let folders = m.log(&s).iter().find_map(|e| match &e.event {
        Event::System(sys) => sys.context.as_ref().map(|c| c.folders.clone()),
        _ => None,
    });
    assert_eq!(folders, Some(vec!["Design".to_string()]));
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
