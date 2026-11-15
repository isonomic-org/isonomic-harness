# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.

## 2026-11-14 — Schema v1 published (not yet frozen)

- `harness-core`: ids, `SessionEvent` and every payload, blocks, open enums,
  session classes, `ErrorInfo`, `Clock`, `LiveDelta`, `LogHeader`, and a
  JSON `Value` type, so the crate depends on serde only. `serde_json` is a
  dev-dependency for its tests.
- JSON Schema at `crates/harness-core/schema/session-event.v1.json`; a test
  checks that it names exactly the Rust event and block types.
- Forward compatibility: unknown event types, block types and enum values
  deserialize as `Unknown` / `Other` and round-trip; a malformed known event
  is an error.
- Deviation: two event types beyond the brief's eighteen, `session.start`
  and `session.status`; reasons in `schema.md`.
- Interpretations: refusals are `attempt(refused)`; post-turn steps run after
  `turn.end`; context additions are `system(context)` events.

## 2026-11-14 — Step 0

- Repository created as a private repository in isonomic-org, with a README.
- Workspace, `harness-core` placeholder, `xtask deps` and CI pushed through
  the connector on branch `step-0-scaffold`.
- CI calls the shared `rust-ci.yml`. The shared workflow has no `--locked`
  input, so this repository passes `--locked` through its `features` input,
  which reaches both clippy and test.
- The no-I/O allow-list runs as `cargo xtask deps` through the shared
  workflow's `xtask` input. It checks resolved dependencies transitively and
  scans each library crate's `src/` for file, network, process, environment
  and OS paths.
- Deviation: the repository could not be created through the connector (the
  integration lacks permission to create organization repositories). It was
  created in the browser as @QadmiriOmar.
- Deviation: the connector cannot write `.github/workflows/` (the integration
  lacks the workflows permission), so `ci.yml` was committed in the browser
  as @QadmiriOmar. Every other file goes through the connector.
