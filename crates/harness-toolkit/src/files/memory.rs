// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`MemoryFs`]: the files service fake. It keeps folders in memory and
//! models what the tools rely on a real service for: links per app, checked
//! again at every call; read-only folders; protected names; links on disk,
//! followed only while they stay in the folder; never replacing on a move;
//! a Trash; the text check; and pre-images, so every change can be undone.
//! It passes [`super::contract`].

use super::lines::display_starts;
use super::{Entry, FsError, FsResult, Hit, Linked, Place};
use harness_core::kits::files_errors as code;
use harness_core::{AppId, CallId};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

mod service;

/// A node: a file's text, a folder, or a link to a path.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    File(String),
    Dir,
    /// A link on disk: to a path in a folder, or `None` for anywhere
    /// outside every linked folder.
    Link(Option<(String, String)>),
}

type Nodes = BTreeMap<(String, String), Node>;

/// What a change replaced, for its undo.
struct PreImage {
    app: AppId,
    call: CallId,
    folder: String,
    nodes: Nodes,
    trash: usize,
}

#[derive(Default)]
struct State {
    links: HashMap<AppId, Vec<Linked>>,
    nodes: Nodes,
    trash: Vec<String>,
    irreversible: Vec<String>,
    undo: Vec<PreImage>,
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

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// A forgiving name match: every character of `query`, in order, without
/// case. More when the characters run together and start early; `None`
/// when they are not all there.
fn score(name: &str, query: &str) -> Option<u32> {
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let mut at = 0;
    let (mut score, mut last): (u32, Option<usize>) = (0, None);
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let i = (at..name.len()).find(|&i| name[i] == q)?;
        score += if last == Some(i.wrapping_sub(1)) {
            8
        } else {
            1
        };
        if i == 0 {
            score += 4;
        }
        last = Some(i);
        at = i + 1;
    }
    Some(score)
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

    /// Unlink a folder for `app`.
    pub fn unlink(&self, app: &str, id: &str) {
        if let Some(l) = self.state().links.get_mut(&AppId::from(app)) {
            l.retain(|f| f.id != id);
        }
    }

    /// Mark a folder's changes as ones the engine cannot undo.
    pub fn irreversible(&self, folder: &str) {
        self.state().irreversible.push(folder.into());
    }

    /// Put a file, making its parent folders.
    pub fn put(&self, folder: &str, path: &str, text: &str) {
        self.insert(folder, path, Node::File(text.into()));
    }

    /// Make a folder and its parents.
    pub fn mkdir(&self, folder: &str, path: &str) {
        self.insert(folder, path, Node::Dir);
    }

    /// Put a link at `path`: to `to` in `to_folder`, or, with `None`, to a
    /// place outside every linked folder.
    pub fn symlink(&self, folder: &str, path: &str, to: Option<(&str, &str)>) {
        let to = to.map(|(f, p)| (f.to_owned(), p.to_owned()));
        self.insert(folder, path, Node::Link(to));
    }

    fn insert(&self, folder: &str, path: &str, node: Node) {
        let mut s = self.state();
        Self::make_parents(&mut s.nodes, folder, path);
        s.nodes.insert((folder.into(), path.into()), node);
    }

    /// A file's text.
    pub fn get(&self, folder: &str, path: &str) -> Option<String> {
        match self.state().nodes.get(&(folder.into(), path.into())) {
            Some(Node::File(t)) => Some(t.clone()),
            _ => None,
        }
    }

    /// Every node in a folder, as (path, text or `None` for a folder),
    /// for comparing a folder before and after.
    pub fn snapshot(&self, folder: &str) -> Vec<(String, Option<String>)> {
        self.state()
            .nodes
            .iter()
            .filter(|((f, _), _)| f == folder)
            .map(|((_, p), n)| {
                let body = match n {
                    Node::File(t) => Some(t.clone()),
                    _ => None,
                };
                (p.clone(), body)
            })
            .collect()
    }

    /// What was moved to the Trash, as `folder/path`, oldest first.
    pub fn trashed(&self) -> Vec<String> {
        self.state().trash.clone()
    }

    fn make_parents(nodes: &mut Nodes, folder: &str, path: &str) {
        let mut p = parent(path);
        while !p.is_empty() {
            nodes.entry((folder.into(), p.into())).or_insert(Node::Dir);
            p = parent(p);
        }
    }

    /// Where `at` really is: the folder must still be linked for the app,
    /// and every link on the way must stay inside the folder.
    fn locate(s: &State, at: &Place) -> FsResult<String> {
        let linked = s
            .links
            .get(&at.app)
            .and_then(|l| l.iter().find(|f| f.id == at.folder));
        if linked.is_none() {
            return Err(FsError::new(
                code::NOT_LINKED,
                format!("Permission denied: {} is not a linked folder.", at.label),
            ));
        }
        let outside = || {
            FsError::new(
                code::OUTSIDE_FOLDER,
                format!("{} leads out of {}.", at.logical(), at.label),
            )
        };
        let mut path = at.path.clone();
        for _ in 0..16 {
            let mut so_far = String::new();
            let mut jumped = None;
            for seg in path.split('/').filter(|s| !s.is_empty()) {
                so_far = join(&so_far, seg);
                match s.nodes.get(&(at.folder.clone(), so_far.clone())) {
                    Some(Node::Link(Some((f, to)))) if *f == at.folder => {
                        jumped = Some((so_far.len(), to.clone()));
                        break;
                    }
                    Some(Node::Link(_)) => return Err(outside()),
                    Some(Node::File(_)) if so_far.len() < path.len() => {
                        return Err(FsError::new(
                            code::NOT_FOUND,
                            format!("{so_far} in {} is a file, not a folder.", at.label),
                        ))
                    }
                    _ => {}
                }
            }
            match jumped {
                None => return Ok(path),
                Some((len, to)) => path = format!("{to}{}", &path[len..]),
            }
        }
        Err(outside())
    }

    fn writable(s: &State, at: &Place) -> FsResult<String> {
        let path = Self::locate(s, at)?;
        let read_only = s.links[&at.app]
            .iter()
            .find(|f| f.id == at.folder)
            .is_some_and(|f| f.read_only);
        if read_only {
            return Err(FsError::new(
                code::READ_ONLY,
                format!("{} is linked for reading only.", at.label),
            ));
        }
        let real = Place {
            path: path.clone(),
            ..at.clone()
        };
        if at.protected() || real.protected() || path.is_empty() {
            return Err(FsError::new(
                code::PROTECTED,
                format!("{} is not changed by these tools.", at.logical()),
            ));
        }
        Ok(path)
    }

    /// A folder that holds a protected place is not moved or trashed.
    fn holds_no_protected(s: &State, at: &Place, path: &str) -> FsResult<()> {
        let held = s.nodes.keys().any(|(f, p)| {
            let inside = Place {
                path: p.clone(),
                ..at.clone()
            };
            *f == at.folder && under(p, path) && inside.protected()
        });
        if held {
            return Err(FsError::new(
                code::PROTECTED,
                format!("{} holds a protected place.", at.logical()),
            ));
        }
        Ok(())
    }

    fn node(s: &State, folder: &str, path: &str) -> Option<Node> {
        if path.is_empty() {
            return Some(Node::Dir);
        }
        s.nodes.get(&(folder.to_owned(), path.to_owned())).cloned()
    }

    fn missing(at: &Place) -> FsError {
        FsError::new(code::NOT_FOUND, format!("Nothing is at {}.", at.logical()))
    }

    fn keep(s: &mut State, at: &Place, call: &CallId) {
        let nodes = s
            .nodes
            .iter()
            .filter(|((f, _), _)| *f == at.folder)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let trash = s.trash.len();
        s.undo.push(PreImage {
            app: at.app.clone(),
            call: call.clone(),
            folder: at.folder.clone(),
            nodes,
            trash,
        });
        s.clock += 1;
    }

    fn entry(name: &str, n: &Node, clock: i64) -> Entry {
        Entry {
            name: name.to_owned(),
            dir: matches!(n, Node::Dir),
            size: match n {
                Node::File(t) => t.len() as u64,
                _ => 0,
            },
            modified: clock,
        }
    }

    fn stat_now(&self, at: &Place) -> FsResult<Option<Entry>> {
        let s = self.state();
        let path = Self::locate(&s, at)?;
        Ok(Self::node(&s, &at.folder, &path).map(|n| Self::entry(at.name(), &n, s.clock)))
    }

    fn list_now(&self, at: &Place) -> FsResult<Vec<Entry>> {
        let s = self.state();
        let path = Self::locate(&s, at)?;
        match Self::node(&s, &at.folder, &path) {
            None => return Err(Self::missing(at)),
            Some(Node::Dir) => {}
            Some(_) => {
                return Err(FsError::new(
                    code::NOT_FOUND,
                    format!("{} is a file, not a folder.", at.logical()),
                ))
            }
        }
        Ok(s.nodes
            .iter()
            .filter(|((f, p), _)| *f == at.folder && !p.is_empty() && parent(p) == path)
            .map(|((_, p), n)| Self::entry(p.rsplit('/').next().unwrap_or(p), n, s.clock))
            .collect())
    }

    fn read_now(&self, at: &Place, max: usize) -> FsResult<String> {
        let s = self.state();
        let path = Self::locate(&s, at)?;
        match Self::node(&s, &at.folder, &path) {
            Some(Node::File(t)) if t.len() > max => Err(FsError::new(
                code::TOO_LARGE,
                format!("{} is {} bytes; the limit is {max}.", at.logical(), t.len()),
            )),
            Some(Node::File(t)) if t.contains('\0') => Err(FsError::new(
                code::NOT_TEXT,
                format!("{} is not a text file.", at.logical()),
            )),
            Some(Node::File(t)) => Ok(t),
            Some(_) => Err(FsError::new(
                code::NOT_TEXT,
                format!("{} is a folder.", at.logical()),
            )),
            None => Err(Self::missing(at)),
        }
    }

    fn search_now(&self, at: &Place, query: &str, text: bool) -> FsResult<Vec<Hit>> {
        let s = self.state();
        let root = Self::locate(&s, at)?;
        let mut ranked: Vec<(u32, Hit)> = Vec::new();
        let needle = query.to_lowercase();
        for ((f, p), n) in &s.nodes {
            let inside = Place {
                path: p.clone(),
                ..at.clone()
            };
            if *f != at.folder || p.is_empty() || !under(p, &root) || inside.in_vcs() {
                continue;
            }
            let shown = format!("{}{}", at.path, &p[root.len()..]);
            let shown = shown.trim_start_matches('/').to_owned();
            match (n, text) {
                (Node::File(body), true) if !body.contains('\0') => {
                    let starts = display_starts(body);
                    let src = super::lines::Source::parse(body);
                    for (i, line) in src.lines.iter().enumerate() {
                        if line.text.to_lowercase().contains(&needle) {
                            ranked.push((
                                0,
                                Hit {
                                    path: shown.clone(),
                                    dir: false,
                                    line: Some((starts[i], line.text.to_owned())),
                                },
                            ));
                        }
                    }
                }
                (Node::Link(_), _) | (_, true) => {}
                (node, false) => {
                    let name = p.rsplit('/').next().unwrap_or(p);
                    if let Some(sc) = score(name, query) {
                        ranked.push((
                            sc,
                            Hit {
                                path: shown,
                                dir: matches!(node, Node::Dir),
                                line: None,
                            },
                        ));
                    }
                }
            }
        }
        ranked.sort_by(|(a, x), (b, y)| b.cmp(a).then_with(|| x.path.cmp(&y.path)));
        Ok(ranked.into_iter().map(|(_, h)| h).collect())
    }

    fn write_now(&self, at: &Place, text: &str, append: bool, call: &CallId) -> FsResult<()> {
        let mut s = self.state();
        let path = Self::writable(&s, at)?;
        let key = (at.folder.clone(), path.clone());
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
        Self::keep(&mut s, at, call);
        Self::make_parents(&mut s.nodes, &at.folder, &path);
        s.nodes.insert(key, Node::File(new));
        Ok(())
    }

    fn rename_now(&self, from: &Place, to: &Place, call: &CallId) -> FsResult<()> {
        if from.folder != to.folder {
            return Err(FsError::new(
                code::CROSS_FOLDER,
                "Moves stay inside one linked folder.",
            ));
        }
        let mut s = self.state();
        let (src, dst) = (Self::writable(&s, from)?, Self::writable(&s, to)?);
        if Self::node(&s, &from.folder, &src).is_none() {
            return Err(Self::missing(from));
        }
        if Self::node(&s, &to.folder, &dst).is_some() {
            return Err(FsError::new(
                code::EXISTS,
                format!("Something is already at {}.", to.logical()),
            ));
        }
        Self::holds_no_protected(&s, from, &src)?;
        if under(&dst, &src) {
            return Err(FsError::new(
                code::EXISTS,
                "A folder cannot move inside itself.",
            ));
        }
        Self::keep(&mut s, from, call);
        let moved: Vec<(String, String)> = s
            .nodes
            .keys()
            .filter(|(f, p)| *f == from.folder && under(p, &src))
            .cloned()
            .collect();
        Self::make_parents(&mut s.nodes, &to.folder, &dst);
        for key in moved {
            if let Some(n) = s.nodes.remove(&key) {
                let rest = &key.1[src.len()..];
                s.nodes
                    .insert((to.folder.clone(), format!("{dst}{rest}")), n);
            }
        }
        Ok(())
    }

    fn trash_now(&self, at: &Place, call: &CallId) -> FsResult<()> {
        let mut s = self.state();
        let path = Self::writable(&s, at)?;
        if Self::node(&s, &at.folder, &path).is_none() {
            return Err(Self::missing(at));
        }
        Self::holds_no_protected(&s, at, &path)?;
        Self::keep(&mut s, at, call);
        s.nodes
            .retain(|(f, p), _| !(*f == at.folder && under(p, &path)));
        s.trash.push(format!("{}/{}", at.folder, path));
        Ok(())
    }

    fn undo_now(&self, app: &AppId, call: &CallId) -> FsResult<()> {
        let mut s = self.state();
        let Some(i) = s
            .undo
            .iter()
            .rposition(|u| u.app == *app && u.call == *call)
        else {
            return Ok(());
        };
        let pre = s.undo.remove(i);
        s.nodes.retain(|(f, _), _| *f != pre.folder);
        s.nodes.extend(pre.nodes);
        s.trash.truncate(pre.trash);
        s.clock += 1;
        Ok(())
    }
}
