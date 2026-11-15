//! The tools one session may call.

use crate::Tool;
use harness_core::*;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Who runs a tool.
#[derive(Clone)]
pub enum Handler {
    /// The host runs it.
    Engine(Arc<dyn Tool>),
    /// The app that registered it runs it.
    Client {
        /// That app.
        app: AppId,
    },
    /// A Cell runs it (refused until Milestone 6).
    Cell,
}

impl core::fmt::Debug for Handler {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Handler::Engine(_) => f.write_str("Engine"),
            Handler::Client { app } => write!(f, "Client({app})"),
            Handler::Cell => f.write_str("Cell"),
        }
    }
}

/// A registered tool.
#[derive(Clone, Debug)]
pub struct Entry {
    /// How it is offered to the model.
    pub spec: ToolSpec,
    /// Who runs it.
    pub handler: Handler,
    /// Calls to it create a delegated child session.
    pub delegates: bool,
}

/// Why a registration was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    /// A client registration contained a spec of another kind.
    NotClient(String),
    /// The name is taken by a tool the caller does not own.
    Taken(String),
}

/// The tools of one session, by name. Scoped per session: building one is
/// the host's job, from the preset's allow-list and the owner app's client
/// tools.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    entries: BTreeMap<String, Entry>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an engine tool.
    pub fn add_engine(&mut self, tool: Arc<dyn Tool>) {
        let spec = tool.spec();
        self.entries.insert(
            spec.name.clone(),
            Entry {
                spec,
                handler: Handler::Engine(tool),
                delegates: false,
            },
        );
    }

    /// Add an engine tool whose calls create a delegated child session.
    pub fn add_delegating(&mut self, tool: Arc<dyn Tool>) {
        self.add_engine(tool.clone());
        if let Some(e) = self.entries.get_mut(&tool.spec().name) {
            e.delegates = true;
        }
    }

    /// Register `app`'s client tools, replacing its earlier set. Every spec
    /// must be of kind `client`; names held by other tools are refused.
    pub fn register_client(
        &mut self,
        app: &AppId,
        specs: Vec<ToolSpec>,
    ) -> Result<(), RegistryError> {
        for s in &specs {
            if s.kind != ToolKind::Client {
                return Err(RegistryError::NotClient(s.name.clone()));
            }
            if let Some(e) = self.entries.get(&s.name) {
                if !matches!(&e.handler, Handler::Client { app: a } if a == app) {
                    return Err(RegistryError::Taken(s.name.clone()));
                }
            }
        }
        self.entries
            .retain(|_, e| !matches!(&e.handler, Handler::Client { app: a } if a == app));
        for spec in specs {
            self.entries.insert(
                spec.name.clone(),
                Entry {
                    spec,
                    handler: Handler::Client { app: app.clone() },
                    delegates: false,
                },
            );
        }
        Ok(())
    }

    /// Add a Cell tool's spec (calls are refused until Milestone 6).
    pub fn add_cell(&mut self, spec: ToolSpec) {
        self.entries.insert(
            spec.name.clone(),
            Entry {
                spec,
                handler: Handler::Cell,
                delegates: false,
            },
        );
    }

    /// A tool by name.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.get(name)
    }

    /// Keep only the named tools (the preset's allow-list); client tools are
    /// always kept.
    pub fn retain_named(&mut self, names: &[String]) {
        self.entries.retain(|n, e| {
            matches!(e.handler, Handler::Client { .. }) || names.iter().any(|x| x == n)
        });
    }

    /// The specs offered to the model, by name.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.entries.values().map(|e| e.spec.clone()).collect()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
