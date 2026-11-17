// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isonomic Harness host: the logic the engine links to run every session
//! for every app.
//!
//! - **Registry and isolation.** Sessions, owner app (from the capability
//!   token), principal, class, archive, rename, fork. An app reaches only
//!   its own sessions, presets and artifacts; another app's ids answer
//!   `NotFound`. The shell sees everything.
//! - **Dispatch and enforcement.** Engine and client tools, with policy
//!   applied to every kind equally, approvals that need a user-action
//!   token, budgets per session and per app, and an audit record of every
//!   tool call and approval.
//! - **Events.** Persist, then publish; subscribers resume from a sequence
//!   number with no gaps or duplicates; a slow subscriber never blocks the
//!   loop; live deltas ride a separate lossy channel.
//! - **Crash isolation and recovery.** A panic or a hang in one session
//!   seals its turn and marks it failed; nothing else notices. On start,
//!   interrupted turns are sealed.
//! - **Purge.** An app deletes all of its own sessions in one call.
//!
//! Everything outside goes through traits the engine implements
//! ([`traits`]); [`MemoryHost`] implements them all in memory. The host
//! names no async runtime: futures come from the traits.

mod admin;
mod api;
mod events;
pub mod exec;
mod host;
mod memory;
mod sessions;
pub mod traits;
mod types;
mod watch;

pub use events::{Subscription, Update};
pub use exec::{InProcessExecutor, ManualClock, MemorySpawner};
pub use host::{Deps, Host, HostConfig};
pub use memory::{
    memory_config, FixedLocale, MemoryBlobs, MemoryClients, MemoryHost, MemoryScopes, ScriptedModel,
};
pub use traits::*;
pub use types::*;
