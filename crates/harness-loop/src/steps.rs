//! Steps: budgets, links and compaction before each reply step, the
//! request, streamed model events, retries and failures.

use crate::machine::{Phase, Session, Step};
use crate::*;
use harness_compact::{Plan, Probe};
use harness_log::history::request_for;
use harness_log::ModelStop;

impl Session {
    /// Start the next reply step: budget, links, compaction, then the request.
    pub(crate) fn reply_step(&mut self) {
        let Some(t) = self.turn.as_ref() else { return };
        if let Some(hit) = t.meter.check(&self.cfg.limits, self.now) {
            let reason = format!("The turn reached its {:?} budget.", hit.kind).to_lowercase();
            return self.end_turn(TurnStop::Budget, Some(reason), None, Some(hit));
        }
        let (turn_id, steps, summarized) = (t.id.clone(), t.steps, t.summarized);
        if steps > 0 {
            let info = StepInfo {
                turn: turn_id.clone(),
                purpose: Purpose::Reply,
                first: false,
                input: Vec::new(),
            };
            if let Link::Deny(reason) = self.link_pre_step(&info) {
                return self.end_turn(TurnStop::Failed, Some(reason), None, None);
            }
        }
        let start = self.reply_start();
        let probe = Probe {
            session: self.id.clone(),
            turn: turn_id,
            start: start.clone(),
        };
        let mut cfg = harness_compact::Config {
            budget_tokens: self.cfg.preset.compaction.budget_tokens,
            trim_over_bytes: self.cfg.preset.compaction.trim_over_bytes,
            head_bytes: self.cfg.preset.compaction.head_bytes,
            tail_bytes: self.cfg.preset.compaction.tail_bytes,
            summaries: self
                .cfg
                .preset
                .compaction
                .summaries_allowed(self.cfg.profile.as_ref()),
            keep_turns: self.cfg.preset.compaction.keep_turns,
        };
        cfg.summaries &= !summarized;
        match harness_compact::plan(&self.events, &probe, &cfg) {
            Plan::Fits { .. } => {}
            Plan::Compact(c) => {
                self.emit(None, Event::Compacted(c));
            }
            Plan::Summarize {
                trim,
                turns,
                tokens_before,
            } => {
                if let Some(c) = trim {
                    self.emit(None, Event::Compacted(c));
                }
                if let Some(t) = self.turn.as_mut() {
                    t.summarized = true;
                }
                let s = StepStart {
                    purpose: Purpose::Summary,
                    model: start.model.clone(),
                    params: Params {
                        max_output_tokens: 512,
                        reasoning: Reasoning::Off,
                        options: Default::default(),
                    },
                    tools: None,
                };
                return self.start_step(s, Some((turns, tokens_before)));
            }
        }
        if let Some(t) = self.turn.as_mut() {
            t.steps += 1;
            t.meter.step();
        }
        self.start_step(start, None);
    }

    pub(crate) fn reply_start(&self) -> StepStart {
        let last = self.events.iter().rev().find_map(|e| match &e.event {
            Event::StepStart(s) if s.purpose == Purpose::Reply => s.tools.clone(),
            _ => None,
        });
        let offered = &self.cfg.tools;
        let tools = match last {
            Some(prev) if &prev == offered => None,
            None if offered.is_empty() => None,
            _ => Some(offered.clone()),
        };
        StepStart {
            purpose: Purpose::Reply,
            model: self.cfg.preset.model.id.clone(),
            params: self.cfg.preset.model.params(),
            tools,
        }
    }

    pub(crate) fn start_step(&mut self, start: StepStart, summary: Option<(Vec<TurnId>, u64)>) {
        let id = StepId::new(self.next_id("stp"));
        let purpose = start.purpose.clone();
        self.emit(Some(id.clone()), Event::StepStart(start));
        let request = match request_for(&self.events, &id) {
            Ok(r) => r,
            Err(e) => {
                return self.fail_step(id, purpose, ErrorInfo::new("loop.history", e.to_string()))
            }
        };
        for l in &mut self.links {
            if let Link::Deny(reason) = l.request(&request) {
                let err = ErrorInfo::new("interceptor.denied", reason);
                return self.fail_step(id, purpose, err);
            }
        }
        self.out.push(Output::Request {
            step: id.clone(),
            request: request.clone(),
        });
        if let Some(t) = self.turn.as_mut() {
            t.phase = Phase::Model(Step {
                id,
                purpose,
                started: self.now,
                request,
                blocks: Vec::new(),
                stream: Vec::new(),
                calls: Vec::new(),
                refusal: None,
                retry: 0,
                usage: Usage::default(),
                summary,
            });
        }
    }

    /// A step that cannot go on: record it and close the turn.
    fn fail_step(&mut self, id: StepId, purpose: Purpose, err: ErrorInfo) {
        self.emit(
            Some(id.clone()),
            Event::Attempt(Attempt {
                outcome: AttemptOutcome::Error,
                error: Some(err.clone()),
                content: Vec::new(),
                usage: None,
                retry: 0,
                will_retry: false,
            }),
        );
        self.end_step(id, StepOutcome::Failed, None);
        self.after_failed_step(purpose, err);
    }

    fn after_failed_step(&mut self, purpose: Purpose, err: ErrorInfo) {
        if purpose == Purpose::Title {
            self.heuristic_title();
            self.turn = None;
            return self.claim();
        }
        let reason = err.message.clone();
        self.end_turn(TurnStop::Failed, Some(reason), Some(err), None);
    }

    pub(crate) fn end_step(&mut self, id: StepId, outcome: StepOutcome, usage: Option<Usage>) {
        let started = match self.turn.as_ref().map(|t| &t.phase) {
            Some(Phase::Model(s)) => s.started,
            Some(Phase::Tools(c)) => c.started,
            _ => self.now,
        };
        self.emit(
            Some(id),
            Event::StepEnd(StepEnd {
                outcome,
                duration_ms: self.now.saturating_sub(started),
                usage,
            }),
        );
        if let Some(t) = self.turn.as_mut() {
            t.phase = Phase::Between;
        }
    }

    pub(crate) fn model_event(&mut self, ev: ModelEvent) {
        let now = self.now;
        let (session, after) = (self.id.clone(), self.seq);
        let Some(t) = self.turn.as_mut() else { return };
        let turn = t.id.clone();
        let Phase::Model(step) = &mut t.phase else {
            return;
        };
        match ev {
            ModelEvent::Text(text) | ModelEvent::Reasoning(text) if text.is_empty() => {}
            ModelEvent::Text(text) => {
                let i = append(&mut step.blocks, &text, false);
                step.stream.push(chunk(i, &text, now, step.started));
                let d = LiveDelta {
                    session,
                    turn,
                    step: step.id.clone(),
                    after_seq: after,
                    block: i as u32,
                    kind: DeltaKind::Text,
                    text,
                };
                self.out.push(Output::Delta(d));
            }
            ModelEvent::Reasoning(text) => {
                let i = append(&mut step.blocks, &text, true);
                step.stream.push(chunk(i, &text, now, step.started));
                let d = LiveDelta {
                    session,
                    turn,
                    step: step.id.clone(),
                    after_seq: after,
                    block: i as u32,
                    kind: DeltaKind::Reasoning,
                    text,
                };
                self.out.push(Output::Delta(d));
            }
            ModelEvent::ToolCall { tool, input } => step.calls.push((tool, input)),
            ModelEvent::Refusal(text) => step.refusal = Some(text),
            ModelEvent::Done { stop, usage } => {
                step.usage = step.usage.plus(usage);
                self.finish_model_step(stop);
            }
        }
    }

    fn finish_model_step(&mut self, stop: ModelStop) {
        let Some(t) = self.turn.as_mut() else { return };
        let Phase::Model(step) = std::mem::replace(&mut t.phase, Phase::Between) else {
            return;
        };
        t.usage = t.usage.plus(step.usage);
        t.meter.add(&step.usage);
        let continues = if step.purpose == Purpose::Reply {
            t.continues.take()
        } else {
            None
        };
        let id = step.id.clone();
        // Keep the step's start time for step.end.
        t.phase = Phase::Model(Step {
            blocks: Vec::new(),
            stream: Vec::new(),
            calls: Vec::new(),
            ..step_shell(&step)
        });
        if let Some(text) = step.refusal.clone() {
            let mut content = step.blocks.clone();
            content.push(Block::text(text));
            self.emit(
                Some(id.clone()),
                Event::Attempt(Attempt {
                    outcome: AttemptOutcome::Refused,
                    error: None,
                    content,
                    usage: Some(step.usage),
                    retry: step.retry,
                    will_retry: false,
                }),
            );
            self.end_step(id, StepOutcome::Refused, Some(step.usage));
            if step.purpose == Purpose::Title {
                self.heuristic_title();
                self.turn = None;
                return self.claim();
            }
            return self.end_turn(TurnStop::Refused, None, None, None);
        }
        let stop = match stop {
            ModelStop::End => StopReason::End,
            ModelStop::ToolUse if !step.calls.is_empty() => StopReason::ToolUse,
            ModelStop::ToolUse => StopReason::End,
            ModelStop::MaxTokens => StopReason::MaxTokens,
        };
        self.emit(
            Some(id.clone()),
            Event::Assistant(Assistant {
                content: step.blocks.clone(),
                stop,
                stream: step.stream.clone(),
                usage: Some(step.usage),
                continues,
            }),
        );
        match step.purpose {
            Purpose::Title => {
                self.end_step(id, StepOutcome::Complete, Some(step.usage));
                let title = text_of(&step.blocks);
                let title = title.trim().trim_matches('"').trim().to_owned();
                if title.is_empty() {
                    self.heuristic_title();
                } else {
                    self.emit(
                        None,
                        Event::Title(Title {
                            title,
                            source: TitleSource::Model,
                        }),
                    );
                }
                self.turn = None;
                self.claim();
            }
            Purpose::Summary => {
                self.end_step(id, StepOutcome::Complete, Some(step.usage));
                let (turns, before) = step.summary.clone().unwrap_or_default();
                let probe = Probe {
                    session: self.id.clone(),
                    turn: self
                        .turn
                        .as_ref()
                        .map(|t| t.id.clone())
                        .unwrap_or_else(|| TurnId::from("")),
                    start: self.reply_start(),
                };
                let c = harness_compact::summarized(
                    &self.events,
                    &probe,
                    turns,
                    text_of(&step.blocks),
                    before,
                );
                self.emit(None, Event::Compacted(c));
                self.reply_step();
            }
            _ if !step.calls.is_empty() => self.begin_calls(step),
            _ => {
                self.end_step(id, StepOutcome::Complete, Some(step.usage));
                let turn = self.turn.as_ref().map(|t| t.id.clone());
                if let Some(turn) = turn {
                    for l in &mut self.links {
                        if let Link::Deny(reason) = l.turn_stopping(&turn) {
                            return self.end_turn(TurnStop::Failed, Some(reason), None, None);
                        }
                    }
                }
                self.end_turn(TurnStop::Done, None, None, None);
            }
        }
    }

    pub(crate) fn model_error(&mut self, err: ErrorInfo) {
        let max = self.cfg.max_retries;
        let Some(t) = self.turn.as_mut() else { return };
        let Phase::Model(step) = &mut t.phase else {
            return;
        };
        let will_retry = err.transient && step.retry < max;
        let attempt = Attempt {
            outcome: AttemptOutcome::Error,
            error: Some(err.clone()),
            content: std::mem::take(&mut step.blocks),
            usage: None,
            retry: step.retry,
            will_retry,
        };
        let (id, purpose, request) = (step.id.clone(), step.purpose.clone(), step.request.clone());
        if will_retry {
            step.retry += 1;
            step.stream.clear();
            step.calls.clear();
            step.refusal = None;
        }
        self.emit(Some(id.clone()), Event::Attempt(attempt));
        if will_retry {
            self.out.push(Output::Request { step: id, request });
            return;
        }
        let usage = match self.turn.as_ref().map(|t| &t.phase) {
            Some(Phase::Model(s)) => Some(s.usage),
            _ => None,
        };
        self.end_step(id, StepOutcome::Failed, usage);
        self.after_failed_step(purpose, err);
    }
}

/// A step with no output, keeping its identity and timing.
pub(crate) fn step_shell(step: &Step) -> Step {
    Step {
        id: step.id.clone(),
        purpose: step.purpose.clone(),
        started: step.started,
        request: step.request.clone(),
        blocks: Vec::new(),
        stream: Vec::new(),
        calls: Vec::new(),
        refusal: None,
        retry: step.retry,
        usage: step.usage,
        summary: None,
    }
}

/// Append streamed text to the last block of its kind, or a new one.
fn append(blocks: &mut Vec<Block>, text: &str, reasoning: bool) -> usize {
    match (blocks.last_mut(), reasoning) {
        (Some(Block::Text { text: t }), false) => t.push_str(text),
        (Some(Block::Reasoning { text: t, .. }), true) => t.push_str(text),
        (_, false) => blocks.push(Block::text(text)),
        (_, true) => blocks.push(Block::Reasoning {
            text: text.to_owned(),
            elapsed_ms: None,
        }),
    }
    blocks.len() - 1
}

fn chunk(block: usize, text: &str, now: Millis, started: Millis) -> Chunk {
    Chunk(
        block as u32,
        text.len() as u32,
        now.saturating_sub(started) as u32,
    )
}

pub(crate) fn text_of(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
