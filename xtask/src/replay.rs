// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! `cargo xtask replay [fixture]`: rebuild every model request of every
//! fixture from its log alone, through the JSONL interchange format.

use harness_core::*;
use harness_log::history::{request_for, resolve_fork};
use harness_log::{decode, ModelRequest};
use std::collections::BTreeMap;
use std::time::Instant;

/// Every session of a log, each resolved (a fork carries its parent's prefix).
pub fn sessions(events: &[SessionEvent]) -> BTreeMap<SessionId, Vec<SessionEvent>> {
    let mut own: BTreeMap<SessionId, Vec<SessionEvent>> = BTreeMap::new();
    for e in events {
        own.entry(e.session.clone()).or_default().push(e.clone());
    }
    let mut resolved = BTreeMap::new();
    for (id, evs) in &own {
        resolved.insert(id.clone(), resolve(&own, evs));
    }
    resolved
}

fn resolve(
    own: &BTreeMap<SessionId, Vec<SessionEvent>>,
    evs: &[SessionEvent],
) -> Vec<SessionEvent> {
    let parent = evs.iter().find_map(|e| match &e.event {
        Event::Forked(f) => Some(f.from.clone()),
        _ => None,
    });
    match parent.and_then(|p| own.get(&p)) {
        Some(p) => resolve_fork(&resolve(own, p), evs),
        None => evs.to_vec(),
    }
}

/// Every request of a resolved session, by step.
pub fn requests(
    session: &SessionId,
    resolved: &[SessionEvent],
) -> Result<Vec<(StepId, ModelRequest)>, String> {
    resolved
        .iter()
        .filter(|e| &e.session == session && matches!(e.event, Event::StepStart(_)))
        .filter_map(|e| e.step.clone())
        .map(|s| {
            request_for(resolved, &s)
                .map(|r| (s.clone(), r))
                .map_err(|e| e.to_string())
        })
        .collect()
}

pub fn run(only: Option<&str>) -> Result<(), String> {
    let all = harness_testkit::fixtures::all();
    let mut failed = false;
    println!(
        "{:<16} {:>7} {:>9} {:>10}  digest of every request",
        "fixture", "events", "requests", "ms"
    );
    for f in all.iter().filter(|f| only.is_none_or(|o| o == f.name)) {
        let t0 = Instant::now();
        let generation = decode(&f.jsonl()).map_err(|e| format!("{}: {e:?}", f.name))?;
        let mut digests = Vec::new();
        for (id, resolved) in sessions(&generation.events) {
            match requests(&id, &resolved) {
                Ok(rs) => digests.extend(rs.into_iter().map(|(_, r)| r.digest())),
                Err(e) => {
                    eprintln!("{}: {id}: {e}", f.name);
                    failed = true;
                }
            }
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let all_digest = future::fnv1a64(digests.join("\n").as_bytes());
        println!(
            "{:<16} {:>7} {:>9} {:>10.1}  {all_digest}",
            f.name,
            generation.events.len(),
            digests.len(),
            ms
        );
    }
    if failed {
        Err("some requests could not be rebuilt".into())
    } else {
        Ok(())
    }
}
