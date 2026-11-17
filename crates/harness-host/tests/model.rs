// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! A session created with a picked model runs on it: recorded in
//! `session.start`, used by every request, kept across a restart and a
//! fork, and shown in the summary.

use harness_core::*;
use harness_host::*;
use harness_log::SessionStore;

const PICKED: &str = "local:picked";
const PRESET_MODEL: &str = "test:scripted";

fn host() -> (MemoryHost, Caller) {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    (m, a)
}

fn create(m: &MemoryHost, c: &Caller, model: Option<&str>) -> SessionId {
    let opts = CreateOptions {
        model: model.map(str::to_owned),
        ..CreateOptions::default()
    };
    m.block_on(m.host.create_session(c, &PresetId::from("test"), opts))
        .unwrap()
}

fn turn(m: &MemoryHost, c: &Caller, s: &SessionId, ask: &str, reply: &str) {
    m.model.text(reply);
    let input = Input {
        content: vec![Block::text(ask)],
        client_ref: None,
    };
    m.block_on(m.host.send(c, s, input)).unwrap();
}

fn start(m: &MemoryHost, s: &SessionId) -> SessionStart {
    m.log(s)
        .iter()
        .find_map(|e| match &e.event {
            Event::SessionStart(st) if &e.session == s => Some(st.clone()),
            _ => None,
        })
        .unwrap()
}

/// The model of every step this session started, in order.
fn step_models(m: &MemoryHost, s: &SessionId) -> Vec<String> {
    m.log(s)
        .iter()
        .filter(|e| &e.session == s)
        .filter_map(|e| match &e.event {
            Event::StepStart(st) => Some(st.model.clone()),
            _ => None,
        })
        .collect()
}

fn requested(m: &MemoryHost) -> Vec<String> {
    let reqs = m.model.requests.lock().unwrap();
    reqs.iter().map(|r| r.model.clone()).collect()
}

fn summary(m: &MemoryHost, c: &Caller, s: &SessionId) -> SessionSummary {
    m.block_on(m.host.open_session(c, s)).unwrap().summary
}

#[test]
fn a_picked_model_replaces_the_presets_for_the_session() {
    let (m, a) = host();
    let s = create(&m, &a, Some(PICKED));
    assert_eq!(start(&m, &s).model.as_deref(), Some(PICKED));
    turn(&m, &a, &s, "Hi", "Hello.");
    m.model.text("Greeting");
    m.host.tick();
    // The reply and the title step both ran on the picked model.
    let steps = step_models(&m, &s);
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|x| x == PICKED), "{steps:?}");
    assert!(requested(&m).iter().all(|x| x == PICKED));
    // It is visible where the session's settings are read.
    assert_eq!(summary(&m, &a, &s).model.as_deref(), Some(PICKED));
    let listed = m
        .block_on(m.host.list_sessions(&a, SessionFilter::default()))
        .unwrap();
    assert_eq!(listed.items[0].model.as_deref(), Some(PICKED));
}

#[test]
fn without_a_pick_nothing_changes() {
    let (m, a) = host();
    for model in [None, Some(""), Some("  ")] {
        let s = create(&m, &a, model);
        let st = start(&m, &s);
        assert_eq!(st.model, None);
        turn(&m, &a, &s, "Hi", "Hello.");
        assert!(step_models(&m, &s).iter().all(|x| x == PRESET_MODEL));
        assert_eq!(summary(&m, &a, &s).model, None);
    }
}

#[test]
fn a_fork_keeps_its_parents_model() {
    let (m, a) = host();
    let s = create(&m, &a, Some(PICKED));
    turn(&m, &a, &s, "A color?", "Blue.");
    let turn_id = m.log(&s).iter().find_map(|e| e.turn.clone()).unwrap();
    let at = ForkPoint::AfterTurn(turn_id);
    let child = m
        .block_on(m.host.fork_session(&a, &s, at, ForkReason::Branch))
        .unwrap();
    assert_eq!(start(&m, &child).model.as_deref(), Some(PICKED));
    assert_eq!(summary(&m, &a, &child).model.as_deref(), Some(PICKED));
    turn(&m, &a, &child, "Another?", "Red.");
    let steps = step_models(&m, &child);
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|x| x == PICKED), "{steps:?}");
}

/// A second host over a copy of the first one's store. `keep_model` false
/// strips the model from every registry row, as a store that does not hold
/// it yet would, so only the log carries it.
fn restart(m: &MemoryHost, cfg: HostConfig, keep_model: bool) -> (MemoryHost, Caller) {
    let m2 = MemoryHost::with(cfg);
    for id in m.store.sessions() {
        let events = m.store.events(&id);
        m2.block_on(SessionStore::append(&*m2.store, &id, &events))
            .unwrap();
    }
    for mut meta in m
        .block_on(SessionStore::list_meta(&*m.store, None))
        .unwrap()
    {
        if !keep_model {
            meta.model = None;
        }
        m2.block_on(SessionStore::put_meta(&*m2.store, &meta))
            .unwrap();
    }
    let a2 = m2.caller(&m2.app_token("chat", AppKind::FirstParty));
    (m2, a2)
}

fn row_model(m: &MemoryHost, s: &SessionId) -> Option<String> {
    m.block_on(SessionStore::list_meta(&*m.store, None))
        .unwrap()
        .into_iter()
        .find(|meta| &meta.id == s)
        .unwrap()
        .model
}

#[test]
fn a_restarted_host_resumes_on_the_logged_model() {
    let (m, a) = host();
    let s = create(&m, &a, Some(PICKED));
    turn(&m, &a, &s, "Hi", "Hello.");
    let before = step_models(&m, &s).len();

    let (m2, a2) = restart(&m, memory_config(), true);
    assert_eq!(summary(&m2, &a2, &s).model.as_deref(), Some(PICKED));
    turn(&m2, &a2, &s, "Still there?", "Yes.");
    let steps = step_models(&m2, &s);
    assert!(steps.len() > before);
    assert!(steps.iter().all(|x| x == PICKED), "{steps:?}");
    assert!(requested(&m2).iter().all(|x| x == PICKED));
}

#[test]
fn the_log_is_the_source_when_the_registry_row_disagrees() {
    let (m, a) = host();
    let picked = create(&m, &a, Some(PICKED));
    let plain = create(&m, &a, None);
    turn(&m, &a, &picked, "Hi", "Hello.");

    // Rows without the model: the log alone carries it.
    let (m2, a2) = restart(&m, memory_config(), false);
    assert_eq!(row_model(&m2, &picked), None);
    assert_eq!(summary(&m2, &a2, &picked).model.as_deref(), Some(PICKED));
    // The row is put right once the session loads.
    assert_eq!(row_model(&m2, &picked).as_deref(), Some(PICKED));
    turn(&m2, &a2, &picked, "Still there?", "Yes.");
    assert!(step_models(&m2, &picked).iter().all(|x| x == PICKED));

    // A row that names a model its log never did follows the log too.
    let mut wrong = m
        .block_on(SessionStore::list_meta(&*m.store, None))
        .unwrap()
        .into_iter()
        .find(|meta| meta.id == plain)
        .unwrap();
    wrong.model = Some("local:other".into());
    m2.block_on(SessionStore::put_meta(&*m2.store, &wrong))
        .unwrap();
    assert_eq!(summary(&m2, &a2, &plain).model, None);
    assert_eq!(row_model(&m2, &plain), None);
}

#[test]
fn a_fork_after_a_restart_gets_a_new_id_and_keeps_the_model() {
    let (m, a) = host();
    let s = create(&m, &a, Some(PICKED));
    turn(&m, &a, &s, "A color?", "Blue.");
    let events = m.log(&s).len();
    let turn_id = m.log(&s).iter().find_map(|e| e.turn.clone()).unwrap();

    // The new host's clock and counter start where the first one's did, so
    // the first id it would mint is the one `s` already has.
    let (m2, a2) = restart(&m, memory_config(), false);
    let at = ForkPoint::AfterTurn(turn_id);
    let child = m2
        .block_on(m2.host.fork_session(&a2, &s, at, ForkReason::Branch))
        .unwrap();
    assert_ne!(child, s);
    let fresh = create(&m2, &a2, None);
    assert!(fresh != s && fresh != child);
    // The parent is untouched and still its own session.
    assert_eq!(m2.log(&s).len(), events);
    assert_eq!(summary(&m2, &a2, &s).forked_from, None);
    assert_eq!(summary(&m2, &a2, &child).forked_from.unwrap().0, s);

    assert_eq!(start(&m2, &child).model.as_deref(), Some(PICKED));
    assert_eq!(summary(&m2, &a2, &child).model.as_deref(), Some(PICKED));
    turn(&m2, &a2, &child, "Another?", "Red.");
    let steps = step_models(&m2, &child);
    assert!(!steps.is_empty());
    assert!(steps.iter().all(|x| x == PICKED), "{steps:?}");
}

#[test]
fn a_model_name_is_trimmed_and_bounded() {
    let (m, a) = host();
    let s = create(&m, &a, Some("  local:picked \n"));
    assert_eq!(start(&m, &s).model.as_deref(), Some(PICKED));
    assert_eq!(summary(&m, &a, &s).model.as_deref(), Some(PICKED));

    let sessions = m.store.sessions().len();
    let long = "m".repeat(5000);
    for bad in [long.as_str(), "local:\npicked"] {
        let opts = CreateOptions {
            model: Some(bad.to_owned()),
            ..CreateOptions::default()
        };
        let got = m.block_on(m.host.create_session(&a, &PresetId::from("test"), opts));
        assert!(
            matches!(&got, Err(HostError::Model(e)) if e.code == "model.name"),
            "{got:?}"
        );
    }
    // Nothing was created for a refused name.
    assert_eq!(m.store.sessions().len(), sessions);
    // The longest name allowed is recorded whole.
    let longest = "m".repeat(200);
    let s = create(&m, &a, Some(&longest));
    assert_eq!(start(&m, &s).model, Some(longest));
}

#[test]
fn on_the_low_profile_the_presets_title_model_still_titles() {
    let mut cfg = memory_config();
    cfg.profile = Some(Profile::Low);
    let m = MemoryHost::with(cfg);
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let opts = CreateOptions {
        model: Some(PICKED.into()),
        ..CreateOptions::default()
    };
    let s = m
        .block_on(m.host.create_session(&a, &PresetId::from("chat"), opts))
        .unwrap();
    m.model.text("Hello.");
    m.model.text("Greeting");
    let input = Input {
        content: vec![Block::text("Hi")],
        client_ref: None,
    };
    m.block_on(m.host.send(&a, &s, input)).unwrap();
    let steps: Vec<(Purpose, String)> = m
        .log(&s)
        .iter()
        .filter_map(|e| match &e.event {
            Event::StepStart(st) => Some((st.purpose.clone(), st.model.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        steps,
        [
            (Purpose::Reply, PICKED.to_owned()),
            (Purpose::Title, "local:smallest".to_owned())
        ]
    );
}
