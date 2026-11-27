# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.
Entries for the Milestone 0 crates (step 0 through harness-host) are in
`status-milestone-0.md`.

## 2026-11-27 — Tools offered only where they can work

A tool can now decline a new session with `Tool::offered(app)`, true by
default; the host leaves a declined tool out of that session's registry
and out of the specs the model sees. The connection tools forward to a new
`Connections::offered(owner)`, also true by default, so an engine whose
owner has nothing connected no longer hands the model `connections`,
`connection_read` and `connection_act`. That also drops the preset's tools
prompt for such a session. Found on the Mac: with the three tools offered
and nothing connected, a 2B local model answered "plan one day in Lisbon"
by saying it had no travel planner connected. A session keeps the tools it
started with; one started after a connection is added gets them.

## 2026-11-27 — Connected-app tools in the toolkit

Replaces the entry of the same day that had the chat preset allow
`app_read` and `app_act`, tools the engine would have built itself. The
plan puts connected-app tools in harness-toolkit over the
`services::Connections` trait, so the toolkit now ships them:

- `connections` lists the owner's connections, by handle and label, with
  each operation's name, description, input schema and whether it only
  reads. Effect: reads private data.
- `connection_read` runs an operation that only reads. Effects: reads
  private data, ingests untrusted content. Policy lets it run, until the
  three-flag rule applies.
- `connection_act` runs any other operation. Effects as above plus acts
  outward, so a supervised session asks the person every time.
- Each call names a connection the owner holds and an operation it offers;
  an operation marked read-only must go through `connection_read` and any
  other through `connection_act`, else a readable `connection.denied`.
  Results over 16,000 characters are cut short.
- `services::Connections` gains `operations(owner, handle)`, a default
  method answering none, and the `Operation` type. Existing implementations
  keep compiling. `connected(service)` gives the three tools to register.
- The chat preset is version 5: it allows the three tools instead of
  `app_read` and `app_act`, and its tools paragraph names them.

No change to the event schema, the preset file format or the host API.

## 2026-11-16 — the model's window is hard

Closes the two limits listed in the entry below. Compaction logic only: no
change to the event schema, the preset file format or the host API.

- No request goes out larger than the model's window (its context less
  room for the reply). The preset's own budget stays soft, as before: when
  the context is unknown, or the request is over the budget but under the
  window, nothing changes.
- Against the window, where summaries are allowed: kept turns
  (`keep_turns`) are given up one at a time, so the summary replaces the
  fewest turns that make the request fit. The summary step's own request
  has to fit as well (the context less its 512-token output); when it does
  not, the next rule applies.
- Against the window, with no summary step (the Intel profile, a turn that
  already summarized, or a summary request that would not fit): the oldest
  turns are replaced in the request by the fixed note "Earlier messages
  are not shown to the model.", with no model step. It is recorded as an
  ordinary `compacted` whose `summary.text` is the note, so replay is
  unchanged. The log keeps every turn; only the request leaves them out.
  The note replaces an earlier summary's text.
- The system prompt and the current turn always stay. When they alone pass
  the window, nothing is sent: the turn ends `failed` with error code
  `context.too_large` and the reason "This message is too long for the
  model: it needs about N tokens and the model takes M. Shorten it and send
  it again." The next turn works; the oversized turn gives way like any
  other.
- Not covered: the title step's request, which is built from the first
  message alone and is not planned.
- Tests: `harness-loop/tests/window.rs` (Intel past the window; four kept
  turns of 2,000 tokens against 8,192; giving up one kept turn to a
  summary; a message that cannot fit; no known window).
- harness-compact: `Config` gains `context_tokens` and `reply_tokens`,
  `Plan` gains `TooLarge`, and `NOT_SHOWN` is the note. harness-loop:
  `Config::window_tokens()` and `Config::reply_tokens()`.

## 2026-11-16 — chat preset: short answers; compaction follows the window

From measurements on the Intel Mac: the first reply to a plain request ran
to about 3,000 characters and 91 to 109 seconds.

- The built-in chat preset is version 3. Its persona ends with: "Keep
  answers short: lead with the answer and use at most a few short
  paragraphs or a short list, unless the person asks for more detail."
  `max_output_tokens` is 1024 (was 4096), a backstop; a reply cut there can
  be continued.
- The compaction budget follows the model's real window for every session,
  not only one with a picked model: the host asks `ModelClient::caps` for
  the model the session runs on and passes its `context_tokens` to the
  loop. With the chat preset on an 8,192-token model the budget is 7,168
  (the window less the 1,024-token reply), not 24,000. A model that
  reports 0 leaves the preset's budget.
- Tests: `harness-loop/tests/picked_model.rs` (the chat preset at 8,192,
  with and without a picked model) and `harness-host/tests/window.rs` (a
  chat session on an 8,192-token model compacts and every request fits;
  on a roomy model nothing changes).
- No change to the preset file format, the event schema or any signature.
  Added for tests: `ScriptedModel::set_context`.
- Two limits found, not fixed here; each needs a decision:
  1. On the Intel profile the chat preset allows no summary step
     (`summaries = "profile"`), so a plain chat has nothing to compact:
     only tool results are trimmed. Once a conversation passes the window,
     requests go out over it.
  2. Where summaries are allowed, the preset keeps the last four turns
     whole (`keep_turns = 4`). When those four and the current turn are
     larger than the window, the request still goes out over it.
  One way to close both: against the window (not the preset's budget),
  let the planner replace the oldest turns with a fixed note and no model
  step when summaries are off, and give up kept turns one by one when they
  are on. It needs no schema change (`compacted` already carries a summary
  and its turns).

## 2026-11-16 — picked model: review follow-ups

No change to the event schema or to any host signature or type.

- Compaction fits the picked model. The budget was the preset's
  `budget_tokens` whatever the model. For a session with a picked model the
  host now passes that model's `ModelCaps::context_tokens` to the loop, and
  the budget is the smaller of the preset's and the context less room for
  the reply (the preset's `max_output_tokens`, or half the context when
  that is smaller). Sessions without a pick are unchanged. `ModelCaps` has
  no output limit of its own, so the request's limit stands in for it.
- The model name is trimmed, and the host refuses one over 200 bytes or
  with a control character: `HostError::Model`, code `model.name`, and no
  session is created. Reusing `HostError::Model` avoids a new error
  variant.
- `Session::create` and `Session::fork` trim the name too and drop a blank
  one, so `"model":""` is never written.
- The log is the source for a session's model. When a session loads and
  its registry row names a different model, or none, the row is rewritten
  from `session.start`. `list_sessions` reads rows without loading, so a
  wrong row shows until the session loads; `recover` loads them all.
- Session ids: the counter restarts with the host, so a restarted host
  could mint an id that exists (seen with a fork after a restart, which
  replaced the parent's slot). The host now reads the store's ids once, at
  its first mint, and never mints one it has seen, also after a purge.
- Tests: `harness-host/tests/model.rs` (a restart whose registry rows lack
  the model, a fork after a restart, name bounds, the Low profile with a
  `title_model`) and `harness-loop/tests/picked_model.rs` (blank and padded
  names, the budget, a summary step on the picked model).
- Correction to the consumer notes for the picked model: Chat does build a
  `SessionStart` literal, so it needs `model: None` when it moves its pin
  past `61438c9`.
- Added to harness-loop, not required of anyone: `Config.context_tokens`,
  `Config::with_context`, `Config::compaction_budget`, `picked_model`.

## 2026-11-15 — a picked model for a session

Schema and host API change, labelled `needs-master`. For the model picker.

- `CreateOptions.model: Option<String>`: when present (and not blank) it
  replaces the preset's reply model for that session. Absent, everything
  is as before.
- Recorded as a new optional field, `session.start`'s `model`. It is
  omitted when absent, so logs written before it decode, encode and digest
  as before, and every fixture is byte for byte unchanged. Replay never
  depended on it: each `step.start` already records its model.
- Resume reads the model from the log (`Session::resume` and the host's
  reload). A fork copies its parent's `session.start`, so it keeps the
  model.
- Visible as `SessionSummary.model` and `SessionMeta.model` (the override
  only; `None` means the preset's model).
- What runs on the picked model: reply steps, summary steps (they use the
  reply model) and the title step where it uses the reply model. A preset's
  own `title_model` (used on the Low profile) is not replaced.
- The host carries the name and checks nothing; the engine owns the model
  list and must check the name before it calls `create_session`.
- Types that changed: `SessionStart` (+`model`), `SessionMeta` (+`model`),
  `CreateOptions` (+`model`), `SessionSummary` (+`model`). Struct literals
  need the new field. Added: `harness_loop::Config::with_model`.
- Decision: `SCHEMA_REVISION` stays 0, as it did for the memory fields,
  because schema v1 is not frozen yet.
- Not built, design only: changing the model inside a session. A
  `session.model` event `{model}` would be written between turns by a new
  host call `set_model(caller, session, model)`, refused while a turn is
  open. The loop would use it from the next step, resume would take the
  latest one after `session.start`, and `SessionMeta.model` would follow.
  Replay needs nothing, since each `step.start` records its model. A fork
  would take the model in force at its fork point.

## 2026-11-15 — chat preset: no talk of tools when there are none

From the engine's runs on a real model: the chat persona's talk of tools,
approvals and asking back pushed small local models to refuse or ask to
clarify plain requests (about 1 reply in 6 on the smallest).

- The prompt section with the id `tools` is part of `system(preset)` only
  while the session offers tools. A session created with no tools gets the
  persona alone. When registering or removing tools changes that, the loop
  records a new `system(preset)`; requests already use the latest one, so
  replay is unchanged.
- The chat persona no longer says "ask when a request is unclear". It now
  says to take the most likely reading and answer it.
- The built-in chat preset is version 2. A session stored with
  `{chat, 1}` loads with the latest chat preset, as before.
- The request context: the preset says nothing about where it sits. The
  harness records it as its own `system(context)` and hands it to the model
  client as a separate `Part::Context`, so the engine placing it at the end
  of the system message conflicts with nothing here.
- No change to the preset file format, the session event schema or any
  signature. Added: `Preset::system_prompt_for(tools)` and `TOOLS_SECTION`.
  `Preset::system_prompt()` is unchanged (every section).
- Tests: `harness-loop/tests/preset_prompt.rs` and
  `harness-preset/tests/preset.rs`.

## 2026-11-15 — review follow-up for #37: tests and notes

Tests and documentation only; no code or contract change.

- Tests added in `harness-host/tests/pending.rs`: two sessions holding the
  same question id; a parent and its fork holding the same approval id and
  the same call id; a result for a call id never issued.
- Tests added in `harness-host/tests/updates.rs` for `next_update()`: a
  lagging subscriber backfilling, a subscriber resuming mid-log, polling
  then dropping, and the end after a purge.
- `docs/host-api.md`, under "Pending ids", now states three things for
  consumers: a result for a never-issued call id answers `Ok` with a
  `tool.result.late` audit record; `list_approvals` is ordered by
  (session, id); and shell-created sessions are owned by app id `shell`, so
  the engine must never grant that id to an app.

## 2026-11-15 — pending work keyed by (session, id) (#30), `next_update` (#32)

Host API change, labelled `needs-master`. No change to the session event
schema: `SCHEMA_VERSION`, `SCHEMA_REVISION`, the JSON Schema and every
fixture are untouched.

- Fixed: approval, question and tool-call ids are minted per session but
  were indexed host-wide by id alone, so two sessions collided (one of two
  approvals listed; deciding one decided the other app's). The host's three
  indexes and harness-policy's `Approvals` are now keyed by (session, id).
  `harness-host/tests/pending.rs` reproduces the collision for approvals
  and for client tool calls.
- Signatures that changed (the session goes before the id):
  - `Host::decide_approval(caller, session, id, decision, token)`;
  - `Host::answer_question(caller, session, id, answer)`;
  - `Host::client_tool_result(caller, session, call, outcome)`;
  - `Host::purge(caller, app)`: an app may name only itself (`NotFound`
    otherwise); the shell may now purge any named app (it was refused);
  - `ScopeResolver::verify_user_action(caller, token, session, approval)`:
    a user-action token is for one approval of one session. Without this a
    token minted for `apv_0001` in one session passed for `apv_0001` in
    another session of the same caller;
  - `Approvals::get(session, id)` and `Approvals::take(session, id)`;
  - `MemoryHost::user_action(caller, session, approval)`.
- Behaviour that changed: `client_tool_result` for a session the caller does
  not own answers `NotFound` (it used to be dropped silently); a late result
  is audited against the session named. `AlreadyDecided` is answered only to
  a caller that can see the session.
- Added: `Subscription::next_update()` and `Update::{Event, Delta}`. One
  call awaits a durable event or a live delta, each delta before the
  `assistant` event that replaces it. `next()` and `deltas()` are unchanged.
  `harness-host/tests/updates.rs` covers it.
- Decision: `next_update` yields one delta at a time, as #32 proposed, so
  each keeps its place among the events.

## 2026-11-15 — live deltas wake a subscriber (#32)

- Fixed: `Hub::delta` queued a `LiveDelta` on each feed without waking it,
  so a task waiting on `Subscription::next()` saw nothing until the next
  durable event. A queued delta now wakes the feed, as a durable event does.
- `next()` still returns durable events only. It is safe to drop between
  polls, so a waiter can poll it, drop it when pending and drain `deltas()`;
  no timer is needed. A unit test in `harness-host/src/events.rs` covers it.
- No type or signature change. Consumers only move their pin.
- Not done here: the one call that awaits either kind
  (`Subscription::next_update()`), which #32 also asks for. It adds to the
  host API, so it goes in a `needs-master` pull request.

## 2026-11-15 — continue after a title step

- Fixed: a first turn whose reply was cut short (output limit) ends done, so
  its title step runs inside the same turn. Continue then read the title's
  `assistant` entry as the turn's reply and refused with "The reply is
  complete." It now skips the entries of every step whose purpose is not
  `reply` (title and summary). `harness-loop/tests/resume_reply.rs`
  reproduces it.
- No schema, type or signature change. Consumers only move their pin.

## 2026-11-15 — memory schema pieces (spec approved, §10)

- "Spec: Isonomic memory" is approved. Its four §10 schema pieces are in
  schema v1, labelled `needs-master` because they change the frozen schema:
  - `memory_ref` block `{id, revision, hash}`; `hash` is a salted SHA-256
    over the store's salt and the revision's text, lowercase hex;
  - `memory` event `{op: created | revised | deleted, ref, scope: user | app
    | agent, evidence_seq?}`;
  - `ModelRequest.privacy` with `contains_memory`, set when a request
    carries a `memory_ref`, unless its `step.start` withholds memory;
  - `memory_policy` (`used`, `withheld`, `off`) on the context record
    (`system`, beside `memory`, the revisions placed in context) and on the
    request record (`step.start`).
  Every new field is omitted when empty, so existing logs encode and digest
  as before. The JSON Schema, `docs/schema.md`, the testkit validator (one
  hex `pattern` form) and chat-basic (a saved-memory turn) are updated.
- Types the SDK re-exports that changed: `Block` (+`MemoryRef`), `Event`
  (+`Memory`), `System` (+`memory`, `memory_policy`), `StepStart`
  (+`memory_policy`); new `MemoryRef`, `MemoryEvent`, `MemoryId`,
  `MemoryOp`, `MemoryScope`, `MemoryPolicy`, `PrivacyTag`. In harness-log:
  `Part::MemoryRef` and `ModelRequest.privacy`. Struct literals of `System`
  and `StepStart` need the new fields (`memory: Vec::new()`,
  `memory_policy: None`).
- Decision: the fixture's tool is `memory_write`, because tool names here are
  lowercase with `_`. The spec writes `memory.write`; the name is settled
  when the tools ship.
- The rest of the memory work is queued as issues #15–#20, one per pull
  request. Per spec §10 it starts only after the first end-to-end Chat turn
  runs in the desktop window on the Intel Mac (M-a).

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
