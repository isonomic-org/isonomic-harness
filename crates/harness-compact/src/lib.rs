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
}

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
    let turns = if cfg.summaries {
        summarizable(events, &probe.turn, cfg.keep_turns)
    } else {
        Vec::new()
    };
    if turns.is_empty() {
        return if c.trimmed.is_empty() {
            Plan::Fits { tokens: before }
        } else {
            Plan::Compact(c)
        };
    }
    Plan::Summarize {
        trim: (!c.trimmed.is_empty()).then_some(c),
        turns,
        tokens_before: before,
    }
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
