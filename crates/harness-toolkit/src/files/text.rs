// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small text helpers for the files tools' results: clipping, dates, and
//! a buffer that never passes the result cap.

/// At most `max` bytes of `s`, cut at a character boundary, marked when cut.
pub(crate) fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut at = max.saturating_sub(3);
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    format!("{}...", &s[..at])
}

/// A UTC date and minute, like `2026-11-28 06:04`, from epoch milliseconds.
pub fn date(ms: i64) -> String {
    let (days, rem) = (
        ms.div_euclid(86_400_000),
        ms.rem_euclid(86_400_000) / 60_000,
    );
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 60, rem % 60)
}

/// Lines into a result that never passes its cap. `room` is held back for
/// the closing line.
pub(crate) struct Out {
    pub(crate) buf: String,
    cap: usize,
}

impl Out {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            buf: String::new(),
            cap,
        }
    }

    /// Add `line` if it fits with `room` bytes still free.
    pub(crate) fn add(&mut self, line: &str, room: usize) -> bool {
        if self.buf.len() + line.len() + room > self.cap {
            return false;
        }
        self.buf.push_str(line);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_clips() {
        assert_eq!(date(0), "1970-01-01 00:00");
        assert_eq!(date(1_794_646_800_000), "2026-11-14 09:00");
        assert_eq!(date(951_782_399_000), "2000-02-28 23:59");
        assert_eq!(clip("héllo wörld", 8), "héll...");
        assert_eq!(clip("short", 8), "short");
        let mut out = Out::new(10);
        assert!(out.add("12345", 5));
        assert!(!out.add("6", 5));
        assert_eq!(out.buf, "12345");
    }
}
