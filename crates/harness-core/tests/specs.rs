// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! A snapshot of every tool spec harness-core defines, so a change to what
//! the model is offered shows up in review. `UPDATE_SNAPSHOTS=1 cargo test
//! -p harness-core --test specs` rewrites it.

use harness_core::display::DISPLAY_SPECS;
use harness_core::ToolSpec;

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots/specs.json");

#[test]
fn every_spec_matches_its_snapshot() {
    let specs: Vec<ToolSpec> = DISPLAY_SPECS.iter().map(|d| d.tool_spec()).collect();
    let mut now = serde_json::to_string_pretty(&specs).unwrap();
    now.push('\n');
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(PATH, &now).unwrap();
    }
    let saved = std::fs::read_to_string(PATH).unwrap_or_default();
    assert!(saved == now, "specs changed: rerun with UPDATE_SNAPSHOTS=1 and review the diff");
}
