// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`MemoryFs`] as the files service: every call is ready at once.

use super::super::{ChangeId, Entry, FileSystem, FsResult, Hit, Linked, Place};
use super::MemoryFs;
use harness_core::future::{ready, BoxFuture};
use harness_core::AppId;

impl FileSystem for MemoryFs {
    fn linked(&self, app: &AppId) -> Vec<Linked> {
        self.state().links.get(app).cloned().unwrap_or_default()
    }

    fn stat<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Option<Entry>>> {
        ready(self.stat_now(at))
    }

    fn list<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Vec<Entry>>> {
        ready(self.list_now(at))
    }

    fn read<'a>(&'a self, at: &'a Place, max: usize) -> BoxFuture<'a, FsResult<String>> {
        ready(self.read_now(at, max))
    }

    fn search<'a>(
        &'a self,
        at: &'a Place,
        query: &'a str,
        text: bool,
    ) -> BoxFuture<'a, FsResult<Vec<Hit>>> {
        ready(self.search_now(at, query, text))
    }

    fn write<'a>(
        &'a self,
        at: &'a Place,
        text: &'a str,
        append: bool,
        by: &'a ChangeId,
    ) -> BoxFuture<'a, FsResult<()>> {
        ready(self.write_now(at, text, append, by))
    }

    fn rename<'a>(
        &'a self,
        from: &'a Place,
        to: &'a Place,
        by: &'a ChangeId,
    ) -> BoxFuture<'a, FsResult<()>> {
        ready(self.rename_now(from, to, by))
    }

    fn trash<'a>(&'a self, at: &'a Place, by: &'a ChangeId) -> BoxFuture<'a, FsResult<()>> {
        ready(self.trash_now(at, by))
    }

    fn undo<'a>(&'a self, app: &'a AppId, by: &'a ChangeId) -> BoxFuture<'a, FsResult<()>> {
        ready(self.undo_now(app, by))
    }

    fn reversible(&self, folder: &str) -> bool {
        !self.state().irreversible.iter().any(|f| f == folder)
    }
}
