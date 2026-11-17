// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stopping: cancel and budget stops at every await point, and sealing a
//! turn that a restart interrupted.

use crate::machine::{Phase, Session, Turn};
use crate::steps::step_shell;
use crate::*;
use harness_policy::Meter;

const STOPPED: &str = "Stopped by the person.";

impl Session {
    /// Cancel or a budget stop: seal everything outstanding, close the turn.
    pub(crate) fn stop(&mut self, stop: TurnStop, budget: Option<BudgetHit>) {
        self.stop_with(stop, budget, None);
    }

    /// The host found a fault (a panic or a hang): seal the turn as failed,
    /// mark the session failed, and take no more input.
    pub(crate) fn fail_now(&mut self, err: ErrorInfo) {
        if self.turn.is_some() {
            self.stop_with(TurnStop::Failed, None, Some(err.clone()));
        }
        self.mark_failed(&err);
    }

    pub(crate) fn mark_failed(&mut self, err: &ErrorInfo) {
        self.turn = None;
        self.inbox.clear();
        self.out.retain(|o| {
            !matches!(
                o,
                Output::Request { .. } | Output::RunTool { .. } | Output::Idle
            )
        });
        self.emit(
            None,
            Event::SessionStatus(SessionStatus {
                status: Status::Failed,
                reason: Some(err.message.clone()),
                calls: Vec::new(),
                deadline: None,
            }),
        );
        self.failed = true;
    }

    fn stop_with(&mut self, stop: TurnStop, budget: Option<BudgetHit>, error: Option<ErrorInfo>) {
        let Some(t) = self.turn.as_mut() else {
            self.out.push(Output::Refused(Refusal::NotRunning));
            return;
        };
        let post = t.closed;
        let reason = match (&budget, &error) {
            (Some(_), _) => Some("A budget ran out.".to_string()),
            (_, Some(e)) => Some(e.message.clone()),
            _ => Some(STOPPED.to_string()),
        };
        let outcome = match (&budget, &error) {
            (Some(_), _) => StepOutcome::Budget,
            (_, Some(_)) => StepOutcome::Failed,
            _ => StepOutcome::Cancelled,
        };
        match std::mem::replace(&mut t.phase, Phase::Between) {
            Phase::Between => {}
            Phase::Model(step) => {
                self.out.push(Output::CancelRequest(step.id.clone()));
                let id = step.id.clone();
                if step.purpose == Purpose::Reply && !step.blocks.is_empty() {
                    let continues = self.turn.as_mut().and_then(|t| t.continues.take());
                    self.emit(
                        Some(id.clone()),
                        Event::Assistant(Assistant {
                            content: step.blocks.clone(),
                            stop: StopReason::Cancelled,
                            stream: step.stream.clone(),
                            usage: None,
                            continues,
                        }),
                    );
                }
                self.emit(
                    Some(id.clone()),
                    Event::Attempt(Attempt {
                        outcome: AttemptOutcome::Cancelled,
                        error: None,
                        content: Vec::new(),
                        usage: None,
                        retry: step.retry,
                        will_retry: false,
                    }),
                );
                if let Some(t) = self.turn.as_mut() {
                    t.phase = Phase::Model(step_shell(&step));
                }
                self.end_step(id, outcome, Some(step.usage));
            }
            Phase::Tools(calls) => self.cancel_calls(calls, outcome),
        }
        if post {
            self.turn = None;
            if error.is_none() {
                self.claim();
            }
            return;
        }
        self.end_turn(stop, reason, error, budget);
    }

    /// Seal a turn left open by a crash or restart: `interrupted`, or
    /// `failed` with the error when a fault caused it.
    pub(crate) fn seal_interrupted(&mut self, error: Option<ErrorInfo>) {
        let mut open_turn = None;
        let mut open_step = None;
        let mut open_calls: Vec<CallId> = Vec::new();
        for e in self.events.iter().filter(|e| e.session == self.id) {
            match &e.event {
                Event::TurnStart(_) => open_turn = e.turn.clone(),
                Event::TurnEnd(_) => open_turn = None,
                Event::StepStart(_) => open_step = e.step.clone().zip(e.turn.clone()),
                Event::StepEnd(_) => open_step = None,
                Event::ToolCall(c) => open_calls.push(c.call.clone()),
                Event::ToolResult(r) => open_calls.retain(|c| c != &r.call),
                _ => {}
            }
        }
        let turn = open_turn.or_else(|| open_step.as_ref().map(|(_, t)| t.clone()));
        let Some(turn) = turn else { return };
        let closed = !self
            .events
            .iter()
            .any(|e| e.turn.as_ref() == Some(&turn) && matches!(e.event, Event::TurnStart(_)))
            || self
                .events
                .iter()
                .any(|e| e.turn.as_ref() == Some(&turn) && matches!(e.event, Event::TurnEnd(_)));
        self.turn = Some(Turn {
            id: turn,
            started: self.now,
            meter: Meter::start(self.now),
            steps: 0,
            usage: Usage::default(),
            continues: None,
            summarized: false,
            closed,
            phase: Phase::Between,
        });
        let step = open_step.map(|(s, _)| s);
        for call in open_calls {
            let r = ToolResult {
                call,
                outcome: ToolOutcome::Cancelled,
                content: vec![Block::text(
                    "Interrupted: the session restarted before this call finished.",
                )],
                error: None,
                truncated: None,
                duration_ms: None,
            };
            self.emit(step.clone(), Event::ToolResult(r));
        }
        if let Some(s) = step {
            self.emit(
                Some(s.clone()),
                Event::Attempt(Attempt {
                    outcome: AttemptOutcome::Cancelled,
                    error: None,
                    content: Vec::new(),
                    usage: None,
                    retry: 0,
                    will_retry: false,
                }),
            );
            self.end_step(s, StepOutcome::Cancelled, None);
        }
        if closed {
            self.turn = None;
            return;
        }
        let (stop, reason) = match &error {
            Some(e) => (TurnStop::Failed, e.message.clone()),
            None => (
                TurnStop::Interrupted,
                "The session restarted during this turn.".to_string(),
            ),
        };
        self.end_turn(stop, Some(reason), error, None);
        // Recovery never starts new work by itself.
        self.out.retain(|o| !matches!(o, Output::Request { .. }));
        if let Some(t) = &self.turn {
            if t.closed {
                self.turn = None;
            }
        }
    }
}
