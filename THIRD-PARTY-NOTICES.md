# Third-party notices

Code in this repository that is ported from other projects, with each
project's licence notice. Repository text elsewhere names these projects by
reference ID only.

## R1: openai/codex (codex-rs), commit df06542

Ported into `crates/harness-toolkit/src/files/ladder.rs` (the line steps of
the match ladder and the punctuation table) and
`crates/harness-toolkit/tests/parity/ladder.rs` (its test cases), from
`codex-rs/apply-patch/src/seek_sequence.rs`.

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

The ported code was translated and changed: each step must match exactly
one place, and the search works on a file's lines with their endings kept.
