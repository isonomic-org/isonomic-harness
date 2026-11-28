// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files fixtures' results are what the files tools return. Every
//! recorded call is resolved and run again over the fixtures' seed, in
//! order: its content and error code must match the recorded result, and
//! an approval request must show the scope, preview and reversibility the
//! tool's access gave. A denied call is resolved and never run.

use harness_core::access::Access;
use harness_core::*;
use harness_testkit::fixtures::{kits_files as seed, kits_files_read, kits_files_write, Fixture};
use harness_toolkit::files::contract::wait;
use harness_toolkit::files::{files_kit, MemoryFs};
use harness_tools::{ToolCtx, ToolDone};
use std::collections::HashMap;
use std::sync::Arc;

fn replay(fixture: Fixture) -> (usize, Arc<MemoryFs>) {
    let fs = Arc::new(MemoryFs::new());
    for (id, label, read_only, reversible) in seed::LINKS {
        fs.link("chat", id, label, *read_only);
        if !reversible {
            fs.irreversible(id);
        }
    }
    for (folder, path, body) in seed::SEED {
        fs.put(folder, path, body);
    }
    let tools = files_kit(fs.clone());
    let mut open: HashMap<CallId, (ToolCall, ToolCtx, Result<Access, ToolDone>)> = HashMap::new();
    let mut checked = 0;
    for e in fixture.events {
        match e.event {
            Event::ToolCall(c) => {
                let t = tools.iter().find(|t| t.spec().name == c.tool).unwrap();
                let cx = ToolCtx {
                    session: e.session.clone(),
                    app: AppId::from("chat"),
                    call: c.call.clone(),
                    now: 0,
                    child: None,
                    result_cap: Some(seed::CAP),
                };
                let access = wait(t.access(&c.input, &cx).unwrap());
                open.insert(c.call.clone(), (c, cx, access));
            }
            Event::ApprovalRequest(a) => {
                let (_, _, access) = &open[a.call.as_ref().unwrap()];
                let access = access.as_ref().expect("only a resolved call asks");
                assert_eq!(a.scope.as_ref(), Some(&access.scope));
                assert_eq!(a.preview, access.preview);
                assert_eq!(a.reversible, Some(access.reversible));
            }
            Event::ToolResult(r) => {
                let (c, cx, access) = open.remove(&r.call).expect("a result follows its call");
                if r.outcome == ToolOutcome::Denied {
                    assert!(access.is_ok());
                    continue;
                }
                let t = tools.iter().find(|t| t.spec().name == c.tool).unwrap();
                let done = match access {
                    Err(d) => d,
                    Ok(_) => match wait(t.call(&c.input, &cx)) {
                        harness_tools::ToolOutput::Done(d) => d,
                        harness_tools::ToolOutput::Ask(_) => panic!("files tools never ask"),
                    },
                };
                assert_eq!(done.content, r.content, "{}", r.call.as_str());
                assert_eq!(done.meta, r.meta, "{}", r.call.as_str());
                assert_eq!(
                    done.error.map(|e| e.code),
                    r.error.map(|e| e.code),
                    "{}",
                    r.call.as_str()
                );
                checked += 1;
            }
            _ => {}
        }
    }
    (checked, fs)
}

#[test]
fn the_read_fixture_is_what_the_tools_return() {
    let (checked, fs) = replay(kits_files_read());
    assert_eq!(checked, 10);
    assert_eq!(fs.trashed().len(), 0);
}

#[test]
fn the_write_fixture_is_what_the_tools_return() {
    let (checked, fs) = replay(kits_files_write());
    assert_eq!(checked, 10);
    // The denied write was never made; the allowed ones were.
    assert_eq!(fs.get("f3", "plan.md").as_deref(), Some("Plan\n"));
    assert_eq!(
        fs.get("f4", "shots.md").as_deref(),
        Some("Shot list\nSunset\n")
    );
    assert_eq!(
        fs.get("f1", "status.md").as_deref(),
        Some("Status: draft\nNext: review\n")
    );
    assert_eq!(fs.trashed(), ["f1/icons/logo.bin"]);
}
