// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pre-images and undo in [`MemoryFs`]: a change keeps the places it
//! touches as they were, and its undo puts back those places only.

use super::super::{ChangeId, FsResult, Place};
use super::{parent, under, MemoryFs, Node, PreImage, State};
use harness_core::AppId;

impl MemoryFs {
    /// Keep what is at and under `roots` before `by` changes them.
    pub(super) fn keep(
        s: &mut State,
        at: &Place,
        by: &ChangeId,
        roots: &[&str],
        trash: Option<String>,
    ) {
        let nodes = s
            .nodes
            .iter()
            .filter(|((f, p), _)| *f == at.folder && roots.iter().any(|r| under(p, r)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut made = Vec::new();
        for root in roots {
            let mut p = parent(root);
            while !p.is_empty() && !s.nodes.contains_key(&(at.folder.clone(), p.to_owned())) {
                made.push(p.to_owned());
                p = parent(p);
            }
        }
        s.undo.push(PreImage {
            app: at.app.clone(),
            by: by.clone(),
            folder: at.folder.clone(),
            roots: roots.iter().map(|r| (*r).to_owned()).collect(),
            nodes,
            made,
            trash,
        });
        s.clock += 1;
    }

    pub(super) fn undo_now(&self, app: &AppId, by: &ChangeId) -> FsResult<()> {
        let mut s = self.state();
        let Some(i) = s.undo.iter().rposition(|u| u.app == *app && u.by == *by) else {
            return Ok(());
        };
        // Put back the places this change touched; the rest of the folder,
        // and so another session's change elsewhere in it, stays.
        let pre = s.undo.remove(i);
        s.nodes
            .retain(|(f, p), _| !(*f == pre.folder && pre.roots.iter().any(|r| under(p, r))));
        s.nodes.extend(pre.nodes);
        for dir in &pre.made {
            let key = (pre.folder.clone(), dir.clone());
            let holds = s
                .nodes
                .keys()
                .any(|(f, p)| *f == pre.folder && p != dir && under(p, dir));
            if !holds && s.nodes.get(&key) == Some(&Node::Dir) {
                s.nodes.remove(&key);
            }
        }
        if let Some(i) = pre
            .trash
            .and_then(|t| s.trash.iter().rposition(|x| *x == t))
        {
            s.trash.remove(i);
        }
        s.clock += 1;
        Ok(())
    }
}
