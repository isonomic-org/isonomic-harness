//! The chat preset's tools and approvals section is in the system prompt
//! only while the session offers tools.

mod common;
use common::*;
use harness_core::*;
use harness_loop::*;

const TOOLS: &str = "Use a tool only when it helps.";

fn preset_texts(d: &Driver) -> Vec<String> {
    d.own()
        .iter()
        .filter_map(|e| match &e.event {
            Event::System(s) if s.origin == SystemOrigin::Preset => Some(s.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_chat_session_with_no_tools_gets_the_persona_alone() {
    let mut d = Driver::with(Config::new(preset("chat"), None, vec![]));
    d.user("What is the capital of France?");
    let system = d.last_request().system.clone();
    assert!(system.starts_with("You are Isonomic"));
    for word in ["tool", "approv", "unclear"] {
        assert!(!system.to_lowercase().contains(word), "{word} in {system}");
    }
    assert!(d.last_request().tools.is_empty());
    d.reply("Paris.");
    d.reply("Capital of France");
    assert_eq!(preset_texts(&d).len(), 1);
    d.check();
}

#[test]
fn the_tools_section_follows_the_tools_offered() {
    let now = spec("now", ToolKind::Engine, &[], Concurrency::Parallel);
    let mut d = Driver::with(Config::new(preset("chat"), None, vec![now.clone()]));
    assert!(preset_texts(&d)[0].contains(TOOLS));
    d.user("Hello.");
    assert!(d.last_request().system.contains(TOOLS));
    d.reply("Hello!");
    d.reply("Greeting");

    // The tools go: the section goes with them, from the next request on.
    d.send(Input::Tools(vec![]));
    d.user("And again.");
    assert!(!d.last_request().system.contains(TOOLS));
    d.reply("Hello again!");

    // The same set again changes nothing; tools coming back restore it.
    d.send(Input::Tools(vec![]));
    assert_eq!(preset_texts(&d).len(), 2);
    d.send(Input::Tools(vec![now]));
    let texts = preset_texts(&d);
    assert_eq!(texts.len(), 3);
    assert_eq!(texts[2], texts[0]);
    d.user("Once more.");
    assert!(d.last_request().system.contains(TOOLS));
    d.reply("Hello once more!");
    // Every request is rebuilt from the log alone.
    d.check();
}
