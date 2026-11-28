# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.
Entries for the Milestone 0 crates (step 0 through harness-host) are in
`status-milestone-0.md`; later entries from 15 November and before are in
`status-2026-11-15.md`.

## 2026-11-28 — Tool kits: the frame

The shared frame every tool kit runs on, per "Tool kits: architecture
v0.3" (`docs/kits.md`); no kit ships yet. Deviations are in
`docs/kits-deviations.md`.

- **harness-core** (schema, additive, still revision 1): `modifies_data`,
  rule `grant`; `scope?` on tool specs; `scope?`, `preview?`,
  `reversible?`, `remember?` on `approval.request`; `remember?` on
  `approval.decision`; `meta?` on `tool.result`; `folders?` on the context
  record. `access` and `offer` modules; `display_capable` wraps the offer
  table. The revision stays 1, so every existing fixture replays byte for
  byte, header included.
- **harness-policy**: `Policy::judge` with an optional resolved access and
  grant answer; `evaluate` is `judge` with none, so every existing tool
  gets the verdict it got. `rememberable`.
- **harness-tools / harness-loop**: `Tool::kit`, `Tool::access`; the
  resolving state between waiting and the verdict.
- **harness-toolkit**: the `Grants` trait, `MemoryGrants`, the contract.
- **harness-preset**: `[kits] use`; an unknown kit fails at `kits.use[i]`.
  The test preset names `files`.
- **harness-host** (API change): `Deps.grants`; `decide_approval` takes
  `remember: Option<Remember>` before the token; the offer filters, resolve,
  the recheck at run, remember on decide, folders, forget on purge.
- **harness-testkit**: the kits-offer fixture and fake kit specs.
## 2026-11-28 — No display tools for a model that does not call tools

- **harness-loop**: `Config::calls_tools` (true by default) and
  `with_tool_calls`; `offer_display` offers none when it is false.
- **harness-host**: `model_config` sets it from `ModelCaps::tools`, so an
  engine whose smallest model cannot call tools reports `tools: false` for
  it and that model is offered no display tools on any profile.
  `ScriptedModel::without_tools` for tests. No schema change.

## 2026-11-28 — A display call is the host's to answer

- **harness-host**: `client_tool_result` for a display call is `NotFound`
  (an app renders display calls and never answers them; the wire answers
  `not_found`). Before, it was dropped as a late result. No schema change.

## 2026-11-28 — Display widgets (revision 1)

The model can show a component inline: `show_table`, `show_steps`,
`show_metrics` and `show_chart`, per "Display widgets: spec v1"
(`docs/display.md`).

- **harness-core** (schema revision 0 to 1, additive): `ToolKind::Display`;
  optional `display` on `tool.result` (`shown`, `retry`, `fallback`), absent
  on every other call; `display::{DisplaySpec, DISPLAY_SPECS, display_spec,
  display_tool_spec, validate, display_capable}`; the schemas as files in
  `crates/harness-core/schema/display/`. Logs without display calls keep
  every event line byte for byte; only the header's `revision` reads 1.
- **No new dependency.** harness-core keeps serde alone: a small strict
  reader turns the shipped schema text into `Value` (`display/json.rs`),
  and the validator covers the subset the spec names.
- **harness-preset:** `[display] tools, enabled`. The built-in `chat`
  preset is version 6 (the spec said 4; 5 had shipped since) and offers the
  four starters. An unknown name is an error, more than five entries in an
  interactive preset is an error, and a duplicate warns.
- **harness-loop:** `Config::offer_display` puts the display tools after
  the others when the preset enables them and `display_capable(profile,
  model)` holds; it runs again when the model is picked and when client
  tools change. A display call never asks, never reaches the host, and
  records no duration. One retry per widget per turn, counted from the
  turn's events, so a resumed session counts the same.
- **Fixture `display`.** Two tests that measure the context window now use
  the chat preset without display tools: the four schemas add about 700
  tokens to each request on a capable model.
- **Downstream:** `ToolResult` gains a field, so code that builds one with
  a struct literal adds `display: None`.

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

