// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isonomic Harness log.
//!
//! The append-only session log is the only record of what the model saw.
//! This crate holds:
//!
//! - [`SessionStore`], the storage trait the engine implements (an
//!   append-only table in its database), and [`MemoryStore`].
//! - The JSONL interchange format: versioned generations, one-step
//!   migrations, refusal of future versions ([`jsonl`], [`migrate`]).
//! - [`ModelRequest`] and [`request_for`], which rebuild every model request
//!   from the log alone, byte for byte.

pub mod history;
pub mod jsonl;
pub mod migrate;
pub mod model;
pub mod store;

pub use history::{heuristic_title, request_for, resolve_fork, HistoryError};
pub use jsonl::{decode, encode, Generation, LogError};
pub use model::{Message, ModelEvent, ModelRequest, ModelStop, Part, Role};
pub use store::{MemoryStore, SessionMeta, SessionStore, StoreError};
