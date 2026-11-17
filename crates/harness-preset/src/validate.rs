// Copyright 2026 Anud Inc.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parsing and validation: every problem becomes a diagnostic with a path.

use crate::file::PresetFile;
use crate::*;
use std::collections::BTreeSet;

struct Check {
    out: Vec<Diagnostic>,
}

impl Check {
    fn error(&mut self, path: &str, message: impl Into<String>) {
        self.out.push(Diagnostic {
            severity: Severity::Error,
            path: path.into(),
            message: message.into(),
        });
    }

    fn warn(&mut self, path: &str, message: impl Into<String>) {
        self.out.push(Diagnostic {
            severity: Severity::Warning,
            path: path.into(),
            message: message.into(),
        });
    }

    fn pick<T: Clone>(&mut self, path: &str, value: &str, options: &[(&str, T)]) -> Option<T> {
        if let Some((_, v)) = options.iter().find(|(k, _)| *k == value) {
            return Some(v.clone());
        }
        let names: Vec<&str> = options.iter().map(|(k, _)| *k).collect();
        self.error(
            path,
            format!(
                "unknown value \"{value}\"; expected one of: {}",
                names.join(", ")
            ),
        );
        None
    }

    fn positive(&mut self, path: &str, v: i64) -> u64 {
        if v <= 0 {
            self.error(path, "must be greater than zero");
            0
        } else {
            v as u64
        }
    }

    fn limit(&mut self, path: &str, v: Option<i64>) -> Option<u64> {
        v.map(|v| self.positive(path, v))
    }
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        && s.as_bytes()[0].is_ascii_lowercase()
}

fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
    (line, col)
}

pub fn parse(text: &str) -> Parsed {
    let file: PresetFile = match toml::from_str(text) {
        Ok(f) => f,
        Err(e) => {
            let path = match e.span() {
                Some(span) => {
                    let (l, c) = line_col(text, span.start);
                    format!("{l}:{c}")
                }
                None => String::new(),
            };
            return Parsed {
                preset: None,
                diagnostics: vec![Diagnostic {
                    severity: Severity::Error,
                    path,
                    message: e.message().trim().to_string(),
                }],
            };
        }
    };
    let mut c = Check { out: Vec::new() };
    let preset = build(&mut c, file);
    c.out.sort_by_key(|d| d.severity != Severity::Error);
    let failed = c.out.iter().any(|d| d.severity == Severity::Error);
    Parsed {
        preset: if failed { None } else { Some(preset) },
        diagnostics: c.out,
    }
}

fn build(c: &mut Check, f: PresetFile) -> Preset {
    if !is_name(&f.id) {
        c.error(
            "id",
            "must be 1 to 64 lowercase letters, digits, - or _, starting with a letter",
        );
    }
    if f.version == 0 {
        c.error("version", "must be 1 or more");
    }
    let name = if f.name.trim().is_empty() {
        f.id.clone()
    } else {
        f.name.clone()
    };
    let class = c
        .pick(
            "class",
            if f.class.is_empty() {
                "interactive"
            } else {
                &f.class
            },
            &[
                ("interactive", SessionClass::Interactive),
                ("autonomous", SessionClass::Autonomous),
                ("delegated", SessionClass::Delegated),
            ],
        )
        .unwrap_or(SessionClass::Interactive);

    let mut prompt = Vec::new();
    if !f.persona.prompt.trim().is_empty() {
        prompt.push(PromptSection {
            id: "persona".into(),
            text: f.persona.prompt.clone(),
        });
    }
    let mut ids = BTreeSet::from(["persona".to_string()]);
    for (i, s) in f.prompt.iter().enumerate() {
        let path = format!("prompt[{i}]");
        if !is_name(&s.id) {
            c.error(&format!("{path}.id"), "must be a short lowercase name");
        } else if !ids.insert(s.id.clone()) {
            c.error(&format!("{path}.id"), format!("\"{}\" is used twice", s.id));
        }
        if s.text.trim().is_empty() {
            c.error(&format!("{path}.text"), "is empty");
        }
        prompt.push(PromptSection {
            id: s.id.clone(),
            text: s.text.clone(),
        });
    }
    if prompt.is_empty() {
        c.warn(
            "prompt",
            "no persona prompt or prompt sections; the system prompt is empty",
        );
    }

    if f.model.id.trim().is_empty() {
        c.error("model.id", "is empty");
    }
    if f.model.max_output_tokens == 0 {
        c.error("model.max_output_tokens", "must be greater than zero");
    }
    let reasoning = c
        .pick(
            "model.reasoning",
            &f.model.reasoning,
            &[
                ("off", Reasoning::Off),
                ("low", Reasoning::Low),
                ("medium", Reasoning::Medium),
                ("high", Reasoning::High),
            ],
        )
        .unwrap_or(Reasoning::Off);
    let model = ModelSettings {
        id: f.model.id.clone(),
        max_output_tokens: f.model.max_output_tokens,
        reasoning,
        title_model: Some(f.model.title_model.clone()).filter(|m| !m.trim().is_empty()),
    };

    let mut seen = BTreeSet::new();
    for (i, t) in f.tools.allow.iter().enumerate() {
        if !is_name(t) {
            c.error(
                &format!("tools.allow[{i}]"),
                format!("\"{t}\" is not a tool name"),
            );
        } else if !seen.insert(t.as_str()) {
            c.warn(
                &format!("tools.allow[{i}]"),
                format!("\"{t}\" is listed twice"),
            );
        }
    }
    let tools = ToolSettings {
        allow: f.tools.allow.clone(),
        client: f.tools.client,
    };

    let limits = Limits {
        tokens: c.limit("limits.tokens", f.limits.tokens),
        steps: c.limit("limits.steps", f.limits.steps),
        wall_ms: c.limit("limits.wall_ms", f.limits.wall_ms),
        cost_micros: c.limit("limits.cost_micros", f.limits.cost_micros),
    };

    let autonomy = c
        .pick(
            "policy.autonomy",
            &f.policy.autonomy,
            &[
                ("strict", Autonomy::Strict),
                ("supervised", Autonomy::Supervised),
                ("trusted", Autonomy::Trusted),
            ],
        )
        .unwrap_or_default();
    for (i, t) in f.policy.allow_outward.iter().enumerate() {
        if !f.tools.allow.contains(t) {
            c.error(
                &format!("policy.allow_outward[{i}]"),
                format!("\"{t}\" is not in tools.allow"),
            );
        }
    }
    let depth = f.policy.max_delegation_depth;
    if !(0..=4).contains(&depth) {
        c.error("policy.max_delegation_depth", "must be between 0 and 4");
    }
    let policy = PolicySettings {
        policy: Policy {
            autonomy,
            allow_outward: f.policy.allow_outward.clone(),
        },
        approval_timeout_ms: c.positive("policy.approval_timeout_ms", f.policy.approval_timeout_ms),
        client_timeout_ms: c.positive("policy.client_timeout_ms", f.policy.client_timeout_ms),
        hang_ms: c.positive("policy.hang_ms", f.policy.hang_ms),
        max_delegation_depth: depth.clamp(0, 4) as u32,
    };

    let location = c
        .pick(
            "context.location",
            &f.context.location,
            &[
                ("opt_in", LocationMode::OptIn),
                ("never", LocationMode::Never),
            ],
        )
        .unwrap_or(LocationMode::Never);
    let context = ContextSettings {
        link: f.context.link,
        date: f.context.date,
        language: f.context.language,
        location,
    };
    if !context.link && (f.context.date || f.context.language) {
        c.warn("context.link", "is off, so date and language are not added");
    }

    let title = c
        .pick(
            "post_turn.title",
            &f.post_turn.title,
            &[
                ("off", TitleMode::Off),
                ("auto", TitleMode::Auto),
                ("model", TitleMode::Model),
                ("heuristic", TitleMode::Heuristic),
            ],
        )
        .unwrap_or(TitleMode::Off);

    let k = &f.compaction;
    let summaries = c
        .pick(
            "compaction.summaries",
            &k.summaries,
            &[
                ("off", SummaryMode::Off),
                ("profile", SummaryMode::Profile),
                ("on", SummaryMode::On),
            ],
        )
        .unwrap_or(SummaryMode::Off);
    let compaction = CompactionSettings {
        budget_tokens: c.positive("compaction.budget_tokens", k.budget_tokens),
        trim_over_bytes: c.positive("compaction.trim_over_bytes", k.trim_over_bytes),
        head_bytes: c.positive("compaction.head_bytes", k.head_bytes),
        tail_bytes: c.positive("compaction.tail_bytes", k.tail_bytes),
        summaries,
        keep_turns: k.keep_turns.clamp(0, 1_000) as u32,
    };
    if k.keep_turns < 0 {
        c.error("compaction.keep_turns", "must not be negative");
    }
    if compaction.head_bytes + compaction.tail_bytes >= compaction.trim_over_bytes {
        c.error(
            "compaction",
            "head_bytes plus tail_bytes must be less than trim_over_bytes",
        );
    }

    let mut wake = Vec::new();
    for (i, w) in f.wake.iter().enumerate() {
        let path = format!("wake[{i}]");
        if !matches!(w.on.as_str(), "schedule" | "trigger" | "channel") {
            c.error(
                &format!("{path}.on"),
                format!(
                    "unknown value \"{}\"; expected one of: schedule, trigger, channel",
                    w.on
                ),
            );
        }
        if w.spec.trim().is_empty() {
            c.error(&format!("{path}.spec"), "is empty");
        }
        wake.push(WakeRule {
            on: w.on.clone(),
            spec: w.spec.clone(),
        });
    }
    if !wake.is_empty() && class != SessionClass::Autonomous {
        c.error("wake", "wake-up rules apply only to autonomous sessions");
    }
    if wake.is_empty() && class == SessionClass::Autonomous {
        c.warn(
            "wake",
            "an autonomous session with no wake-up rules never wakes",
        );
    }

    Preset {
        reference: PresetRef {
            id: PresetId::from(f.id.as_str()),
            version: f.version,
        },
        name,
        class,
        persona: Some(f.persona.name.clone()).filter(|n| !n.trim().is_empty()),
        prompt,
        model,
        tools,
        limits,
        policy,
        context,
        title,
        compaction,
        wake,
    }
}
