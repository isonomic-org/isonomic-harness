// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! The TOML file as written. Every section is optional and has defaults;
//! unknown keys are errors. Values are checked in `validate`.

use serde::Deserialize;

/// A preset file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PresetFile {
    pub id: String,
    pub version: u32,
    pub name: String,
    pub class: String,
    pub persona: PersonaFile,
    pub prompt: Vec<SectionFile>,
    pub model: ModelFile,
    pub tools: ToolsFile,
    pub display: DisplayFile,
    pub limits: LimitsFile,
    pub policy: PolicyFile,
    pub context: ContextFile,
    pub post_turn: PostTurnFile,
    pub compaction: CompactionFile,
    pub wake: Vec<WakeFile>,
}

/// `[persona]`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PersonaFile {
    pub name: String,
    pub prompt: String,
}

/// `[[prompt]]`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SectionFile {
    pub id: String,
    pub text: String,
}

/// `[model]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModelFile {
    pub id: String,
    pub max_output_tokens: u32,
    pub reasoning: String,
    pub title_model: String,
}

impl Default for ModelFile {
    fn default() -> Self {
        Self {
            id: "local:default".into(),
            max_output_tokens: 4096,
            reasoning: "off".into(),
            title_model: String::new(),
        }
    }
}

/// `[tools]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ToolsFile {
    pub allow: Vec<String>,
    pub client: bool,
}

impl Default for ToolsFile {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            client: true,
        }
    }
}

/// `[display]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct DisplayFile {
    pub tools: Vec<String>,
    pub enabled: bool,
}

impl Default for DisplayFile {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            enabled: true,
        }
    }
}

/// `[limits]`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsFile {
    pub tokens: Option<i64>,
    pub steps: Option<i64>,
    pub wall_ms: Option<i64>,
    pub cost_micros: Option<i64>,
}

/// `[policy]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PolicyFile {
    pub autonomy: String,
    pub allow_outward: Vec<String>,
    pub approval_timeout_ms: i64,
    pub client_timeout_ms: i64,
    pub hang_ms: i64,
    pub max_delegation_depth: i64,
}

impl Default for PolicyFile {
    fn default() -> Self {
        Self {
            autonomy: "supervised".into(),
            allow_outward: Vec::new(),
            approval_timeout_ms: 600_000,
            client_timeout_ms: 60_000,
            hang_ms: 300_000,
            max_delegation_depth: 1,
        }
    }
}

/// `[context]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ContextFile {
    pub link: bool,
    pub date: bool,
    pub language: bool,
    pub location: String,
}

impl Default for ContextFile {
    fn default() -> Self {
        Self {
            link: true,
            date: true,
            language: true,
            location: "opt_in".into(),
        }
    }
}

/// `[post_turn]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PostTurnFile {
    pub title: String,
}

impl Default for PostTurnFile {
    fn default() -> Self {
        Self {
            title: "auto".into(),
        }
    }
}

/// `[compaction]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CompactionFile {
    pub budget_tokens: i64,
    pub trim_over_bytes: i64,
    pub head_bytes: i64,
    pub tail_bytes: i64,
    pub summaries: String,
    pub keep_turns: i64,
}

impl Default for CompactionFile {
    fn default() -> Self {
        Self {
            budget_tokens: 24_000,
            trim_over_bytes: 8_192,
            head_bytes: 2_048,
            tail_bytes: 1_024,
            summaries: "profile".into(),
            keep_turns: 4,
        }
    }
}

/// `[[wake]]`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct WakeFile {
    pub on: String,
    pub spec: String,
}
