// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files kit (`docs/kits.md`): the service the seven tools work
//! through, and the logical paths they take. A path is a linked folder's
//! label, then a path inside it, like `Design/notes.md`. The service owns
//! the real locations; the tools never see one.

pub mod ladder;
mod memory;
mod path;
mod tools;

pub use memory::MemoryFs;
pub use path::{resolve, Place};
pub use tools::{files_kit, FilesTool, DEFAULT_CAP, READ_MAX};

use harness_core::future::BoxFuture;
use harness_core::AppId;

/// A folder the person linked for an app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linked {
    /// Stable id, used as the grant scope's id.
    pub id: String,
    /// The name the model and the person see; the first path segment.
    pub label: String,
    /// Linked for reading only.
    pub read_only: bool,
}

/// One entry of a listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Its name inside the listed folder.
    pub name: String,
    /// A folder rather than a file.
    pub dir: bool,
    /// Size in bytes; zero for a folder.
    pub size: u64,
    /// Last modified, in milliseconds since the epoch.
    pub modified: i64,
}

/// A service failure, with one of [`harness_core::kits::files_errors`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsError {
    /// The error code.
    pub code: &'static str,
    /// What the model reads.
    pub message: String,
}

impl FsError {
    /// An error with a code and a message.
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// The result of a service call.
pub type FsResult<T> = Result<T, FsError>;

/// Where the files kit reads and writes. The host gives one per engine;
/// [`MemoryFs`] is the fake. Every method takes a resolved
/// [`Place`], so the service checks links and protected names once, at
/// the boundary, and never follows a link out of the folder.
pub trait FileSystem: Send + Sync {
    /// The folders linked for `app`, in the person's order.
    fn linked(&self, app: &AppId) -> Vec<Linked>;

    /// Whether something is there, and whether it is a folder.
    fn stat<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Option<Entry>>>;

    /// A folder's entries, sorted by name.
    fn list<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Vec<Entry>>>;

    /// A text file's whole content, refused over `max` bytes.
    fn read<'a>(&'a self, at: &'a Place, max: usize) -> BoxFuture<'a, FsResult<String>>;

    /// Create or replace a text file, or add to its end. Keeps what was
    /// there so the write can be undone.
    fn write<'a>(
        &'a self,
        at: &'a Place,
        text: &'a str,
        append: bool,
    ) -> BoxFuture<'a, FsResult<()>>;

    /// Move inside one folder. Never replaces what is at `to`.
    fn rename<'a>(&'a self, from: &'a Place, to: &'a Place) -> BoxFuture<'a, FsResult<()>>;

    /// Move to the Trash.
    fn trash<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<()>>;

    /// Whether changes in `folder` can be undone by the engine.
    fn reversible(&self, folder: &str) -> bool;
}
