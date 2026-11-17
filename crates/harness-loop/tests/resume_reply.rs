// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Continue on a first turn whose title step has already run: the title's
//! `assistant` entry is not the turn's reply.

mod common;
use common::*;
use harness_core::*;
use harness_log::{ModelEvent, ModelStop, Role};
use harness_loop::*;

fn last_assistant(d: &Driver) -> Assistant {
    d.last_of(|e| match e {
        Event::Assistant(a) => Some(a.clone()),
        _ => None,
    })
    .unwrap()
}

#[test]
fn continue_extends_a_cut_first_reply_after_its_title_step() {
    let mut d = Driver::new(vec![]);
    d.user("Write a welcome note.");
    // The reply runs out of output tokens: sealed, and the turn ends done.
    d.send(Input::Model(ModelEvent::Text(
        "Welcome, everyone! Tonight".into(),
    )));
    d.done(ModelStop::MaxTokens);
    // The title step runs in the same turn, after the reply.
    d.reply("Welcome note");
    let tail: Vec<String> = d.types().into_iter().rev().take(3).collect();
    assert_eq!(tail, ["title", "step.end", "assistant"]);
    assert_eq!(last_assistant(&d).stop, StopReason::End);

    // Continue looks past the title step to the sealed reply.
    let outs = d.send(Input::Continue);
    assert!(
        !outs.iter().any(|o| matches!(o, Output::Refused(_))),
        "continue was refused: {outs:?}"
    );
    let req = d.last_request().clone();
    assert!(req.prefill);
    assert_eq!(req.messages.last().unwrap().role, Role::Assistant);
    d.reply(" we meet.");
    assert!(last_assistant(&d).continues.is_some());
    let msgs = d.history();
    let assistant: Vec<_> = msgs.iter().filter(|m| m.role == Role::Assistant).collect();
    assert_eq!(assistant.len(), 1);
    assert_eq!(
        texts(&msgs).last().unwrap(),
        "Welcome, everyone! Tonight we meet."
    );
    d.check();
}

#[test]
fn a_complete_first_reply_is_still_refused_after_its_title_step() {
    let mut d = Driver::new(vec![]);
    d.user("Write a welcome note.");
    d.reply("Welcome, everyone!");
    d.reply("Welcome note");
    let outs = d.send(Input::Continue);
    assert_eq!(
        outs,
        [Output::Refused(Refusal::ContinueRefused(
            "The reply is complete.".into()
        ))]
    );
    d.check();
}
