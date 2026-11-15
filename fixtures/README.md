# Recorded fixtures

Session logs in the JSONL interchange format (see `docs/schema.md`), schema
v1. Regenerate with `cargo xtask fixtures`; the tests fail if a committed
file or `MANIFEST.json` differs from the generator.

| Fixture | Sessions | Contents |
| --- | --- | --- |
| `chat-basic` | 3 | Three prompts, streaming, reasoning, a model title, a stop then continue, a refusal, and two forks after turn 1 (an edited prompt and a regenerated reply), so three siblings share that boundary |
| `tool-heavy` | 1 | Intel profile: a transient model error retried, three parallel calls with one failing, a long result cut by the tool's output limit, an artifact created then updated, a heuristic title |
| `approval` | 1 | An outward action approved in the shell's sheet, one denied by the owning app, an ambiguous answer treated as a denial, an ask-user question answered; location opted in |
| `autonomous-run` | 2 | Two scheduled wakes; compaction that trims two long results and summarizes day 1; a fake delegated child session; a step-budget stop |
| `long-thread` | 1 | Exactly 10,000 events. Generated, not committed (about 2 MB); its size and digest are in `MANIFEST.json` |
| `client-gone` | 1 | A client tool whose app is gone: awaiting client, a 30-second timeout as a readable failure, then active again on reconnect |

Every fixture validates against the JSON Schema, round-trips through the
Rust types byte for byte, and together they cover every event type.

Until harness-loop lands, the fixtures are scripted with the testkit's
recorder in exactly the order the loop will produce. When it lands,
`cargo xtask fixtures` records them from scripted runs of the real loop;
any difference is reviewed in that pull request, and the shapes stay
within schema v1.

From Rust, use `harness_testkit::fixtures::{all, by_name, long_thread}`.
