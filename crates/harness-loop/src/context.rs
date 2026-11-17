// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The context link: the date, the language and, only when the person
//! opted in, a location, added to each turn that has user input and
//! recorded as `system(context)`.

use harness_core::*;
use harness_preset::{ContextSettings, LocationMode};

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

/// `2026-11-15` as `Sunday, 15 November 2026`; `None` if it is not a date.
pub fn long_date(date: &str) -> Option<String> {
    let mut it = date.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days since 1970-01-01 (a Thursday), proleptic Gregorian.
    let (yy, mm) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm as i64 + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let wd = DAYS[days.rem_euclid(7) as usize];
    Some(format!("{wd}, {d} {} {y}", MONTHS[m as usize - 1]))
}

/// What the link records for a turn, or `None` when it adds nothing.
pub fn context_text(
    settings: &ContextSettings,
    given: &RequestContext,
) -> Option<(String, RequestContext)> {
    if !settings.link {
        return None;
    }
    let mut parts = Vec::new();
    let mut kept = given.clone();
    if settings.date {
        let long = long_date(&given.date).unwrap_or_else(|| given.date.clone());
        parts.push(format!("Current date: {long} ({}).", given.time_zone));
    } else {
        kept.date = String::new();
        kept.time_zone = String::new();
    }
    if settings.language {
        parts.push(format!("Language: {}.", given.language));
    } else {
        kept.language = String::new();
    }
    match (&given.location, settings.location) {
        (Some(place), LocationMode::OptIn) => parts.push(format!("Location: {place}.")),
        _ => kept.location = None,
    }
    if parts.is_empty() {
        return None;
    }
    Some((parts.join(" "), kept))
}
