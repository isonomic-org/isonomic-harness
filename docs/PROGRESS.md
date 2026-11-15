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
| 6 | harness-loop | `loop-v1` | pull request open, stacked on #9 |
| 7 | harness-policy | `policy-v1` | pull request open, stacked on #6 |
| 8 | harness-preset | `preset-v1` | pull request open, stacked on #7 |
| 9 | harness-toolkit | `toolkit-v1` | pull request open, stacked on #10 |
| 10 | harness-compact | `compact-v1` | pull request open, stacked on #8 |
| 11 | harness-host and MemoryHost | `host-v1` | pull request open, stacked on #11 |
| 12 | `cargo xtask replay`, `report`, `smoke`; the gate | `report-v1` | pull request open, stacked on #13 |

## Gate

| # | Item | State |
| --- | --- | --- |
| 1 | Replay reproduces every model request byte for byte, six fixtures | `cargo xtask replay` rebuilds all 1,643 requests; loop tests show each request made equals its replay. Re-recording the fixtures through the loop is open |
| 2 | Cancel mid-stream and mid-tool; resume; continue | passing locally (harness-loop tests) |
| 3 | Fork at a turn boundary | passing locally (harness-loop tests) |
| 4 | Rejected input, pre_tool denial, approvals, questions, tokens | passing locally (harness-loop, harness-host tests) |
| 5 | Three-flag rule, budget stop | passing locally (harness-loop tests) |
| 6 | Compaction trims first; replay exact | passing locally (harness-compact, harness-loop tests) |
| 7 | History rules, titles on both paths, context link logged | passing locally (harness-log, harness-loop tests) |
| 8 | v0 migrates to v1; future version refused | passing (harness-log tests) |
| 9 | Schema v1 frozen; JSON Schema and six fixtures published | published, not yet frozen |
| 10 | No-I/O allow-list; harness-core depends on serde only | passing in CI from Step 0 |
| 11 | MemoryHost isolation, purge, client-gone, crash isolation, fan-out | passing locally (harness-host tests; slow subscriber measured) |
| 12 | `cargo xtask report` budgets | passing locally: 0.095 and 0.138 ms per step, 34.8 ms replay (sandbox; Intel Mac run pending) |
| 13 | Smoke test on a local model | `cargo xtask smoke` passes against a stand-in server; the real run is on Omar's Mac |
