// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display calls: no approval, no client, full validation against the
//! widget's schema, one retry per widget per turn, then the fallback
//! (`docs/display.md`). The turn continues in every case.

use crate::machine::Session;
use crate::*;
use harness_core::display;
use harness_tools::ToolDone;

/// What the model is told for a valid call.
pub const SHOWN: &str = "shown";
/// What the model is told when the call falls back to its text.
pub const SHOWN_AS_TEXT: &str = "shown as text";

impl Session {
    /// Record the result of a display call to `rec.tool`.
    pub(crate) fn display_call(&mut self, id: &CallId, rec: &ToolCall) {
        let checked = match rec.input_raw {
            Some(_) => Err(format!("{}: the input is not JSON.", rec.tool)),
            None => display::validate(&rec.tool, &rec.input).map_err(|e| e.reason),
        };
        let (done, outcome) = match checked {
            Ok(()) => (ToolDone::ok(SHOWN), DisplayOutcome::Shown),
            Err(reason) if !self.display_retried(&rec.tool) => (
                ToolDone::error("display.invalid", &reason),
                DisplayOutcome::Retry,
            ),
            Err(_) => (ToolDone::ok(SHOWN_AS_TEXT), DisplayOutcome::Fallback),
        };
        self.finish_as(id, done, false, Some(outcome));
    }

    /// Whether a call to `tool` already used its retry in this turn.
    fn display_retried(&self, tool: &str) -> bool {
        let mut retried = Vec::new();
        for e in self.events.iter().rev() {
            match &e.event {
                Event::TurnStart(_) => break,
                Event::ToolResult(r) if r.display == Some(DisplayOutcome::Retry) => {
                    retried.push(r.call.clone());
                }
                Event::ToolCall(c) if c.tool == tool && retried.contains(&c.call) => {
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}
