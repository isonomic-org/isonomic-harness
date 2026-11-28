// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The registry, guards, batches, output limits and the client lifecycle.

use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use harness_tools::*;
use std::sync::Arc;

struct Echo;

impl Tool for Echo {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".into(),
            description: "Echo the text.".into(),
            input_schema: Value::object([
                ("type", Value::from("object")),
                (
                    "properties",
                    Value::object([
                        ("text", Value::object([("type", Value::from("string"))])),
                        ("times", Value::object([("type", Value::from("integer"))])),
                    ]),
                ),
                ("required", Value::from(vec!["text"])),
            ]),
            kind: ToolKind::Engine,
            effects: vec![],
            concurrency: Concurrency::Parallel,
            scope: None,
        }
    }

    fn call<'a>(&'a self, input: &'a Value, _cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        let text = input
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        ready(ToolOutput::Done(ToolDone::ok(text)))
    }
}

fn client_spec(name: &str) -> ToolSpec {
    ToolSpec {
        kind: ToolKind::Client,
        name: name.into(),
        ..Echo.spec()
    }
}

#[test]
fn guards_refuse_bad_input_with_a_readable_reason() {
    let spec = Echo.spec();
    let ok = Value::object([("text", Value::from("hi")), ("times", Value::from(2u64))]);
    assert!(guard(&spec, &ok, None).is_ok());
    let missing = Value::object([("times", Value::from(2u64))]);
    assert!(guard(&spec, &missing, None)
        .unwrap_err()
        .message
        .contains("`text`"));
    let wrong = Value::object([("text", Value::from(3u64))]);
    assert!(guard(&spec, &wrong, None)
        .unwrap_err()
        .message
        .contains("type string"));
    let float = Value::object([("text", Value::from("a")), ("times", Value::from(1.5))]);
    assert!(guard(&spec, &float, None).is_err());
    assert!(guard(&spec, &Value::Null, Some("{not json"))
        .unwrap_err()
        .message
        .contains("not valid JSON"));
}

#[test]
fn client_registration_is_scoped_and_refuses_other_kinds() {
    let mut r = Registry::new();
    r.add_engine(Arc::new(Echo));
    let docs = AppId::from("docs");
    let other = AppId::from("tables");
    r.register_client(
        &docs,
        vec![client_spec("insert_block"), client_spec("read_doc")],
    )
    .unwrap();
    assert_eq!(r.specs().len(), 3);
    assert_eq!(
        r.register_client(&docs, vec![Echo.spec()]),
        Err(RegistryError::NotClient("echo".into()))
    );
    assert_eq!(
        r.register_client(&other, vec![client_spec("insert_block")]),
        Err(RegistryError::Taken("insert_block".into()))
    );
    // A new registration replaces the app's earlier set.
    r.register_client(&docs, vec![client_spec("insert_block")])
        .unwrap();
    assert!(r.get("read_doc").is_none());
    assert!(matches!(
        r.get("insert_block").unwrap().handler,
        Handler::Client { .. }
    ));
    r.retain_named(&[]);
    assert!(r.get("echo").is_none());
    assert!(r.get("insert_block").is_some());
}

#[test]
fn batches_group_parallel_calls_and_isolate_serial_ones() {
    let c = |n: &str| CallId::from(n);
    let calls = vec![
        (c("a"), Concurrency::Parallel),
        (c("b"), Concurrency::Parallel),
        (c("s"), Concurrency::Serial),
        (c("d"), Concurrency::Parallel),
    ];
    assert_eq!(
        plan_batches(&calls),
        vec![vec![c("a"), c("b")], vec![c("s")], vec![c("d")]]
    );
}

#[test]
fn long_output_keeps_head_and_tail() {
    let long = "x".repeat(10_000);
    let done = limit_output(ToolDone::ok(long.clone()), 1_000);
    let t = done.truncated.unwrap();
    assert_eq!(t.original_bytes, 10_000);
    assert_eq!(t.head_bytes + t.tail_bytes, 1_000);
    let short = limit_output(ToolDone::ok("short"), 1_000);
    assert!(short.truncated.is_none());
}

#[test]
fn client_calls_wait_time_out_and_resume() {
    let mut cc = ClientCalls::new();
    let docs = AppId::from("docs");
    cc.start(CallId::from("c1"), docs.clone(), "insert_block".into());
    assert!(!cc.awaiting());
    assert_eq!(cc.gone(&docs, 1_000, 30_000), vec![CallId::from("c1")]);
    assert!(cc.awaiting());
    assert_eq!(cc.next_deadline(), Some(31_000));
    assert!(cc.expired(30_999).is_empty());
    let out = cc.expired(31_000);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].1.outcome, ToolOutcome::Timeout);
    assert!(
        !cc.finish(&CallId::from("c1")),
        "a late result is not accepted"
    );

    cc.start(CallId::from("c2"), docs.clone(), "insert_block".into());
    cc.gone(&docs, 5_000, 30_000);
    assert_eq!(cc.back(&docs), vec![CallId::from("c2")]);
    assert!(!cc.awaiting());
    assert!(cc.finish(&CallId::from("c2")));
}

#[test]
fn engine_tools_run() {
    let cx = ToolCtx {
        session: SessionId::from("s"),
        app: AppId::from("chat"),
        call: CallId::from("c"),
        now: 0,
        child: None,
    };
    let input = Value::object([("text", Value::from("hello"))]);
    let fut = Echo.call(&input, &cx);
    let waker = std::task::Waker::noop();
    let mut ctx = std::task::Context::from_waker(waker);
    let mut fut = fut;
    match fut.as_mut().poll(&mut ctx) {
        std::task::Poll::Ready(ToolOutput::Done(d)) => {
            assert_eq!(d.content, vec![Block::text("hello")])
        }
        other => panic!("{other:?}"),
    }
}
