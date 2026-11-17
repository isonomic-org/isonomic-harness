// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The fixtures against the schema, the committed files and the manifest.

use harness_core::*;
use harness_testkit::fixtures::{self, Fixture};
use harness_testkit::schema_check;
use std::collections::{BTreeMap, BTreeSet};

const COMMITTED: &[(&str, &str)] = &[
    (
        "chat-basic",
        include_str!("../../../fixtures/chat-basic.jsonl"),
    ),
    (
        "tool-heavy",
        include_str!("../../../fixtures/tool-heavy.jsonl"),
    ),
    ("approval", include_str!("../../../fixtures/approval.jsonl")),
    (
        "autonomous-run",
        include_str!("../../../fixtures/autonomous-run.jsonl"),
    ),
    (
        "client-gone",
        include_str!("../../../fixtures/client-gone.jsonl"),
    ),
];

const MANIFEST: &str = include_str!("../../../fixtures/MANIFEST.json");

fn all() -> Vec<Fixture> {
    fixtures::all()
}

#[test]
fn committed_files_match_the_generator() {
    for (name, text) in COMMITTED {
        let fixture = fixtures::by_name(name).expect("known fixture");
        assert!(fixture.committed);
        assert!(
            *text == fixture.jsonl(),
            "{name}: run `cargo xtask fixtures`"
        );
        assert!(text.len() < 20 * 1024, "{name} is {} bytes", text.len());
    }
}

#[test]
fn the_manifest_matches_the_generator() {
    assert_eq!(
        MANIFEST,
        fixtures::manifest(&all()),
        "run `cargo xtask fixtures`"
    );
}

#[test]
fn every_line_validates_and_round_trips_byte_for_byte() {
    let schema: serde_json::Value = serde_json::from_str(JSON_SCHEMA).unwrap();
    for fixture in all() {
        let text = fixture.jsonl();
        let mut lines = text.lines();
        let header: LogHeader = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header, LogHeader::current());
        for (n, line) in lines.enumerate() {
            let json: serde_json::Value = serde_json::from_str(line).unwrap();
            if let Err(errors) = schema_check::check(&schema, &json) {
                panic!("{} line {}: {errors:?}", fixture.name, n + 2);
            }
            let event: SessionEvent = serde_json::from_str(line).unwrap();
            assert!(
                !matches!(event.event, Event::Unknown(_)),
                "{}: {line}",
                fixture.name
            );
            assert_eq!(serde_json::to_string(&event).unwrap(), line);
        }
    }
}

#[test]
fn the_fixtures_cover_every_event_type() {
    let seen: BTreeSet<String> = all()
        .iter()
        .flat_map(|f| f.events.iter().map(|e| e.event.type_name().to_owned()))
        .collect();
    for name in EVENT_TYPES {
        assert!(seen.contains(*name), "no fixture has {name}");
    }
}

#[test]
fn sessions_are_well_formed() {
    for fixture in all() {
        let mut last: BTreeMap<&SessionId, Seq> = BTreeMap::new();
        let mut calls: BTreeMap<&CallId, usize> = BTreeMap::new();
        for e in &fixture.events {
            let prev = last.insert(&e.session, e.seq).unwrap_or(0);
            assert_eq!(e.seq, prev + 1, "{} {}: seq gap", fixture.name, e.session);
            match (&e.event, e.seq) {
                (Event::SessionStart(_), 1) => {}
                (_, 1) => panic!("{}: seq 1 is not session.start", fixture.name),
                (Event::SessionStart(_), _) => panic!("{}: late session.start", fixture.name),
                (Event::Forked(_), n) => assert_eq!(n, 2, "{}: forked not at seq 2", fixture.name),
                (Event::ToolCall(c), _) => {
                    calls.insert(&c.call, 0);
                }
                (Event::ToolResult(r), _) => {
                    *calls.get_mut(&r.call).expect("result for a known call") += 1;
                }
                _ => {}
            }
        }
        for (call, results) in calls {
            assert_eq!(results, 1, "{}: {call} has {results} results", fixture.name);
        }
    }
}

#[test]
fn long_thread_has_ten_thousand_events() {
    let fixture = fixtures::long_thread();
    assert_eq!(fixture.events.len(), 10_000);
    assert!(!fixture.committed);
}

#[test]
fn streams_rebuild_each_reply() {
    for fixture in all() {
        for e in &fixture.events {
            let Event::Assistant(a) = &e.event else {
                continue;
            };
            let mut sizes = vec![0u64; a.content.len()];
            let mut at = 0;
            for chunk in &a.stream {
                assert!(chunk.2 >= at, "{}: stream goes back in time", fixture.name);
                at = chunk.2;
                sizes[chunk.0 as usize] += u64::from(chunk.1);
            }
            for (block, size) in a.content.iter().zip(sizes) {
                if let Block::Text { text } | Block::Reasoning { text, .. } = block {
                    assert_eq!(size, text.len() as u64, "{} seq {}", fixture.name, e.seq);
                }
            }
        }
    }
}

#[test]
fn a_session_start_with_a_picked_model_validates() {
    let schema: serde_json::Value = serde_json::from_str(JSON_SCHEMA).unwrap();
    let line = |model: &str| -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{"session":"ses_1","seq":1,"at":1,"type":"session.start","class":"interactive","app":"chat","principal":{{"kind":"user"}},"preset":{{"id":"chat","version":2}}{model}}}"#
        ))
        .unwrap()
    };
    assert!(schema_check::check(&schema, &line("")).is_ok());
    assert!(schema_check::check(&schema, &line(r#","model":"local:picked""#)).is_ok());
    assert!(schema_check::check(&schema, &line(r#","model":7"#)).is_err());
}
