// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The kits-files fixture's results are what the files tools return: every
//! recorded call is run again over the fixture's seed, in order, and its
//! content and error code must match the recorded result.

use harness_core::*;
use harness_testkit::fixtures::{kits_files, kits_files as fixture};
use harness_toolkit::files::{files_kit, MemoryFs};
use harness_tools::{ToolCtx, ToolOutput};
use std::collections::HashMap;
use std::sync::Arc;

fn poll<T>(f: harness_core::future::BoxFuture<'_, T>) -> T {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let mut f = f;
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("the memory store is ready at once"),
    }
}

#[test]
fn recorded_results_are_what_the_tools_return() {
    let fs = Arc::new(MemoryFs::new());
    for (id, label, ro) in fixture::LINKS {
        fs.link("chat", id, label, *ro);
    }
    for (folder, path, body) in fixture::SEED {
        fs.put(folder, path, body);
    }
    let tools = files_kit(fs);
    let mut ran: HashMap<CallId, (Vec<Block>, Option<String>)> = HashMap::new();
    let mut checked = 0;
    for e in kits_files().events {
        match e.event {
            Event::ToolCall(c) => {
                let t = tools.iter().find(|t| t.spec().name == c.tool).unwrap();
                let cx = ToolCtx {
                    session: e.session.clone(),
                    app: AppId::from("chat"),
                    call: c.call.clone(),
                    now: 0,
                    child: None,
                    result_cap: Some(fixture::CAP),
                };
                let out = match poll(t.access(&c.input, &cx).unwrap()) {
                    Err(d) => d,
                    Ok(_) => match poll(t.call(&c.input, &cx)) {
                        ToolOutput::Done(d) => d,
                        ToolOutput::Ask(_) => panic!("files tools never ask"),
                    },
                };
                ran.insert(c.call, (out.content, out.error.map(|e| e.code)));
            }
            Event::ToolResult(r) => {
                let (content, code) = ran.remove(&r.call).expect("a result follows its call");
                assert_eq!(content, r.content, "{}", r.call.as_str());
                assert_eq!(code, r.error.map(|e| e.code), "{}", r.call.as_str());
                checked += 1;
            }
            _ => {}
        }
    }
    assert_eq!(checked, 6);
}
