# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.

## 2026-11-15 — harness-preset

- TOML presets (`docs/presets.md`): persona and prompt sections, model,
  tools, limits, policy profile, class, context settings, the title step,
  compaction and wake-up rules. `parse` never fails: invalid presets come
  back with diagnostics (key path, or line and column) and no preset, so the
  host keeps them listed. Unknown keys are errors.
- Reference presets `chat`, `assistant-panel` and `test`, built in.
- `title_path` by profile: heuristic on Intel, the small title model (or
  the heuristic) on Low, the reply model elsewhere. `summaries_allowed`: not
  on Intel unless the preset says `on`.
- `check_ceiling`: an app's preset may only tighten the host's limits and
  autonomy; no limit where the host has one counts as loosening.
- Decision: harness-preset depends on harness-policy (for `Limits`,
  `Autonomy`, `Policy`) and on the `toml` crate (parsing only); both are on
  the allow-list. `docs/spec.md` updated.

## 2026-11-15 — harness-policy

- Deviation (order): the brief orders loop before policy, presets and
  compaction, but `docs/spec.md` has harness-loop depend on all three. I
  build policy, presets and compaction first, then the loop.
- Deviation (CI): from PR #6 on, no CI job starts. The run reports that
  the organization's Actions payments failed or its spending limit must be
  raised. Pull requests are stacked, each checked locally (fmt, clippy,
  test, `xtask deps`), and none merges until a re-run is green.
- `Caller` (shell, or an app with its kind), `may_decide`: the shell
  decides any approval; a first-party app only its own sessions' (another
  app's approval answers as not found); Views and third-party code never;
  every decision needs a verified user-action token.
- `Policy::evaluate`: deny by default for *acts outward* and *executes
  code*; the three-flag rule (private data and untrusted content both
  present: every outward action asks, pre-approval or not); `Flags` are
  sticky; Cell tools are refused. Autonomy levels: strict (any effect
  asks), supervised (the default), trusted (outward runs unless the
  three-flag rule applies; code still asks).
- `interpret`: a clear yes or no is the person's decision; anything else is
  a denial with basis `ambiguous`, keeping the answer as given.
- `Limits` (tokens, steps, wall time, cost; an app may only tighten) and
  `Meter`, checked before each step so a turn stops cleanly between steps;
  the host keeps a meter per turn, session and app.
- `Approvals`: the host's pending approvals, listed per owner, expired
  (denial with basis `timeout`) and dropped with their sessions.

## 2026-11-15 — harness-tools

- `Tool` (engine tools, async, runtime-agnostic), `ToolOutput` (done, or
  ask the person), `ToolDone` with readable failures, denials, timeouts and
  cancellations.
- `Registry`: scoped per session; client registrations are scoped to the
  registering app, replace its earlier set, refuse other kinds and refuse
  names held by others; Cell specs can be listed but calls are refused
  until Milestone 6.
- `guard`: unparseable input, non-object input, missing required fields and
  wrong top-level types become results the model can read; nothing runs.
- `plan_batches`: consecutive parallel calls share a batch; serial calls run
  alone. `limit_output`: long output keeps head and tail and records it.
- `ClientCalls`: an app going away gives its calls a deadline (awaiting
  client); calls past it time out readably; late results are refused; the
  app coming back clears deadlines.

## 2026-11-15 — harness-log

- `SessionStore` (async, runtime-agnostic, appends refuse gaps and
  overwrites) with `MemoryStore`; registry rows (`SessionMeta`) live in the
  same store.
- JSONL generations: `encode`, `decode`; future versions refused; one-step
  migrations on read (v0, the pre-release draft, to v1), never rewriting
  the input.
- `ModelRequest` with one canonical byte encoding, and `request_for`, which
  rebuilds any step's request from the log alone (reply, title and summary
  steps; forks through `resolve_fork`). Tested on every fixture: refusals,
  attempts and reasoning never reach history; continue is a prefill that
  extends the sealed reply; forks share only the prefix; tool results follow
  call order; compaction trims and summarizes from its position on.
- `harness_core::future`: boxed future and stream types and `fnv1a64`, std
  only, so no crate names an async runtime.

## 2026-11-14 — Fixtures published

- `harness-testkit`: manual clock, a deterministic recorder, the fixture
  tools, the six fixtures and a JSON Schema checker that refuses keywords it
  does not implement.
- `cargo xtask fixtures` writes `fixtures/*.jsonl` and `MANIFEST.json`.
  Tests check that the committed files and manifest match the generator,
  that every line validates against the JSON Schema and round-trips byte for
  byte, that every event type appears, that seqs have no gaps and that every
  tool call has exactly one result.
- Deviation: long-thread (10,000 events, about 2 MB) is generated, not
  committed, because of the 20 KB file rule and the connector. Its size and
  digest are in `MANIFEST.json` and checked by the tests.
- Deviation: the fixtures are scripted with the recorder, not recorded from
  harness-loop, which does not exist yet. They switch to recorded runs when
  the loop lands.
- The fixture data files (`fixtures/*.jsonl`, `MANIFEST.json`, `README.md`)
  were committed in the browser as @QadmiriOmar so their bytes match the
  generator exactly; code and docs went through the connector.

## 2026-11-14 — Schema v1 published (not yet frozen)

- `harness-core`: ids, `SessionEvent` and every payload, blocks, open enums,
  session classes, `ErrorInfo`, `Clock`, `LiveDelta`, `LogHeader`, and a
  JSON `Value` type, so the crate depends on serde only. `serde_json` is a
  dev-dependency for its tests.
- JSON Schema at `crates/harness-core/schema/session-event.v1.json`; a test
  checks that it names exactly the Rust event and block types.
- Forward compatibility: unknown event types, block types and enum values
  deserialize as `Unknown` / `Other` and round-trip; a malformed known event
  is an error.
- Deviation: two event types beyond the brief's eighteen, `session.start`
  and `session.status`; reasons in `schema.md`.
- Interpretations: refusals are `attempt(refused)`; post-turn steps run after
  `turn.end`; context additions are `system(context)` events.

## 2026-11-14 — Step 0

- Repository created as a private repository in isonomic-org, with a README.
- Workspace, `harness-core` placeholder, `xtask deps` and CI pushed through
  the connector on branch `step-0-scaffold`.
- CI calls the shared `rust-ci.yml`. The shared workflow has no `--locked`
  input, so this repository passes `--locked` through its `features` input,
  which reaches both clippy and test.
- The no-I/O allow-list runs as `cargo xtask deps` through the shared
  workflow's `xtask` input. It checks resolved dependencies transitively and
  scans each library crate's `src/` for file, network, process, environment
  and OS paths.
- Deviation: the repository could not be created through the connector (the
  integration lacks permission to create organization repositories). It was
  created in the browser as @QadmiriOmar.
- Deviation: the connector cannot write `.github/workflows/` (the integration
  lacks the workflows permission), so `ci.yml` was committed in the browser
  as @QadmiriOmar. Code and docs go through the connector.
