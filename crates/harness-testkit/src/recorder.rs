// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! A deterministic recorder for session logs.
//!
//! It assigns sequence numbers, ids and timestamps, synthesizes the compact
//! stream for each reply from a profile's output rate, and keeps usage
//! totals, so fixture scripts read like the turns they describe.

use crate::FIXTURE_EPOCH;
use harness_core::*;
use std::collections::BTreeMap;

/// Records events for one fixture.
#[derive(Debug)]
pub struct Recorder {
    now: Millis,
    seqs: BTreeMap<SessionId, Seq>,
    counters: BTreeMap<&'static str, u32>,
    /// Every event recorded, in persistence order.
    pub events: Vec<SessionEvent>,
}

/// Where a script is: the session, its open turn and step, and running
/// totals.
#[derive(Clone, Debug)]
pub struct Ctx {
    /// The session.
    pub session: SessionId,
    /// The open (or last) turn.
    pub turn: Option<TurnId>,
    /// The open step.
    pub step: Option<StepId>,
    /// The model for reply steps.
    pub model: String,
    /// Output rate, bytes a second.
    pub rate: u32,
    turn_at: Millis,
    step_at: Millis,
    turn_steps: u32,
    turn_usage: Usage,
    step_usage: Usage,
    history_bytes: u64,
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Recorder {
    /// A recorder starting at [`FIXTURE_EPOCH`].
    pub fn new() -> Self {
        Self {
            now: FIXTURE_EPOCH,
            seqs: BTreeMap::new(),
            counters: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    /// The next id with this prefix: `ses_0001`, `ses_0002`, …
    pub fn id(&mut self, prefix: &'static str) -> String {
        let n = self.counters.entry(prefix).or_insert(0);
        *n += 1;
        format!("{prefix}_{n:04}")
    }

    /// The current time.
    pub fn now(&self) -> Millis {
        self.now
    }

    /// Move time forward.
    pub fn advance(&mut self, ms: u64) {
        self.now += ms;
    }

    /// Append an event in `cx`'s session, turn and step. Returns its seq.
    pub fn emit(&mut self, cx: &Ctx, event: Event) -> Seq {
        let seq = self.seqs.entry(cx.session.clone()).or_insert(0);
        *seq += 1;
        self.events.push(SessionEvent {
            session: cx.session.clone(),
            seq: *seq,
            at: self.now,
            turn: cx.turn.clone(),
            step: cx.step.clone(),
            event,
        });
        *seq
    }

    /// Start a session: `session.start`.
    pub fn session(&mut self, start: SessionStart, model: &str) -> Ctx {
        let id = SessionId::new(self.id("ses"));
        self.session_as(id, start, model)
    }

    /// Start a session with an id taken earlier from [`Recorder::id`].
    pub fn session_as(&mut self, id: SessionId, start: SessionStart, model: &str) -> Ctx {
        let rate = match start.profile {
            Some(Profile::Intel) => 12,
            Some(Profile::Low) => 60,
            _ => 120,
        };
        let cx = Ctx {
            session: id,
            turn: None,
            step: None,
            model: model.to_owned(),
            rate,
            turn_at: 0,
            step_at: 0,
            turn_steps: 0,
            turn_usage: Usage::default(),
            step_usage: Usage::default(),
            history_bytes: 0,
        };
        self.emit(&cx, Event::SessionStart(start));
        cx
    }

    /// `system(preset)` before the first turn.
    pub fn preset_prompt(&mut self, cx: &mut Ctx, text: &str) {
        cx.history_bytes += text.len() as u64;
        self.emit(
            cx,
            Event::System(System {
                origin: SystemOrigin::Preset,
                memory: Vec::new(),
                memory_policy: None,
                text: text.to_owned(),
                context: None,
            }),
        );
    }

    /// `turn.start`.
    pub fn turn(&mut self, cx: &mut Ctx, cause: Cause, wake: Option<Wake>) {
        cx.turn = Some(TurnId::new(self.id("trn")));
        cx.step = None;
        cx.turn_at = self.now;
        cx.turn_steps = 0;
        cx.turn_usage = Usage::default();
        self.emit(cx, Event::TurnStart(TurnStart { cause, wake }));
    }

    /// `user` with text.
    pub fn user(&mut self, cx: &mut Ctx, text: &str) {
        self.user_blocks(cx, vec![Block::text(text)]);
    }

    /// `user` with any blocks.
    pub fn user_blocks(&mut self, cx: &mut Ctx, content: Vec<Block>) {
        cx.history_bytes += content.iter().map(block_len).sum::<u64>();
        self.emit(cx, Event::User(User { content }));
        self.advance(150);
    }

    /// `system(context)`: what the context link adds to the turn.
    pub fn context(&mut self, cx: &mut Ctx, location: Option<&str>) {
        let (date, long) = civil_date(self.now);
        let mut text = format!("Current date: {long} (America/New_York). Language: en.");
        if let Some(place) = location {
            text.push_str(&format!(" Location: {place}."));
        }
        cx.history_bytes += text.len() as u64;
        self.emit(
            cx,
            Event::System(System {
                origin: SystemOrigin::Context,
                memory: Vec::new(),
                memory_policy: None,
                text,
                context: Some(RequestContext {
                    folders: Vec::new(),
                    date,
                    time_zone: "America/New_York".into(),
                    language: "en".into(),
                    location: location.map(str::to_owned),
                }),
            }),
        );
    }

    /// `step.start`.
    pub fn step(&mut self, cx: &mut Ctx, purpose: Purpose, tools: Option<Vec<ToolSpec>>) {
        self.step_with(cx, purpose, tools, Reasoning::Off);
    }

    /// `step.start` with a reasoning level.
    pub fn step_with(
        &mut self,
        cx: &mut Ctx,
        purpose: Purpose,
        tools: Option<Vec<ToolSpec>>,
        reasoning: Reasoning,
    ) {
        let model = if purpose == Purpose::Title {
            "local:tiny".to_owned()
        } else {
            cx.model.clone()
        };
        cx.step = Some(StepId::new(self.id("stp")));
        cx.step_at = self.now;
        cx.step_usage = Usage::default();
        if purpose == Purpose::Reply {
            cx.turn_steps += 1;
        }
        let max_output_tokens = if purpose == Purpose::Reply { 2048 } else { 64 };
        self.emit(
            cx,
            Event::StepStart(StepStart {
                purpose,
                memory_policy: None,
                model,
                params: Params {
                    max_output_tokens,
                    reasoning,
                    options: Default::default(),
                },
                tools,
            }),
        );
    }

    fn usage_for(&self, cx: &Ctx, content: &[Block]) -> Usage {
        let out: u64 = content.iter().map(block_len).sum();
        let reasoning: u64 = content
            .iter()
            .filter(|b| matches!(b, Block::Reasoning { .. }))
            .map(block_len)
            .sum();
        Usage {
            input_tokens: cx.history_bytes.div_ceil(4) + 180,
            output_tokens: out.div_ceil(4),
            reasoning_tokens: reasoning.div_ceil(4),
            ..Usage::default()
        }
    }

    fn add_usage(cx: &mut Ctx, usage: Usage) {
        cx.step_usage = cx.step_usage.plus(usage);
        cx.turn_usage = cx.turn_usage.plus(usage);
    }

    /// `assistant`: streams `content` at the session's rate, then records it.
    pub fn reply(
        &mut self,
        cx: &mut Ctx,
        content: Vec<Block>,
        stop: StopReason,
        continues: Option<Seq>,
    ) -> Seq {
        let first = 450 + (self.now - cx.step_at);
        let (stream, took) = stream_for(&content, cx.rate, first as u32);
        self.now = cx.step_at + u64::from(took);
        let usage = self.usage_for(cx, &content);
        Self::add_usage(cx, usage);
        cx.history_bytes += content.iter().map(block_len).sum::<u64>();
        self.emit(
            cx,
            Event::Assistant(Assistant {
                content,
                stop,
                stream,
                usage: Some(usage),
                continues,
            }),
        )
    }

    /// `attempt`.
    pub fn attempt(&mut self, cx: &mut Ctx, attempt: Attempt) {
        self.advance(match attempt.outcome {
            AttemptOutcome::Cancelled => 0,
            AttemptOutcome::Refused => 1600,
            _ => 900,
        });
        if let Some(u) = attempt.usage {
            Self::add_usage(cx, u);
        }
        self.emit(cx, Event::Attempt(attempt));
    }

    /// Usage for text the model produced in an attempt.
    pub fn attempt_usage(&self, cx: &Ctx, content: &[Block]) -> Usage {
        self.usage_for(cx, content)
    }

    /// `tool.call`.
    pub fn call(&mut self, cx: &mut Ctx, call: ToolCall) {
        self.emit(cx, Event::ToolCall(call));
    }

    /// `tool.result`, after `ms` of work.
    pub fn result(&mut self, cx: &mut Ctx, mut result: ToolResult, ms: u64) {
        self.advance(ms);
        result.duration_ms = Some(ms);
        cx.history_bytes += result.content.iter().map(block_len).sum::<u64>();
        self.emit(cx, Event::ToolResult(result));
    }

    /// `tool.result` for a display call: the host answers at once, and
    /// records no duration.
    pub fn display_result(&mut self, cx: &mut Ctx, result: ToolResult) {
        self.advance(1);
        cx.history_bytes += result.content.iter().map(block_len).sum::<u64>();
        self.emit(cx, Event::ToolResult(result));
    }

    /// `step.end`.
    pub fn end_step(&mut self, cx: &mut Ctx, outcome: StepOutcome) {
        self.advance(20);
        let event = Event::StepEnd(StepEnd {
            outcome,
            duration_ms: self.now - cx.step_at,
            usage: Some(cx.step_usage),
        });
        self.emit(cx, event);
        cx.step = None;
    }

    /// `turn.end`.
    pub fn end_turn(&mut self, cx: &mut Ctx, stop: TurnStop, reason: Option<&str>) {
        self.end_turn_with(cx, stop, reason, None);
    }

    /// `turn.end` with a budget hit.
    pub fn end_turn_with(
        &mut self,
        cx: &mut Ctx,
        stop: TurnStop,
        reason: Option<&str>,
        budget: Option<BudgetHit>,
    ) {
        self.advance(10);
        let event = Event::TurnEnd(TurnEnd {
            stop,
            steps: cx.turn_steps,
            duration_ms: self.now - cx.turn_at,
            usage: cx.turn_usage,
            reason: reason.map(str::to_owned),
            error: None,
            budget,
        });
        self.emit(cx, event);
    }

    /// The post-turn title step on a model, then `title`.
    pub fn title_step(&mut self, cx: &mut Ctx, title: &str) {
        let saved_rate = cx.rate;
        cx.rate = 200;
        self.step(cx, Purpose::Title, None);
        let content = vec![Block::text(title)];
        let usage = self.usage_for(cx, &content);
        let (stream, took) = stream_for(&content, cx.rate, 300);
        self.now = cx.step_at + u64::from(took);
        cx.step_usage = usage;
        self.emit(
            cx,
            Event::Assistant(Assistant {
                content,
                stop: StopReason::End,
                stream,
                usage: Some(usage),
                continues: None,
            }),
        );
        self.end_step(cx, StepOutcome::Complete);
        cx.rate = saved_rate;
        self.emit_title(cx, title, TitleSource::Model);
    }

    /// `title` without a step: the heuristic or a rename.
    pub fn emit_title(&mut self, cx: &mut Ctx, title: &str, source: TitleSource) {
        self.emit(
            cx,
            Event::Title(Title {
                title: title.to_owned(),
                source,
            }),
        );
    }

    /// The events recorded so far.
    pub fn finish(self) -> Vec<SessionEvent> {
        self.events
    }
}

/// The UTC calendar date of `at`: `YYYY-MM-DD` and a long form such as
/// `Saturday, 14 November 2026`.
pub fn civil_date(at: Millis) -> (String, String) {
    const DAYS: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let days = (at / 86_400_000) as i64;
    // Days to civil date, proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let weekday = DAYS[days.rem_euclid(7) as usize];
    let name = MONTHS[(month - 1) as usize];
    (
        format!("{year:04}-{month:02}-{day:02}"),
        format!("{weekday}, {day} {name} {year}"),
    )
}

/// The byte length a block contributes to history.
pub fn block_len(block: &Block) -> u64 {
    match block {
        Block::Text { text } | Block::Reasoning { text, .. } => text.len() as u64,
        Block::Attachment(a) => a.text.as_ref().map_or(0, |t| t.len() as u64),
        Block::Mention { label, .. } => label.len() as u64,
        Block::Json { .. } | Block::MemoryRef(_) => 64,
        Block::Unknown(_) => 0,
    }
}

/// The compact stream for `content` at `rate` bytes a second, starting at
/// `first_ms` after the step began. Returns the chunks and when the last one
/// landed.
pub fn stream_for(content: &[Block], rate: u32, first_ms: u32) -> (Vec<Chunk>, u32) {
    let interval = (8_000 / rate.max(1)).max(100);
    let per_chunk = (rate * interval / 1000).max(1) as usize;
    let mut chunks = Vec::new();
    let mut at = first_ms;
    for (i, block) in content.iter().enumerate() {
        let text = match block {
            Block::Text { text } | Block::Reasoning { text, .. } => text.as_str(),
            _ => continue,
        };
        let mut start = 0;
        while start < text.len() {
            let mut end = (start + per_chunk).min(text.len());
            while !text.is_char_boundary(end) {
                end += 1;
            }
            chunks.push(Chunk(i as u32, (end - start) as u32, at));
            at += interval;
            start = end;
        }
    }
    (chunks, at)
}
