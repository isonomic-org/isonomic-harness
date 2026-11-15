//! `Subscription::next_update`: durable events and live deltas from one
//! call, each delta before the `assistant` event that replaces it.

use harness_core::*;
use harness_host::*;
use std::sync::Arc;
use std::task::{Context, Wake, Waker};

struct Quiet;
impl Wake for Quiet {
    fn wake(self: Arc<Self>) {}
}

fn host_with_queue(queue: usize) -> (MemoryHost, Caller, SessionId) {
    let mut cfg = memory_config();
    cfg.subscriber_queue = queue;
    let m = MemoryHost::with(cfg);
    let a = m.caller(&m.app_token("chat", AppKind::FirstParty));
    let s = m
        .block_on(
            m.host
                .create_session(&a, &PresetId::from("test"), CreateOptions::default()),
        )
        .unwrap();
    (m, a, s)
}

fn turn(m: &MemoryHost, a: &Caller, s: &SessionId, reply: &str) {
    m.model.text(reply);
    m.model.text("Greeting");
    let input = Input {
        content: vec![Block::text("Hi")],
        client_ref: None,
    };
    m.block_on(m.host.send(a, s, input)).unwrap();
}

/// Every durable event from `sub` up to `total`, by seq.
fn drain(m: &MemoryHost, sub: &mut Subscription, total: u64) -> Vec<Seq> {
    let mut seqs = Vec::new();
    while sub.last_seq() < total {
        if let Update::Event(e) = m.block_on(sub.next_update()).unwrap() {
            seqs.push(e.seq);
        }
    }
    seqs
}

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

#[test]
fn a_lagging_subscriber_backfills_with_no_gaps_or_duplicates() {
    // A live queue of two: the turn overflows it while nobody reads.
    let (m, a, s) = host_with_queue(2);
    let mut sub = m.block_on(m.host.subscribe(&a, &s, 0)).unwrap();
    turn(&m, &a, &s, "Hello there.");
    let total = m.log(&s).len() as u64;
    assert!(total > 4);
    assert_eq!(drain(&m, &mut sub, total), (1..=total).collect::<Vec<_>>());

    // Caught up, it reads live again, and overflows again.
    turn(&m, &a, &s, "Hello again.");
    let more = m.log(&s).len() as u64;
    assert_eq!(
        drain(&m, &mut sub, more),
        (total + 1..=more).collect::<Vec<_>>()
    );
}

#[test]
fn a_subscriber_resuming_mid_log_backfills_from_its_seq() {
    let (m, a, s) = host_with_queue(256);
    turn(&m, &a, &s, "Hello there.");
    let total = m.log(&s).len() as u64;
    let mut sub = m.block_on(m.host.subscribe(&a, &s, 3)).unwrap();
    assert_eq!(drain(&m, &mut sub, total), (4..=total).collect::<Vec<_>>());
}

#[test]
fn polling_then_dropping_loses_nothing() {
    let (m, a, s) = host_with_queue(256);
    let start = m.log(&s).len() as u64;
    let mut sub = m.block_on(m.host.subscribe(&a, &s, start)).unwrap();
    let waker = Waker::from(Arc::new(Quiet));
    let mut cx = Context::from_waker(&waker);

    // Caught up: pending. Drop the future, more than once.
    for _ in 0..3 {
        let mut next = sub.next_update();
        assert!(next.as_mut().poll(&mut cx).is_pending());
        drop(next);
    }
    assert_eq!(sub.last_seq(), start);

    turn(&m, &a, &s, "Hello there.");
    let total = m.log(&s).len() as u64;
    // Poll once and drop between every update: still every event, once.
    let mut seqs = Vec::new();
    let mut deltas = 0;
    while sub.last_seq() < total {
        let mut next = sub.next_update();
        let got = next.as_mut().poll(&mut cx);
        drop(next);
        match got {
            std::task::Poll::Ready(Some(Update::Event(e))) => seqs.push(e.seq),
            std::task::Poll::Ready(Some(Update::Delta(_))) => deltas += 1,
            other => panic!("expected an update, got {other:?}"),
        }
    }
    assert_eq!(seqs, (start + 1..=total).collect::<Vec<_>>());
    assert!(deltas > 0);
}

#[test]
fn updates_end_after_a_purge() {
    let (m, a, s) = host_with_queue(256);
    turn(&m, &a, &s, "Hello there.");
    let total = m.log(&s).len() as u64;
    let mut caught_up = m.block_on(m.host.subscribe(&a, &s, total)).unwrap();
    let mut behind = m.block_on(m.host.subscribe(&a, &s, 0)).unwrap();
    let waker = Waker::from(Arc::new(Quiet));
    let mut cx = Context::from_waker(&waker);
    let mut waiting = caught_up.next_update();
    assert!(waiting.as_mut().poll(&mut cx).is_pending());

    m.block_on(m.host.purge(&a, &a.owner())).unwrap();
    // The waiter ends; so does one that never read: the log is gone.
    assert_eq!(m.block_on(waiting), None);
    assert_eq!(m.block_on(behind.next_update()), None);
    assert_eq!(m.block_on(behind.next()), None);
}
