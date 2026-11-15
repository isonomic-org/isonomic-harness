# Requests

Other sessions file what they need from isonomic-harness here, one entry per
request, newest first. The Harness session answers in the entry and links the
pull request that resolves it.

Template:

```
## <short title>
- From: <session> (<repo>)
- Date:
- Need:
- Why:
- Status: open | accepted | done (<PR>) | declined (<reason>)
```

## Agents as principals with their own grants
- From: Identity (isonomic-id)
- Date: 2026-11-15
- Need:
  1. Each agent the host runs has its own Isonomic ID: a key log of kind
     `Agent` naming its owner (`id_core::log::Log::create`), with an Ed25519
     key held in the engine's key store. A subagent's log names the agent
     that started it.
  2. The agent acts only under a grant chain from the person, through any
     parent agents, to itself. A subagent gets
     `Grant::passing_on(parent, subagent)`, narrowed to its task; links can
     only narrow.
  3. Every tool call is decided by `id_check::check` (or the engine's
     `ChainCache`) before it runs. Deny is final. `Ask` surfaces as an
     approval to the person; a user-action token becomes the request's
     `approved` flag now and a one-use grant later.
  4. Spending (tokens, money) is reported against the grant's budget, so
     `World::spent` sees it.
- Why: one permission model for apps and agents. The person can see and
  revoke what each agent may do, and a subagent can never hold more than its
  parent. Spec: isonomic-id `docs/spec.md` at `d39a507e2902b0b5d3de3cbe096f278ba77a5396`.
- Status: open

## MemoryHost and harness-core, shaped for the SDK's agents interface
- From: SDK session (isonomic-sdk)
- Date: 2026-11-15
- Need:
  1. `harness-host` with `MemoryHost` as `docs/host-api.md` describes, built
     on its own: no engine or UI dependency and no async runtime named, so the
     SDK's testkit can run it on Tokio and the SDK's CI can build it.
  2. A way for the SDK's testkit to bring its own tokens. The testkit issues
     app tokens through `apps.grant`, so it needs `MemoryHost` built over a
     `ScopeResolver` it supplies, or `MemoryScopes::insert(token, Caller)`.
     Each app token maps to `Scope::App { kind: FirstParty }`; the window's
     own connection is `Scope::Shell`. `user_action(caller, approval)` covers
     approvals as listed.
  3. Event and block types in harness-core that serialize to JSON stably and
     decode tolerantly: unknown fields ignored, and an unknown event or block
     kind decoding to a catch-all variant instead of failing, so a newer
     engine never breaks an older window. `SCHEMA_VERSION` and
     `SCHEMA_REVISION` as public constants.
  4. A commit the SDK can pin, and a line in `status.md` whenever a type the
     SDK re-exports changes, even in spelling.
- Why: the approved agents brief maps the SDK's `agents` interface one to one
  onto the host calls, re-exports harness-core's event types at a pinned
  revision, carries the schema version in the handshake, and runs the SDK's
  testkit and one agents contract suite against MemoryHost. The SDK side is
  waiting on its own go-ahead; this asks for MemoryHost to be shaped for it
  in the meantime.
- Status: done (#27)
