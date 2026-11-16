//! Compaction follows the model's real context window, whether the session
//! runs on its preset's model or on a picked one. The chat preset's budget
//! is 24,000 tokens; on a model that reports 8,192 the host must compact
//! well before that.

use harness_core::*;
use harness_host::*;

/// Eight turns of about 1,300 tokens each on the chat preset: more than an
/// 8,192-token window holds, far less than the preset's 24,000, and few
/// enough per turn that the four turns the preset keeps whole still fit. Returns
/// how many times the session compacted, and the largest request sent, in
/// estimated tokens.
fn run(context: u64, model: Option<&str>) -> (usize, u64) {
    let m = MemoryHost::new();
    m.model.set_context(context);
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let opts = CreateOptions {
        model: model.map(str::to_owned),
        ..CreateOptions::default()
    };
    let s = m
        .block_on(m.host.create_session(&a, &PresetId::from("chat"), opts))
        .unwrap();
    let long = "A sentence about gardens and the weather in spring. ".repeat(100);
    for i in 0..8 {
        let input = Input {
            content: vec![Block::text(format!("Question {i}. {long}"))],
            client_ref: None,
        };
        m.block_on(m.host.send(&a, &s, input)).unwrap();
    }
    let compacted = m
        .log(&s)
        .iter()
        .filter(|e| matches!(e.event, Event::Compacted(_)))
        .count();
    let reqs = m.model.requests.lock().unwrap();
    let largest = reqs.iter().map(|r| r.estimate_tokens()).max().unwrap();
    (compacted, largest)
}

#[test]
fn a_roomy_model_leaves_the_presets_budget_alone() {
    let (compacted, largest) = run(32_768, None);
    assert_eq!(compacted, 0);
    // The turns really do outgrow an 8,192-token window.
    assert!(largest > 8192, "{largest}");
    assert!(largest < 24_000, "{largest}");
}

#[test]
fn an_8192_token_model_is_compacted_for_with_or_without_a_pick() {
    for model in [None, Some("local:picked")] {
        let (compacted, largest) = run(8192, model);
        assert!(compacted > 0, "{model:?}");
        // Every request fit the window. (A summary request carries the
        // history that went over the budget, so it may pass the budget; the
        // room kept for the reply covers it.)
        assert!(largest <= 8192, "{model:?}: {largest}");
    }
}
