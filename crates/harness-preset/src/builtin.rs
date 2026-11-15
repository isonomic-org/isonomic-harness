//! The reference presets, shipped with the library.

use crate::{parse, Parsed};

/// The built-in preset ids.
pub const BUILTIN_IDS: &[&str] = &["chat", "assistant-panel", "test"];

const TEXTS: &[(&str, &str)] = &[
    ("chat", include_str!("../presets/chat.toml")),
    (
        "assistant-panel",
        include_str!("../presets/assistant-panel.toml"),
    ),
    ("test", include_str!("../presets/test.toml")),
];

/// A built-in preset's TOML text and parsed form.
pub fn builtin(id: &str) -> Option<(&'static str, Parsed)> {
    TEXTS
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, text)| (*text, parse(text)))
}

/// Every built-in preset, in order.
pub fn builtins() -> Vec<(&'static str, Parsed)> {
    TEXTS.iter().map(|(_, text)| (*text, parse(text))).collect()
}
