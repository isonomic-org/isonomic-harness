// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tool calls inside a step: guards, policy and approvals, the pre_tool and
//! post_tool links, questions, batches, client tools whose app is gone, and
//! sealing on cancel. Every call gets exactly one recorded result.

use crate::machine::{Phase, Session, Step};
use crate::*;
use harness_policy::Verdict;
use harness_tools::{guard, limit_output, plan_batches, QuestionSpec, ToolDone};
use std::collections::BTreeMap;

pub(crate) struct Calls {
    pub(crate) step: StepId,
    pub(crate) started: Millis,
    pub(crate) usage: Usage,
    batches: Vec<Vec<CallId>>,
    next: usize,
    calls: BTreeMap<CallId, Call>,
}

struct Call {
    record: ToolCall,
    spec: Option<ToolSpec>,
    state: State,
    began: Millis,
}

#[derive(Clone, PartialEq)]
enum State {
    Waiting,
    Approval(ApprovalId, Option<Millis>),
    Question(QuestionId, QuestionSpec),
    Running,
    Done,
}

fn host() -> Decider {
    Decider {
        scope: DeciderScope::Host,
        app: None,
    }
}

impl Session {
    fn calls_mut(&mut self) -> Option<&mut Calls> {
        match self.turn.as_mut().map(|t| &mut t.phase) {
            Some(Phase::Tools(c)) => Some(c),
            _ => None,
        }
    }

    /// The model called tools: record the calls, then run the first batch.
    pub(crate) fn begin_calls(&mut self, step: Step) {
        let mut planned = Vec::new();
        let mut list = Vec::new();
        for (tool, raw) in &step.calls {
            let id = CallId::new(self.next_id("cal"));
            let spec = self.cfg.tools.iter().find(|t| &t.name == tool).cloned();
            let (input, input_raw) = match serde_json::from_str::<Value>(raw) {
                Ok(v) => (v, None),
                Err(_) => (Value::Null, Some(raw.clone())),
            };
            let conc = spec
                .as_ref()
                .map_or(Concurrency::Serial, |s| s.concurrency.clone());
            planned.push((id.clone(), conc));
            list.push(ToolCall {
                call: id,
                tool: tool.clone(),
                kind: spec.as_ref().map_or(ToolKind::Engine, |s| s.kind.clone()),
                input,
                input_raw,
                effects: spec.as_ref().map(|s| s.effects.clone()).unwrap_or_default(),
                batch: None,
                child: None,
            });
        }
        let batches = plan_batches(&planned);
        let mut calls = BTreeMap::new();
        for (n, batch) in batches.iter().enumerate() {
            for id in batch {
                let Some(rec) = list.iter_mut().find(|c| &c.call == id) else {
                    continue;
                };
                if batch.len() > 1 {
                    rec.batch = Some(n as u32);
                }
            }
        }
        for rec in list {
            self.emit(Some(step.id.clone()), Event::ToolCall(rec.clone()));
            let spec = self.cfg.tools.iter().find(|t| t.name == rec.tool).cloned();
            calls.insert(
                rec.call.clone(),
                Call {
                    record: rec,
                    spec,
                    state: State::Waiting,
                    began: self.now,
                },
            );
        }
        if let Some(t) = self.turn.as_mut() {
            t.phase = Phase::Tools(Calls {
                step: step.id.clone(),
                started: step.started,
                usage: step.usage,
                batches,
                next: 0,
                calls,
            });
        }
        self.next_batch();
    }

    fn next_batch(&mut self) {
        let Some(c) = self.calls_mut() else { return };
        if c.next >= c.batches.len() {
            let (step, usage) = (c.step.clone(), c.usage);
            self.end_step(step, StepOutcome::ToolCalls, Some(usage));
            return self.reply_step();
        }
        let batch = c.batches[c.next].clone();
        c.next += 1;
        for id in batch {
            self.dispatch(id);
        }
        self.batch_done();
    }

    /// When every call of the current batch has a result, go on.
    fn batch_done(&mut self) {
        let Some(c) = self.calls_mut() else { return };
        let current = c.next.saturating_sub(1);
        let done = c.batches.get(current).is_none_or(|b| {
            b.iter()
                .all(|id| c.calls.get(id).is_none_or(|x| x.state == State::Done))
        });
        if done {
            self.next_batch();
        }
    }

    fn dispatch(&mut self, id: CallId) {
        let policy = self.cfg.preset.policy.clone();
        let flags = self.flags;
        let now = self.now;
        let Some(c) = self.calls_mut() else { return };
        let Some(call) = c.calls.get(&id) else { return };
        let rec = call.record.clone();
        let Some(spec) = call.spec.clone() else {
            let msg = format!("There is no tool named {}.", rec.tool);
            return self.finish(&id, ToolDone::error("tool.unknown", &msg), false);
        };
        if spec.kind == ToolKind::Display {
            return self.display_call(&id, &rec);
        }
        if let Err(g) = guard(&spec, &rec.input, rec.input_raw.as_deref()) {
            return self.finish(&id, ToolDone::error("tool.input", &g.message), false);
        }
        match policy.policy.evaluate(&spec, &flags) {
            Verdict::Allow => self.run(id),
            Verdict::Deny(reason) => self.finish(&id, ToolDone::denied(&reason), false),
            Verdict::Ask(rule) => {
                let approval = ApprovalId::new(self.next_id("apv"));
                let expires = now + policy.approval_timeout_ms;
                let step = self.calls_mut().map(|c| c.step.clone());
                self.emit(
                    step,
                    Event::ApprovalRequest(ApprovalRequest {
                        approval: approval.clone(),
                        call: Some(id.clone()),
                        summary: summary(&rec),
                        rule,
                        effects: spec.effects.clone(),
                        expires_at: Some(expires),
                        scope: None,
                        preview: Vec::new(),
                        reversible: None,
                        remember: None,
                    }),
                );
                if let Some(c) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) {
                    c.state = State::Approval(approval, Some(expires));
                }
            }
        }
    }

    /// Policy allowed the call: the pre_tool links, then run it.
    fn run(&mut self, id: CallId) {
        let now = self.now;
        let Some(call) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) else {
            return;
        };
        call.began = now;
        let (rec, spec) = (call.record.clone(), call.spec.clone());
        for l in &mut self.links {
            if let Link::Deny(reason) = l.pre_tool(&rec) {
                return self.finish(&id, ToolDone::denied(&reason), false);
            }
        }
        let Some(spec) = spec else { return };
        if let Some(call) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) {
            call.state = State::Running;
        }
        if spec.kind == ToolKind::Client {
            self.client
                .start(id.clone(), self.app.clone(), spec.name.clone());
            if self.client_gone {
                self.awaiting();
            }
        }
        self.out.push(Output::RunTool { call: rec, spec });
    }

    /// Record a call's result (once) and move on when its batch is done.
    fn finish(&mut self, id: &CallId, done: ToolDone, ran: bool) {
        self.finish_as(id, done, ran, None);
    }

    /// [`finish`](Self::finish), with a display call's outcome.
    pub(crate) fn finish_as(
        &mut self,
        id: &CallId,
        done: ToolDone,
        ran: bool,
        display: Option<DisplayOutcome>,
    ) {
        let max = self.cfg.max_tool_output;
        let now = self.now;
        let Some(c) = self.calls_mut() else { return };
        let step = c.step.clone();
        let Some(call) = c.calls.get_mut(id) else {
            return;
        };
        if call.state == State::Done {
            return;
        }
        call.state = State::Done;
        let (rec, began) = (call.record.clone(), call.began);
        let mut done = done;
        if ran {
            for l in &mut self.links {
                if let PostTool::Replace(d) = l.post_tool(&rec, &done) {
                    done = d;
                    break;
                }
            }
        }
        let done = limit_output(done, max);
        for mut a in done.artifacts.clone() {
            a.call = Some(id.clone());
            self.emit(Some(step.clone()), Event::Artifact(a));
        }
        if done.outcome == ToolOutcome::Ok {
            self.flags.note(&rec.effects);
        }
        self.emit(
            Some(step),
            Event::ToolResult(ToolResult {
                call: id.clone(),
                outcome: done.outcome,
                content: done.content,
                error: done.error,
                truncated: done.truncated,
                duration_ms: ran.then(|| now.saturating_sub(began)),
                display,
                meta: None,
            }),
        );
        self.batch_done();
    }

    pub(crate) fn tool_output(&mut self, id: CallId, output: ToolOutput) {
        let running = self
            .calls_mut()
            .and_then(|c| c.calls.get(&id))
            .is_some_and(|c| c.state == State::Running);
        if !running {
            self.out.push(Output::Refused(Refusal::NotOpen));
            return;
        }
        let late = self
            .calls_mut()
            .and_then(|c| c.calls.get(&id))
            .is_some_and(|c| c.record.kind == ToolKind::Client)
            && !self.client.finish(&id);
        if late {
            self.out.push(Output::Refused(Refusal::NotOpen));
            return;
        }
        match output {
            ToolOutput::Done(done) => self.finish(&id, done, true),
            ToolOutput::Ask(q) => {
                let question = QuestionId::new(self.next_id("qst"));
                let step = self.calls_mut().map(|c| c.step.clone());
                self.emit(
                    step,
                    Event::Question(Question {
                        question: question.clone(),
                        call: Some(id.clone()),
                        prompt: q.prompt.clone(),
                        options: q.options.clone(),
                        multiple: q.multiple,
                        free_text: q.free_text,
                    }),
                );
                if let Some(c) = self.calls_mut().and_then(|c| c.calls.get_mut(&id)) {
                    c.state = State::Question(question, q);
                }
            }
        }
    }

    pub(crate) fn answer(&mut self, a: Answer) {
        let found = self.calls_mut().and_then(|c| {
            c.calls.iter().find_map(|(id, x)| match &x.state {
                State::Question(q, spec) if q == &a.question => Some((id.clone(), spec.clone())),
                _ => None,
            })
        });
        let Some((id, spec)) = found else {
            self.out.push(Output::Refused(Refusal::NotOpen));
            return;
        };
        let step = self.calls_mut().map(|c| c.step.clone());
        self.emit(step, Event::Answer(a.clone()));
        let text = if a.skipped {
            "The person skipped this question.".to_string()
        } else {
            let mut parts: Vec<String> = a
                .selected
                .iter()
                .map(|s| {
                    spec.options
                        .iter()
                        .find(|o| &o.id == s)
                        .map_or(s.clone(), |o| o.label.clone())
                })
                .collect();
            parts.extend(a.text.clone().filter(|t| !t.trim().is_empty()));
            parts.join(", ")
        };
        self.finish(&id, ToolDone::ok(text), true);
    }

    pub(crate) fn decision(&mut self, d: ApprovalDecision) {
        let found = self.calls_mut().and_then(|c| {
            c.calls.iter().find_map(|(id, x)| match &x.state {
                State::Approval(a, _) if a == &d.approval => Some(id.clone()),
                _ => None,
            })
        });
        let Some(id) = found else {
            self.out.push(Output::Refused(Refusal::NotOpen));
            return;
        };
        self.decide(id, d);
    }

    fn decide(&mut self, id: CallId, d: ApprovalDecision) {
        let step = self.calls_mut().map(|c| c.step.clone());
        let (decision, basis) = (d.decision.clone(), d.basis.clone());
        self.emit(step, Event::ApprovalDecision(d));
        if decision == Decision::Allow {
            return self.run(id);
        }
        let reason = match basis {
            Basis::Ambiguous => "The answer was unclear, so this was not done.",
            Basis::Timeout => "No one approved this in time, so it was not done.",
            Basis::Cancelled => "The turn was stopped before this was approved.",
            _ => "The person denied this action.",
        };
        self.finish(&id, ToolDone::denied(reason), false);
    }

    pub(crate) fn tick(&mut self) {
        let now = self.now;
        let expired: Vec<(CallId, ApprovalId)> = self
            .calls_mut()
            .map(|c| {
                c.calls
                    .iter()
                    .filter_map(|(id, x)| match &x.state {
                        State::Approval(a, Some(t)) if *t <= now => Some((id.clone(), a.clone())),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        for (id, approval) in expired {
            let d = ApprovalDecision {
                remember: None,
                approval,
                decision: Decision::Deny,
                basis: Basis::Timeout,
                by: host(),
                answer: None,
                reason: None,
            };
            self.decide(id, d);
        }
        for (id, done) in self.client.expired(now) {
            self.out.push(Output::CancelTool(id.clone()));
            self.finish(&id, done, false);
        }
    }

    fn awaiting(&mut self) {
        let timeout = self.cfg.preset.policy.client_timeout_ms;
        let calls = self.client.gone(&self.app.clone(), self.now, timeout);
        let mut open = self.client.open();
        open.sort();
        if calls.is_empty() && open.is_empty() {
            return;
        }
        let deadline = self.client.next_deadline();
        self.emit(
            None,
            Event::SessionStatus(SessionStatus {
                status: Status::AwaitingClient,
                reason: Some("The app that runs these tools is not connected.".into()),
                calls: open,
                deadline,
            }),
        );
    }

    pub(crate) fn client_gone(&mut self) {
        if self.client_gone {
            return;
        }
        self.client_gone = true;
        self.awaiting();
    }

    pub(crate) fn client_back(&mut self) {
        if !self.client_gone {
            return;
        }
        self.client_gone = false;
        self.client.back(&self.app.clone());
        self.emit(
            None,
            Event::SessionStatus(SessionStatus {
                status: Status::Active,
                reason: None,
                calls: Vec::new(),
                deadline: None,
            }),
        );
    }

    /// Seal every outstanding call on cancel or a budget stop.
    pub(crate) fn cancel_calls(&mut self, mut calls: Calls, outcome: StepOutcome) {
        let step = calls.step.clone();
        let ids: Vec<CallId> = calls.calls.keys().cloned().collect();
        for id in ids {
            let Some(c) = calls.calls.get_mut(&id) else {
                continue;
            };
            match std::mem::replace(&mut c.state, State::Done) {
                State::Done => continue,
                State::Running => {
                    self.client.finish(&id);
                    self.out.push(Output::CancelTool(id.clone()));
                }
                State::Approval(approval, _) => {
                    let d = ApprovalDecision {
                        remember: None,
                        approval,
                        decision: Decision::Deny,
                        basis: Basis::Cancelled,
                        by: host(),
                        answer: None,
                        reason: None,
                    };
                    self.emit(Some(step.clone()), Event::ApprovalDecision(d));
                }
                State::Question(question, _) => {
                    let a = Answer {
                        question,
                        selected: Vec::new(),
                        text: None,
                        skipped: true,
                        by: host(),
                    };
                    self.emit(Some(step.clone()), Event::Answer(a));
                }
                State::Waiting => {}
            }
            let done = ToolDone::cancelled();
            self.emit(
                Some(step.clone()),
                Event::ToolResult(ToolResult {
                    call: id,
                    outcome: done.outcome,
                    content: done.content,
                    error: None,
                    truncated: None,
                    duration_ms: None,
                    display: None,
                    meta: None,
                }),
            );
        }
        let usage = calls.usage;
        if let Some(t) = self.turn.as_mut() {
            t.phase = Phase::Tools(calls);
        }
        self.end_step(step, outcome, Some(usage));
    }
}

/// What the person sees on an approval: the tool and its input, short.
fn summary(rec: &ToolCall) -> String {
    let input = serde_json::to_string(&rec.input).unwrap_or_default();
    let mut s = format!("{} {}", rec.tool, input);
    if s.len() > 200 {
        let mut cut = 197;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}
