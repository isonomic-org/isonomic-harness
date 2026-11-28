# Display widgets

A display widget is a tool the model calls to show a component inline: a
table, a step list, headline figures or a chart. The call has no side
effect. The host validates it, records it and tells the model `shown`. The
conversation kit renders the component, or the call's `fallback_text`, or a
generic block for a widget it does not know. This page is the harness side
of "Display widgets: spec v1".

Code: `harness_core::display` (specs, validator, gate), `harness-preset`
(`[display]`), `harness-loop` (`src/display.rs`), the `display` fixture.

## The starter widgets

Each spec is a strict JSON schema in `crates/harness-core/schema/display/`,
named `<widget>.v<major>.json`. Every schema is an object with
`additionalProperties: false`, requires `fallback_text` (1 to 4000
characters), and bounds every array and string.

| Widget | Required | Limits |
| --- | --- | --- |
| `show_table` | `columns`, `rows` | 1 to 8 columns, 1 to 50 rows, cells are strings of at most 120 characters; optional `sort {column, ascending}`, per-column `numeric` and `total`. |
| `show_steps` | `steps` | 1 to 12 steps, each with `title`, `state` (`done`, `current`, `pending`, `failed`), optional `detail` and up to 6 `substeps`. |
| `show_metrics` | `metrics` | 1 to 6 figures, each with `label`, `value` (a string), optional `change_percent` (-10000 to 10000), `lower_is_better`, `trend` (2 to 24 numbers) and `hint`. |
| `show_chart` | `kind`, `points` | `kind` is `bar`, `line` or `area`; 2 to 60 points of `label` (1 to 24 characters) and `value`; optional `series` and `unit`. |

Every widget also takes an optional `title` of at most 80 characters.

## Validation

`display::validate(name, input)` is pure and deterministic. It checks the
input against the schema, then the widget's cross-field rules.

The schema subset: `type` (`object`, `array`, `string`, `number`,
`integer`, `boolean`), `properties`, `required`, `additionalProperties:
false`, `items`, `enum`, `minItems`, `maxItems`, `minLength`, `maxLength`,
`minimum`, `maximum`. String lengths count characters, not bytes.

Cross-field rules:

- `show_table`: every row has as many cells as there are columns;
  `sort.column` names an existing column; `total` only on a `numeric`
  column.
- `show_steps`: at most one step is `current`.
- `show_chart`: point labels are unique.

A refusal is a `DisplayError` whose reason names the widget, the field and
the rule, for the model to correct, for example:

- `show_chart: fallback_text is required`
- `show_table: rows[2] has 3 cells; there are 4 columns.`
- `show_steps: steps[0].state must be one of done, current, pending, failed`
- `show_metrics: metrics[0] has a field "colour" that is not allowed`

A reason quotes at most `QUOTE_MAX` (80) characters of the input.

## Offering

A preset lists the widgets it offers in `[display] tools` (see
`docs/presets.md`); they are not listed in `tools.allow`. Each name must be
a known spec; a duplicate warns; more than 5 in an interactive preset is an
error. `enabled = false` turns them off. The `chat` preset (version 6)
offers all four.

The loop offers them only when `display_capable(profile, model)` holds:

| Profile | Offered |
| --- | --- |
| `high`, `standard`, `intel` | yes, except with `local:smallest` (`SMALLEST_MODEL`) |
| `low` | no |
| an unknown profile, or none | no |

The host also offers none when its model caps say the session's model does
not call tools (`ModelCaps::tools`), so an engine marks its smallest local
model that way; `local:smallest` is the id the gate knows today. The spec's
open point 3 expects this to move to a capability flag in the model
catalog.

`Config::offer_display` recomputes the set when the session starts, when the
model changes and when the offered tools change. Each widget is a tool of
kind `display`, with no effects and parallel concurrency. The four schemas
add about 700 tokens to a request.

## The loop

A display call skips the guard, policy and approval; it never reaches the
client, and an app's result for one is refused as `NotFound`. Its `tool.result` carries `display`:

| Input | Result | `display` |
| --- | --- | --- |
| valid | `ok`, content `shown` | `shown` |
| invalid, first time for this widget in the turn | `error` `display.invalid`, the reason | `retry` |
| invalid again in the same turn | `ok`, content `shown as text` | `fallback` |

Input that is not JSON counts as invalid. The retry is per widget per turn:
a bad table and a bad chart in one turn each get their own retry. The turn
continues in every case. On `fallback` the kit shows `fallback_text` if the
call has one, and a generic block if not.

## Versioning

The event schema went to revision 1 for display widgets, additively: tool
kind `display` and the `display` field on `tool.result` (`docs/schema.md`).
Each spec records the revision that added it (`since_revision`, 1 for all
four). After the v1 freeze a spec changes only additively; a breaking change
is a new widget name. A reader that meets a widget it does not know shows
its `fallback_text`, or a generic block.

## Fixture

`fixtures/display.jsonl` (built by `cargo xtask fixtures`):

- Turn 1: one valid call to each of the four widgets, all `shown`.
- Turn 2: a bad table (`retry`), the corrected table (`shown`), a bad chart
  (`retry`), the same bad chart again (`fallback`), and a call to
  `show_map`, as a newer host would record it, with no `fallback_text`, so
  readers can test an unknown widget.
