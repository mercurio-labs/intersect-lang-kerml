# Intersect KerML contributor guidance

Read README.md before changing source. This is the KerML/SysML language
package of Intersect; architecture and roadmap live in the core repository
(mercurio-labs/intersect-model and mercurio-labs/intersect-text: docs/architecture.md, docs/release-plan.md).

- Model and Text are separate Git dependencies pinned by full `rev` values.
  Keep Model/build-support/project-host at one canonical Model URL/revision;
  keep Text/Web at one canonical Text URL/revision whose Model pin agrees.
  Land Model first, then Text, then update this repository and all lockfiles.
  No default sibling checkout or committed local patch is permitted.

- Use `develop` as the default base for new branches and managed worktrees.
  Specify it explicitly; use another base only when the user requests it.
- Keep the dependency direction: host/application -> language package -> core
  -> definitions. Neither core nor definitions may depend on a language package.
- Preserve imported information and source identities. Unsupported constructs
  must be explicit; a successful extraction is not native support.
- Track imported, consumed, and semantically verified as separate accomplishments.
  Every qualification claim needs implementation and verification evidence.
- Prefer native import, generation, and execution; use Java only as an independent
  verification oracle where possible. Document any necessary build-time Java
  exception, its scope, and replacement plan. Never require Java in production.
  Keep core libraries compatible with wasm32-unknown-unknown.
- Keep semantic paths deterministic. Clocks, I/O, scheduling, and host resources
  belong behind explicit adapters.
- Language packages own domain algorithms, standard libraries, delegates,
  scoping policies, and language-specific interpretation.
- Mark generated code and handwritten Rust boundaries clearly.
- Share semantic implementations across typed APIs, reflection, text, AI,
  native hosts, and browser consumers.
- Avoid a second implementation during Mercurio migration. Use compatibility
  facades and bounded vertical slices.
- Batch related changes with focused tests. Reserve broad language comparisons
  and performance assessments for milestone boundaries.
- Two test tiers. The quick tier is what you run after a change: it must finish
  in under 10 minutes on a developer machine, build included. Anything longer
  (whole-library loads, language-pack preparation or equivalence, binding
  recompiles, Pilot comparisons, the full workspace suite) is milestone-only;
  run it at milestone boundaries or when the user asks. After a change, run
  only the crates you changed, without `milestone-tests`, as README.md "Test
  tiers" shows. A new test that takes 10 seconds or more must be marked
  milestone-only (`#[cfg_attr(not(feature = "milestone-tests"), ignore =
  "milestone")]`), not added to the quick tier. Add integration tests as
  modules of the crate's suite binary, not as new top-level test files. Do not start long test runs in
  the background and leave them running. Run suites through
  `python -B tools/test_report.py run -- <cargo test args>`; every run from
  every worktree appears at http://localhost:8765/ (the tool starts the
  server). Pass `--baseline` with an earlier run's `summary.json` to see new
  failures.
- No unwrap or expect in library code. Return meaningful errors.
- After a change, run the applicable formatting, linting, tests, and WASM checks
  documented in README.md.
- Follow user-authorized scope. A starter, proposal, or metadata registration
  must not be described as a completed modeling framework.
