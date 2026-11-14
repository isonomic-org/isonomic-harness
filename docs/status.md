# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.

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
