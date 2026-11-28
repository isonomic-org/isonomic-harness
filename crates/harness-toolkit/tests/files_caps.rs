// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Invariant 9 over generated inputs: a files result never exceeds its
//! cap, and the pages of a read or a listing cover the whole content with
//! no gap and no overlap. Inputs come from a small seeded generator: long
//! and multibyte lines, every line ending, long names and paths, many
//! entries, and caps from the smallest up.

use harness_core::*;
use harness_toolkit::files::contract::wait;
use harness_toolkit::files::{files_kit, MemoryFs, MIN_CAP};
use harness_tools::{Tool, ToolCtx, ToolDone, ToolOutput};
use std::sync::Arc;

/// A small deterministic generator (xorshift).
struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> usize {
        (self.next() % n) as usize
    }

    fn word(&mut self, max: usize) -> String {
        const ALPHABET: &[&str] = &[
            "a", "b", "Z", "é", "漢", "🙂", " ", "\t", "-", "\u{2014}", ".",
        ];
        let len = self.below(max as u64 + 1);
        (0..len)
            .map(|_| ALPHABET[self.below(ALPHABET.len() as u64)])
            .collect()
    }

    fn content(&mut self) -> String {
        let mut out = String::new();
        for _ in 0..self.below(60) {
            let max = if self.below(6) == 0 { 2000 } else { 60 };
            out.push_str(&self.word(max));
            out.push_str(["\n", "\n", "\r\n", "\r"][self.below(4)]);
        }
        if self.below(2) == 0 {
            out.push_str(&self.word(40));
        }
        out
    }

    fn name(&mut self) -> String {
        let max = if self.below(4) == 0 { 240 } else { 20 };
        let n: String = self
            .word(max)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        format!("n{}{n}", self.below(1_000_000))
    }

    fn cap(&mut self) -> usize {
        [MIN_CAP, 1500, 2048, 8192, 16_384][self.below(5)]
    }
}

fn run(tools: &[Arc<dyn Tool>], n: u32, name: &str, input: Value, cap: usize) -> ToolDone {
    let t = tools.iter().find(|t| t.spec().name == name).unwrap();
    let cx = ToolCtx {
        session: SessionId::from("s1"),
        app: AppId::from("chat"),
        call: CallId::from(format!("c{n}").as_str()),
        now: 0,
        child: None,
        result_cap: Some(cap),
    };
    let done = match wait(t.access(&input, &cx).unwrap()) {
        Err(d) => d,
        Ok(_) => match wait(t.call(&input, &cx)) {
            ToolOutput::Done(d) => d,
            ToolOutput::Ask(_) => panic!("files tools never ask"),
        },
    };
    let size: usize = done
        .content
        .iter()
        .map(|b| match b {
            Block::Text { text } => text.len(),
            _ => 0,
        })
        .sum();
    assert!(size <= cap, "{name}: {size} bytes over a cap of {cap}");
    done
}

fn body(d: &ToolDone) -> String {
    d.content
        .iter()
        .map(|b| match b {
            Block::Text { text } => text.clone(),
            _ => String::new(),
        })
        .collect()
}

fn s(v: &str) -> Value {
    Value::from(v)
}

#[test]
fn read_pages_fit_the_cap_and_cover_the_file_exactly() {
    let mut g = Gen(0x9E37_79B9_7F4A_7C15);
    for case in 0..300 {
        let fs = Arc::new(MemoryFs::new());
        fs.link("chat", "f1", "Design", false);
        let dir = if g.below(3) == 0 {
            g.name()
        } else {
            "d".into()
        };
        let path = format!("{dir}/{}", g.name());
        let content = g.content();
        fs.put("f1", &path, &content);
        let tools = files_kit(fs);
        let cap = g.cap();
        let mut from = 1u64;
        let mut seen = String::new();
        let mut n = 0;
        loop {
            n += 1;
            let input = Value::object([
                ("path", s(&format!("Design/{path}"))),
                ("from_line", Value::from(from)),
            ]);
            let page = body(&run(&tools, n, "read_file", input, cap));
            let (header, text) = page.split_once('\n').unwrap();
            seen.push_str(text);
            match header.split("from_line = ").nth(1) {
                Some(next) => {
                    let next: u64 = next.trim_end_matches('.').parse().unwrap();
                    assert!(next > from, "case {case}: a page must move on");
                    from = next;
                }
                None => break,
            }
        }
        assert_eq!(seen, content, "case {case}");
    }
}

#[test]
fn listings_fit_the_cap_and_cover_every_entry_once() {
    let mut g = Gen(0xD1B5_4A32_D192_ED03);
    for case in 0..120 {
        let fs = Arc::new(MemoryFs::new());
        fs.link("chat", "f1", "Design", false);
        let mut names: Vec<String> = (0..g.below(120)).map(|_| g.name()).collect();
        names.sort();
        names.dedup();
        for name in &names {
            fs.put("f1", &format!("d/{name}"), "x");
        }
        fs.mkdir("f1", "d");
        let tools = files_kit(fs);
        let cap = g.cap();
        let mut after = String::new();
        let mut rows = 0;
        let mut n = 0;
        loop {
            n += 1;
            let input = Value::object([("path", s("Design/d")), ("after", s(&after))]);
            let page = body(&run(&tools, n, "list_dir", input, cap));
            rows += page.lines().filter(|l| l.contains(" bytes  ")).count();
            match page.split("Call again with after = ").nth(1) {
                Some(rest) => {
                    let next = rest.trim_end().trim_end_matches(" to go on.").to_owned();
                    assert!(next > after, "case {case}: a page must move on");
                    after = next;
                }
                None => break,
            }
        }
        assert_eq!(rows, names.len(), "case {case}");
    }
}

#[test]
fn the_linked_folders_page_in_one_order_whatever_order_they_were_linked_in() {
    let mut g = Gen(0x2545_F491_4F6C_DD1D);
    for case in 0..40 {
        let fs = Arc::new(MemoryFs::new());
        let mut labels: Vec<String> = (0..1 + g.below(150)).map(|_| g.name()).collect();
        labels.sort();
        labels.dedup();
        // Link them in an order that is not the labels' order.
        let mut order: Vec<usize> = (0..labels.len()).collect();
        for i in (1..order.len()).rev() {
            order.swap(i, g.below(i as u64 + 1));
        }
        for i in order {
            fs.link("chat", &format!("f{i}"), &labels[i], false);
        }
        let tools = files_kit(fs);
        let cap = g.cap();
        let mut after = String::new();
        let mut seen = Vec::new();
        let mut n = 0;
        loop {
            n += 1;
            let input = Value::object([("path", s("")), ("after", s(&after))]);
            let page = body(&run(&tools, n, "list_dir", input, cap));
            seen.extend(
                page.lines()
                    .skip(1)
                    .filter_map(|l| l.strip_suffix('/'))
                    .map(str::to_owned),
            );
            match page.split("Call again with after = ").nth(1) {
                Some(rest) => {
                    let next = rest.trim_end().trim_end_matches(" to go on.").to_owned();
                    assert!(next > after, "case {case}: a page must move on");
                    after = next;
                }
                None => break,
            }
        }
        // A long label is shown cut, so compare what is shown.
        assert_eq!(seen.len(), labels.len(), "case {case}");
        for (shown, label) in seen.iter().zip(&labels) {
            let shown = shown.strip_suffix("...").unwrap_or(shown);
            assert!(label.starts_with(shown), "case {case}: {shown}");
        }
    }
}

#[test]
fn every_other_result_fits_the_cap() {
    let mut g = Gen(0x2545_F491_4F6C_DD1D);
    for _ in 0..150 {
        let fs = Arc::new(MemoryFs::new());
        for i in 0..g.below(40) + 1 {
            fs.link(
                "chat",
                &format!("f{i}"),
                &format!("L{i}{}", g.name()),
                i % 2 == 1,
            );
        }
        let label = fs_label(&fs);
        let mut paths = Vec::new();
        for _ in 0..g.below(30) + 1 {
            let path = format!("{}/{}", g.name(), g.name());
            fs.put("f0", &path, &g.content());
            paths.push(path);
        }
        let tools = files_kit(fs);
        let cap = g.cap();
        let path = format!("{label}/{}", paths[0]);
        let query = g.word(3000);
        let find = g.word(8);
        let cases: Vec<(&str, Value)> = vec![
            ("list_dir", Value::object([("path", s(""))])),
            (
                "read_file",
                Value::object([("path", s(&format!("/{query}")))]),
            ),
            (
                "write_file",
                Value::object([("path", s(&format!("Nowhere/{query}")))]),
            ),
            (
                "search_files",
                Value::object([("path", s(&label)), ("query", s("n"))]),
            ),
            (
                "search_files",
                Value::object([("path", s(&label)), ("query", s(&query))]),
            ),
            (
                "search_files",
                Value::object([("path", s(&label)), ("query", s("a")), ("in", s("text"))]),
            ),
            (
                "read_file",
                Value::object([("path", s(&format!("{path}/missing/{query}")))]),
            ),
            (
                "edit_file",
                Value::object([
                    ("path", s(&path)),
                    ("find", s(&find)),
                    ("replace", s(&query)),
                ]),
            ),
            (
                "edit_file",
                Value::object([("path", s(&path)), ("find", s("a")), ("replace", s("b"))]),
            ),
            (
                "write_file",
                Value::object([("path", s(&path)), ("text", s(&query))]),
            ),
            (
                "move_path",
                Value::object([("from", s(&path)), ("to", s(&format!("{path}2")))]),
            ),
            (
                "trash_path",
                Value::object([("path", s(&format!("{path}2")))]),
            ),
        ];
        for (n, (name, input)) in cases.into_iter().enumerate() {
            run(&tools, n as u32, name, input, cap);
        }
    }
}

fn fs_label(fs: &MemoryFs) -> String {
    use harness_toolkit::files::FileSystem;
    fs.linked(&AppId::from("chat"))[0].label.clone()
}
