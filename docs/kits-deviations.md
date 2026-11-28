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
edit tool. Code is taken from R1 only (entry 20): the ladder's line steps and
punctuation table (`files/ladder.rs`) and the line-ending model
(`files/lines.rs`), with R1's tests as parity vectors in
`crates/harness-toolkit/tests/parity/R1/`, and R2's edit cases in
`tests/parity/R2/`. R3, R5, R6 and D1 are the
engine service's dependencies; the toolkit has the trait, the contract and
the memory store.

1. **Every ladder step must match exactly one place** (planned by the
   design). R1 returns the first match; here a step that matches more than
   one place stops the ladder with `files.many_matches`. Overlapping exact
   matches count as separate places. R1 tries each start line in turn;
   here a line step is one pass over the lines' keys with a failure table,
   so its work grows with the file plus the find, not their product. The
   comparisons are R1's.
2. **A ladder replacement ends where the replaced lines ended.** As in R1,
   unchanged lines keep their endings and inserted lines take the file's
   first ending. R1 also gives every line of an updated file an ending;
   here a last line without one stays without one, because `edit_file`
   changes one place and nothing else.
3. **An identical find and replace succeeds with "No change"**, without
   reading the file. R13 refuses it; nothing is wrong with the call.
4. **Lines whose text is longer than 512 bytes are shown as several
   display lines**, cut at character boundaries, the ending staying on the
   last. Every line number the tools show or take counts display lines:
   the read header and `from_line`, search hits and edit results. A search
   hit is numbered on the display line its match starts on, which is the
   line `read_file` shows, and its row shows at most 200 bytes around the
   match. Every page fits the smallest cap, and pages concatenate to the
   file exactly, endings included (invariant 9). R2 and R4 return the
   whole file or cut it.
5. **The smallest cap is 1 KiB.** A smaller `result_cap` is raised to it:
   one display line and its header must fit. The offer table's smallest is
   2 KiB. Paths and names in results are clipped to 160 bytes with `...`;
   the `after` cursor is always a whole name. So a listing stops before a
   name too long for the cap to continue from, and going on fails with
   `files.too_large`, naming it: at 1 KiB a name of about 900 bytes, and
   at the offer table's 2 KiB none that a file system allows.
6. **Paths are logical**: a linked folder's label, then the path inside it.
   Absolute, home, drive and `..` paths and control characters are refused
   before any service call. R4 checks real paths against allowed
   directories; here the tools never see a real path, and the service
   checks again (the contract's link rows).
7. **Protected names.** `.git`, `.hg`, `.svn` and `.jj` anywhere in a path
   are neither read nor changed, and `AGENTS.md` and `AGENTS.override.md`
   at any depth are never changed (`files.protected`), compared in lower
   case. Nor is the linked
   folder itself, nor a folder that holds a protected place (it cannot be
   moved or trashed). The toolkit sees spellings; the design's rule that
   the check "compares what was opened" is the service's, and the contract
   has the rows for a link that leads to a protected place, for a change
   and for a read of version-control metadata. Other character forms are
   covered only by the engine's Mac tier.
8. **An empty `list_dir` path lists the linked folders**, and its access
   names the first linked folder, because an access has one scope. A read
   grant on any linked folder therefore covers the listing of names. The
   folders are listed by label, not in the order they were linked, so
   `after` continues the same order on every page.
9. **Name search in the memory store is a simple forgiving match** (the
   query's characters in order, without case; runs and an early start
   score higher; ties by path, R1's order). The engine's service uses D1
   through the same trait method; the contract fixes only that a slip
   still finds the name and that the exact name comes first. No D1
   dependency is added to the harness.
10. **Search returns the first results that fit and a count of the rest**,
    with no cursor, and tells the model to narrow the folder or lengthen
    the query. This falls short of the design's rule that every tool says
    how to get the rest. The design's table gives `search_files` three
    inputs (`path`, `query`, `in`); a fourth, such as `after`, fits the
    four-field limit but changes the spec beyond the design. Open, for the
    owner: add the field, or keep this.
11. **`list_dir` shows dates as UTC minutes** (`2026-11-28 06:04`). The
    harness has the person's zone only inside the `now` tool; R4 prints the
    full timestamp.
12. **The places resolved by `access` are kept per session and call**, at
    most 64 for one session, its own oldest dropped first (4,096 in all),
    and the run uses them; a call that runs
    without its access check, or with another app's places, is refused
    (`tool.unresolved`). The service still checks every place itself.
13. **`edit_file` reads, then writes.** A change another session makes to
    the same file between the two is lost. Open, for the owner, before the
    engine implements the trait: an expected-content argument on `write`,
    or an `edit` method on the service (the design's module matrix puts
    the matcher in the engine's file service, which suggests the second).
14. **Invariant 9 is tested over generated inputs from a seeded generator,
    not a property-testing crate** (`tests/files_caps.rs`), for the reason
    in the frame's entry 6: no new dependency. It has no shrinking. The
    design names D4 for this as a development dependency, so this is the
    owner's choice: open.
    Invariants 6 and 7 belong to the engine's service; the contract carries
    their rows (links never lead out; changes then undos in reverse leave
    the folder as it began) and the memory store passes them.
15. **The files contract lives in harness-toolkit** (`files::contract`),
    beside the trait, as the grants contract does (the frame's entry 7).
16. **Fuzz targets, the path escape table with the two published escapes,
    the prefix canary, the crash test and the Mac tier are the engine
    service's** (issue 5). Nothing in the toolkit compares path prefixes or
    touches a disk. The resolver and the matcher live in this crate, so
    the engine's fuzz targets drive `files::resolve` and
    `files::ladder::replace_one`; a timed round on the matcher belongs
    with them (a test here bounds an edit that matches 960,000
    places).
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
20. **No code is ported from R2.** Its replacement is an exact match that
    must be unique, which is the ladder's first step; its listing walks a
    tree on disk, which is the engine service's. R2's edit cases are
    parity vectors (`tests/parity/R2/edit.rs`), and its three write cases
    run through the service (`write.rs`). One differs: R2 fails an
    edit whose find and replace are the same; here it is a result that
    says "No change", and nothing is written.
21. **A change is named by its session and its call** (`ChangeId`), for
    pre-images and undo, because call ids restart in every session. The
    result's meta gives the call id; the view knows the session. An undo
    puts back the places its change touched and nothing else, so another
    session's change elsewhere in the folder stays (the contract has the
    row). A later change to the same place is replaced by the undo; the
    order of undos is the caller's.
22. **Text search ignores letter case.** The design says a plain text
    match; R4's search ignores case, and a small model often gets the case
    of a word wrong. It is still not a pattern language.
23. **Folder rows in a listing show the name only**, with no size or date:
    a folder's size is not one number and its date changes with its
    contents.
24. **The resolver is forgiving about spelling.** It treats `\` as a
    separator, trims spaces around the whole path, and matches a folder's
    label without ASCII case when no label matches exactly. A file whose
    name holds a backslash, or begins or ends with a space, cannot be
    named. Two linked folders with one label are refused by name; the
    listing shows both, and one row can be lost where a page ends between
    them.
25. **The kits-files-write fixture shows the last clause of its row in
    part.** The design's row ends "a change that cannot be undone asks
    despite the grant". A log does not record grants, and whether a change
    can be undone is a fact of the folder, so the fixture shows that such
    a change asks, offers "once" only, and asks again the next time. That
    it asks under a standing allow is a host test over the real policy
    (harness-host `tests/files_kit.rs`). For the owner to confirm.
