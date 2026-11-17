// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Service traits for the tools of later waves, each with an in-memory
//! fake. The engine implements them; secrets never pass through them:
//! tools receive handles.
//!
//! | Trait | Tools | Wave |
//! | --- | --- | --- |
//! | [`ArtifactStore`] | artifact create, update, read | 3 |
//! | [`Egress`] | web search, URL reading | 4 |
//! | [`MemoryStore`] | memory (spec pending) | after approval |
//! | [`Connections`] | connected apps | later |

use harness_core::future::{ready, BoxFuture};
use harness_core::*;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// A service failure, readable by the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceError {
    /// A stable code.
    pub code: String,
    /// A sentence.
    pub message: String,
}

impl ServiceError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Artifacts a session writes. Versions start at 1.
pub trait ArtifactStore: Send + Sync {
    /// Write a new version (or the first) of an artifact.
    fn put(
        &self,
        session: &SessionId,
        artifact: Option<ArtifactId>,
        title: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> BoxFuture<'_, Result<ArtifactEvent, ServiceError>>;
    /// Read an artifact's latest version.
    fn get(
        &self,
        session: &SessionId,
        artifact: &ArtifactId,
    ) -> BoxFuture<'_, Result<Vec<u8>, ServiceError>>;
}

/// An artifact's owner session and its versions.
type Versions = (SessionId, Vec<Vec<u8>>);

/// An in-memory artifact store.
#[derive(Debug, Default)]
pub struct FakeArtifacts {
    items: Mutex<BTreeMap<ArtifactId, Versions>>,
}

impl ArtifactStore for FakeArtifacts {
    fn put(
        &self,
        session: &SessionId,
        artifact: Option<ArtifactId>,
        title: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> BoxFuture<'_, Result<ArtifactEvent, ServiceError>> {
        let mut items = lock(&self.items);
        let id = artifact.unwrap_or_else(|| ArtifactId::new(format!("art_{:04}", items.len() + 1)));
        let entry = items
            .entry(id.clone())
            .or_insert_with(|| (session.clone(), Vec::new()));
        if &entry.0 != session {
            return ready(Err(ServiceError::new(
                "artifact.not_found",
                "No such artifact.",
            )));
        }
        let size = bytes.len() as u64;
        entry.1.push(bytes);
        let version = entry.1.len() as u32;
        ready(Ok(ArtifactEvent {
            artifact: id.clone(),
            version,
            op: if version == 1 {
                ArtifactOp::Create
            } else {
                ArtifactOp::Update
            },
            title: title.into(),
            media_type: media_type.into(),
            blob: BlobId::new(format!("{id}_v{version}")),
            bytes: size,
            call: None,
        }))
    }

    fn get(
        &self,
        session: &SessionId,
        artifact: &ArtifactId,
    ) -> BoxFuture<'_, Result<Vec<u8>, ServiceError>> {
        let items = lock(&self.items);
        ready(match items.get(artifact) {
            Some((owner, versions)) if owner == session => {
                Ok(versions.last().cloned().unwrap_or_default())
            }
            _ => Err(ServiceError::new("artifact.not_found", "No such artifact.")),
        })
    }
}

/// A page read or a search hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    /// Where it came from.
    pub url: String,
    /// Its title.
    pub title: String,
    /// Its text (or a snippet, for a hit).
    pub text: String,
}

/// Reaching the web. Everything it returns is untrusted content.
pub trait Egress: Send + Sync {
    /// Search the web.
    fn search(&self, query: &str) -> BoxFuture<'_, Result<Vec<Page>, ServiceError>>;
    /// Read a URL.
    fn read(&self, url: &str) -> BoxFuture<'_, Result<Page, ServiceError>>;
}

/// Canned pages, keyed by URL; search matches titles and text.
#[derive(Debug, Default)]
pub struct FakeEgress {
    /// The pages it knows.
    pub pages: Vec<Page>,
}

impl Egress for FakeEgress {
    fn search(&self, query: &str) -> BoxFuture<'_, Result<Vec<Page>, ServiceError>> {
        let q = query.to_lowercase();
        let hits = self
            .pages
            .iter()
            .filter(|p| p.title.to_lowercase().contains(&q) || p.text.to_lowercase().contains(&q))
            .cloned()
            .collect();
        ready(Ok(hits))
    }

    fn read(&self, url: &str) -> BoxFuture<'_, Result<Page, ServiceError>> {
        ready(
            self.pages
                .iter()
                .find(|p| p.url == url)
                .cloned()
                .ok_or_else(|| {
                    ServiceError::new("egress.not_found", format!("Could not read {url}."))
                }),
        )
    }
}

/// What the person asked to be remembered. The spec is pending; this is
/// the narrowest shape that the research session's draft needs.
pub trait MemoryStore: Send + Sync {
    /// Notes relevant to `query`.
    fn recall(
        &self,
        owner: &AppId,
        query: &str,
    ) -> BoxFuture<'_, Result<Vec<String>, ServiceError>>;
    /// Keep a note.
    fn remember(&self, owner: &AppId, note: &str) -> BoxFuture<'_, Result<(), ServiceError>>;
}

/// In-memory notes per owner.
#[derive(Debug, Default)]
pub struct FakeMemory {
    notes: Mutex<BTreeMap<AppId, Vec<String>>>,
}

impl MemoryStore for FakeMemory {
    fn recall(
        &self,
        owner: &AppId,
        query: &str,
    ) -> BoxFuture<'_, Result<Vec<String>, ServiceError>> {
        let q = query.to_lowercase();
        let notes = lock(&self.notes);
        let hits = notes
            .get(owner)
            .map(|n| {
                n.iter()
                    .filter(|x| x.to_lowercase().contains(&q))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        ready(Ok(hits))
    }

    fn remember(&self, owner: &AppId, note: &str) -> BoxFuture<'_, Result<(), ServiceError>> {
        lock(&self.notes)
            .entry(owner.clone())
            .or_default()
            .push(note.into());
        ready(Ok(()))
    }
}

/// A connected app the person authorized, by handle (never a secret).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    /// The handle tools use.
    pub handle: String,
    /// What it is, for the person.
    pub label: String,
    /// Operations it allows.
    pub operations: Vec<String>,
}

/// Connected apps.
pub trait Connections: Send + Sync {
    /// The connections an owner may use.
    fn list(&self, owner: &AppId) -> BoxFuture<'_, Vec<Connection>>;
    /// Run an operation through a connection.
    fn call(
        &self,
        owner: &AppId,
        handle: &str,
        operation: &str,
        input: Value,
    ) -> BoxFuture<'_, Result<Value, ServiceError>>;
}

/// Fixed connections that echo their input.
#[derive(Debug, Default)]
pub struct FakeConnections {
    /// Connections, per owner.
    pub by_owner: BTreeMap<AppId, Vec<Connection>>,
}

impl Connections for FakeConnections {
    fn list(&self, owner: &AppId) -> BoxFuture<'_, Vec<Connection>> {
        ready(self.by_owner.get(owner).cloned().unwrap_or_default())
    }

    fn call(
        &self,
        owner: &AppId,
        handle: &str,
        operation: &str,
        input: Value,
    ) -> BoxFuture<'_, Result<Value, ServiceError>> {
        let allowed = self
            .by_owner
            .get(owner)
            .and_then(|cs| cs.iter().find(|c| c.handle == handle))
            .is_some_and(|c| c.operations.iter().any(|o| o == operation));
        ready(if allowed {
            Ok(Value::object([
                ("operation", Value::from(operation)),
                ("input", input),
            ]))
        } else {
            Err(ServiceError::new(
                "connection.denied",
                "That connection does not allow this.",
            ))
        })
    }
}
