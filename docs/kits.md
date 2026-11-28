# Tool kits: the frame

Per "Tool kits: architecture v0.3". This page covers what every kit shares;
each kit adds its own section when it ships. Differences from the
references are in `kits-deviations.md`.

## Pieces

| Piece | Where |
| --- | --- |
| `Effect::ModifiesData`, `ApprovalRule::Grant`, `ScopeKind`, `Remember` | harness-core `kinds` |
| `Scope`, `PreviewItem`, `Access`, `GrantAnswer`, `HeldGrant`, `grant_effect` | harness-core `access` |
| The offer table (`offer`), `admit`, `KITS`, `KitMember` | harness-core `offer` |
| The verdict (`Policy::judge`, `Resolved`) and `rememberable` | harness-policy |
| `Tool::kit`, `Tool::access`, `ToolCtx::result_cap`, `ToolDone::meta` | harness-tools |
| The resolving state (`Output::Resolve`, `Input::Resolved`) | harness-loop `resolve` |
| `Grants`, `MemoryGrants`, `Standing`, `grants::contract` | harness-toolkit |
| `[kits] use` | harness-preset |
| Offer filters, resolve, recheck, remember, folders, forget | harness-host `kits` |

## Offering

A kit tool is a tool whose spec has a `scope` and whose `kit()` names one
of `files`, `shell`, `web`, `browser`, `computer`. It is never offered by
`tools.allow`. A new interactive session is offered the preset's kits'
tools that pass three filters:

1. **Capability.** The offer table, by profile and model:

   | Profile | Display tools | Kit tools | Result cap |
   | --- | --- | --- | --- |
   | high | 5 | 20 | 16 KiB |
   | standard | 5 | 12 | 8 KiB |
   | intel | 5 | 4 | 2 KiB |
   | low, unknown, none | 0 | 0 | none |

   The smallest local model, and a model whose caps say it cannot call
   tools, get nothing. These are starting values, to be measured on the
   Intel machine.
2. **Grants.** A tool passes only when the app holds a grant (standing, or
   remembered for this session) for its act on its scope kind. No grant,
   no tool.
3. **Budget.** Core tools first, kit by kit in the preset's order, then the
   rest in the same order, up to the table's count.

The context record lists the linked folders' names when a folder tool is
offered.

## A call

1. The loop puts the call in the **resolving** state and asks the host.
2. The host runs the tool's `access`: what it will touch, a preview,
   whether it can be undone, the longest remember, whether it is confined.
   A failure, a missing access, or a scope of the wrong kind is the call's
   result (`kit.no_access`, `kit.scope`, or the tool's own code), never an
   approval.
3. The host checks the grants for the act (`grant_effect`): allow, ask, or
   deny with a reason.
4. Policy judges the call on its effects (narrowed only by a confined
   access, never widened), the session's flags and the grant answer, in
   this order: cell, grant denied, three-flag rule, strict autonomy,
   unconfined code, outward action, unconfined change to data that is not
   both granted and reversible, else allow.
5. An ask emits `approval.request` with the scope, preview, `reversible`
   and `remember`: `once` for the three-flag rule, code, autonomy, an
   irreversible act or no access; otherwise the access's.
6. At run the host checks the grants again. A grant revoked since the
   verdict refuses the call (`grant.revoked`).

## Remembering

`decide_approval` takes an optional `remember`. With a verified user-action
token and an allow, the host holds it to the request's `remember` and calls
`Grants::remember`: `session` covers the same effect and scope in that
session only (a fork has its own id), `always` issues a standing grant
without "ask first", `once` changes nothing. Purge forgets the purged
sessions' approvals.

## Tests

- The verdict table row by row at each autonomy level, and every existing
  tool's verdict unchanged (harness-policy `verdict_tests`).
- The grants contract, passed by the fake (harness-toolkit `grants`).
- The offer table and admission (harness-core `offer_tests`), and the
  kits-offer fixture.
- The host flows with fake kit tools (harness-host `tests/kits.rs`).
- A snapshot of every spec (harness-core `tests/specs.rs`).
