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

#[test]
fn a_restarted_host_resumes_on_the_logged_model() {
    let (m, a) = host();
    let s = create(&m, &a, Some(PICKED));
    turn(&m, &a, &s, "Hi", "Hello.");
    let before = step_models(&m, &s).len();

    // A second host over a copy of the store: nothing but the log and the
    // registry row carries over.
    let m2 = MemoryHost::new();
    for id in m.store.sessions() {
        let events = m.store.events(&id);
        m2.block_on(SessionStore::append(&*m2.store, &id, &events))
            .unwrap();
    }
    for meta in m
        .block_on(SessionStore::list_meta(&*m.store, None))
        .unwrap()
    {
        m2.block_on(SessionStore::put_meta(&*m2.store, &meta))
            .unwrap();
    }
    let a2 = m2.caller(&m2.app_token("chat", AppKind::FirstParty));
    assert_eq!(summary(&m2, &a2, &s).model.as_deref(), Some(PICKED));
    turn(&m2, &a2, &s, "Still there?", "Yes.");
    let steps = step_models(&m2, &s);
    assert!(steps.len() > before);
    assert!(steps.iter().all(|x| x == PICKED), "{steps:?}");
    assert!(requested(&m2).iter().all(|x| x == PICKED));
}
