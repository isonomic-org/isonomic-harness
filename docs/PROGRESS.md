# Progress

Order of work from the brief: core and schema, fixtures, log, tools, loop,
policy, presets, toolkit, compaction, host, report.

| # | Task | Branch | State |
| --- | --- | --- | --- |
| 0 | Step 0: repo, workspace, CI, allow-list | `step-0-scaffold` | merged (#1) |
| 1 | harness-core: ids, schema v1, JSON Schema | `core-schema-v1` | merged (#2) |
| 2 | Six recorded fixtures, `cargo xtask fixtures` | `fixtures-v1` | merged (#3) |
| 3 | `docs/host-api.md` | `docs-host-api` | merged (#4) |
| 4 | harness-log: SessionStore, JSONL generations, migrations, `derive_history()` | `log-v1` | merged (#5) |
| 5 | harness-tools | `tools-v1` | #6 open; CI blocked (Actions billing) |
| 6 | harness-loop | | |
| 7 | harness-policy | `policy-v1` | pull request open, stacked on #6 |
| 8 | harness-preset | | |
| 9 | harness-toolkit | | |
| 10 | harness-compact | | |
| 11 | harness-host and MemoryHost | | |
| 12 | `cargo xtask report`, `smoke`; the gate | | |

## Gate

| # | Item | State |
| --- | --- | --- |
| 1 | Replay reproduces every model request byte for byte, six fixtures | |
| 2 | Cancel mid-stream and mid-tool; resume; continue | |
| 3 | Fork at a turn boundary | |
| 4 | Rejected input, pre_tool denial, approvals, questions, tokens | |
| 5 | Three-flag rule, budget stop | |
| 6 | Compaction trims first; replay exact | |
| 7 | History rules, titles on both paths, context link logged | |
| 8 | v0 migrates to v1; future version refused | passing (harness-log tests) |
| 9 | Schema v1 frozen; JSON Schema and six fixtures published | published, not yet frozen |
| 10 | No-I/O allow-list; harness-core depends on serde only | passing in CI from Step 0 |
| 11 | MemoryHost isolation, purge, client-gone, crash isolation, fan-out | |
| 12 | `cargo xtask report` budgets | |
| 13 | Smoke test on a local model | |
