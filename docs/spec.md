# Isonomic Harness: specification

The binding source is the Milestone 0 brief. This file records the design as
built and every interpretation the brief left open. The event schema has its
own reference in `schema.md`; the host calls are in `host-api.md`; the
preset format is in `presets.md`.

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| harness-core | Ids, `SessionEvent` schema v1 and its JSON Schema, blocks, classes, errors, clock | serde |
| harness-log | `SessionStore`, JSONL generations, migrations, `derive_history()` | core |
| harness-loop | Turn and step driver, inbox, cancel, continue, retry, interceptors, context link, post-turn steps, subagent activation | core, log, tools, policy, preset, compact |
| harness-tools | Tool trait, kinds, scoped registry, guards, batches, client lifecycle | core |
| harness-toolkit | ask-user, todo, plan, time and context; service traits with fakes | core, tools |
| harness-preset | TOML presets and validation; chat, assistant-panel, test | core, policy, toml |
| harness-policy | Approvals, decision tokens, three-flag tracker, budgets, autonomy | core |
| harness-compact | Tool-result trimming, profile-gated summaries | core, log |
| harness-host | Registry, isolation, dispatch, enforcement, fan-out, crash isolation, recovery, purge; engine traits; MemoryHost | all of the above |
| harness-testkit | Scripted model, fake tools, manual clock, recorded fixtures | core |

No library crate has file, network or process access. The core crates are
not tied to one async runtime.

## Decisions taken as given

The sixteen design points of the brief, in short: the log is the truth; the
event schema is the main contract; turns and steps; three session classes on
one code path (interactive only in Milestone 0); interception points with a
built-in context link; tools with kinds and effects; standard tools one wave
at a time; deny-by-default policy with the three-flag rule and user-action
decision tokens; TOML presets; post-turn steps (title); trim-first
compaction; subagent activation trait with a fake; cancel, continue, retry;
a versioned interchange format; no I/O and no secrets; harness-host over
engine traits.
