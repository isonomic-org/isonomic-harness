//! Fan-out. Persist, then publish: an event reaches subscribers only after
//! the store accepted it. Each subscriber has a bounded live queue; when it
//! fills, the subscriber is marked lagging and reads from the store until it
//! catches up, so the loop never waits for a subscriber. Live deltas go on
//! a separate, smaller, lossy queue.

use crate::exec::lock;
use harness_core::future::BoxFuture;
use harness_core::*;
use harness_log::SessionStore;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Poll, Waker};

#[derive(Default)]
struct Live {
    events: VecDeque<SessionEvent>,
    deltas: VecDeque<LiveDelta>,
    lagging: bool,
    closed: bool,
    waker: Option<Waker>,
}

/// One subscriber's queues.
pub(crate) struct Feed {
    live: Mutex<Live>,
    cap: usize,
    delta_cap: usize,
}

impl Feed {
    fn wake(l: &mut Live) {
        if let Some(w) = l.waker.take() {
            w.wake();
        }
    }
}

/// The subscribers of one session.
#[derive(Default)]
pub(crate) struct Hub {
    feeds: Mutex<Vec<Weak<Feed>>>,
}

impl Hub {
    pub(crate) fn join(&self, cap: usize) -> Arc<Feed> {
        let feed = Arc::new(Feed {
            live: Mutex::new(Live::default()),
            cap: cap.max(1),
            delta_cap: (cap / 4).max(8),
        });
        lock(&self.feeds).push(Arc::downgrade(&feed));
        feed
    }

    /// Publish a persisted event. Never blocks.
    pub(crate) fn event(&self, e: &SessionEvent) {
        let mut feeds = lock(&self.feeds);
        feeds.retain(|f| f.strong_count() > 0);
        for f in feeds.iter().filter_map(Weak::upgrade) {
            let mut l = lock(&f.live);
            if l.events.len() >= f.cap {
                l.events.clear();
                l.lagging = true;
            } else if !l.lagging {
                l.events.push_back(e.clone());
            }
            Feed::wake(&mut l);
        }
    }

    /// Publish a live delta; dropped when a subscriber's delta queue is full.
    pub(crate) fn delta(&self, d: &LiveDelta) {
        for f in lock(&self.feeds).iter().filter_map(Weak::upgrade) {
            let mut l = lock(&f.live);
            if l.deltas.len() < f.delta_cap {
                l.deltas.push_back(d.clone());
            }
        }
    }

    /// The session is gone (purged): subscribers end.
    pub(crate) fn close(&self) {
        for f in lock(&self.feeds).iter().filter_map(Weak::upgrade) {
            let mut l = lock(&f.live);
            l.closed = true;
            Feed::wake(&mut l);
        }
    }
}

/// A subscription to one session: durable events with `seq` above where it
/// started, with no gaps or duplicates, then live ones; and live deltas.
pub struct Subscription {
    store: Arc<dyn SessionStore>,
    session: SessionId,
    last: Seq,
    buffer: VecDeque<SessionEvent>,
    backfill: bool,
    feed: Arc<Feed>,
}

impl Subscription {
    pub(crate) fn new(
        store: Arc<dyn SessionStore>,
        session: SessionId,
        from: Seq,
        feed: Arc<Feed>,
    ) -> Self {
        Self {
            store,
            session,
            last: from,
            buffer: VecDeque::new(),
            backfill: true,
            feed,
        }
    }

    /// The last seq delivered.
    pub fn last_seq(&self) -> Seq {
        self.last
    }

    /// The next durable event; `None` once the session is purged and every
    /// event is delivered. (Async, so not an `Iterator`.)
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> BoxFuture<'_, Option<SessionEvent>> {
        Box::pin(async move {
            loop {
                if let Some(e) = self.buffer.pop_front() {
                    if e.seq <= self.last {
                        continue;
                    }
                    if e.seq == self.last + 1 {
                        self.last = e.seq;
                        return Some(e);
                    }
                    self.buffer.clear();
                    self.backfill = true;
                }
                if self.backfill {
                    match self.store.read(&self.session, self.last, 256).await {
                        Ok(got) if !got.is_empty() => {
                            self.buffer.extend(got);
                            continue;
                        }
                        _ => self.backfill = false,
                    }
                }
                let feed = self.feed.clone();
                let got = std::future::poll_fn(|cx| {
                    let mut l = lock(&feed.live);
                    if let Some(e) = l.events.pop_front() {
                        return Poll::Ready(Some(Some(e)));
                    }
                    if l.lagging {
                        l.lagging = false;
                        return Poll::Ready(Some(None));
                    }
                    if l.closed {
                        return Poll::Ready(None);
                    }
                    l.waker = Some(cx.waker().clone());
                    Poll::Pending
                })
                .await;
                match got {
                    Some(Some(e)) => self.buffer.push_back(e),
                    Some(None) => self.backfill = true,
                    None => {
                        // Closed: deliver what the store still has, then end.
                        if let Ok(rest) = self.store.read(&self.session, self.last, 256).await {
                            if !rest.is_empty() {
                                self.buffer.extend(rest);
                                continue;
                            }
                        }
                        return None;
                    }
                }
            }
        })
    }

    /// Live deltas received so far (lossy; never stored).
    pub fn deltas(&mut self) -> Vec<LiveDelta> {
        lock(&self.feed.live).deltas.drain(..).collect()
    }
}
