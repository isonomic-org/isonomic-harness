//! Isonomic Harness compaction.
//!
//! Before a reply step, the loop asks [`plan`] whether the request it is
//! about to make fits the budget. If not:
//!
//! 1. **Trim first.** Tool results longer than the threshold are cut to
//!    head and tail, oldest first, until the request fits.
//! 2. **Then summarize,** only when the hardware profile allows it: older
//!    completed turns are replaced by a summary that a `summary` step writes.
//!
//! Compaction is itself an event (`compacted`), recorded before the step it
//! serves, so replaying the log rebuilds every request exactly: harness-log
//! applies trims and summaries from their position on.
//!
//! A summary step summarizes every completed turn before the current one;
//! the most recent `keep_turns` of them stay verbatim as well.
//!
//! The budget is soft: when nothing more is allowed, the step runs over
//! it. The model's window ([`Config::window`]: its context less room for
//! the reply) is hard: a request never goes out larger than it.
//!
//! 3. **Against the window, give up kept turns,** one at a time, so the
//!    summary replaces more of them.
//! 4. **Against the window, without a summary step** (not allowed, or its
//!    own request would not fit): the oldest turns are replaced by a fixed
//!    note, [`NOT_SHOWN`], with no model step. The log keeps them.
//! 5. The current turn and the system prompt always stay. If they alone
//!    pass the window, the plan is [`Plan::TooLarge`] and nothing is sent.

use harness_core::*;
use harness_log::history::request_for;
use std::collections::BTreeSet;

/// Compaction settings (from the preset, with the profile applied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Compact when the request's estimate exceeds this.
    pub budget_tokens: u64,
    /// Results longer than this (in bytes of text) may be trimmed.
    pub trim_over_bytes: u64,
    /// Bytes kept from a trimmed result's start.
    pub head_bytes: u64,
    /// Bytes kept from its end.
    pub tail_bytes: u64,
    /// Whether summaries are allowed on this session's profile.
    pub summaries: bool,
    /// Recent completed turns never replaced by a summary.
    pub keep_turns: u32,
    /// The model's context size. `None`: unknown, and only the budget
    /// applies.
    pub context_tokens: Option<u64>,
    /// Room kept in the context for a reply.
    pub reply_tokens: u64,
}

impl Config {
    /// The most a reply step's request may hold: the context less room
    /// for the reply.
    pub fn window(&self) -> Option<u64> {
        self.context_tokens
            .map(|c| c.saturating_sub(self.reply_tokens))
    }

    /// The most a summary step's request may hold: the context less room
    /// for the summary (at most half the context).
    fn summary_window(&self) -> Option<u64> {
        self.context_tokens.map(|c| c - SUMMARY_TOKENS.min(c / 2))
    }
}

/// What stands in for turns left out of a request to fit the window.
pub const NOT_SHOWN: &str = "Earlier messages are not shown to the model.";

/// The reply step the loop is about to start.
#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    /// The session.
    pub session: SessionId,
    /// The current turn.
    pub turn: TurnId,
    /// The step's `step.start`, as it would be written.
    pub start: StepStart,
}

/// What to do before the step.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// The request fits; nothing to record.
    Fits {
        /// The estimate.
        tokens: u64,
    },
    /// Record this `compacted` event (trims only). `tokens_after` may
    /// still exceed the budget when nothing more is allowed; the step then
    /// runs as is.
    Compact(Compacted),
    /// Trimming is not enough. Record `trim` if present, run a summary
    /// step, then record [`summarized`] with its text.
    Summarize {
        /// Trims to record first.
        trim: Option<Compacted>,
        /// The turns the summary will replace.
        turns: Vec<TurnId>,
        /// The estimate before any compaction.
        tokens_before: u64,
    },
    /// The system prompt and the current turn alone pass the model's
    /// window. Nothing may be sent; the turn fails.
    TooLarge {
        /// The smallest the request can be made.
        tokens: u64,
        /// The window it has to fit.
        window: u64,
    },
}

/// The text of a result as harness-log renders it for trimming.
fn result_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn with(
    events: &[SessionEvent],
    probe: &Probe,
    extra: Vec<(Option<StepId>, Event)>,
) -> Vec<SessionEvent> {
    let mut out = events.to_vec();
    let (mut seq, at) = events.last().map(|e| (e.seq, e.at)).unwrap_or_default();
    for (step, event) in extra {
        seq += 1;
        out.push(SessionEvent {
            session: probe.session.clone(),
            seq,
            at,
            turn: Some(probe.turn.clone()),
            step,
            event,
        });
    }
    out
}

/// The estimate for the probed step's request, after `compacted` (if any).
pub fn estimate(events: &[SessionEvent], probe: &Probe, compacted: Option<&Compacted>) -> u64 {
    let step = StepId::from("probe");
    let mut extra = Vec::new();
    if let Some(c) = compacted {
        extra.push((None, Event::Compacted(c.clone())));
    }
    extra.push((Some(step.clone()), Event::StepStart(probe.start.clone())));
    request_for(&with(events, probe, extra), &step)
        .map(|r| r.estimate_tokens())
        .unwrap_or(0)
}

/// Trimmed calls so far, and results eligible for trimming (oldest first).
fn candidates(events: &[SessionEvent], cfg: &Config) -> Vec<(CallId, u64, Seq)> {
    let mut trimmed = BTreeSet::new();
    for e in events {
        if let Event::Compacted(c) = &e.event {
            trimmed.extend(c.trimmed.iter().map(|t| t.call.clone()));
        }
    }
    events
        .iter()
        .filter_map(|e| match &e.event {
            Event::ToolResult(r) if !trimmed.contains(&r.call) => {
                let len = result_text(&r.content).len() as u64;
                (len > cfg.trim_over_bytes).then(|| (r.call.clone(), len, e.seq))
            }
            _ => None,
        })
        .collect()
}

/// Completed turns that a summary may replace: every turn that closed
/// before the current one and is not already summarized, minus the most
/// recent `keep_turns`.
pub fn summarizable(events: &[SessionEvent], current: &TurnId, keep_turns: u32) -> Vec<TurnId> {
    let mut done = Vec::new();
    let mut summarized = BTreeSet::new();
    for e in events {
        match &e.event {
            Event::TurnEnd(t) if !matches!(t.stop, TurnStop::Rejected | TurnStop::Refused) => {
                if let Some(id) = &e.turn {
                    if id != current && !done.contains(id) {
                        done.push(id.clone());
                    }
                }
            }
            Event::Compacted(c) => {
                if let Some(s) = &c.summary {
                    summarized.extend(s.turns.iter().cloned());
                }
            }
            _ => {}
        }
    }
    let keep = done.len().saturating_sub(keep_turns as usize);
    done.into_iter()
        .take(keep)
        .filter(|t| !summarized.contains(t))
        .collect()
}

/// Decide what compaction the probed step needs.
pub fn plan(events: &[SessionEvent], probe: &Probe, cfg: &Config) -> Plan {
    let before = estimate(events, probe, None);
    if before <= cfg.budget_tokens {
        return Plan::Fits { tokens: before };
    }
    let through = events.last().map(|e| e.seq).unwrap_or_default();
    let mut c = Compacted {
        through_seq: through,
        trimmed: Vec::new(),
        summary: None,
        tokens_before: before,
        tokens_after: before,
    };
    for (call, len, _) in candidates(events, cfg) {
        c.trimmed.push(Trim {
            call,
            original_bytes: len,
            head_bytes: cfg.head_bytes,
            tail_bytes: cfg.tail_bytes,
        });
        c.tokens_after = estimate(events, probe, Some(&c));
        if c.tokens_after <= cfg.budget_tokens {
            return Plan::Compact(c);
        }
    }
    let soft = if cfg.summaries {
        summarizable(events, &probe.turn, cfg.keep_turns)
    } else {
        Vec::new()
    };
    let summarize = |turns: Vec<TurnId>, c: Compacted| Plan::Summarize {
        trim: (!c.trimmed.is_empty()).then_some(c),
        turns,
        tokens_before: before,
    };
    // The window is hard. Without one, or under it, the budget is soft:
    // summarize what the preset allows, or run the step as is.
    let window = match cfg.window() {
        Some(w) if c.tokens_after > w => w,
        _ if soft.is_empty() => {
            return if c.trimmed.is_empty() {
                Plan::Fits { tokens: before }
            } else {
                Plan::Compact(c)
            };
        }
        _ => return summarize(soft, c),
    };
    // A summary that replaces enough turns, keeping as many as fit. Its
    // own request carries the turns it reads, so that has to fit too.
    let room = cfg.summary_window().unwrap_or(window);
    if cfg.summaries && summary_request(events, probe, &c) <= room {
        for keep in (0..=cfg.keep_turns).rev() {
            let turns = summarizable(events, &probe.turn, keep);
            if turns.is_empty() {
                continue;
            }
            let mut after = c.clone();
            after.summary = Some(Summary {
                text: "x".repeat(SUMMARY_BYTES),
                turns: turns.clone(),
            });
            if estimate(events, probe, Some(&after)) <= window {
                return summarize(turns, c);
            }
        }
    }
    // No model step: leave out the oldest turns behind a fixed note.
    let old = summarizable(events, &probe.turn, 0);
    for n in 1..=old.len() {
        c.summary = Some(Summary {
            text: NOT_SHOWN.into(),
            turns: old[..n].to_vec(),
        });
        c.tokens_after = estimate(events, probe, Some(&c));
        if c.tokens_after <= window {
            return Plan::Compact(c);
        }
    }
    Plan::TooLarge {
        tokens: c.tokens_after,
        window,
    }
}

/// A summary step's output limit, in tokens, and the bytes that come to.
const SUMMARY_TOKENS: u64 = 512;
const SUMMARY_BYTES: usize = 2048;

/// The estimate for the summary step's own request, after `trims`.
fn summary_request(events: &[SessionEvent], probe: &Probe, trims: &Compacted) -> u64 {
    let step = StepId::from("probe");
    let start = StepStart {
        purpose: Purpose::Summary,
        ..probe.start.clone()
    };
    let mut extra = Vec::new();
    if !trims.trimmed.is_empty() {
        extra.push((None, Event::Compacted(trims.clone())));
    }
    extra.push((Some(step.clone()), Event::StepStart(start)));
    request_for(&with(events, probe, extra), &step)
        .map(|r| r.estimate_tokens())
        .unwrap_or(u64::MAX)
}

/// The `compacted` event that records a finished summary step. `events`
/// includes the summary step; `tokens_before` comes from the plan.
pub fn summarized(
    events: &[SessionEvent],
    probe: &Probe,
    turns: Vec<TurnId>,
    text: String,
    tokens_before: u64,
) -> Compacted {
    let mut c = Compacted {
        through_seq: events.last().map(|e| e.seq).unwrap_or_default(),
        trimmed: Vec::new(),
        summary: Some(Summary { text, turns }),
        tokens_before,
        tokens_after: 0,
    };
    c.tokens_after = estimate(events, probe, Some(&c));
    c
}
