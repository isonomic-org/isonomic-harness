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
6. **Fixture headers read revision 2.** Every event line of every existing
   fixture is unchanged; only the header line moves, as it did for display
   widgets.
7. **Invariants are exhaustive loops, not property tests.** The inputs are
   small closed sets (effects, flags, autonomy, grant answers, reversible,
   confined), so the tests walk every combination instead of sampling, and
   no property-testing dependency is added.
8. **The grants contract lives in harness-toolkit** (`grants::contract`),
   beside the trait, not in harness-testkit: harness-testkit depends on
   harness-core alone, and the engine's implementation reaches the contract
   through the same crate it implements.
9. **No unknown-profile session in the kits-offer fixture.** The event
   schema lists the known profiles, so a fixture line with another would
   not validate. The offer table's test covers an unknown profile.
10. **Remember records the act from the request's effects.** These are the
    narrowed effects, which differ from the tool's own only for a confined
    call. No confined backend exists yet; when Cells arrive, the remembered
    act must be the one the grant check used.
