//! The session state machine: construction, inputs, turns, continue and
//! the title step. Steps and the model are in `steps.rs`, stopping and
//! recovery in `stop.rs`, tool calls in `tools.rs`.

use crate::context::context_text;
use crate::tools::Calls;
use crate::*;
use harness_log::history::heuristic_title;
use harness_log::ModelRequest;
use harness_policy::{Flags, Meter};
use harness_preset::TitlePath;
use harness_tools::ClientCalls;
use std::collections::{BTreeMap, VecDeque};

/// One session's loop. See the crate docs.
pub struct Session {
    pub(crate) cfg: Config,
    pub(crate) id: SessionId,
    pub(crate) app: AppId,
    pub(crate) events: Vec<SessionEvent>,
    pub(crate) seq: Seq,
    pub(crate) now: Millis,
    pub(crate) links: Vec<Box<dyn Interceptor>>,
    pub(crate) flags: Flags,
    pub(crate) counters: BTreeMap<&'static str, u32>,
    pub(crate) turn: Option<Turn>,
    pub(crate) inbox: VecDeque<(Vec<Block>, Option<RequestContext>)>,
    pub(crate) titled: bool,
    pub(crate) failed: bool,
    pub(crate) client: ClientCalls,
    pub(crate) client_gone: bool,
    pub(crate) out: Vec<Output>,
}

pub(crate) struct Turn {
    pub(crate) id: TurnId,
    pub(crate) started: Millis,
    pub(crate) meter: Meter,
    pub(crate) steps: u32,
    pub(crate) usage: Usage,
    pub(crate) continues: Option<Seq>,
    pub(crate) summarized: bool,
    pub(crate) closed: bool,
    pub(crate) phase: Phase,
}

// One phase per session: the size difference does not matter.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Phase {
    Between,
    Model(Step),
    Tools(Calls),
}

pub(crate) struct Step {
    pub(crate) id: StepId,
    pub(crate) purpose: Purpose,
    pub(crate) started: Millis,
    pub(crate) request: ModelRequest,
    pub(crate) blocks: Vec<Block>,
    pub(crate) stream: Vec<Chunk>,
    pub(crate) calls: Vec<(String, String)>,
    pub(crate) refusal: Option<String>,
    pub(crate) retry: u32,
    pub(crate) usage: Usage,
    pub(crate) summary: Option<(Vec<TurnId>, u64)>,
}

impl Session {
    fn blank(cfg: Config, id: SessionId, app: AppId, events: Vec<SessionEvent>) -> Self {
        Session {
            cfg,
            id,
            app,
            events,
            seq: 0,
            now: 0,
            links: Vec::new(),
            flags: Flags::default(),
            counters: BTreeMap::new(),
            turn: None,
            inbox: VecDeque::new(),
            titled: false,
            failed: false,
            client: ClientCalls::new(),
            client_gone: false,
            out: Vec::new(),
        }
    }

    /// A new session: `session.start`, then `system(preset)`.
    pub fn create(
        cfg: Config,
        id: SessionId,
        start: SessionStart,
        now: Millis,
    ) -> (Self, Vec<Output>) {
        let mut s = Self::blank(cfg, id, start.app.clone(), Vec::new());
        s.now = now;
        s.emit(None, Event::SessionStart(start));
        let text = s.cfg.preset.system_prompt();
        s.emit(
            None,
            Event::System(System {
                origin: SystemOrigin::Preset,
                text,
                context: None,
            }),
        );
        let out = std::mem::take(&mut s.out);
        (s, out)
    }

    /// A fork: `parent` is the parent's resolved history through the fork
    /// point; writes `session.start` and `forked`.
    pub fn fork(
        cfg: Config,
        id: SessionId,
        start: SessionStart,
        parent: &[SessionEvent],
        forked: Forked,
        now: Millis,
    ) -> (Self, Vec<Output>) {
        let mut s = Self::resume_state(cfg, id, start.app.clone(), parent.to_vec());
        s.now = now;
        s.emit(None, Event::SessionStart(start));
        s.emit(None, Event::Forked(forked));
        let out = std::mem::take(&mut s.out);
        (s, out)
    }

    /// Resume from a resolved history. An interrupted turn is sealed.
    pub fn resume(
        cfg: Config,
        id: SessionId,
        events: Vec<SessionEvent>,
        now: Millis,
    ) -> (Self, Vec<Output>) {
        let app = app_of(&id, &events);
        let mut s = Self::resume_state(cfg, id, app, events);
        s.now = now;
        s.seal_interrupted(None);
        let out = std::mem::take(&mut s.out);
        (s, out)
    }

    /// Rebuild a session whose loop faulted (a panic): its open turn is
    /// sealed as failed with `err`, and the session is marked failed.
    pub fn fail(
        cfg: Config,
        id: SessionId,
        events: Vec<SessionEvent>,
        now: Millis,
        err: ErrorInfo,
    ) -> (Self, Vec<Output>) {
        let app = app_of(&id, &events);
        let mut s = Self::resume_state(cfg, id, app, events);
        s.now = now;
        s.seal_interrupted(Some(err.clone()));
        if !s.failed {
            s.mark_failed(&err);
        }
        let out = std::mem::take(&mut s.out);
        (s, out)
    }

    /// Whether the session failed and takes no more input.
    pub fn failed(&self) -> bool {
        self.failed
    }

    fn resume_state(cfg: Config, id: SessionId, app: AppId, events: Vec<SessionEvent>) -> Self {
        let mut s = Self::blank(cfg, id, app, events);
        let mut effects: BTreeMap<CallId, Vec<Effect>> = BTreeMap::new();
        for e in &s.events {
            if e.session == s.id {
                s.seq = s.seq.max(e.seq);
            }
            let key = match &e.event {
                Event::TurnStart(_) => "trn",
                Event::StepStart(_) => "stp",
                Event::ToolCall(c) => {
                    effects.insert(c.call.clone(), c.effects.clone());
                    "cal"
                }
                Event::ApprovalRequest(_) => "apv",
                Event::Question(_) => "qst",
                Event::Title(_) => {
                    s.titled = true;
                    continue;
                }
                Event::ToolResult(r) if r.outcome == ToolOutcome::Ok => {
                    if let Some(fx) = effects.get(&r.call) {
                        s.flags.note(fx);
                    }
                    continue;
                }
                Event::SessionStatus(st) => {
                    s.failed = st.status == Status::Failed;
                    continue;
                }
                _ => continue,
            };
            *s.counters.entry(key).or_default() += 1;
        }
        s
    }

    /// Add a link to the interception chain.
    pub fn add_link(&mut self, link: Box<dyn Interceptor>) {
        self.links.push(link);
    }

    /// The session's id.
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// The resolved history, including this session's events so far.
    pub fn events(&self) -> &[SessionEvent] {
        &self.events
    }

    /// Whether a turn (or a post-turn step) is running.
    pub fn running(&self) -> bool {
        self.turn.is_some()
    }

    /// Handle one input at `now`.
    pub fn handle(&mut self, now: Millis, input: Input) -> Vec<Output> {
        self.now = self.now.max(now);
        if self.failed && !matches!(input, Input::Tick | Input::Rename(_)) {
            return vec![Output::Refused(Refusal::SessionFailed)];
        }
        match input {
            Input::User { content, context } => {
                self.inbox.push_back((content, context));
                if self.turn.is_none() {
                    self.claim();
                }
            }
            Input::Continue => self.continue_turn(),
            Input::Cancel => self.stop(TurnStop::Cancelled, None),
            Input::BudgetStop(hit) => self.stop(TurnStop::Budget, Some(hit)),
            Input::Fail(err) => self.fail_now(err),
            Input::Model(ev) => self.model_event(ev),
            Input::ModelError(err) => self.model_error(err),
            Input::Tool { call, output } => self.tool_output(call, output),
            Input::Decision(d) => self.decision(d),
            Input::Answer(a) => self.answer(a),
            Input::ClientGone => self.client_gone(),
            Input::ClientBack => self.client_back(),
            Input::Tools(specs) => self.cfg.tools = specs,
            Input::Rename(title) => {
                self.titled = true;
                self.emit(
                    None,
                    Event::Title(Title {
                        title,
                        source: TitleSource::User,
                    }),
                );
            }
            Input::Tick => self.tick(),
        }
        std::mem::take(&mut self.out)
    }

    // ----- plumbing -----

    pub(crate) fn next_id(&mut self, key: &'static str) -> String {
        let n = self.counters.entry(key).or_default();
        *n += 1;
        format!("{key}_{n:04}")
    }

    pub(crate) fn emit(&mut self, step: Option<StepId>, event: Event) -> Seq {
        self.seq += 1;
        let e = SessionEvent {
            session: self.id.clone(),
            seq: self.seq,
            at: self.now,
            turn: self.turn.as_ref().map(|t| t.id.clone()),
            step,
            event,
        };
        self.events.push(e.clone());
        self.out.push(Output::Event(e));
        self.seq
    }

    pub(crate) fn link_pre_step(&mut self, info: &StepInfo) -> Link {
        for l in &mut self.links {
            if let Link::Deny(r) = l.pre_step(info) {
                return Link::Deny(r);
            }
        }
        Link::Pass
    }

    // ----- turns -----

    pub(crate) fn claim(&mut self) {
        let Some((content, context)) = self.inbox.pop_front() else {
            self.out.push(Output::Idle);
            return;
        };
        let id = TurnId::new(self.next_id("trn"));
        self.turn = Some(Turn {
            id: id.clone(),
            started: self.now,
            meter: Meter::start(self.now),
            steps: 0,
            usage: Usage::default(),
            continues: None,
            summarized: false,
            closed: false,
            phase: Phase::Between,
        });
        self.emit(
            None,
            Event::TurnStart(TurnStart {
                cause: Cause::User,
                wake: None,
            }),
        );
        self.emit(
            None,
            Event::User(User {
                content: content.clone(),
            }),
        );
        if let Some((text, kept)) = context
            .as_ref()
            .and_then(|c| context_text(&self.cfg.preset.context, c))
        {
            self.emit(
                None,
                Event::System(System {
                    origin: SystemOrigin::Context,
                    text,
                    context: Some(kept),
                }),
            );
        }
        let info = StepInfo {
            turn: id,
            purpose: Purpose::Reply,
            first: true,
            input: content,
        };
        if let Link::Deny(reason) = self.link_pre_step(&info) {
            return self.end_turn(TurnStop::Rejected, Some(reason), None, None);
        }
        self.reply_step();
    }

    fn continue_turn(&mut self) {
        if self.turn.is_some() {
            self.out.push(Output::Refused(Refusal::Busy));
            return;
        }
        match self.continuable() {
            Err(reason) => self
                .out
                .push(Output::Refused(Refusal::ContinueRefused(reason.into()))),
            Ok(seq) => {
                let id = TurnId::new(self.next_id("trn"));
                self.turn = Some(Turn {
                    id,
                    started: self.now,
                    meter: Meter::start(self.now),
                    steps: 0,
                    usage: Usage::default(),
                    continues: Some(seq),
                    summarized: true,
                    closed: false,
                    phase: Phase::Between,
                });
                self.emit(
                    None,
                    Event::TurnStart(TurnStart {
                        cause: Cause::Continue,
                        wake: None,
                    }),
                );
                self.reply_step();
            }
        }
    }

    /// The sealed partial reply that continue would extend.
    fn continuable(&self) -> Result<Seq, &'static str> {
        let last_turn = self.events.iter().rev().find_map(|e| match &e.event {
            Event::TurnEnd(t) => Some((e.turn.clone(), t.stop.clone())),
            _ => None,
        });
        let Some((turn, stop)) = last_turn else {
            return Err("There is no reply to continue.");
        };
        if stop == TurnStop::Refused {
            return Err("A refusal cannot be continued.");
        }
        let mut sealed = None;
        for e in self.events.iter().rev() {
            if e.turn != turn {
                continue;
            }
            match &e.event {
                Event::ToolCall(_) | Event::ToolResult(_) => {
                    return Err("The reply ended with tool calls.");
                }
                Event::Assistant(a) if sealed.is_none() => {
                    if !matches!(a.stop, StopReason::Cancelled | StopReason::MaxTokens) {
                        return Err("The reply is complete.");
                    }
                    sealed = Some(e.seq);
                }
                Event::StepStart(s) if sealed.is_some() && s.purpose == Purpose::Reply => break,
                _ => {}
            }
        }
        sealed.ok_or("There is no partial reply to continue.")
    }

    pub(crate) fn end_turn(
        &mut self,
        stop: TurnStop,
        reason: Option<String>,
        error: Option<ErrorInfo>,
        budget: Option<BudgetHit>,
    ) {
        let Some(t) = self.turn.as_mut() else { return };
        t.closed = true;
        t.phase = Phase::Between;
        let end = TurnEnd {
            stop: stop.clone(),
            steps: t.steps,
            duration_ms: self.now.saturating_sub(t.started),
            usage: t.usage,
            reason,
            error,
            budget,
        };
        self.emit(None, Event::TurnEnd(end));
        if stop == TurnStop::Done && !self.titled {
            self.titled = true;
            match self.cfg.preset.title_path(self.cfg.profile.as_ref()) {
                TitlePath::Off => {}
                TitlePath::Heuristic => self.heuristic_title(),
                TitlePath::Model(model) => {
                    let start = StepStart {
                        purpose: Purpose::Title,
                        model,
                        params: Params {
                            max_output_tokens: 64,
                            reasoning: Reasoning::Off,
                            options: Default::default(),
                        },
                        tools: None,
                    };
                    return self.start_step(start, None);
                }
            }
        }
        self.turn = None;
        self.claim();
    }

    pub(crate) fn heuristic_title(&mut self) {
        let title = heuristic_title(&self.events);
        if !title.is_empty() {
            self.emit(
                None,
                Event::Title(Title {
                    title,
                    source: TitleSource::Heuristic,
                }),
            );
        }
    }
}

fn app_of(id: &SessionId, events: &[SessionEvent]) -> AppId {
    events
        .iter()
        .filter(|e| &e.session == id)
        .find_map(|e| match &e.event {
            Event::SessionStart(s) => Some(s.app.clone()),
            _ => None,
        })
        .unwrap_or_else(|| AppId::from(""))
}
