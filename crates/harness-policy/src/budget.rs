//! Budgets: tokens, steps, wall time and cost. A budget is checked before a
//! step starts, so a turn stops cleanly between steps. The host keeps one
//! meter per turn, per session and per app, each with its own limits.

use harness_core::*;

/// Budget limits for one scope (a turn, a session or an app); `None` is
/// unlimited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    /// Tokens (input plus output).
    pub tokens: Option<u64>,
    /// Model steps.
    pub steps: Option<u64>,
    /// Wall time since the scope started, in milliseconds.
    pub wall_ms: Option<u64>,
    /// Cost, in micro-units.
    pub cost_micros: Option<u64>,
}

impl Limits {
    /// The tighter of two limit sets: each limit is the smaller. An app may
    /// only tighten its preset's defaults this way.
    pub fn tighten(&self, other: &Limits) -> Limits {
        fn min(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            match (a, b) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, None) | (None, x) => x,
            }
        }
        Limits {
            tokens: min(self.tokens, other.tokens),
            steps: min(self.steps, other.steps),
            wall_ms: min(self.wall_ms, other.wall_ms),
            cost_micros: min(self.cost_micros, other.cost_micros),
        }
    }
}

/// What a scope has used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meter {
    /// Tokens used.
    pub tokens: u64,
    /// Steps started.
    pub steps: u64,
    /// Cost so far.
    pub cost_micros: u64,
    /// When the scope started.
    pub started_at: Millis,
}

impl Meter {
    /// A meter for a scope starting now.
    pub fn start(now: Millis) -> Self {
        Self {
            started_at: now,
            ..Self::default()
        }
    }

    /// A step started.
    pub fn step(&mut self) {
        self.steps += 1;
    }

    /// A request used this much.
    pub fn add(&mut self, usage: &Usage) {
        self.tokens += usage.input_tokens + usage.output_tokens;
        self.cost_micros += usage.cost_micros.unwrap_or(0);
    }

    /// The first limit reached, checked before starting another step.
    pub fn check(&self, limits: &Limits, now: Millis) -> Option<BudgetHit> {
        let hit = |kind: BudgetKind, limit: u64, used: u64| BudgetHit { kind, limit, used };
        if let Some(l) = limits.steps.filter(|l| self.steps >= *l) {
            return Some(hit(BudgetKind::Steps, l, self.steps));
        }
        if let Some(l) = limits.tokens.filter(|l| self.tokens >= *l) {
            return Some(hit(BudgetKind::Tokens, l, self.tokens));
        }
        let wall = now.saturating_sub(self.started_at);
        if let Some(l) = limits.wall_ms.filter(|l| wall >= *l) {
            return Some(hit(BudgetKind::WallTime, l, wall));
        }
        if let Some(l) = limits.cost_micros.filter(|l| self.cost_micros >= *l) {
            return Some(hit(BudgetKind::Cost, l, self.cost_micros));
        }
        None
    }
}
