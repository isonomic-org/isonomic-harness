// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! A driver for loop tests: feeds inputs, records requests and tool runs,
//! and checks the invariants every scenario must keep.
#![allow(dead_code)]

use harness_core::*;
use harness_log::history::{history, request_for};
use harness_log::{Message, ModelEvent, ModelRequest, ModelStop, Part};
use harness_loop::*;
use harness_preset::{builtin, Preset};
use harness_tools::{ToolDone, ToolOutput};

pub const T0: Millis = 1_794_646_800_000;

pub fn preset(id: &str) -> Preset {
    builtin(id).unwrap().1.preset.unwrap()
}

pub fn spec(name: &str, kind: ToolKind, effects: &[Effect], conc: Concurrency) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        description: format!("The {name} tool."),
        input_schema: Value::object([("type", Value::from("object"))]),
        kind,
        effects: effects.to_vec(),
        concurrency: conc,
    }
}

pub fn ctx(location: Option<&str>) -> RequestContext {
    RequestContext {
        date: "2026-11-15".into(),
        time_zone: "America/New_York".into(),
        language: "en".into(),
        location: location.map(str::to_owned),
    }
}

pub struct Driver {
    pub s: Session,
    pub now: Millis,
    pub requests: Vec<(StepId, ModelRequest)>,
    pub runs: Vec<(ToolCall, ToolSpec)>,
    pub outs: Vec<Output>,
}

impl Driver {
    pub fn with(cfg: Config) -> Self {
        Self::with_profile(cfg, Profile::Standard)
    }

    pub fn with_profile(mut cfg: Config, profile: Profile) -> Self {
        cfg.profile = Some(profile.clone());
        let start = SessionStart {
            class: SessionClass::Interactive,
            app: AppId::from("chat"),
            principal: Principal {
                kind: PrincipalKind::User,
                id: None,
            },
            preset: cfg.preset.reference.clone(),
            parent: None,
            profile: Some(profile),
            model: None,
        };
        let (s, outs) = Session::create(cfg, SessionId::from("ses_0001"), start, T0);
        let mut d = Driver {
            s,
            now: T0,
            requests: Vec::new(),
            runs: Vec::new(),
            outs: Vec::new(),
        };
        d.take(outs);
        d
    }

    pub fn new(tools: Vec<ToolSpec>) -> Self {
        Self::with(Config::new(preset("test"), None, tools))
    }

    fn take(&mut self, outs: Vec<Output>) -> Vec<Output> {
        for o in &outs {
            match o {
                Output::Request { step, request } => {
                    self.requests.push((step.clone(), request.clone()))
                }
                Output::RunTool { call, spec } => self.runs.push((call.clone(), spec.clone())),
                _ => {}
            }
        }
        self.outs.extend(outs.iter().cloned());
        outs
    }

    pub fn send(&mut self, input: Input) -> Vec<Output> {
        self.now += 100;
        let outs = self.s.handle(self.now, input);
        self.take(outs)
    }

    pub fn advance(&mut self, ms: u64) {
        self.now += ms;
    }

    pub fn user(&mut self, text: &str) -> Vec<Output> {
        self.send(Input::User {
            content: vec![Block::text(text)],
            context: Some(ctx(None)),
        })
    }

    pub fn reply(&mut self, text: &str) -> Vec<Output> {
        self.send(Input::Model(ModelEvent::Text(text.into())));
        self.done(ModelStop::End)
    }

    pub fn done(&mut self, stop: ModelStop) -> Vec<Output> {
        self.send(Input::Model(ModelEvent::Done {
            stop,
            usage: Usage {
                input_tokens: 100,
                output_tokens: 10,
                ..Usage::default()
            },
        }))
    }

    pub fn call_tools(&mut self, calls: &[(&str, &str)]) -> Vec<Output> {
        for (tool, input) in calls {
            self.send(Input::Model(ModelEvent::ToolCall {
                tool: (*tool).into(),
                input: (*input).into(),
            }));
        }
        self.done(ModelStop::ToolUse)
    }

    pub fn tool_ok(&mut self, call: &CallId, text: &str) -> Vec<Output> {
        self.send(Input::Tool {
            call: call.clone(),
            output: ToolOutput::Done(ToolDone::ok(text)),
        })
    }

    /// This session's events.
    pub fn own(&self) -> Vec<&SessionEvent> {
        self.s
            .events()
            .iter()
            .filter(|e| &e.session == self.s.id())
            .collect()
    }

    pub fn types(&self) -> Vec<String> {
        self.own()
            .iter()
            .map(|e| e.event.type_name().to_owned())
            .collect()
    }

    pub fn last_of<T>(&self, f: impl Fn(&Event) -> Option<T>) -> Option<T> {
        self.s.events().iter().rev().find_map(|e| f(&e.event))
    }

    pub fn history(&self) -> Vec<Message> {
        history(self.s.events(), None)
    }

    pub fn last_request(&self) -> &ModelRequest {
        &self.requests.last().unwrap().1
    }

    /// The invariants: replay rebuilds every request byte for byte, seqs
    /// have no gaps, and every call has exactly one result.
    pub fn check(&self) {
        let events = self.s.events();
        for (step, req) in &self.requests {
            let replayed = request_for(events, step).unwrap();
            assert_eq!(replayed.bytes(), req.bytes(), "step {step}");
        }
        let own = self.own();
        for (i, e) in own.iter().enumerate() {
            assert_eq!(e.seq, own[0].seq + i as u64, "seq gap at {i}");
        }
        let calls: Vec<&CallId> = own
            .iter()
            .filter_map(|e| match &e.event {
                Event::ToolCall(c) => Some(&c.call),
                _ => None,
            })
            .collect();
        for c in calls {
            let n = own
                .iter()
                .filter(|e| matches!(&e.event, Event::ToolResult(r) if &r.call == c))
                .count();
            assert_eq!(n, 1, "call {c} has {n} results");
        }
    }
}

/// Every text the model sees, tool results included.
pub fn texts(msgs: &[Message]) -> Vec<String> {
    fn walk(parts: &[Part], out: &mut Vec<String>) {
        for p in parts {
            match p {
                Part::Text { text } => out.push(text.clone()),
                Part::ToolResult { content, .. } => walk(content, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for m in msgs {
        walk(&m.content, &mut out);
    }
    out
}
