//! `Subscription::next_update`: durable events and live deltas from one
//! call, each delta before the `assistant` event that replaces it.

use harness_core::*;
use harness_host::*;

#[test]
fn deltas_and_events_arrive_in_order_from_one_call() {
    let m = MemoryHost::new();
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = m
        .block_on(
            m.host
                .create_session(&a, &PresetId::from("test"), CreateOptions::default()),
        )
        .unwrap();
    let mut sub = m.block_on(m.host.subscribe(&a, &s, 0)).unwrap();
    m.model.text("Hello there.");
    m.model.text("Greeting");
    let input = Input {
        content: vec![Block::text("Hi")],
        client_ref: None,
    };
    m.block_on(m.host.send(&a, &s, input)).unwrap();

    let total = m.log(&s).len() as u64;
    let mut seqs = Vec::new();
    let mut streamed: Vec<(StepId, String)> = Vec::new();
    let mut replies: Vec<(StepId, String)> = Vec::new();
    while sub.last_seq() < total {
        match m.block_on(sub.next_update()).unwrap() {
            Update::Delta(d) => {
                // Every event before the delta is already delivered, and
                // the reply it belongs to is not.
                assert!(d.after_seq <= sub.last_seq());
                assert!(replies.iter().all(|(step, _)| step != &d.step));
                match streamed.last_mut() {
                    Some((step, text)) if step == &d.step => text.push_str(&d.text),
                    _ => streamed.push((d.step, d.text)),
                }
            }
            Update::Event(e) => {
                seqs.push(e.seq);
                if let Event::Assistant(a) = &e.event {
                    let text: String = a
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            Block::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect();
                    replies.push((e.step.clone().unwrap(), text));
                }
            }
        }
    }
    assert_eq!(seqs, (1..=total).collect::<Vec<_>>());
    assert_eq!(replies.len(), 2);
    // The deltas of each step add up to its reply.
    assert_eq!(streamed, replies);
    // Nothing is left over for the lossy drain.
    assert!(sub.deltas().is_empty());
}
