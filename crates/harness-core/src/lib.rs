// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isonomic Harness core types.
//!
//! The session event schema, ids, content blocks, session classes, errors and
//! the clock. Pure types with serde as the only dependency, so the SDK can
//! re-export them to every app.
//!
//! The schema reference is `docs/schema.md`; the JSON Schema is
//! [`JSON_SCHEMA`], also at `crates/harness-core/schema/session-event.v1.json`.

pub mod access;
mod approval;
pub mod block;
pub mod class;
pub mod clock;
pub mod display;
pub mod error;
pub mod event;
pub mod future;
pub mod ids;
mod kinds;
pub mod kits;
pub mod live;
mod memory;
pub mod offer;
mod payload;
pub mod value;

pub use block::{Attachment, Block, MemoryRef};
pub use class::SessionClass;
pub use clock::Clock;
pub use error::{Error, ErrorInfo};
pub use event::*;
pub use ids::*;
pub use live::{DeltaKind, LiveDelta};
pub use memory::MemoryEvent;
pub use value::{Number, Value};

use serde::{Deserialize, Serialize};

/// The schema version. It changes only with a migration (one version per
/// migration); readers refuse a newer version.
pub const SCHEMA_VERSION: u32 = 1;

/// The schema revision within [`SCHEMA_VERSION`]. Revisions are additive:
/// new event types, new optional fields and new enum values. Readers accept
/// every revision of their version and render what they don't know
/// generically.
pub const SCHEMA_REVISION: u32 = 1;

/// The `format` value of a JSONL session-log header line.
pub const LOG_FORMAT: &str = "isonomic.session-log";

/// The JSON Schema (draft 2020-12) for one session event, schema v1.
pub const JSON_SCHEMA: &str = include_str!("../schema/session-event.v1.json");

/// The first line of every JSONL session-log generation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogHeader {
    /// Always [`LOG_FORMAT`].
    pub format: String,
    /// The schema version of every event in the generation.
    pub version: u32,
    /// The schema revision the writer knew.
    pub revision: u32,
}

impl LogHeader {
    /// The header this build writes.
    pub fn current() -> Self {
        Self {
            format: LOG_FORMAT.to_owned(),
            version: SCHEMA_VERSION,
            revision: SCHEMA_REVISION,
        }
    }

    /// Accept this header for reading, or say why not.
    pub fn check(&self) -> Result<(), Error> {
        if self.format != LOG_FORMAT {
            return Err(Error::Format(self.format.clone()));
        }
        if self.version > SCHEMA_VERSION {
            return Err(Error::FutureVersion {
                found: self.version,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(())
    }
}
