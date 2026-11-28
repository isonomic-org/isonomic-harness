// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Kit tool specs, beside the display specs (`docs/kits.md`). A spec is
//! everything the model and the offer rule see: a flat input of at most four
//! fields, a one-line description, the effects, how it runs, the scope kind
//! it acts on, whether it is a core tool, and the revision that added it.
//! The tools themselves live in harness-toolkit.

use crate::kinds::{Concurrency, Effect, ScopeKind, ToolKind};
use crate::offer::KitMember;
use crate::payload::ToolSpec;
use crate::value::Value;

/// One input field. Inputs are flat: strings, whole numbers and booleans.
#[derive(Clone, Copy, Debug)]
pub struct KitField {
    /// Its name.
    pub name: &'static str,
    /// `string`, `integer` or `boolean`.
    pub kind: &'static str,
    /// What the model puts there, short.
    pub description: &'static str,
    /// Whether the call must give it.
    pub required: bool,
}

/// A kit tool as specified.
#[derive(Clone, Debug)]
pub struct KitSpec {
    /// One of [`crate::offer::KITS`].
    pub kit: &'static str,
    /// The name the model calls.
    pub name: &'static str,
    /// One line, at most 140 characters.
    pub description: &'static str,
    /// At most four fields.
    pub fields: &'static [KitField],
    /// What it can do.
    pub effects: &'static [Effect],
    /// How it runs beside other calls.
    pub concurrency: Concurrency,
    /// What it acts on.
    pub scope: ScopeKind,
    /// Admitted before the kit's other tools.
    pub core: bool,
    /// The schema revision that added it.
    pub since_revision: u32,
}

impl KitSpec {
    /// The spec as offered to the model.
    pub fn tool_spec(&self) -> ToolSpec {
        let props = self.fields.iter().map(|f| {
            let field = Value::object([
                ("type", Value::from(f.kind)),
                ("description", Value::from(f.description)),
            ]);
            (f.name, field)
        });
        let required: Vec<&str> = self
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        ToolSpec {
            name: self.name.into(),
            description: self.description.into(),
            input_schema: Value::object([
                ("type", Value::from("object")),
                ("properties", Value::object(props)),
                ("required", Value::from(required)),
            ]),
            kind: ToolKind::Engine,
            effects: self.effects.to_vec(),
            concurrency: self.concurrency.clone(),
            scope: Some(self.scope.clone()),
        }
    }

    /// Its place in the offer rule.
    pub fn member(&self) -> KitMember {
        KitMember {
            kit: self.kit,
            core: self.core,
        }
    }
}

const fn field(
    name: &'static str,
    kind: &'static str,
    description: &'static str,
    required: bool,
) -> KitField {
    KitField {
        name,
        kind,
        description,
        required,
    }
}

const PATH: KitField = field(
    "path",
    "string",
    "A linked folder's name, then the path inside it, like Design/notes.md",
    true,
);
const READ: &[Effect] = &[Effect::ReadsPrivateData];
const READ_IN: &[Effect] = &[Effect::ReadsPrivateData, Effect::IngestsUntrustedContent];
const CHANGE: &[Effect] = &[Effect::ModifiesData];

const fn files(
    name: &'static str,
    description: &'static str,
    fields: &'static [KitField],
    effects: &'static [Effect],
    core: bool,
) -> KitSpec {
    KitSpec {
        kit: "files",
        name,
        description,
        fields,
        effects,
        concurrency: if core {
            Concurrency::Parallel
        } else {
            Concurrency::Serial
        },
        scope: ScopeKind::Folder,
        core,
        since_revision: 1,
    }
}

/// The files kit, in a fixed order: the three read tools (core), then the
/// four that change files.
pub const FILES_SPECS: &[KitSpec] = &[
    files(
        "list_dir",
        "List a folder: one line per entry with its size and date. An empty path lists the linked folders.",
        &[
            field(
                "path",
                "string",
                "A folder, like Design/icons; empty for the linked folders",
                true,
            ),
            field(
                "after",
                "string",
                "Continue after this name, from the last listing",
                false,
            ),
        ],
        READ,
        true,
    ),
    files(
        "read_file",
        "Read a text file, as much as fits. The header says which lines are shown and where to go on.",
        &[
            PATH,
            field("from_line", "integer", "First line to show, from 1", false),
            field("lines", "integer", "How many lines to show at most", false),
        ],
        READ_IN,
        true,
    ),
    files(
        "search_files",
        "Find files by name, or lines that contain some plain text, under a folder.",
        &[
            field("path", "string", "The folder to search, like Design", true),
            field("query", "string", "The name or the text to find", true),
            field("in", "string", "names or text; names when left out", false),
        ],
        READ_IN,
        true,
    ),
    files(
        "write_file",
        "Create or replace a text file with the whole text, or add text to its end.",
        &[
            PATH,
            field("text", "string", "The whole new text, or the text to add", true),
            field("append", "boolean", "Add to the end instead of replacing", false),
        ],
        CHANGE,
        false,
    ),
    files(
        "edit_file",
        "Replace one place in a text file. find must match exactly one place; give more lines if it matches more.",
        &[
            PATH,
            field("find", "string", "The text to find, copied from the file", true),
            field("replace", "string", "What to put in its place", true),
        ],
        CHANGE,
        false,
    ),
    files(
        "move_path",
        "Move or rename a file or folder inside one linked folder. Never replaces what is there.",
        &[
            field("from", "string", "What to move, like Design/old.md", true),
            field("to", "string", "Where it goes, like Design/new.md", true),
        ],
        CHANGE,
        false,
    ),
    files(
        "trash_path",
        "Move a file or folder to the Trash, where the person can get it back.",
        &[PATH],
        CHANGE,
        false,
    ),
];

/// A kit spec by tool name.
pub fn kit_spec(name: &str) -> Option<&'static KitSpec> {
    FILES_SPECS.iter().find(|s| s.name == name)
}

/// The files kit's error codes (`docs/kits.md`).
pub mod files_errors {
    /// The path does not start with a linked folder's name.
    pub const NOT_LINKED: &str = "files.not_linked";
    /// The folder is linked for reading only.
    pub const READ_ONLY: &str = "files.read_only";
    /// The path, or a link on it, leads out of the linked folder.
    pub const OUTSIDE_FOLDER: &str = "files.outside_folder";
    /// Version-control metadata or the project instruction file.
    pub const PROTECTED: &str = "files.protected";
    /// Nothing is there.
    pub const NOT_FOUND: &str = "files.not_found";
    /// Something is already there.
    pub const EXISTS: &str = "files.exists";
    /// The file is not text.
    pub const NOT_TEXT: &str = "files.not_text";
    /// The file is over the size limit.
    pub const TOO_LARGE: &str = "files.too_large";
    /// `find` matched nowhere.
    pub const NO_MATCH: &str = "files.no_match";
    /// `find` matched more than one place.
    pub const MANY_MATCHES: &str = "files.many_matches";
    /// A move between two linked folders.
    pub const CROSS_FOLDER: &str = "files.cross_folder";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_flat_short_and_scoped() {
        assert_eq!(FILES_SPECS.len(), 7);
        for s in FILES_SPECS {
            assert!(s.fields.len() <= 4, "{}", s.name);
            assert!(s.description.chars().count() <= 140, "{}", s.name);
            for f in s.fields {
                assert!(["string", "integer", "boolean"].contains(&f.kind));
                assert!(f.description.chars().count() <= 80, "{}.{}", s.name, f.name);
            }
            let t = s.tool_spec();
            assert_eq!(t.scope, Some(ScopeKind::Folder));
            assert_eq!(kit_spec(s.name).map(|k| k.name), Some(s.name));
        }
        let core: Vec<&str> = FILES_SPECS
            .iter()
            .filter(|s| s.core)
            .map(|s| s.name)
            .collect();
        assert_eq!(core, ["list_dir", "read_file", "search_files"]);
    }
}
