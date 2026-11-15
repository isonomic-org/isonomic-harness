//! Isonomic Harness testkit.
//!
//! Hermetic test support: a manual clock, a deterministic recorder for
//! session logs, the fake tools the fixtures use, the six recorded fixtures,
//! and a small JSON Schema checker for the event schema.
//!
//! Until harness-loop lands, the fixtures are scripted with the recorder in
//! exactly the order the loop will produce; once it lands, `cargo xtask
//! fixtures` records them from scripted runs and any difference is reviewed.

pub mod fixtures;
pub mod recorder;
pub mod schema_check;
pub mod tools;

pub use fixtures::Fixture;
pub use recorder::{Ctx, Recorder};

use harness_core::{Clock, Millis};
use std::sync::atomic::{AtomicU64, Ordering};

/// The instant every fixture starts at: 2026-11-14 09:00:00 UTC.
pub const FIXTURE_EPOCH: Millis = 1_794_646_800_000;

/// A clock tests move by hand.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    /// A clock reading `at`.
    pub fn new(at: Millis) -> Self {
        Self(AtomicU64::new(at))
    }

    /// Move the clock forward.
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }

    /// Set the clock.
    pub fn set(&self, at: Millis) {
        self.0.store(at, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
}

/// FNV-1a, 64-bit, as lowercase hex: the fixture manifest's digest.
pub fn digest(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}
