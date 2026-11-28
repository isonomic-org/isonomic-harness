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
and its tests, `file_update.rs`, `file_update_tests.rs`, `text_file.rs`,
`parser.rs`; R2 (Port) the developer extension's `edit.rs` and `tree.rs`;
R4 (Behaviour) the filesystem server's `lib.ts` and `path-validation.ts`;
R13 (Behaviour) the edit tool. Code is taken from R1 only (the ladder's line
steps and punctuation table, `files/ladder.rs`), with R1's tests as parity
vectors in `crates/harness-toolkit/tests/parity/`. R3, R5 and R6 are the
engine's service's dependencies; the toolkit has only the memory store.

1. **Every ladder step must match exactly one place.** R1 returns the first
   match; here a step that matches more than one place stops the ladder
   with `files.many_matches` and the lines, so an edit never lands on the
   wrong copy. Step one is an exact substring, not R1's exact line match,
   so a find inside one line works as in R2 and R13.
2. **An identical find and replace succeeds with "No change"**, without
   reading the file. R13 refuses it; nothing is wrong with the call.
3. **Reads page by display lines.** A line longer than half the result cap
   is split at a character boundary into display lines, and `from_line` and
   the header count display lines. Every page then fits the cap and pages
   cover the file with no gap or overlap (invariant 9). R2 and R4 return
   the whole file or cut it.
4. **Paths are logical**: a linked folder's label, then the path inside it.
   Absolute, home, drive and `..` paths are refused before any service
   call (`files.not_linked`, `files.outside_folder`). R4 checks real paths
   against allowed directories; here the tools never see a real path.
5. **Protected names.** `.git`, `.hg`, `.svn` and `.jj` anywhere in a path
   (compared without case) are neither read nor changed, and `AGENTS.md` at
   a folder's top is never changed (`files.protected`). Neither is the
   linked folder itself.
6. **An empty `list_dir` path lists the linked folders**, and its access
   names the first linked folder, because an access has one scope. A read
   grant on any linked folder therefore covers the listing of names.
7. **Moves never replace and stay in one folder** (`files.exists`,
   `files.cross_folder`). R2 overwrites; R4 refuses an existing target too.
8. **Text search is plain, case-insensitive text, and the tool walks the
   folder through `list` and `read`.** R5 is named as the engine's
   dependency for search; the trait has no search method yet, so the
   engine service cannot use it. Open: move search into `FileSystem` so the
   engine's service does it with R5.
9. **The places resolved by `access` are kept per call** and the run uses
   them; a call that runs without its access check is refused
   (`tool.unresolved`). A call denied after resolving leaves its entry until
   the tool is dropped. Open: clear it when the host settles a call that
   never runs.
10. **The chat preset uses the files kit** (`[kits] use = ["files"]`,
    version 7). Kit tools are offered only where a grant could cover them,
    so sessions without a linked folder, and every existing fixture, are
    unchanged.
11. **The kits-files fixture is scripted, then checked.** Its results are
    written in the recorder, and a harness-toolkit test runs every recorded
    call again over the fixture's seed and requires the same content and
    error code.
