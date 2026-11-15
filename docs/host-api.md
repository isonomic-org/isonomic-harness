# harness-host API

Every call the host serves, the traits the engine implements, and
MemoryHost. The SDK's `agents` interface maps onto the calls one to one; the
engine's `crates/agents` implements the traits and serves the calls over the
broker after checking the token.

Status: **built** (`crates/harness-host`, pull request #12). The call set,
who may call what, and the semantics below are the contract. Where the
build differs from the first publication, "As built" at the end says so;
every such change is also in `status.md`.

## Conventions

- Every call takes a `Caller`, which the host gets from the capability token
  through `ScopeResolver`. A caller is the **shell** (sees every app) or an
  **app**, which is first-party, a View or third-party.
- **Isolation.** An app can list, open, drive and purge only its own
  sessions, presets and artifacts. Another app's id answers `NotFound`, so an
  app cannot learn what exists outside its scope. The shell sees everything.
- All calls are async and runtime-agnostic: futures come from the traits; the
  host never names an async runtime.
- Event types are harness-core's (`SessionEvent`, `LiveDelta`, `Block`,
  `ToolSpec`, …), re-exported by the SDK. The SDK handshake carries
  `harness_core::SCHEMA_VERSION` and `SCHEMA_REVISION`; `Host::schema()`
  returns both.

## Calls

| SDK call | Host method | Who may call | Effect |
| --- | --- | --- | --- |
| `sessions.create(preset, scope)` | `create_session(caller, preset, opts)` → `SessionId` | app (own presets and built-ins), shell | Class comes from the preset. Writes `session.start` and `system(preset)`. Milestone 0 refuses autonomous and delegated classes with `ClassNotSupported`. |
| `sessions.list(filter)` | `list_sessions(caller, filter)` → `Page<SessionSummary>` | app (own), shell (all) | Registry read. |
| `sessions.open(id)` | `open_session(caller, id)` → `SessionSnapshot` | owner app, shell | Summary plus what is pending; events come from `subscribe`. |
| `sessions.fork(id, at)` | `fork_session(caller, id, at, reason)` → `SessionId` | owner app | `at` must be a turn boundary, else `NotAtTurnBoundary`. The child starts with `session.start` and `forked`; it shares the parent's prefix and is independent from then on. |
| `sessions.archive(id)` | `archive_session(caller, id, archived)` | owner app, shell | Registry flag; the log is untouched. |
| `sessions.rename(id)` | `rename_session(caller, id, title)` | owner app, shell | Writes `title` with source `user`. |
| `sessions.send(id, input)` | `send(caller, id, input)` → `Accepted { queued }` | owner app | Queues input in the inbox; a turn opens when it is claimed. Attachment blobs must belong to the caller's app. |
| `sessions.continue(id)` | `continue_reply(caller, id)` | owner app | Opens a `continue` turn that extends the last sealed partial reply. Refused (`ContinueRefused`) after tool calls or a refusal, when there is nothing to continue, or when the model cannot take an assistant prefix. |
| `sessions.cancel(id)` | `cancel(caller, id)` | owner app, shell | Stops the turn at its next await point. A streaming reply is sealed with its partial text, the request is recorded as `attempt(cancelled)`, every open tool call gets a `cancelled` result, and the turn ends `cancelled`. Idempotent. |
| `sessions.purge(app)` | `purge(caller, app)` → `PurgeReport` | app (naming itself only), shell (any app) | Stops the named app's in-flight turns, then deletes its sessions' logs, attachments and artifacts. Keys, settings and every other app's data are untouched. An app that names another app gets `NotFound`. |
| `sessions.subscribe(id, from_seq)` | `subscribe(caller, id, from_seq)` → `Subscription` | owner app, shell | Durable events with `seq > from_seq`, then live ones, with no gaps or duplicates; plus a lossy stream of `LiveDelta`. `Subscription::next_update()` awaits either. See "Events". |
| `approvals.list(scope)` | `list_approvals(caller)` → `Vec<PendingApproval>` | app (own sessions), shell (all) | Pending `approval.request`s. |
| `approvals.decide(session, id, decision, user_action_token)` | `decide_approval(caller, session, id, decision, token)` | shell; the owning first-party app | See "Approvals" and "Pending ids". Writes `approval.decision`. |
| `questions.answer(session, id, answer)` | `answer_question(caller, session, id, answer)` | owning app, shell | Writes `answer`, then the ask-user call's `tool.result`. `NotFound` when that session has no such open question. |
| `tools.register_client(session, specs)` | `register_client_tools(caller, session, specs)` | owner app | Registers `client` tools for that session and app only; replaces that app's earlier set. Specs of any other kind are refused. |
| `tools.result(session, call_id, outcome)` | `client_tool_result(caller, session, call, outcome)` | the app that registered the tool | Writes the call's `tool.result`. A late result after a timeout is dropped and audited against that session. `NotFound` when the session is not the caller's. |
| `presets.list()` | `list_presets(caller)` → `Vec<PresetSummary>` | app (own and built-in), shell | Invalid presets stay listed with their diagnostics. |
| `presets.get(id)` | `get_preset(caller, id)` → `Preset` | app (own and built-in), shell | TOML text, parsed form, diagnostics. |
| `presets.put(preset)` | `put_preset(caller, toml)` → `PresetRef` | app (own namespace) | Validates and stores a new version. Limits and policy may only be tightened from the host defaults. |
| `artifacts.get(id, version)` | `get_artifact(caller, id, version)` → `ArtifactVersion` | owner app, shell | Metadata from the log, body from `BlobStore`. |
| `artifacts.list(session)` | `list_artifacts(caller, session)` → `Vec<ArtifactMeta>` | owner app, shell | |
| `activity.list(filter)` *(proposed)* | `activity(caller, filter)` → `Page<AuditRecord>` | shell | The audit record of every tool call and approval, for the Activity tab. The SDK session decides whether it rides on `agents`. |

### Engine-only calls (not in the SDK)

| Method | Called by | Effect |
| --- | --- | --- |
| `Host::new(deps, config)` | engine start | Builds the host over the engine's traits. |
| `recover()` | engine start, before serving | Every turn without a `turn.end` gets `turn.end(interrupted)` (a streaming reply is sealed first); sessions resume. |
| `client_connected(app)` / `client_disconnected(app)` | the broker | Disconnect moves sessions with that app's open client calls to `session.status(awaiting_client)`; those calls time out under policy as readable failures. Reconnect writes `session.status(active)`. |
| `shutdown()` | engine stop | Cancels in-flight turns, flushes, stops executors. |

## Arguments and results

```rust
pub struct Caller { pub scope: Scope, pub principal: Principal }
pub enum Scope { Shell, App { app: AppId, kind: AppKind } }
pub enum AppKind { FirstParty, View, ThirdParty }

pub struct CreateOptions { pub title: Option<String>, pub location_opt_in: bool,
    pub limits: Option<Limits> }               // may only tighten the preset's
pub struct SessionFilter { pub archived: Option<bool>, pub preset: Option<PresetId>,
    pub text: Option<String>, pub cursor: Option<String>, pub limit: u32 }
pub struct SessionSummary { pub id: SessionId, pub app: AppId, pub class: SessionClass,
    pub preset: PresetRef, pub title: Option<String>, pub status: Status,
    pub archived: bool, pub forked_from: Option<(SessionId, Seq)>,
    pub created_at: Millis, pub updated_at: Millis, pub last_seq: Seq }
pub struct SessionSnapshot { pub summary: SessionSummary, pub turn_open: Option<TurnId>,
    pub approvals: Vec<ApprovalId>, pub questions: Vec<QuestionId>,
    pub awaiting_client: Vec<CallId> }
pub enum ForkPoint { BeforeTurn(TurnId), AfterTurn(TurnId) }
pub struct Input { pub content: Vec<Block>, pub client_ref: Option<String> }
pub enum DecisionInput { Allow, Deny, Text(String) }   // Text that is not a clear yes is a denial
pub struct AnswerInput { pub selected: Vec<String>, pub text: Option<String>, pub skipped: bool }
pub enum ClientOutcome { Ok(Vec<Block>), Error(ErrorInfo, Vec<Block>) }
pub struct PurgeReport { pub sessions: u64, pub events: u64, pub blobs: u64 }
```

### Errors

`HostError`: `NotFound` (also for another app's ids), `Unauthorized`,
`ClassNotSupported`, `InvalidPreset(diagnostics)`, `NotAtTurnBoundary`,
`ContinueRefused(reason)`, `NotAllowedToDecide`, `MissingUserAction`,
`AlreadyDecided`, `SessionFailed`, `Store(..)`, `Model(..)`. Errors inside a
turn are not call errors: they are recorded in the log (`attempt`, a
`tool.result` with outcome `error`, `turn.end(failed)`).

## Approvals

- Policy is enforced here and only here: deny by default for *acts outward*
  and *executes code*; the three-flag rule (once private data and untrusted
  content have both entered a session, every outward action asks; the
  untrusted flag is sticky); budgets per session and per app; autonomy
  levels.
- A decision needs a **user-action token**, minted by the engine's UI broker
  when a person acts and checked through `ScopeResolver`. No token:
  `MissingUserAction`, and nothing is written. The token is for one approval
  of one session: `verify_user_action` takes both.
- The shell's approvals sheet may decide any approval. A first-party app may
  decide only for its own sessions. Views and third-party code may request
  approvals and never decide them (`NotAllowedToDecide`).
- An ambiguous answer is a denial: `approval.decision` with decision `deny`
  and basis `ambiguous`, keeping the answer as given. An expired request is a
  denial with basis `timeout`.
- A denied call gets a `tool.result` with outcome `denied` and a reason the
  model can read.

### Pending ids

Approval, question and tool-call ids are minted per session (`apv_0001`,
`qst_0001`, `cal_0001`), and a fork continues its parent's counters. So an
id alone does not name one pending item: two sessions regularly hold the
same id. Everything pending is keyed by **(session, id)**, in the host and
in harness-policy's `Approvals` index, and `decide_approval`,
`answer_question` and `client_tool_result` take the session. A
`PendingApproval` carries its `session`; `SessionSnapshot` lists ids for its
own session only.

## Events

- **Persist, then publish.** An event reaches subscribers only after
  `SessionStore::append` returns.
- `subscribe(from_seq)` first replays `seq > from_seq` from the store, then
  switches to live events with no gap or duplicate at the seam. A resuming
  subscriber passes the last `seq` it saw.
- Each subscriber has a bounded queue. When it fills, that subscriber drops
  back to reading from the store until it catches up; the loop never waits
  for a subscriber.
- `LiveDelta`s go on a separate lossy channel, dropped first under pressure.
  The durable `assistant` event replaces them. A queued delta wakes the
  task waiting on `Subscription::next()`, as a durable event does.
- `Subscription::next_update()` awaits either kind and returns
  `Update::Event(SessionEvent)` or `Update::Delta(LiveDelta)`. Durable
  events keep their order with no gaps or duplicates; deltas stay lossy. A
  delta comes after the event its `after_seq` names and before the next
  one, so it never follows the `assistant` event that replaces it.

## Crash isolation and recovery

Each session's loop runs behind `Executor`. A panic, or a hang caught by the
watchdog under policy, seals that turn (`turn.end(failed)` with the error),
writes `session.status(failed)` and leaves the engine and every other session
running. Milestone 0 ships an in-process executor; the trait lets loops move
into a supervised worker process later.

## Traits the engine implements

```rust
pub trait SessionStore: Send + Sync {
    /// Appends after the session's last seq, atomically; refuses a gap or
    /// an overwrite. The engine refuses updates and deletes except purge.
    fn append(&self, s: &SessionId, events: &[SessionEvent]) -> BoxFuture<'_, Result<(), StoreError>>;
    fn read(&self, s: &SessionId, after: Seq, limit: usize) -> BoxFuture<'_, Result<Vec<SessionEvent>, StoreError>>;
    fn put_meta(&self, meta: &SessionMeta) -> BoxFuture<'_, Result<(), StoreError>>;
    fn list_meta(&self, owner: Option<&AppId>, filter: &SessionFilter) -> BoxFuture<'_, Result<Page<SessionMeta>, StoreError>>;
    fn purge(&self, sessions: &[SessionId]) -> BoxFuture<'_, Result<u64, StoreError>>;
}
pub trait ModelClient: Send + Sync {
    fn caps(&self, model: &str) -> ModelCaps;          // prefill, reasoning, tools, context size
    fn stream(&self, req: ModelRequest, cancel: CancelToken) -> BoxStream<'_, Result<ModelChunk, ModelError>>;
}
pub trait ClientToolChannel: Send + Sync {
    fn invoke(&self, app: &AppId, s: &SessionId, call: &ToolCall) -> BoxFuture<'_, Result<(), ChannelError>>;
    fn is_connected(&self, app: &AppId) -> bool;
}
pub trait ScopeResolver: Send + Sync {
    fn resolve(&self, capability: &CapabilityToken) -> Result<Caller, HostError>;
    fn verify_user_action(&self, caller: &Caller, token: &UserActionToken, session: &SessionId, approval: &ApprovalId) -> bool;
}
pub trait BlobStore: Send + Sync {
    fn put(&self, owner: &AppId, s: &SessionId, media_type: &str, bytes: Vec<u8>) -> BoxFuture<'_, Result<BlobId, StoreError>>;
    fn get(&self, id: &BlobId) -> BoxFuture<'_, Result<Blob, StoreError>>;   // carries owner app and session
    fn delete_for(&self, sessions: &[SessionId]) -> BoxFuture<'_, Result<u64, StoreError>>;
}
pub trait Spawner: Send + Sync {
    fn spawn(&self, task: BoxFuture<'static, ()>);
    fn sleep(&self, ms: u64) -> BoxFuture<'static, ()>;
}
pub use harness_core::Clock;   // now_ms()
pub trait Executor: Send + Sync {  // host-provided default; the engine may supply its own later
    fn run(&self, session: SessionId, task: BoxFuture<'static, ()>) -> RunHandle;   // panics and hangs reported, never propagated
}
```

`ModelRequest` is what `derive_history()` builds from the log (system
prompt, messages, tools, model, parameters) and has one canonical byte
encoding, the one replay compares. The Desktop adapter maps it onto the
inference protocol crate's conversation type. `SessionStore` rows are one per
event: session, seq, schema version, payload; JSONL generations from
harness-log are the export format.

## MemoryHost

`harness_host::MemoryHost` is a `Host` over in-memory implementations of
every trait: `MemoryStore`, `MemoryBlobs`, `MemoryClients`, `MemoryScopes`,
a runtime-free `MemorySpawner`, `ManualClock`, and a `ModelClient` you supply
(the testkit's scripted model by default). It is a public, documented type;
the SDK's testkit exposes it through the SDK client API.

Test helpers on `MemoryHost`: `app_token(app, kind)`, `shell_token()`,
`user_action(caller, session, approval)`, `disconnect(app)`, `reconnect(app)`,
`advance(ms)`, `inject_panic(session)`, `inject_hang(session)`, and
`log(session)` to read a session's events directly.

## As built

Differences from the first publication, all in spelling or additive:

- **Calls take a resolved `Caller`.** The engine resolves the capability
  token with `ScopeResolver::resolve` and passes the `Caller`. `Caller` has
  `shell()` and `app(name, kind)` constructors.
- **`decide_approval(caller, session, id, decision, token: Option<UserActionToken>)`.**
  `None` answers `MissingUserAction`, the same as an unverified token.
- **Pending work is named by (session, id).** `decide_approval`,
  `answer_question` and `client_tool_result` take the session before the
  id, and `ScopeResolver::verify_user_action` takes the session before the
  approval. In harness-policy, `Approvals::get` and `Approvals::take` take
  `(session, id)`.
- **`purge(caller, app)`** names the app. An app may name only itself; the
  shell may name any app.
- **`Subscription::next_update()`** and the `Update` enum: one call that
  awaits a durable event or a live delta.
- **`get_artifact(caller, artifact, version: Option<u32>)`.** The latest
  version when `None`.
- **`activity(caller, app: Option<AppId>)`.** Filters by app; shell only.
- **`Subscription::next()`** returns the next durable event, or `None` after
  a purge. `Subscription::deltas()` drains the lossy live deltas. A delta
  wakes the waiter but `next()` stays pending; the future can be dropped
  between polls without losing an event, so a waiter polls it, drops it
  when pending, and drains `deltas()`.
- **`SessionStore::list_meta(owner)`** takes the owner only; the host applies
  `SessionFilter`.
- **New trait `Locale`** (in `Deps`): the request context for a session
  (date, time zone, language, and a location only when the person opted in).
  The engine owns these values, and the context link records them.
- **`Host::new(Deps, HostConfig)`.** `HostConfig` holds the preset ceiling,
  per-session and per-app limits, the engine tools, the subscriber queue
  size, the watchdog period and the hardware profile.
- **New error `HostError::InvalidTools(reason)`** for a refused client tool
  registration.
- **`Host::tick()`** runs the watchdog and deadlines. A background task also
  calls it every `watchdog_ms`.
- **MemoryHost** also has `insert_token(token, caller)` (and
  `MemoryScopes::insert`) for tokens issued elsewhere, such as the SDK
  testkit's `apps.grant`. It has `caller(token)`, `run()` and
  `block_on(fut)`, and it exposes its parts as public fields (`store`,
  `model`, `clients`, `scopes`, `blobs`, `spawner`, `clock`). The scripted
  model is `ScriptedModel`, with `text`, `calls` and `push`.
- **A hang** is a turn that makes no progress for the preset's `hang_ms`
  while it is not waiting on a person or a client app. A **panic** inside a
  loop rebuilds that session from its log with the turn sealed as failed.

## Versioning of this API

Additive changes only once the SDK's `agents` interface ships: new calls,
new optional fields, new error variants (callers must handle unknown ones).
The SDK conformance suite runs against both MemoryHost and the engine host.
