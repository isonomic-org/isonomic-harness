// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! `now`: the current date and time in the person's time zone. The engine
//! knows the zone and its offset at a moment; the tool only formats.

use crate::{engine, schema};
use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

/// The person's time zone for a session, as the engine knows it.
pub trait LocalZone: Send + Sync {
    /// The zone's name and its offset from UTC in minutes at `at`.
    fn zone(&self, session: &SessionId, at: Millis) -> (String, i32);
}

/// A fixed zone, for tests and fixtures.
#[derive(Clone, Debug)]
pub struct FixedZone {
    /// The zone's name, such as `America/New_York`.
    pub name: String,
    /// Its offset from UTC in minutes.
    pub offset_minutes: i32,
}

impl LocalZone for FixedZone {
    fn zone(&self, _session: &SessionId, _at: Millis) -> (String, i32) {
        (self.name.clone(), self.offset_minutes)
    }
}

/// `now`.
pub struct Now {
    zone: Arc<dyn LocalZone>,
}

impl Now {
    /// The tool, reading zones from `zone`.
    pub fn new(zone: Arc<dyn LocalZone>) -> Self {
        Self { zone }
    }

    /// The text for `at` in a zone: `Sunday, 15 November 2026, 09:30
    /// (America/New_York, UTC-05:00)`.
    pub fn format(at: Millis, name: &str, offset_minutes: i32) -> String {
        let local = at as i64 / 1000 + i64::from(offset_minutes) * 60;
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        let (y, m, d) = civil(days);
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
        let sign = if offset_minutes < 0 { '-' } else { '+' };
        let off = offset_minutes.unsigned_abs();
        format!(
            "{}, {d} {} {y}, {:02}:{:02} ({name}, UTC{sign}{:02}:{:02})",
            DAYS[days.rem_euclid(7) as usize],
            MONTHS[m as usize - 1],
            secs / 3600,
            secs % 3600 / 60,
            off / 60,
            off % 60
        )
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

impl Tool for Now {
    fn spec(&self) -> ToolSpec {
        let mut s = engine(
            "now",
            "The current date and time in the person's time zone.",
            schema(&[], &[]),
        );
        s.concurrency = Concurrency::Parallel;
        s
    }

    fn call<'a>(&'a self, _input: &'a Value, cx: &'a ToolCtx) -> BoxFuture<'a, ToolOutput> {
        let (name, offset) = self.zone.zone(&cx.session, cx.now);
        ready(ToolOutput::Done(ToolDone::ok(Self::format(
            cx.now, &name, offset,
        ))))
    }
}
