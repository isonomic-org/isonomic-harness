# Presets

A preset is a TOML file that composes a session. Apps choose settings only
through presets; reusable behaviour (titles, continue, context, keeping
refusals out of history) lives in the harness. Reference presets:
`crates/harness-preset/presets/` (`chat`, `assistant-panel`, `test`).

`harness_preset::parse` never fails. A preset with errors comes back with
no `Preset` and its diagnostics (a dotted key path, or `line:column` for a
syntax error), so the host keeps it listed. Warnings do not block. Unknown
keys are errors.

| Key | Default | Meaning |
| --- | --- | --- |
| `id`, `version` | required | Lowercase name; version 1 or more. The host namespaces app presets. |
| `name` | the id | Display name. |
| `class` | `interactive` | `interactive`, `autonomous`, `delegated`. The host refuses the last two in Milestone 0. |
| `[persona] name, prompt` | none | The persona's prompt is the first section of the system prompt. |
| `[[prompt]] id, text` | none | More sections, in order. `system(preset)` is every section joined by a blank line. The section with the id `tools` is included only while the session offers tools; when the offered tools change that, a new `system(preset)` is recorded and later requests use it. |
| `[model] id, max_output_tokens, reasoning, title_model` | `local:default`, 4096, `off`, none | `reasoning`: `off`, `low`, `medium`, `high`. `title_model`: the small model for titles on the Low profile. |
| `[tools] allow, client` | `[]`, `true` | Engine and toolkit tools by name; whether the app's client tools are offered. |
| `[limits] tokens, steps, wall_ms, cost_micros` | unlimited | Per turn. An app's preset may only tighten the host's ceiling. |
| `[policy] autonomy` | `supervised` | `strict` (any effect asks), `supervised`, `trusted` (outward runs unless the three-flag rule applies; code still asks). May only tighten the ceiling. |
| `[policy] allow_outward` | `[]` | Outward tools pre-approved; each must be in `tools.allow`. Never overrides the three-flag rule. |
| `[policy] approval_timeout_ms, client_timeout_ms, hang_ms, max_delegation_depth` | 600000, 60000, 300000, 1 | Approval expiry (a denial), client-gone timeout, the watchdog's hang limit, delegation depth (0 to 4). |
| `[context] link, date, language, location` | `true`, `true`, `true`, `opt_in` | The context link. `location`: `opt_in` (only when the person opted in) or `never`. |
| `[post_turn] title` | `auto` | `off`, `auto`, `model`, `heuristic`. `auto`: heuristic on Intel; `title_model` (or the heuristic) on Low; the reply model elsewhere. |
| `[compaction] budget_tokens, trim_over_bytes, head_bytes, tail_bytes, summaries, keep_turns` | 24000, 8192, 2048, 1024, `profile`, 4 | Trim first; `summaries`: `off`, `profile` (not on Intel), `on`. |
| `[[wake]] on, spec` | none | Autonomous only: `schedule`, `trigger` or `channel`, and its spec. |
