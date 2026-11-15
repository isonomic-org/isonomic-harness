# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.

## 2026-11-15 — xtask replay, report and smoke

- `cargo xtask replay [fixture]` decodes each fixture's JSONL generation,
  resolves forks, and rebuilds every model request from the log alone. It
  prints a digest of all of a fixture's requests: 32 requests across the
  five committed fixtures and 1,609 in long-thread, with none failing.
- `cargo xtask report` measures the gate budgets in a release build; a
  debug xtask re-runs itself in release. Each figure is the median of five
  runs, measured in this session's Linux sandbox:
  - loop overhead: 0.095 ms per step (210 events, 35 steps);
  - host overhead over MemoryHost: 0.138 ms per step;
  - replaying 10,000 events (decode plus rebuild): 34.8 ms.
  The budgets are for the Intel Mac; Omar has the command to run it there.
- `cargo xtask smoke` runs one two-step turn with the `now` tool through
  the loop against the llama.cpp server that isonomic-inference's local
  provider launches (OpenAI-style chat completions on loopback, with a
  bearer secret). It checks that the turn ends done, that there are two or
  more steps, that the tool was called, and that replay rebuilt every
  request. It passes against a stand-in server here.
- Deviation: the smoke talks to the model server directly, not through
  isonomic-inference's provider crate. That crate lives in another private
  repository, and depending on it would tie this repository's CI to it. The
  server, the model and the wire format are the same.
- Gate 13 waits on one run on Omar's Mac: this sandbox has no model and no
  network to download one.

## 2026-11-15 — harness-host and MemoryHost

- The host feeds each session's sans-IO loop and owns every await. Outputs
  go to the session's outbox and are flushed in order. Events are appended
  to the `SessionStore`, then published. Model requests stream through
  `ModelClient` under the `Executor`. Engine tools run from the session's
  registry; client tools go through `ClientToolChannel`.
- Isolation is checked on every call. Another app's sessions, presets and
  artifacts answer `NotFound`; the shell sees everything.
- Approvals need a verified user-action token (`ScopeResolver`). Views and
  third-party code never decide. An ambiguous answer is a denial.
- Budgets per session and per app come from `step.end` usage and stop a
  turn with `BudgetStop`. Every tool call, result and approval leaves an
  audit record, read through `activity` (shell only).
- **Subscriptions** replay from the store, then switch to live events. Each
  has a bounded live queue; when the queue fills, the subscriber is marked
  lagging and catches up from the store. In a measured run of 100 turns, a
  stalled subscriber did not slow the loop (about 97 ms against 101 ms
  without one).
- **Crash isolation:**
  - A panic in a loop rebuilds that session from its log with
    `Session::fail`: the turn is sealed as failed and the session marked
    failed.
  - A panic in a tool or model task is reported by the executor, and the
    host seals the turn with `Input::Fail`.
  - A hang is a turn with no progress for `hang_ms` while it is not waiting
    on a person or a client app; the watchdog seals it the same way.
  - Other sessions never notice.
- **Recovery:** `recover()` loads every session; `Session::resume` seals
  interrupted turns.
- **Purge** cancels the app's in-flight turns, then deletes its logs and
  blobs, closes its subscriptions and drops its pending approvals,
  questions and client calls.
- MemoryHost is built from in-memory blobs, clients and scopes, a scripted
  model, a fixed locale, a runtime-free `MemorySpawner` and a
  `ManualClock`, with the published test helpers.
- Additions to harness-loop for this PR: `Input::Fail`, `Session::fail` and
  `Session::failed`.
- **Deviations (spelling or additive), listed in `host-api.md` under "As
  built":**
  - Calls take a resolved `Caller`.
  - `decide_approval` takes an optional token.
  - `get_artifact` takes no session.
  - `SessionStore::list_meta` takes the owner only.
  - There is a new `Locale` trait.
  - There is a new `HostError::InvalidTools`.
- `CreateOptions.location_opt_in` and `limits` are not persisted: a session
  reloaded after a restart has no location and the preset's limits. Next
  issue.

## 2026-11-15 — harness-toolkit

- Milestone 0 tools: `ask_user` (options as text or `{id, label}`; the
  answer becomes the result), `todo` (add, done, undo, remove, list),
  `plan` (steps and the step underway) and `now` (date and time in the
  person's zone, from a `LocalZone` the engine implements; the tool only
  formats). `toolkit(zone)` returns them ready to register.
- `todo` and `plan` keep per-session state in memory; `replay` rebuilds it
  from the session's own successful calls after a restart, so the log
  stays the only truth.
- Service traits with in-memory fakes, no tools yet: `ArtifactStore`
  (wave 3), `Egress` (search and URL reading, wave 4), `MemoryStore` (after
  its spec is approved; the narrowest shape: recall and remember per
  owner) and `Connections` (later; handles, never secrets).

## 2026-11-15 — harness-loop

- `Session` is a sans-IO state machine: `handle(now, Input) -> Vec<Output>`.
  Outputs are events to persist then publish, live deltas, model requests,
  tool runs and cancellations. The host does every await; no runtime is
  named.
- Each request is built with `request_for` right after its `step.start` is
  written, so the request made and the request replayed are the same bytes.
  Every loop test checks this for every request it made, along with
  contiguous seqs and exactly one result per call.
- Turns: an inbox queues input while a turn runs; the context link records
  `system(context)` (date as `Sunday, 15 November 2026`, time zone,
  language, and a location only when opted in and the preset allows it);
  `pre_step` denying the first step rejects the input with no step.
- Cancel and host budget stops work while streaming (the partial reply is
  sealed, the attempt recorded) and during tools (every outstanding call
  gets a result; pending approvals and questions are closed). Continue
  extends a sealed partial reply with a prefill and is refused after
  refusals, tool calls or complete replies. Transient errors retry the same
  request in the same step; refusals and errors are attempts only.
- Tools: calls are recorded with batch numbers, then guarded, judged by
  policy (approvals with expiry, the three-flag rule from sticky flags),
  passed through `pre_tool`, run, passed through `post_tool` and limited.
  `ask` outputs become questions; answers become results. Client tools
  whose app is gone put the session in *awaiting client* and time out
  readably; late results are refused.
- Compaction runs before each reply step (trims, then a summary step when
  allowed, at most once a turn). The title step runs after the first
  complete turn: a model step or the heuristic, by profile; a failed or
  refused title step falls back to the heuristic.
- Recovery: `Session::resume` seals an interrupted turn (open calls get
  results, the step ends, `turn.end(interrupted)`) and starts nothing.
  Forks start from the parent's resolved prefix.
- Subagents: the `Activation` trait, `FakeActivation`, and
  `check_delegation` (depth, tools and limits may only narrow).
- Decision: the context link is built in, not a chain link, so its output
  is always recorded the same way; the chain has `pre_step`, `request`,
  `pre_tool`, `post_tool` and `turn_stopping`.

## 2026-11-15 — harness-compact

- `plan` probes the reply step the loop is about to start: it rebuilds that
  request from the log (with harness-log's `request_for`) and compares its
  estimate with the budget. Over budget, it trims tool results longer than
  the threshold to head and tail, oldest first, until the request fits.
- Still over budget, it summarizes only when the profile allows it (the
  preset's `summaries_allowed`): the loop records the trims, runs a
  `summary` step, and records `summarized` with its text. The summary
  replaces completed turns not already summarized, except the most recent
  `keep_turns`; a summary step summarizes every completed turn, so kept
  turns are covered twice (verbatim and in the summary). Decision: this
  matches the published summary request, which carries no turn list.
- Over budget with nothing left to do, the step runs as is (the model's own
  limit applies).
- Tested: the `compacted` event recorded before a step reproduces the
  planned estimate exactly when the step's request is rebuilt from the log.

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
