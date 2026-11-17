// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! JSONL generations: the interchange format for export, import and tests.
//!
//! A generation is one UTF-8 text: a header line naming the format and
//! schema version, then one event per line. Reading migrates older versions
//! one step at a time and refuses newer ones. A committed generation is
//! never rewritten; migration happens on read.

use crate::migrate;
use harness_core::*;

/// A decoded generation.
#[derive(Clone, Debug, PartialEq)]
pub struct Generation {
    /// The version it was written at.
    pub written_version: u32,
    /// Its events, migrated to the current version.
    pub events: Vec<SessionEvent>,
}

/// Why a generation could not be read.
#[derive(Debug, PartialEq)]
pub enum LogError {
    /// No header line.
    Empty,
    /// The header is not a session-log header.
    Header(String),
    /// Written by a newer schema version.
    FutureVersion {
        /// The version found.
        found: u32,
        /// The newest this build reads.
        supported: u32,
    },
    /// No migration from this version.
    NoMigration(u32),
    /// A line did not parse.
    Line {
        /// Its 1-based line number.
        line: usize,
        /// Why.
        error: String,
    },
}

impl core::fmt::Display for LogError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LogError::Empty => f.write_str("empty generation"),
            LogError::Header(e) => write!(f, "bad header: {e}"),
            LogError::FutureVersion { found, supported } => {
                write!(f, "version {found} is newer than {supported}")
            }
            LogError::NoMigration(v) => write!(f, "no migration from version {v}"),
            LogError::Line { line, error } => write!(f, "line {line}: {error}"),
        }
    }
}

impl std::error::Error for LogError {}

/// Write events as a generation at the current version.
pub fn encode(events: &[SessionEvent]) -> String {
    let mut out = serde_json::to_string(&LogHeader::current()).expect("header serializes");
    out.push('\n');
    for e in events {
        out.push_str(&serde_json::to_string(e).expect("event serializes"));
        out.push('\n');
    }
    out
}

/// Read a generation, migrating it to the current version.
pub fn decode(text: &str) -> Result<Generation, LogError> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty());
    let (_, first) = lines.next().ok_or(LogError::Empty)?;
    let header: serde_json::Value =
        serde_json::from_str(first).map_err(|e| LogError::Header(e.to_string()))?;
    if header["format"].as_str() != Some(LOG_FORMAT) {
        return Err(LogError::Header(format!("format {}", header["format"])));
    }
    let version = header["version"]
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| LogError::Header("no version".into()))?;
    if version > SCHEMA_VERSION {
        return Err(LogError::FutureVersion {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    let mut events = Vec::new();
    for (n, line) in lines {
        let mut value: serde_json::Value =
            serde_json::from_str(line).map_err(|e| LogError::Line {
                line: n + 1,
                error: e.to_string(),
            })?;
        for from in version..SCHEMA_VERSION {
            value = migrate::step(from, value).ok_or(LogError::NoMigration(from))?;
        }
        let event: SessionEvent = serde_json::from_value(value).map_err(|e| LogError::Line {
            line: n + 1,
            error: e.to_string(),
        })?;
        events.push(event);
    }
    Ok(Generation {
        written_version: version,
        events,
    })
}
