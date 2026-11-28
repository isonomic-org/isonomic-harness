// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Logical paths: a linked folder's label, then a path inside it. The
//! resolver refuses anything that is not plainly inside a linked folder
//! before any service call: absolute and home paths, parent steps, and an
//! unknown first segment. Links on disk are the service's to refuse.

use super::{FsError, Linked};
use harness_core::kits::files_errors as code;

/// A resolved place: which linked folder, and the path inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The linked folder's id.
    pub folder: String,
    /// The linked folder's label.
    pub label: String,
    /// Linked for reading only.
    pub read_only: bool,
    /// The path inside it, `/`-separated, empty for the folder itself.
    pub path: String,
}

/// Version-control metadata folders, compared without case.
const VCS: &[&str] = &[".git", ".hg", ".svn", ".jj"];
/// The project instruction file at a folder's top, compared without case.
const INSTRUCTIONS: &str = "agents.md";

impl Place {
    /// The logical path, as the model writes it.
    pub fn logical(&self) -> String {
        if self.path.is_empty() {
            self.label.clone()
        } else {
            format!("{}/{}", self.label, self.path)
        }
    }

    /// Inside version-control metadata.
    pub fn in_vcs(&self) -> bool {
        self.path
            .split('/')
            .any(|s| VCS.iter().any(|v| s.eq_ignore_ascii_case(v)))
    }

    /// Something the kit never changes: version-control metadata or the
    /// project instruction file.
    pub fn protected(&self) -> bool {
        self.in_vcs() || self.path.eq_ignore_ascii_case(INSTRUCTIONS)
    }

    /// The name of the last segment, or the label for the folder itself.
    pub fn name(&self) -> &str {
        self.path
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.label)
    }
}

/// Resolve `path` against the folders linked for the session.
pub fn resolve(linked: &[Linked], path: &str) -> Result<Place, FsError> {
    let trimmed = path.trim();
    let not_linked = || {
        let names: Vec<&str> = linked.iter().map(|l| l.label.as_str()).collect();
        let list = if names.is_empty() {
            "No folders are linked.".to_owned()
        } else {
            format!("Linked folders: {}.", names.join(", "))
        };
        FsError::new(
            code::NOT_LINKED,
            format!("Start the path with a linked folder's name, like Design/notes.md. {list}"),
        )
    };
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('\\')
        || trimmed.starts_with('~')
        || trimmed.as_bytes().get(1) == Some(&b':')
    {
        return Err(not_linked());
    }
    let mut segments = Vec::new();
    for s in trimmed.split(['/', '\\']) {
        match s {
            "" | "." => {}
            ".." => {
                return Err(FsError::new(
                    code::OUTSIDE_FOLDER,
                    "Paths cannot step out with `..`; name the place inside the folder.",
                ))
            }
            s => segments.push(s),
        }
    }
    let (first, rest) = segments.split_first().ok_or_else(not_linked)?;
    let exact = linked.iter().find(|l| l.label == *first);
    let folder = match exact {
        Some(f) => f,
        None => {
            let mut loose = linked
                .iter()
                .filter(|l| l.label.eq_ignore_ascii_case(first));
            match (loose.next(), loose.next()) {
                (Some(f), None) => f,
                _ => return Err(not_linked()),
            }
        }
    };
    Ok(Place {
        folder: folder.id.clone(),
        label: folder.label.clone(),
        read_only: folder.read_only,
        path: rest.join("/"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linked() -> Vec<Linked> {
        vec![
            Linked {
                id: "f1".into(),
                label: "Design".into(),
                read_only: false,
            },
            Linked {
                id: "f2".into(),
                label: "Notes".into(),
                read_only: true,
            },
        ]
    }

    fn err(p: &str) -> &'static str {
        resolve(&linked(), p).unwrap_err().code
    }

    #[test]
    fn resolves_inside_a_linked_folder() {
        let p = resolve(&linked(), "Design/icons/./a.svg").unwrap();
        assert_eq!((p.folder.as_str(), p.path.as_str()), ("f1", "icons/a.svg"));
        assert_eq!(p.logical(), "Design/icons/a.svg");
        assert_eq!(resolve(&linked(), "notes").unwrap().folder, "f2");
        assert!(resolve(&linked(), "Notes/x").unwrap().read_only);
        assert_eq!(resolve(&linked(), "Design").unwrap().path, "");
    }

    #[test]
    fn refuses_what_is_not_plainly_inside() {
        for p in ["", "/etc/passwd", "~/x", "C:\\x", "\\\\host\\x", "Other/x"] {
            assert_eq!(err(p), code::NOT_LINKED, "{p}");
        }
        for p in ["Design/../Notes/x", "Design/a/../../x", "Design\\..\\x"] {
            assert_eq!(err(p), code::OUTSIDE_FOLDER, "{p}");
        }
    }

    #[test]
    fn protected_names() {
        let at = |p: &str| resolve(&linked(), p).unwrap();
        assert!(at("Design/.git/config").protected());
        assert!(at("Design/sub/.JJ/x").protected());
        assert!(at("Design/AGENTS.md").protected());
        assert!(!at("Design/sub/AGENTS.md").protected());
        assert!(!at("Design/gitignore").protected());
    }
}
