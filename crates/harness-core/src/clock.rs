//! The clock. The loop never reads system time; the host injects a clock,
//! and tests inject a manual one.

use crate::ids::Millis;

/// A source of the current time.
pub trait Clock: Send + Sync {
    /// Milliseconds since the Unix epoch, UTC.
    fn now_ms(&self) -> Millis;
}

/// A clock that always reads the same instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedClock(pub Millis);

impl Clock for FixedClock {
    fn now_ms(&self) -> Millis {
        self.0
    }
}
