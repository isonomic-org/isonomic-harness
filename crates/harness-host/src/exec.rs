//! In-process execution: a panic-catching executor over any `Spawner`, and
//! a runtime-free spawner for tests and the SDK's in-memory host.

use crate::traits::*;
use harness_core::future::BoxFuture;
use harness_core::*;
use std::collections::VecDeque;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

/// A future that turns a panic into `Err(message)`.
pub(crate) struct CatchUnwind<F>(pub(crate) Pin<Box<F>>);

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let f = &mut self.0;
        match catch_unwind(AssertUnwindSafe(|| f.as_mut().poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(v)) => Poll::Ready(Ok(v)),
            Err(p) => Poll::Ready(Err(message(p))),
        }
    }
}

/// Runs tasks on a spawner and reports panics on their handles.
pub struct InProcessExecutor {
    spawner: Arc<dyn Spawner>,
}

impl InProcessExecutor {
    /// An executor over `spawner`.
    pub fn new(spawner: Arc<dyn Spawner>) -> Self {
        Self { spawner }
    }
}

impl Executor for InProcessExecutor {
    fn run(&self, session: SessionId, task: BoxFuture<'static, ()>) -> RunHandle {
        let handle = RunHandle::new(session);
        let h = handle.clone();
        self.spawner.spawn(Box::pin(async move {
            let out = CatchUnwind(Box::pin(task)).await;
            h.report(out.err().map(Fault::Panic));
        }));
        handle
    }
}

/// A clock moved by hand.
#[derive(Debug, Default)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    /// A clock reading `at`.
    pub fn new(at: Millis) -> Self {
        Self(AtomicU64::new(at))
    }
    /// Move forward.
    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> Millis {
        self.0.load(Ordering::SeqCst)
    }
}

struct Task {
    fut: Mutex<Option<BoxFuture<'static, ()>>>,
    woken: AtomicBool,
}

struct TaskWaker(Arc<Task>);

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.0.woken.store(true, Ordering::SeqCst);
    }
}

/// A deterministic, runtime-free spawner: tasks run when [`run`] is called,
/// in the order they were woken. `sleep` completes when the clock passes
/// its deadline.
///
/// [`run`]: MemorySpawner::run
pub struct MemorySpawner {
    tasks: Mutex<VecDeque<Arc<Task>>>,
    clock: Arc<ManualClock>,
}

impl MemorySpawner {
    /// A spawner on `clock`.
    pub fn new(clock: Arc<ManualClock>) -> Self {
        Self {
            tasks: Mutex::new(VecDeque::new()),
            clock,
        }
    }

    /// Poll woken tasks until none is left to poll. Returns how many polls
    /// ran.
    pub fn run(&self) -> usize {
        let mut polls = 0;
        loop {
            let ready: Vec<Arc<Task>> = lock(&self.tasks)
                .iter()
                .filter(|t| t.woken.swap(false, Ordering::SeqCst))
                .cloned()
                .collect();
            if ready.is_empty() {
                break;
            }
            for t in ready {
                polls += 1;
                let waker = Waker::from(Arc::new(TaskWaker(t.clone())));
                let mut cx = Context::from_waker(&waker);
                let mut slot = lock(&t.fut);
                let done = match slot.as_mut() {
                    Some(f) => f.as_mut().poll(&mut cx).is_ready(),
                    None => true,
                };
                if done {
                    *slot = None;
                }
            }
            lock(&self.tasks).retain(|t| lock(&t.fut).is_some());
        }
        polls
    }

    /// Wake every task waiting on a timer (after the clock moved).
    pub fn wake_timers(&self) {
        for t in lock(&self.tasks).iter() {
            t.woken.store(true, Ordering::SeqCst);
        }
    }

    /// Tasks not yet finished.
    pub fn pending(&self) -> usize {
        lock(&self.tasks).len()
    }

    /// Drive `fut` to completion, running background tasks meanwhile.
    pub fn block_on<T>(&self, fut: impl Future<Output = T>) -> T {
        let mut fut = Box::pin(fut);
        let flag = Arc::new(Task {
            fut: Mutex::new(None),
            woken: AtomicBool::new(true),
        });
        let waker = Waker::from(Arc::new(TaskWaker(flag.clone())));
        let mut cx = Context::from_waker(&waker);
        for _ in 0..1_000_000 {
            if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
            if self.run() == 0 && !flag.woken.swap(false, Ordering::SeqCst) {
                panic!("block_on: the future is waiting on something that never comes");
            }
        }
        panic!("block_on: no progress");
    }
}

impl Spawner for MemorySpawner {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        lock(&self.tasks).push_back(Arc::new(Task {
            fut: Mutex::new(Some(task)),
            woken: AtomicBool::new(true),
        }));
    }

    fn sleep(&self, ms: u64) -> BoxFuture<'static, ()> {
        let clock = self.clock.clone();
        let deadline = clock.now_ms() + ms;
        Box::pin(std::future::poll_fn(move |_| {
            if clock.now_ms() >= deadline {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }))
    }
}
