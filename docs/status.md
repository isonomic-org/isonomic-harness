# Status

Newest first. Every deviation from the Milestone 0 brief is stated here.

## 2026-11-15 — harness-log

- `SessionStore` (async, runtime-agnostic, appends refuse gaps and
  overwrites) with `MemoryStore`; registry rows (`SessionMeta`) live in the
  same store.
- JSONL generations: `encode`, `decode`; future versions refused; one-step
  migrations on read (v0, the pre-release draft, to v1), never rewriting
  the input.
- `ModelRequest` with one canonical byte encoding, and `request_for`, which
  rebuilds any step's request from the log alone (reply, title and summary
  steps; forks through `resolve_fork`). Tested on every fixture: refusals,
  attempts and reasoning never reach history; continue is a prefill that
  extends the sealed reply; forks share only the prefix; tool results follow
  call order; compaction trims and summarizes from its position on.
- `harness_core::future`: boxed future and stream types and `fnv1a64`, std
  only, so no crate names an async runtime.

## 2026-11-14 — Fixtures published

- `harness-testkit`: manual clock, a deterministic recorder, the fixture
  tools, the six fixtures and a JSON Schema checker that refuses keywords it
  does not implement.
- `cargo xtask fixtures` writes `fixtures/*.jsonl` and `MANIFEST.json`.
  Tests check that the committed files and manifest match the generator,
  that every line validates against the JSON Schema and round-trips byte for
  byte, that every event type appears, that seqs have no gaps and that every
  tool call has exactly one result.
- Deviation: long-thread (10,000 events, about 2 MB) is generated, not
  committed, because of the 20 KB file rule and the connector. Its size and
  digest are in `MANIFEST.json` and checked by the tests.
- Deviation: the fixtures are scripted with the recorder, not recorded from
  harness-loop, which does not exist yet. They switch to recorded runs when
  the loop lands.
- The fixture data files (`fixtures/*.jsonl`, `MANIFEST.json`, `README.md`)
  were committed in the browser as @QadmiriOmar so their bytes match the
  generator exactly; code and docs went through the connector.

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
  as @QadmiriOmar. Code and docs go through the connector.
