# Tool kits: deviations

Every place the kit code differs from its reference: what, why, and the
reference file. A deviation without an entry is a defect. References are
named by ID only (the register lives in the design, "Tool kits:
architecture v0.3"). Newest entries go at the end of their section.

## Planned deviations

These are the design's own rules. They differ from every reference by
intent, and each kit follows them.

1. **Grants.** No kit tool acts on anything a grant does not cover. The
   references trust the process they run in, or a sandbox profile; here an
   app holds a grant per effect and scope, the host checks it to decide
   whether to ask, and the engine's service checks it again at the moment
   of the operation. The engine's check is the one that protects the person.
2. **Access before the verdict.** A kit call resolves what it will touch
   (`Tool::access`) before policy judges it, and the approval shows that
   scope, a preview, whether it can be undone, and how long the answer may
   be remembered. The references judge the call on its name and input. A
   resolve failure is the call's result and never an approval.
3. **Flat inputs.** Kit tool inputs are flat objects of strings, numbers and
   booleans with short descriptions, so a small local model can fill them.
   Where a reference takes a nested input, the kit flattens it.
4. **Result caps.** A kit result is capped per hardware profile by the offer
   table (2, 8 or 16 KiB); the output limit's cut stays the backstop. The
   references size results for large models.
5. **The three-flag rule.** Once a session has read private data and taken
   in untrusted content, an outward action asks every time, whatever the
   grants say, and that answer is never remembered. The references have no
   such rule.

## The frame (harness #49)

References opened: R1, for what an approval shows (Behaviour only; no code
copied). The display widgets spec (in house) for specs, presets and the
gate; the identity specification (in house) for grant answers.

1. **The access types live in `harness_core::access`, not at the crate
   root.** `Scope` would clash with the host's caller scope in every
   `use harness_core::*`. The design names them as harness-core types; the
   module path is the only difference.
2. **The decision records the remember actually applied.** A person may
   ask for longer than the request offers; the host holds it to the
   request's `remember`, keeps it only for an allow with a verified
   user-action token, and records that value on `approval.decision`.
3. **`decide_approval` takes `remember` before the token.** The design says
   it takes remember; the position is ours.
4. **The kits-offer fixture uses fake kit tools** (`tools::kit_fakes`),
   until the kits ship their own specs, and a preset with no display
   widgets, which the display fixture already covers. Its sessions are
   derived from the offer table and the admission rule, and a test checks
   each session's tool list against a hand-written expectation.
5. **The test preset names `[kits] use = ["files"]`**, so host tests can
   offer a fake files tool. The built-in toolkit has no kit tools, so no
   existing session or fixture changes.
6. **Invariants are exhaustive loops, not property tests.** The inputs are
   small closed sets (effects, flags, autonomy, grant answers, reversible,
   confined), so the tests walk every combination instead of sampling, and
   no property-testing dependency is added.
7. **The grants contract lives in harness-toolkit** (`grants::contract`),
   beside the trait, not in harness-testkit: harness-testkit depends on
   harness-core alone, and the engine's implementation reaches the contract
   through the same crate it implements.
8. **No unknown-profile session in the kits-offer fixture.** The event
   schema lists the known profiles, so a fixture line with another would
   not validate. The offer table's test covers an unknown profile.
9. **Remember records the act from the request's effects.** These are the
   narrowed effects, which differ from the tool's own only for a confined
   call. No confined backend exists yet; when Cells arrive, the remembered
   act must be the one the grant check used.
10. **The kit fields stay in revision 1.** The design gives them revision 2,
    or lets them share revision 1 if they land together with display
    widgets. Display widgets had already merged as revision 1, and a bump
    would have changed every existing fixture's header, against the rule
    that every fixture replays byte for byte. The cost is that a log's
    header no longer says whether kit fields can appear; readers check the
    version only and ignore fields they don't know, so nothing breaks.

## The files kit (harness #50)

References opened at their pins: R1 (Port) `apply-patch/src/seek_sequence.rs`
and its tests, `text_file.rs`, `file_update.rs`, `file_update_tests.rs`,
`parser.rs`, and `file-search/src/lib.rs` for the ranking order; R2 (Port)
the developer extension's `edit.rs` and `tree.rs`; R4 (Behaviour) the
filesystem server's `lib.ts` and `path-validation.ts`; R13 (Behaviour) the
edit tool. Code is taken from R1 only: the ladder's line steps and
punctuation table (`files/ladder.rs`) and the line-ending model
(`files/lines.rs`), with R1's tests as parity vectors in
`crates/harness-toolkit/tests/parity/R1/`. R3, R5, R6 and D1 are the
engine service's dependencies; the toolkit has the trait, the contract and
the memory store.

1. **Every ladder step must match exactly one place** (planned by the
   design). R1 returns the first match; here a step that matches more than
   one place stops the ladder with `files.many_matches`. Overlapping exact
   matches count as separate places.
2. **A ladder replacement ends where the replaced lines ended.** As in R1,
   unchanged lines keep their endings and inserted lines take the file's
   first ending. R1 also gives every line of an updated file an ending;
   here a last line without one stays without one, because `edit_file`
   changes one place and nothing else.
3. **An identical find and replace succeeds with "No change"**, without
   reading the file. R13 refuses it; nothing is wrong with the call.
4. **Lines longer than 512 bytes are shown as several display lines**, cut
   at character boundaries, and every line number the tools show or take
   counts display lines: the read header and `from_line`, search hits and
   edit results. So a hit's number is the line `read_file` shows, every
   page fits the smallest cap, and pages concatenate to the file exactly,
   endings included (invariant 9). R2 and R4 return the whole file or cut
   it.
5. **The smallest cap is 1 KiB.** A smaller `result_cap` is raised to it:
   one display line and its header must fit. The offer table's smallest is
   2 KiB. Paths and names in results are clipped to 160 bytes with `...`;
   the `after` cursor is always a whole name.
6. **Paths are logical**: a linked folder's label, then the path inside it.
   Absolute, home, drive and `..` paths and control characters are refused
   before any service call. R4 checks real paths against allowed
   directories; here the tools never see a real path, and the service
   checks again (the contract's link rows).
7. **Protected names.** `.git`, `.hg`, `.svn` and `.jj` anywhere in a path
   are neither read nor changed, and `AGENTS.md` at any depth is never
   changed (`files.protected`), compared in lower case. Nor is the linked
   folder itself, nor a folder that holds a protected place (it cannot be
   moved or trashed). The toolkit sees spellings; the design's rule that
   the check "compares what was opened" is the service's, and the contract
   has the row for a link that leads to a protected place. Other character
   forms are covered only by the engine's Mac tier.
8. **An empty `list_dir` path lists the linked folders**, and its access
   names the first linked folder, because an access has one scope. A read
   grant on any linked folder therefore covers the listing of names.
9. **Name search in the memory store is a simple forgiving match** (the
   query's characters in order, without case; runs and an early start
   score higher; ties by path, R1's order). The engine's service uses D1
   through the same trait method; the contract fixes only that a slip
   still finds the name and that the exact name comes first. No D1
   dependency is added to the harness.
10. **Search returns the first results that fit and a count of the rest**,
    with no cursor: the tool has four input fields and none is spare. The
    model narrows the folder or lengthens the query.
11. **`list_dir` shows dates as UTC minutes** (`2026-11-28 06:04`). The
    harness has the person's zone only inside the `now` tool; R4 prints the
    full timestamp.
12. **The places resolved by `access` are kept per session and call**, at
    most 256, oldest dropped first, and the run uses them; a call that runs
    without its access check, or with another app's places, is refused
    (`tool.unresolved`). The service still checks every place itself.
13. **`edit_file` reads, then writes.** A change another session makes to
    the same file between the two is lost. Open: the engine's service
    should compare the file with what was read, and refuse on a mismatch;
    the trait needs an expected-content argument for that.
14. **Invariant 9 is tested over generated inputs from a seeded generator,
    not a property-testing crate** (`tests/files_caps.rs`), for the reason
    in the frame's entry 6: no new dependency. It has no shrinking.
    Invariants 6 and 7 belong to the engine's service; the contract carries
    their rows (links never lead out; changes then undos in reverse leave
    the folder as it began) and the memory store passes them.
15. **The files contract lives in harness-toolkit** (`files::contract`),
    beside the trait, as the grants contract does (the frame's entry 7).
16. **Fuzz targets, the path escape table with the two published escapes,
    the prefix canary, the crash test and the Mac tier are the engine
    service's** (issue 5). Nothing in the toolkit compares path prefixes or
    touches a disk.
17. **The chat preset uses the files kit** (`[kits] use = ["files"]`,
    version 7). Kit tools are offered only where a grant could cover them,
    so sessions without a linked folder, and every existing fixture, are
    unchanged.
18. **The files fixtures are scripted, then checked.** Their results are
    written in the recorder, and a harness-toolkit test resolves and runs
    every recorded call again over the fixtures' seed and requires the same
    content, meta and error code, and the same scope, preview and
    reversibility on every approval request.
19. **A file used as a folder in a path gives `files.not_found`**; the
    design has no code for it.
