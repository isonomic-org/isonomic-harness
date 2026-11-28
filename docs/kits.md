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

A kit tool named in a preset's `tools.allow` is not offered that way: it
still has to pass the three filters. The context record lists the linked
folders' names when a folder tool is offered, and the model's context text
says "Linked folders: …", so the model need not spend a call to learn them.

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
   verdict refuses the call (`grant.revoked`). Only a denial refuses: a
   grant switched to "ask first" after the verdict does not ask again,
   because the verdict already allowed the call or the person answered it.

## Remembering

`decide_approval` takes an optional `remember`. With a verified user-action
token and an allow, the host holds it to the request's `remember` and, once
the loop has taken the decision, calls `Grants::remember`: `session` covers the same effect and scope in that
session only (a fork has its own id), `always` issues a standing grant
without "ask first", `once` changes nothing. A session approval answers a
standing grant's "ask first" and never outlives it: once the grant is
revoked, the approval allows nothing (part of the grants contract). Purge forgets the purged
sessions' approvals.

## The files kit

Seven tools over the host's `FileSystem` (`Deps.files`; `MemoryFs` is the
fake), specified in `harness_core::kits::FILES_SPECS`: `list_dir`,
`read_file` and `search_files` (core, parallel, reads), then `write_file`,
`edit_file`, `move_path` and `trash_path` (serial, `modifies_data`). Paths
are logical: a linked folder's label, then the path inside it. Errors are
the `files.*` codes in `harness_core::kits::files_errors`.

- **Resolving.** `access` resolves every path once, refuses what is not
  plainly inside a linked folder, read-only folders for changes, and
  protected names, and keeps the places for that session's call. The
  approval preview shows whole values: the path, the text of a write, the
  find and replace of an edit.
- **The service checks again.** A `Place` names the app. Whatever a caller
  puts in one, the service refuses a folder that is not linked for that
  app now, a change in a folder linked for reading only now, a link that
  leads out, and a protected place. Changes are tied to their call and can
  be undone (`FileSystem::undo`). `files::contract` is the suite; the
  engine's service runs it too.
- **Lines.** A line longer than 512 bytes is shown as several display
  lines, and every line number counts display lines: `read_file`'s header
  and `from_line`, `search_files` hits and `edit_file` results agree.
- **Results fit the cap** (at least 1 KiB): listings and reads page with
  `after` and `from_line`, and pages cover the content with no gap or
  overlap; search shows what fits and counts the rest.
- **`edit_file`** finds its place with the match ladder (`files::ladder`):
  exact text, then whole lines ignoring trailing spaces, then ignoring
  spaces at both ends, then with typographic punctuation folded. Each step
  must find exactly one place. The result says which step matched and
  shows the changed lines. Line endings are kept.
- **Result meta** (for the person's view, never the model): `scope` (the
  folder's id), `backend` (`host`), `path`, `to` for a move, and
  `pre_image` (the call id) for a change that can be undone.

Differences from the references are in `docs/kits-deviations.md`.

## Tests

- The verdict table row by row at each autonomy level, and every existing
  tool's verdict unchanged (harness-policy `verdict_tests`).
- The grants contract, passed by the fake (harness-toolkit `grants`).
- The offer table and admission (harness-core `offer_tests`), and the
  kits-offer fixture.
- The host flows with fake kit tools (harness-host `tests/kits.rs`).
- A snapshot of every spec (harness-core `tests/specs.rs`).
- The files contract, passed by the memory store (harness-toolkit
  `files::contract`); the tools over it (`tests/files.rs`); invariant 9
  over generated inputs (`tests/files_caps.rs`); R1's parity vectors
  (`tests/parity/R1/`); the two files fixtures replayed through the tools
  (`tests/files_fixture.rs`); and the kit through the host (harness-host
  `tests/files_kit.rs`).
