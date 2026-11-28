// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! kits-files-read: the files kit's three read tools under read grants,
//! none of which asks. The linked folders and a folder are listed, a file
//! is read in two pages, names and text are searched, and paths that are
//! not plainly inside a linked folder, a file that is not text and a
//! missing file fail as results.

use super::build::*;
use super::kits_files::*;
use crate::recorder::Recorder;
use harness_core::*;

/// The fixture's events.
pub fn record() -> Vec<SessionEvent> {
    let mut r = Recorder::new();
    let mut s = interactive(&mut r, "chat", "chat", Profile::Standard);

    open(
        &mut r,
        &mut s,
        "What can you see, and what does the brief say?",
        true,
    );
    ok(
        &mut r,
        &mut s,
        "cal_0001",
        "list_dir",
        args([("path", "")]),
        "Linked folders:\nArchive/ (read only)\nCamera/\nDesign/\nDrafts/\n",
    );
    ok(
        &mut r,
        &mut s,
        "cal_0002",
        "list_dir",
        args([("path", "Design")]),
        "Design:\nbrief.md  61 bytes  1970-01-01 00:00\nicons/\nnotes/\n",
    );
    let page = |from: u64| {
        Value::object([
            ("path", Value::from("Design/brief.md")),
            ("from_line", Value::from(from)),
            ("lines", Value::from(3u64)),
        ])
    };
    ok(&mut r, &mut s, "cal_0003", "read_file", page(1), "Design/brief.md, lines 1 to 3 of 5. Go on with from_line = 4.\nBrief\n\nGoal: a calmer home screen.\n");
    ok(
        &mut r,
        &mut s,
        "cal_0004",
        "read_file",
        page(4),
        "Design/brief.md, lines 4 to 5 of 5.\nOwner: Sam\nReview: Friday\n",
    );
    close(
        &mut r,
        &mut s,
        "I can see Design, Archive, Drafts and Camera. The brief's goal is a calmer home screen; Sam owns it and the review is Friday.",
    );

    open(&mut r, &mut s, "Where is the home icon mentioned?", false);
    let names = args([("path", "Design"), ("query", "hom")]);
    ok(
        &mut r,
        &mut s,
        "cal_0005",
        "search_files",
        names,
        "Design/icons/home.svg\nDesign/notes/homework.md\n",
    );
    let texts = args([("path", "Design"), ("query", "home"), ("in", "text")]);
    ok(&mut r, &mut s, "cal_0006", "search_files", texts, "Design/brief.md:3: Goal: a calmer home screen.\nDesign/notes/homework.md:1: Check the home icon.\n");
    close(
        &mut r,
        &mut s,
        "The brief mentions the home screen, and notes/homework.md says to check the home icon.",
    );

    open(
        &mut r,
        &mut s,
        "Read my hosts file, the logo and the roadmap.",
        false,
    );
    fails(
        &mut r,
        &mut s,
        "cal_0007",
        "read_file",
        args([("path", "/etc/hosts")]),
        "files.not_linked",
        "Permission denied: /etc/hosts is not a linked folder. Linked folders: Design, Archive, Drafts, Camera.",
    );
    fails(
        &mut r,
        &mut s,
        "cal_0008",
        "read_file",
        args([("path", "Design/../../etc/hosts")]),
        "files.outside_folder",
        "Paths cannot step out with `..`; name the place inside the folder.",
    );
    fails(
        &mut r,
        &mut s,
        "cal_0009",
        "read_file",
        args([("path", "Design/icons/logo.bin")]),
        "files.not_text",
        "Design/icons/logo.bin is not a text file.",
    );
    fails(
        &mut r,
        &mut s,
        "cal_0010",
        "read_file",
        args([("path", "Design/roadmap.md")]),
        "files.not_found",
        "Nothing is at Design/roadmap.md.",
    );
    close(
        &mut r,
        &mut s,
        "I can only read inside your linked folders, the logo is not a text file, and there is no roadmap in Design.",
    );
    r.finish()
}
