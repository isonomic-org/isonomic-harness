//! Isonomic Harness presets.
//!
//! A preset is a TOML file that composes a session: persona and prompt
//! sections, the model, tools, limits, policy profile, session class,
//! context settings, post-turn steps, compaction and, for autonomous
//! sessions, wake-up rules. Apps choose settings only through presets.
//!
//! [`parse`] never fails: a preset that does not validate is returned with
//! its [`Diagnostic`]s and no [`Preset`], so the host can keep it listed.

mod builtin;
mod file;
mod validate;

pub use builtin::{builtin, builtins, BUILTIN_IDS};

use harness_core::*;
use harness_policy::{Autonomy, Limits, Policy};

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The preset cannot be used.
    Error,
    /// The preset works, but probably not as meant.
    Warning,
}

/// One finding about a preset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// Where: a dotted key path such as `policy.autonomy`, or `line:column`
    /// for a TOML syntax error.
    pub path: String,
    /// What is wrong, in plain words.
    pub message: String,
}

/// The result of parsing a preset.
#[derive(Clone, Debug)]
pub struct Parsed {
    /// The preset, when there are no errors.
    pub preset: Option<Preset>,
    /// Everything found, errors first.
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// Whether any diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }
}

/// Parse and validate preset TOML.
pub fn parse(text: &str) -> Parsed {
    validate::parse(text)
}

/// The id of the prompt section that is included only while the session
/// offers tools.
pub const TOOLS_SECTION: &str = "tools";

/// A named section of the system prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptSection {
    /// Its id, unique in the preset.
    pub id: String,
    /// Its text.
    pub text: String,
}

/// The model settings.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelSettings {
    /// The model id for replies.
    pub id: String,
    /// The output limit.
    pub max_output_tokens: u32,
    /// The reasoning level.
    pub reasoning: Reasoning,
    /// The small model for the title step on slow profiles, if any.
    pub title_model: Option<String>,
}

impl ModelSettings {
    /// Request parameters for reply steps.
    pub fn params(&self) -> Params {
        Params {
            max_output_tokens: self.max_output_tokens,
            reasoning: self.reasoning.clone(),
            options: Default::default(),
        }
    }
}

/// Which tools the preset offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolSettings {
    /// Engine and toolkit tools, by name.
    pub allow: Vec<String>,
    /// Whether the owning app's client tools are offered.
    pub client: bool,
}

/// The policy profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicySettings {
    /// Autonomy and pre-approved outward tools.
    pub policy: Policy,
    /// How long an approval waits before it is a denial.
    pub approval_timeout_ms: u64,
    /// How long a client tool waits for its app.
    pub client_timeout_ms: u64,
    /// A step running longer than this is a hang; the watchdog seals it.
    pub hang_ms: u64,
    /// How deep delegation may go.
    pub max_delegation_depth: u32,
}

/// When the context link adds a location.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationMode {
    /// Only when the person opted in.
    OptIn,
    /// Never.
    Never,
}

/// The context link's settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextSettings {
    /// Whether the context link runs at all.
    pub link: bool,
    /// Add the date and time zone.
    pub date: bool,
    /// Add the language.
    pub language: bool,
    /// Location.
    pub location: LocationMode,
}

/// How the title step runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TitleMode {
    /// No title step.
    Off,
    /// Chosen by hardware profile.
    Auto,
    /// Always a model request.
    Model,
    /// Always the opening words of the first prompt.
    Heuristic,
}

/// The path the title step takes for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitlePath {
    /// No title step.
    Off,
    /// A short model request to this model.
    Model(String),
    /// The opening words of the first prompt, with no request.
    Heuristic,
}

/// When older turns may be summarized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryMode {
    /// Never; trimming only.
    Off,
    /// When the hardware profile allows it (not on Intel).
    Profile,
    /// Always allowed.
    On,
}

/// Compaction settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionSettings {
    /// Compact when a request's estimate exceeds this.
    pub budget_tokens: u64,
    /// Tool results longer than this are trimmed first.
    pub trim_over_bytes: u64,
    /// Bytes kept from a trimmed result's start.
    pub head_bytes: u64,
    /// Bytes kept from its end.
    pub tail_bytes: u64,
    /// When summaries are allowed.
    pub summaries: SummaryMode,
    /// Recent turns never summarized.
    pub keep_turns: u32,
}

impl CompactionSettings {
    /// Whether summaries are allowed on this profile.
    pub fn summaries_allowed(&self, profile: Option<&Profile>) -> bool {
        match self.summaries {
            SummaryMode::Off => false,
            SummaryMode::On => true,
            SummaryMode::Profile => !matches!(profile, Some(Profile::Intel)),
        }
    }
}

/// What wakes an autonomous session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WakeRule {
    /// `schedule`, `trigger` or `channel`.
    pub on: String,
    /// The schedule, trigger or channel, as the engine reads it.
    pub spec: String,
}

/// A validated preset.
#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    /// Its id and version.
    pub reference: PresetRef,
    /// Its display name.
    pub name: String,
    /// The session class it creates.
    pub class: SessionClass,
    /// The persona's name, if any.
    pub persona: Option<String>,
    /// Prompt sections in order; the persona's prompt comes first.
    pub prompt: Vec<PromptSection>,
    /// The model.
    pub model: ModelSettings,
    /// Tools.
    pub tools: ToolSettings,
    /// Budget limits.
    pub limits: Limits,
    /// The policy profile.
    pub policy: PolicySettings,
    /// The context link.
    pub context: ContextSettings,
    /// The title step.
    pub title: TitleMode,
    /// Compaction.
    pub compaction: CompactionSettings,
    /// Wake-up rules (autonomous sessions only).
    pub wake: Vec<WakeRule>,
}

impl Preset {
    /// Every prompt section, in order, separated by a blank line: the
    /// system prompt of a session that offers tools.
    pub fn system_prompt(&self) -> String {
        self.system_prompt_for(true)
    }

    /// The composed system prompt, the exact text of `system(preset)`. The
    /// section with the id [`TOOLS_SECTION`] is left out when the session
    /// offers no tools: talk of tools and approvals is noise there, and it
    /// makes small models hedge.
    pub fn system_prompt_for(&self, tools: bool) -> String {
        let parts: Vec<&str> = self
            .prompt
            .iter()
            .filter(|s| tools || s.id != TOOLS_SECTION)
            .map(|s| s.text.trim_end())
            .collect();
        parts.join("\n\n")
    }

    /// The title step's path on a hardware profile. With `auto`: on Intel
    /// the heuristic (about 3 tokens a second makes even a short request
    /// slow); on Low the small title model when the preset names one, else
    /// the heuristic; elsewhere the reply model.
    pub fn title_path(&self, profile: Option<&Profile>) -> TitlePath {
        let slow = matches!(profile, Some(Profile::Low));
        match self.title {
            TitleMode::Auto if matches!(profile, Some(Profile::Intel)) => TitlePath::Heuristic,
            TitleMode::Off => TitlePath::Off,
            TitleMode::Heuristic => TitlePath::Heuristic,
            TitleMode::Model => TitlePath::Model(
                self.model
                    .title_model
                    .clone()
                    .unwrap_or_else(|| self.model.id.clone()),
            ),
            TitleMode::Auto if slow => match &self.model.title_model {
                Some(m) => TitlePath::Model(m.clone()),
                None => TitlePath::Heuristic,
            },
            TitleMode::Auto => TitlePath::Model(self.model.id.clone()),
        }
    }

    /// Check an app's preset against the host's ceiling: limits and
    /// autonomy may only be tightened.
    pub fn check_ceiling(&self, ceiling: &Ceiling) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        let pairs = [
            ("limits.tokens", self.limits.tokens, ceiling.limits.tokens),
            ("limits.steps", self.limits.steps, ceiling.limits.steps),
            (
                "limits.wall_ms",
                self.limits.wall_ms,
                ceiling.limits.wall_ms,
            ),
            (
                "limits.cost_micros",
                self.limits.cost_micros,
                ceiling.limits.cost_micros,
            ),
        ];
        for (path, mine, max) in pairs {
            if let Some(max) = max {
                if mine.is_none_or(|m| m > max) {
                    out.push(Diagnostic {
                        severity: Severity::Error,
                        path: path.into(),
                        message: format!("may only tighten the host default of {max}"),
                    });
                }
            }
        }
        if rank(self.policy.policy.autonomy) > rank(ceiling.autonomy) {
            out.push(Diagnostic {
                severity: Severity::Error,
                path: "policy.autonomy".into(),
                message: "may only tighten the host's autonomy level".into(),
            });
        }
        out
    }

    /// This preset with its limits tightened to `ceiling` where it sets none
    /// or more.
    pub fn effective_limits(&self, ceiling: &Limits) -> Limits {
        self.limits.tighten(ceiling)
    }
}

/// The most an app's preset may allow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ceiling {
    /// Host default limits.
    pub limits: Limits,
    /// The highest autonomy level.
    pub autonomy: Autonomy,
}

fn rank(a: Autonomy) -> u8 {
    match a {
        Autonomy::Strict => 0,
        Autonomy::Supervised => 1,
        Autonomy::Trusted => 2,
    }
}
