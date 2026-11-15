# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.
Entries for the Milestone 0 crates (step 0 through harness-host) are in
`status-milestone-0.md`.

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
