// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`MemoryFs`]: the files service fake. It keeps folders in memory and
//! models what the tools rely on a real service for: links per app,
//! read-only folders, protected names, never replacing on a move, a Trash
//! that keeps what was removed, and the text check.

use super::{Entry, FileSystem, FsError, FsResult, Linked, Place};
use harness_core::future::{ready, BoxFuture};
use harness_core::kits::files_errors as code;
use harness_core::AppId;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

/// A node: a file's text, or a folder.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    File(String),
    Dir,
}

#[derive(Default)]
struct State {
    links: HashMap<AppId, Vec<Linked>>,
    nodes: BTreeMap<(String, String), Node>,
    trash: Vec<String>,
    irreversible: Vec<String>,
    clock: i64,
}

/// Folders in memory.
#[derive(Default)]
pub struct MemoryFs {
    state: Mutex<State>,
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

fn under(path: &str, root: &str) -> bool {
    root.is_empty() || path == root || path.starts_with(&format!("{root}/"))
}

impl MemoryFs {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Link a folder for `app`.
    pub fn link(&self, app: &str, id: &str, label: &str, read_only: bool) {
        self.state()
            .links
            .entry(AppId::from(app))
            .or_default()
            .push(Linked {
                id: id.into(),
                label: label.into(),
                read_only,
            });
    }

    /// Mark a folder's changes as ones the engine cannot undo.
    pub fn irreversible(&self, folder: &str) {
        self.state().irreversible.push(folder.into());
    }

    /// Put a file, making its parent folders.
    pub fn put(&self, folder: &str, path: &str, text: &str) {
        let mut s = self.state();
        Self::make_parents(&mut s, folder, path);
        s.nodes
            .insert((folder.into(), path.into()), Node::File(text.into()));
    }

    /// Make a folder and its parents.
    pub fn mkdir(&self, folder: &str, path: &str) {
        let mut s = self.state();
        Self::make_parents(&mut s, folder, path);
        s.nodes.insert((folder.into(), path.into()), Node::Dir);
    }

    /// A file's text.
    pub fn get(&self, folder: &str, path: &str) -> Option<String> {
        match self.state().nodes.get(&(folder.into(), path.into())) {
            Some(Node::File(t)) => Some(t.clone()),
            _ => None,
        }
    }

    /// What was moved to the Trash, as `folder/path`, oldest first.
    pub fn trashed(&self) -> Vec<String> {
        self.state().trash.clone()
    }

    fn make_parents(s: &mut State, folder: &str, path: &str) {
        let mut p = parent(path);
        while !p.is_empty() {
            s.nodes
                .entry((folder.into(), p.into()))
                .or_insert(Node::Dir);
            p = parent(p);
        }
    }

    fn node(s: &State, at: &Place) -> Option<Node> {
        if at.path.is_empty() {
            return Some(Node::Dir);
        }
        s.nodes.get(&(at.folder.clone(), at.path.clone())).cloned()
    }

    fn change(at: &Place) -> FsResult<()> {
        if at.read_only {
            return Err(FsError::new(
                code::READ_ONLY,
                format!("{} is linked for reading only.", at.label),
            ));
        }
        if at.protected() || at.path.is_empty() {
            return Err(FsError::new(
                code::PROTECTED,
                format!("{} is not changed by these tools.", at.logical()),
            ));
        }
        Ok(())
    }

    fn missing(at: &Place) -> FsError {
        FsError::new(code::NOT_FOUND, format!("Nothing is at {}.", at.logical()))
    }

    fn stat_now(&self, at: &Place) -> Option<Entry> {
        let s = self.state();
        let node = Self::node(&s, at)?;
        Some(Entry {
            name: at.name().to_owned(),
            dir: node == Node::Dir,
            size: match &node {
                Node::File(t) => t.len() as u64,
                Node::Dir => 0,
            },
            modified: s.clock,
        })
    }

    fn list_now(&self, at: &Place) -> FsResult<Vec<Entry>> {
        let s = self.state();
        match Self::node(&s, at) {
            None => return Err(Self::missing(at)),
            Some(Node::File(_)) => {
                return Err(FsError::new(
                    code::NOT_FOUND,
                    format!("{} is a file, not a folder.", at.logical()),
                ))
            }
            Some(Node::Dir) => {}
        }
        Ok(s.nodes
            .iter()
            .filter(|((f, p), _)| *f == at.folder && !p.is_empty() && parent(p) == at.path)
            .map(|((_, p), n)| Entry {
                name: p.rsplit('/').next().unwrap_or(p).to_owned(),
                dir: *n == Node::Dir,
                size: match n {
                    Node::File(t) => t.len() as u64,
                    Node::Dir => 0,
                },
                modified: s.clock,
            })
            .collect())
    }

    fn read_now(&self, at: &Place, max: usize) -> FsResult<String> {
        match Self::node(&self.state(), at) {
            Some(Node::File(t)) if t.len() > max => Err(FsError::new(
                code::TOO_LARGE,
                format!("{} is {} bytes; the limit is {max}.", at.logical(), t.len()),
            )),
            Some(Node::File(t)) if t.contains('\0') => Err(FsError::new(
                code::NOT_TEXT,
                format!("{} is not a text file.", at.logical()),
            )),
            Some(Node::File(t)) => Ok(t),
            Some(Node::Dir) => Err(FsError::new(
                code::NOT_TEXT,
                format!("{} is a folder.", at.logical()),
            )),
            None => Err(Self::missing(at)),
        }
    }

    fn write_now(&self, at: &Place, text: &str, append: bool) -> FsResult<()> {
        Self::change(at)?;
        let mut s = self.state();
        let key = (at.folder.clone(), at.path.clone());
        let new = match (s.nodes.get(&key), append) {
            (Some(Node::Dir), _) => {
                return Err(FsError::new(
                    code::EXISTS,
                    format!("{} is a folder.", at.logical()),
                ))
            }
            (Some(Node::File(old)), true) => format!("{old}{text}"),
            _ => text.to_owned(),
        };
        Self::make_parents(&mut s, &at.folder, &at.path);
        s.nodes.insert(key, Node::File(new));
        s.clock += 1;
        Ok(())
    }

    fn rename_now(&self, from: &Place, to: &Place) -> FsResult<()> {
        if from.folder != to.folder {
            return Err(FsError::new(
                code::CROSS_FOLDER,
                "Moves stay inside one linked folder.",
            ));
        }
        Self::change(from)?;
        Self::change(to)?;
        let mut s = self.state();
        if Self::node(&s, from).is_none() {
            return Err(Self::missing(from));
        }
        if Self::node(&s, to).is_some() {
            return Err(FsError::new(
                code::EXISTS,
                format!("Something is already at {}.", to.logical()),
            ));
        }
        if under(&to.path, &from.path) {
            return Err(FsError::new(
                code::EXISTS,
                "A folder cannot move inside itself.",
            ));
        }
        let moved: Vec<(String, String)> = s
            .nodes
            .keys()
            .filter(|(f, p)| *f == from.folder && under(p, &from.path))
            .cloned()
            .collect();
        Self::make_parents(&mut s, &to.folder, &to.path);
        for key in moved {
            if let Some(n) = s.nodes.remove(&key) {
                let rest = &key.1[from.path.len()..];
                s.nodes
                    .insert((to.folder.clone(), format!("{}{rest}", to.path)), n);
            }
        }
        s.clock += 1;
        Ok(())
    }

    fn trash_now(&self, at: &Place) -> FsResult<()> {
        Self::change(at)?;
        let mut s = self.state();
        if Self::node(&s, at).is_none() {
            return Err(Self::missing(at));
        }
        s.nodes
            .retain(|(f, p), _| !(*f == at.folder && under(p, &at.path)));
        s.trash.push(format!("{}/{}", at.folder, at.path));
        s.clock += 1;
        Ok(())
    }
}

impl FileSystem for MemoryFs {
    fn linked(&self, app: &AppId) -> Vec<Linked> {
        self.state().links.get(app).cloned().unwrap_or_default()
    }

    fn stat<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Option<Entry>>> {
        ready(Ok(self.stat_now(at)))
    }

    fn list<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<Vec<Entry>>> {
        ready(self.list_now(at))
    }

    fn read<'a>(&'a self, at: &'a Place, max: usize) -> BoxFuture<'a, FsResult<String>> {
        ready(self.read_now(at, max))
    }

    fn write<'a>(
        &'a self,
        at: &'a Place,
        text: &'a str,
        append: bool,
    ) -> BoxFuture<'a, FsResult<()>> {
        ready(self.write_now(at, text, append))
    }

    fn rename<'a>(&'a self, from: &'a Place, to: &'a Place) -> BoxFuture<'a, FsResult<()>> {
        ready(self.rename_now(from, to))
    }

    fn trash<'a>(&'a self, at: &'a Place) -> BoxFuture<'a, FsResult<()>> {
        ready(self.trash_now(at))
    }

    fn reversible(&self, folder: &str) -> bool {
        !self.state().irreversible.iter().any(|f| f == folder)
    }
}
