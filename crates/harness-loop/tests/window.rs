//! The model's window is hard: no request goes out larger than it. Older
//! turns give way, the current turn and the system prompt never do, and a
//! message that cannot fit fails its turn instead of being sent.

mod common;
use common::*;
use harness_compact::NOT_SHOWN;
use harness_core::*;
use harness_log::{Part, Role};
use harness_loop::*;

/// The chat preset on a model with an 8,192-token context.
fn chat(profile: Profile) -> Driver {
    let cfg = Config::new(preset("chat"), None, vec![]).with_context(8192);
    Driver::with_profile(cfg, profile)
}

/// The window: the context less the reply's 1,024 tokens.
const WINDOW: u64 = 8192 - 1024;

/// Text of about `tokens` tokens.
fn text(tokens: usize) -> String {
    "word ".repeat(tokens * 4 / 5)
}

fn last_step(d: &Driver) -> Option<(Purpose, bool)> {
    let mut open = None;
    for e in d.s.events() {
        match &e.event {
            Event::StepStart(s) => open = Some((s.purpose.clone(), false)),
            Event::StepEnd(_) => open = open.map(|(p, _)| (p, true)),
            _ => {}
        }
    }
    open
}

/// One turn: answer whatever steps it runs (summary, reply, title).
fn turn(d: &mut Driver, ask: &str) {
    d.user(ask);
    while let Some((purpose, false)) = last_step(d) {
        match purpose {
            Purpose::Summary => d.reply("They talked about several things."),
            Purpose::Title => d.reply("A long talk"),
            _ => d.reply("Noted."),
        };
    }
}

fn largest(d: &Driver) -> u64 {
    let sizes = d.requests.iter().map(|(_, r)| r.estimate_tokens());
    sizes.max().unwrap_or(0)
}

fn count(d: &Driver, purpose: Purpose) -> usize {
    d.own()
        .iter()
        .filter(|e| matches!(&e.event, Event::StepStart(s) if s.purpose == purpose))
        .count()
}

fn notes(d: &Driver) -> Vec<Compacted> {
    d.own()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Compacted(c) if c.summary.as_ref().is_some_and(|s| s.text == NOT_SHOWN) => {
                Some(c.clone())
            }
            _ => None,
        })
        .collect()
}

fn texts_of(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Text { text } | Part::Summary { text } | Part::Context { text } => text.as_str(),
            _ => "",
        })
        .collect::<Vec<_>>()
        .join("|")
}

#[test]
fn on_intel_old_turns_give_way_to_a_note_with_no_model_step() {
    // Intel: no summary steps. Ten turns of about 1,300 tokens pass the
    // window more than once.
    let mut d = chat(Profile::Intel);
    for i in 0..10 {
        turn(&mut d, &format!("Question {i}. {}", text(1300)));
        assert!(largest(&d) <= WINDOW, "turn {i}: {}", largest(&d));
    }
    assert_eq!(count(&d, Purpose::Summary), 0);
    assert_eq!(d.requests.len(), 10, "one request a turn, nothing extra");
    assert!(!notes(&d).is_empty());
    // Every turn ended done.
    let stops: Vec<TurnStop> = d
        .own()
        .iter()
        .filter_map(|e| match &e.event {
            Event::TurnEnd(t) => Some(t.stop.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(stops, vec![TurnStop::Done; 10]);

    // The last request: the system prompt, the note in place of the oldest
    // turns, the newest turns whole, and the current question.
    let req = d.last_request();
    assert!(req.system.starts_with("You are Isonomic"));
    let first = texts_of(&req.messages[0].content);
    assert_eq!(first, NOT_SHOWN);
    let all: String = req.messages.iter().map(|m| texts_of(&m.content)).collect();
    assert!(!all.contains("Question 0."));
    assert!(all.contains("Question 8."));
    let last = req.messages.last().unwrap();
    assert_eq!(last.role, Role::User);
    assert!(texts_of(&last.content).contains("Question 9."));
    // The log still holds every turn, and replay rebuilds every request.
    assert_eq!(count(&d, Purpose::Reply), 10);
    d.check();
}

#[test]
fn four_kept_turns_larger_than_the_window_give_way() {
    // Summaries allowed, and the preset keeps four turns whole. At about
    // 2,000 tokens a turn, four of them and the current one come to more
    // than 10,000 tokens: over the window.
    let mut d = chat(Profile::Standard);
    for i in 0..7 {
        turn(&mut d, &format!("Question {i}. {}", text(2000)));
    }
    // No request went out over the window: not a reply, not a summary.
    assert!(largest(&d) <= WINDOW, "{}", largest(&d));
    let ends = d
        .own()
        .iter()
        .filter(|e| matches!(&e.event, Event::TurnEnd(t) if t.stop == TurnStop::Done))
        .count();
    assert_eq!(ends, 7);
    let req = d.last_request();
    assert!(texts_of(&req.messages.last().unwrap().content).contains("Question 6."));
    d.check();
}

#[test]
fn kept_turns_are_given_up_one_at_a_time_to_a_summary() {
    // About 1,500 tokens a turn: four kept turns and the current one pass
    // the window, three and a summary fit.
    let mut d = chat(Profile::Standard);
    for i in 0..5 {
        turn(&mut d, &format!("Question {i}. {}", text(1500)));
    }
    assert!(largest(&d) <= WINDOW, "{}", largest(&d));
    assert_eq!(count(&d, Purpose::Summary), 1);
    assert!(notes(&d).is_empty());
    let summary = d
        .last_of(|e| match e {
            Event::Compacted(c) => c.summary.clone(),
            _ => None,
        })
        .unwrap();
    // One turn gave way; the three after it stayed whole.
    assert_eq!(summary.turns.len(), 1);
    let req = d.last_request();
    let all: String = req.messages.iter().map(|m| texts_of(&m.content)).collect();
    assert!(all.contains("They talked about several things."));
    assert!(!all.contains("Question 0."));
    for kept in ["Question 1.", "Question 2.", "Question 3.", "Question 4."] {
        assert!(all.contains(kept), "{kept}");
    }
    d.check();
}

#[test]
fn a_message_that_cannot_fit_fails_its_turn_and_is_not_sent() {
    for profile in [Profile::Intel, Profile::Standard] {
        let mut d = chat(profile);
        turn(&mut d, "Hello.");
        let sent = d.requests.len();

        // One message larger than the window on its own.
        d.user(&format!("Read this. {}", text(9000)));
        assert_eq!(d.requests.len(), sent, "nothing was sent");
        let end = d
            .last_of(|e| match e {
                Event::TurnEnd(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(end.stop, TurnStop::Failed);
        assert_eq!(end.steps, 0);
        let err = end.error.unwrap();
        assert_eq!(err.code, "context.too_large");
        let reason = end.reason.unwrap();
        assert!(reason.starts_with("This message is too long for the model"));
        assert!(reason.contains(&WINDOW.to_string()), "{reason}");

        // The session goes on: the oversized turn gives way like any other.
        turn(&mut d, "Never mind. What is two plus two?");
        assert!(largest(&d) <= WINDOW);
        let req = d.last_request();
        let all: String = req.messages.iter().map(|m| texts_of(&m.content)).collect();
        assert!(!all.contains("Read this."));
        assert!(all.contains("two plus two"));
        d.check();
    }
}

#[test]
fn without_a_known_window_the_budget_stays_soft() {
    // No context size: nothing is left out and nothing fails, as before.
    let cfg = Config::new(preset("chat"), None, vec![]);
    let mut d = Driver::with_profile(cfg, Profile::Intel);
    for i in 0..6 {
        turn(&mut d, &format!("Question {i}. {}", text(5000)));
    }
    assert!(notes(&d).is_empty());
    assert!(largest(&d) > 24_000);
    assert_eq!(d.requests.len(), 6);
    d.check();
}
