// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Batches: which calls of a step run together.

use harness_core::*;

/// Group a step's calls, in order, into batches that run one after another.
/// Consecutive parallel calls share a batch; a serial call runs alone.
pub fn plan_batches(calls: &[(CallId, Concurrency)]) -> Vec<Vec<CallId>> {
    let mut out: Vec<Vec<CallId>> = Vec::new();
    let mut open_parallel = false;
    for (call, class) in calls {
        let parallel = *class != Concurrency::Serial;
        match out.last_mut() {
            Some(batch) if parallel && open_parallel => batch.push(call.clone()),
            _ => out.push(vec![call.clone()]),
        }
        open_parallel = parallel;
    }
    out
}
