// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The storage trait and an in-memory implementation.

use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Why a store call failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// The events do not continue the session's sequence.
    Conflict {
        /// The seq the store expected next.
        expected: Seq,
        /// The seq it was given.
        found: Seq,
    },
    /// The session is not in the store.
    NotFound,
    /// Anything else, from the engine's storage.
    Backend(String),
}

impl core::fmt::Display for StoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            StoreError::Conflict { expected, found } => {
                write!(f, "append conflict: expected seq {expected}, found {found}")
            }
            StoreError::NotFound => f.write_str("session not found"),
            StoreError::Backend(e) => write!(f, "store: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// A registry row: what the host knows about a session besides its log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMeta {
    /// The session.
    pub id: SessionId,
    /// Its owner app.
    pub app: AppId,
    /// Its class.
    pub class: SessionClass,
    /// The preset it was created from.
    pub preset: PresetRef,
    /// The latest title.
    pub title: Option<String>,
    /// Its status.
    pub status: Status,
    /// Archived by the person.
    pub archived: bool,
    /// The session and seq it was forked from.
    pub forked_from: Option<(SessionId, Seq)>,
    /// Created at.
    pub created_at: Millis,
    /// Last event at.
    pub updated_at: Millis,
    /// The model picked for the session at creation (`session.start`'s
    /// `model`); `None` when it runs on its preset's model.
    pub model: Option<String>,
}

/// Where session logs and registry rows live. The engine implements it over
/// an append-only table; [`MemoryStore`] is the in-memory version.
pub trait SessionStore: Send + Sync {
    /// Append events after the session's last seq, atomically. Refuses a gap
    /// or an overwrite with [`StoreError::Conflict`].
    fn append<'a>(
        &'a self,
        session: &'a SessionId,
        events: &'a [SessionEvent],
    ) -> BoxFuture<'a, Result<(), StoreError>>;

    /// Up to `limit` events with `seq > after`, in order.
    fn read<'a>(
        &'a self,
        session: &'a SessionId,
        after: Seq,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<SessionEvent>, StoreError>>;

    /// Write a registry row.
    fn put_meta<'a>(&'a self, meta: &'a SessionMeta) -> BoxFuture<'a, Result<(), StoreError>>;

    /// Registry rows, optionally for one owner app.
    fn list_meta<'a>(
        &'a self,
        owner: Option<&'a AppId>,
    ) -> BoxFuture<'a, Result<Vec<SessionMeta>, StoreError>>;

    /// Delete sessions' logs and rows. Returns how many events went.
    fn purge<'a>(&'a self, sessions: &'a [SessionId]) -> BoxFuture<'a, Result<u64, StoreError>>;
}

/// An in-memory [`SessionStore`].
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    logs: BTreeMap<SessionId, Vec<SessionEvent>>,
    meta: BTreeMap<SessionId, SessionMeta>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Every event of a session, synchronously (for tests and tools).
    pub fn events(&self, session: &SessionId) -> Vec<SessionEvent> {
        self.lock().logs.get(session).cloned().unwrap_or_default()
    }

    /// Every session with a log.
    pub fn sessions(&self) -> Vec<SessionId> {
        self.lock().logs.keys().cloned().collect()
    }
}

impl SessionStore for MemoryStore {
    fn append<'a>(
        &'a self,
        session: &'a SessionId,
        events: &'a [SessionEvent],
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        let mut inner = self.lock();
        let log = inner.logs.entry(session.clone()).or_default();
        let first = log.last().map_or(1, |e| e.seq + 1);
        for (next, e) in (first..).zip(events) {
            if e.seq != next || &e.session != session {
                return ready(Err(StoreError::Conflict {
                    expected: next,
                    found: e.seq,
                }));
            }
        }
        log.extend_from_slice(events);
        ready(Ok(()))
    }

    fn read<'a>(
        &'a self,
        session: &'a SessionId,
        after: Seq,
        limit: usize,
    ) -> BoxFuture<'a, Result<Vec<SessionEvent>, StoreError>> {
        let inner = self.lock();
        let Some(log) = inner.logs.get(session) else {
            return ready(Err(StoreError::NotFound));
        };
        let start = usize::try_from(after).unwrap_or(usize::MAX).min(log.len());
        let end = start.saturating_add(limit).min(log.len());
        ready(Ok(log[start..end].to_vec()))
    }

    fn put_meta<'a>(&'a self, meta: &'a SessionMeta) -> BoxFuture<'a, Result<(), StoreError>> {
        self.lock().meta.insert(meta.id.clone(), meta.clone());
        ready(Ok(()))
    }

    fn list_meta<'a>(
        &'a self,
        owner: Option<&'a AppId>,
    ) -> BoxFuture<'a, Result<Vec<SessionMeta>, StoreError>> {
        let rows = self
            .lock()
            .meta
            .values()
            .filter(|m| owner.is_none_or(|o| &m.app == o))
            .cloned()
            .collect();
        ready(Ok(rows))
    }

    fn purge<'a>(&'a self, sessions: &'a [SessionId]) -> BoxFuture<'a, Result<u64, StoreError>> {
        let mut inner = self.lock();
        let mut gone = 0;
        for s in sessions {
            gone += inner.logs.remove(s).map_or(0, |l| l.len() as u64);
            inner.meta.remove(s);
        }
        ready(Ok(gone))
    }
}
