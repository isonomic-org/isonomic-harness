# Session event schema v1

The platform's main contract. Defined in `harness-core` (`SessionEvent`,
`Event`), with a JSON Schema at `crates/harness-core/schema/session-event.v1.json`
(also `harness_core::JSON_SCHEMA`). Version 1, revision 0.

Status: **published, not yet frozen.** It freezes at the Milestone 0 gate.
Until then any change is additive where possible and is listed in
`status.md`. After the freeze, changes are additive only.

## Versioning

- `SCHEMA_VERSION` (1) changes only with a migration, one version at a time.
  Readers refuse a newer version.
- `SCHEMA_REVISION` (0) counts additive changes: new event types, new block
  types, new optional fields, new enum values. Readers accept any revision
  of their version.
- **Every consumer must render what it does not know generically:** an
  unknown event as a generic, collapsible block; an unknown block likewise;
  an unknown enum value as its raw string. In Rust these arrive as
  `Event::Unknown`, `Block::Unknown` and each enum's `Other(String)`. A
  malformed known event is an error, never "unknown".
- The schema version travels in the SDK handshake and in the JSONL header.

## JSONL interchange

A generation is one UTF-8 file. Line 1 is the header:

```json
{"format":"isonomic.session-log","version":1,"revision":0}
```

Every further line is one event. A file may hold several sessions (a fork,
a delegated child); each line names its session, and each session's `seq`
runs 1, 2, 3, … in file order. Objects are written with sorted keys inside
`Value`s and in declaration order elsewhere, so the same events always give
the same bytes.

## Envelope

| Field | Type | Meaning |
| --- | --- | --- |
| `session` | id | The session. |
| `seq` | integer ≥ 1 | Position in the session, no gaps. Subscribers resume from the last `seq` they saw (0 for none). |
| `at` | integer | Persisted at, milliseconds since the Unix epoch, UTC. |
| `turn` | id, optional | The turn, for events inside one. |
| `step` | id, optional | The step, for events inside one. |
| `type` | string | The event type. |

Ids are opaque strings. Compare them; never parse them.

## Events

| Type | Payload | Notes |
| --- | --- | --- |
| `session.start` | `class`, `app`, `principal {kind, id?}`, `preset {id, version}`, `parent? {session, call}`, `profile?`, `model?` | Always seq 1. `model` is the model the person picked for the session, replacing the preset's; absent, the preset's model. |
| `session.status` | `status` (`active`, `awaiting_client`, `failed`), `reason?`, `calls?`, `deadline?` | Not in the brief's list; see below. |
| `forked` | `from`, `at_seq`, `after_turn?`, `reason` (`edit`, `regenerate`, `branch`) | Seq 2 of a fork. History = parent events 1..`at_seq`, then this log. |
| `system` | `origin` (`preset`, `context`, `notice`), `text`, `context? {date, time_zone, language, location?}`, `memory? [memory_ref]`, `memory_policy?` (`used`, `withheld`, `off`) | `text` is exactly what the model sees. On the context record, `memory` lists the revisions placed in context and `memory_policy` says whether memory was used. |
| `turn.start` | `cause` (`user`, `continue`, `wake`, `delegate`), `wake? {source, id}` | |
| `user` | `content` (blocks) | |
| `step.start` | `purpose` (`reply`, `title`, `summary`), `model`, `params {max_output_tokens, reasoning, options?}`, `memory_policy?` (`used`, `withheld`, `off`), `tools?` | `tools` appears when the tool set differs from the last step that had one. `memory_policy` is the request record's memory policy. |
| `assistant` | `content`, `stop` (`end`, `tool_use`, `max_tokens`, `cancelled`), `stream`, `usage?`, `continues?` | Tool calls follow as `tool.call`. |
| `attempt` | `outcome` (`error`, `cancelled`, `refused`), `error?`, `content?`, `usage?`, `retry?`, `will_retry?` | Never model history. |
| `tool.call` | `call`, `tool`, `kind` (`engine`, `client`, `cell`), `input`, `input_raw?`, `effects?`, `batch?`, `child?` | |
| `tool.result` | `call`, `outcome` (`ok`, `error`, `denied`, `timeout`, `cancelled`), `content?`, `error?`, `truncated?`, `duration_ms?` | Exactly one per call, even if the step fails. |
| `approval.request` | `approval`, `call?`, `summary`, `rule` (`acts_outward`, `executes_code`, `three_flag`, `autonomy`), `effects?`, `expires_at?` | |
| `approval.decision` | `approval`, `decision` (`allow`, `deny`), `basis` (`user`, `ambiguous`, `timeout`, `policy`, `cancelled`), `by {scope, app?}`, `answer?`, `reason?` | An ambiguous answer is `deny` with basis `ambiguous`. |
| `question` | `question`, `call?`, `prompt`, `options? [{id, label}]`, `multiple?`, `free_text?` | From the ask-user tool. |
| `answer` | `question`, `selected?`, `text?`, `skipped?`, `by` | |
| `artifact` | `artifact`, `version` (from 1), `op` (`create`, `update`), `title`, `media_type`, `blob`, `bytes`, `call?` | The body lives in the blob store. |
| `title` | `title`, `source` (`model`, `heuristic`, `user`) | Renaming is another title event. |
| `step.end` | `outcome`, `duration_ms`, `usage?` | |
| `turn.end` | `stop` (`done`, `cancelled`, `failed`, `budget`, `rejected`, `refused`, `interrupted`), `steps`, `duration_ms`, `usage`, `reason?`, `error?`, `budget? {kind, limit, used}` | |
| `compacted` | `through_seq`, `trimmed? [{call, original_bytes, head_bytes, tail_bytes}]`, `summary? {text, turns}`, `tokens_before`, `tokens_after` | `summary.text` is a summary step's text, or the fixed note "Earlier messages are not shown to the model." when turns were left out to fit the model's window with no model step. |
| `memory` | `op` (`created`, `revised`, `deleted`), `ref {id, revision, hash}`, `scope` (`user`, `app`, `agent`), `evidence_seq?` | One per change a session makes to the memory store. A reference, never the text. For `deleted`, `ref` is the revision that was current. |

### Blocks

`text {text}`, `reasoning {text, elapsed_ms?}`, `attachment {blob, name,
media_type, bytes, text?}`, `mention {target, id, label}`, `json {value}`,
`memory_ref {id, revision, hash}` (`hash` is a salted SHA-256 over the
store's salt and the revision's text, lowercase hex; the text stays in the
memory store, and the salt keeps a short text from being guessed).

### The compact stream

`assistant.stream` is a list of `[block, bytes, at_ms]`: `bytes` of UTF-8
were appended to `content[block]`, `at_ms` after the step started. Chunks are
coalesced (at most one every 100 ms; longer on slow profiles) and always
end on a character boundary.
Playing them in order rebuilds the reply at its original pace; a fixture
player synthesizes live deltas from them.

### Live deltas (not stored)

`LiveDelta {session, turn, step, after_seq, block, kind (text, reasoning,
tool_input), text}` on a separate, lossy channel. The durable `assistant`
event that follows replaces them.

## Order within a session

```
session.start                      seq 1
forked                             seq 2, forks only
system(preset)                     before the first turn
turn.start
  user                             cause user
  system(context)                  turns with user input, when the preset enables the context link
  step.start(reply)
    assistant | attempt+           attempts before a retried success
    tool.call*
    (approval.request approval.decision)?   per gated call, before it runs
    (question answer)?                      for ask-user
    session.status(awaiting_client)?        a client tool's app is gone
    artifact*                               written by a tool
    tool.result*                            completion order
  step.end
  … more steps
turn.end
step.start(title) assistant step.end title   post-turn, after turn.end
session.status(active)?                      the app reconnected
```

A turn with rejected input is `turn.start`, `user`, `turn.end(rejected)`.
A heuristic title is a lone `title` event after `turn.end`.

## Model history (derived)

`derive_history()` (harness-log) builds each request from the log alone:

- System prompt: the latest `system(preset)`. Context: each turn's
  `system(context)`, placed before that turn's user message.
- Included: `user`; `assistant` blocks of `reply` steps except reasoning;
  `tool.call` with its `tool.result`.
- Left out: `attempt` (errors, cancellations, refusals), turns that ended
  `rejected` or `refused`, and every `title` or `summary` step.
- A cancelled reply (`stop: cancelled`) stays, sealed with its partial text.
  A later `assistant` with `continues` extends it in place.
- `compacted` applies from its position on: trimmed results keep their first
  `head_bytes` and last `tail_bytes`; summarized turns are replaced by the
  summary text.
- A fork's history is the parent's through `at_seq`, then its own.
- The request's model, parameters and tools come from `step.start` (tools
  from the last `step.start` that carried them).
- A `memory_ref` block reaches the request as a reference. A request that
  carries one has the privacy tag `contains_memory` (`privacy` on the
  request; absent when empty, so untagged requests encode as before),
  unless its `step.start` records `memory_policy` `withheld` or `off`. The
  model gateway routes on it. Resolving references to text, and the
  `[memory deleted]` redaction for replay, come with the memory resolver.

## Decisions beyond the brief's list

The brief lists eighteen durable events. Two more are needed (and the memory
spec adds a third, below):

- **`session.start`** makes every log self-describing (class, owner app,
  principal, preset, parent, hardware profile), so an exported generation
  replays without the registry.
- **`session.status`** records *awaiting client* and *failed* in the log.
  The client-gone fixture and the crash-isolation gate need both, and the
  log is the truth.

Four additive pieces come from the approved "Spec: Isonomic memory" (§10),
added before v1 freezes so the memory tools can ship later with no schema
change: the `memory_ref` block (id, revision, salted hash), the `memory`
event, the `contains_memory` tag on model requests, and the memory policy
field (`used`, `withheld`, `off`) on the context record and the request
record. All optional fields are omitted when empty, so existing logs and
request digests are unchanged. Until the memory work starts (spec §10: after
the first end-to-end Chat turn in the desktop window), only the chat-basic
fixture produces them.

Refusals are `attempt` events with outcome `refused`: they are failed
requests, never model history, and cannot be regenerated. A cancelled reply
is kept as `assistant` (`stop: cancelled`) plus an `attempt` (`cancelled`)
recording the cut request. Post-turn steps run after `turn.end`, so a reply
is finished for the person before its title arrives.
