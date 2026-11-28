# Third-party notices

Code in this repository that is ported from other projects, with each
project's licence notice. Repository text elsewhere names these projects by
reference ID only.

## R1: openai/codex (codex-rs), commit df06542

Ported into:

- `crates/harness-toolkit/src/files/ladder.rs`: the line steps of the match
  ladder and the punctuation table, from
  `codex-rs/apply-patch/src/seek_sequence.rs`.
- `crates/harness-toolkit/src/files/lines.rs`: the line-ending model, from
  `codex-rs/apply-patch/src/text_file.rs`.
- `crates/harness-toolkit/tests/parity/R1/`: test cases from
  `seek_sequence.rs` and `codex-rs/apply-patch/src/file_update_tests.rs`.

Its NOTICE file reads, in the part that applies:

    OpenAI Codex
    Copyright 2025 OpenAI

Licensed under the Apache License, Version 2.0.

You may obtain a copy of the License at
http://www.apache.org/licenses/LICENSE-2.0. Unless required by applicable
law or agreed to in writing, software distributed under the License is
distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
KIND, either express or implied. See the License for the specific language
governing permissions and limitations under the License.

The ported code was translated and changed: each ladder step must match
exactly one place, lines borrow from the file instead of owning their text,
and a replacement adds no ending to a file's last line.

## R2: aaif-goose/goose, commit d1438fd

Ported into:

- `crates/harness-toolkit/tests/parity/R2/`: test cases from
  `crates/goose/src/agents/platform_extensions/developer/edit.rs`. No other
  code is taken.

Its LICENSE file carries this notice:

    Copyright 2024 Block, Inc.

Licensed under the Apache License, Version 2.0, on the terms given above
for R1.
