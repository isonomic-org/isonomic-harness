// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files kit (`docs/kits.md`): the service the seven tools work
//! through, and the logical paths they take. A path is a linked folder's
//! label, then a path inside it, like `Design/notes.md`. The service owns
//! the real locations; the tools never see one.

pub mod contract;
pub mod ladder;
pub mod lines;
mod memory;
mod path;
mod text;
mod tools;

pub use memory::MemoryFs;
pub use path::{resolve, Place};
pub use text::date;
pub use tools::{files_kit, FilesTool, DEFAULT_CAP, MIN_CAP, READ_MAX};

use harness_core::future::BoxFuture;
use harness_core::{AppId, CallId};

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

/// One search hit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The path inside the linked folder.
    pub path: String,
    /// Whether it is a folder (name search only).
    pub dir: bool,
    /// For a text hit: the display line (see [`lines`]) and its text.
    pub line: Option<(usize, String)>,
}

/// Where the files kit reads and writes. The host gives one per engine;
/// [`MemoryFs`] is the fake, and [`contract`] is the suite every
/// implementation passes. Every method takes a resolved [`Place`], and the
/// service checks it again when it acts: the folder is still linked for
/// `place.app`, still writable for a change, and nothing on the way leads
/// out of the folder. Changes are tied to their call so they can be undone.
pub trait FileSystem: Send + Sync {
    /// The folders linked for `app`, in the person's order.
    fn linked(&self, app: &AppId) -> Vec<Linked>;

    /// Whether something is there, and whether it is a folder.
    fn stat<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Option<Entry>>>;

    /// A folder's entries, sorted by name.
    fn list<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Vec<Entry>>>;

    /// A text file's whole content, refused over `max` bytes.
    fn read<'a>(&'a self, at: &'a Place, max: usize) -> BoxFuture<'a, FsResult<String>>;

    /// Find under a folder, skipping version-control metadata: names by a
    /// forgiving match, best first then by path; or lines holding `query`
    /// as plain text, without case, in path then line order. Files that are
    /// not text are skipped.
    fn search<'a>(
        &'a self,
        at: &'a Place,
        query: &'a str,
        text: bool,
    ) -> BoxFuture<'a, FsResult<Vec<Hit>>>;

    /// Create or replace a text file, or add to its end, all or nothing.
    /// Keeps what was there, for `call`'s undo.
    fn write<'a>(
        &'a self,
        at: &'a Place,
        text: &'a str,
        append: bool,
        call: &'a CallId,
    ) -> BoxFuture<'a, FsResult<()>>;

    /// Move inside one folder. Never replaces what is at `to`.
    fn rename<'a>(
        &'a self,
        from: &'a Place,
        to: &'a Place,
        call: &'a CallId,
    ) -> BoxFuture<'a, FsResult<()>>;

    /// Move to the Trash.
    fn trash<'a>(&'a self, at: &'a Place, call: &'a CallId) -> BoxFuture<'a, FsResult<()>>;

    /// Undo what `call` changed, if it changed anything.
    fn undo<'a>(&'a self, app: &'a AppId, call: &'a CallId) -> BoxFuture<'a, FsResult<()>>;

    /// Whether changes in `folder` can be undone by the engine.
    fn reversible(&self, folder: &str) -> bool;
}
