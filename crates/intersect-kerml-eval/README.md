# KerML expression evaluator

Handwritten evaluation over the immutable frozen model. Scope and semantics
are tracked in [the implementation plan](../../docs/kerml-expression-evaluation-plan.md)
and [the Pilot source reference](../../docs/pilot-expression-evaluator-reference.md).
Unit and fixture results do not establish Pilot agreement.

## Quick fixtures (M0.2)

`tests/support/fixture.rs` caches the **full SysML metamodel** in `OnceLock`.
It compiles the unchanged stored Rcore structure through the existing
`compile_structure_bundle_with_limits` API and freezes its schema. The source
budget equals the stored text's byte length (485,745). This user-approved
replacement is necessary because both `compile_rcore_at` and the convenience
`rcore::compile_structure` entry point impose a 256,000-byte source limit.
There are no core edits for this adaptation.

Measurement on 2026-11-08, Windows/MSVC, `test-fast`:

| Operation | Seconds |
| --- | ---: |
| Full structure compilation and freezing | 4.181910 |
| Build, link with `ById`, and validate 30 expression-model objects | 0.000807 |
| Total | 4.182717 |

The total is below the plan's 5-second threshold, selecting the full schema.
The measurement test was temporary. Permanent quick tests check the warmed
fixture's 2-second construction limit and exercise the fixture constructors.
Fixtures do not use a KerML/SysML parser, language pack, or Java. They retain
the operand membership/parameter/value containment shape and populate the
full schema's required stored `Membership.memberElement` references.

Integration cases are `#[path]` modules of `tests/all.rs`. Pack-backed text
cases belong to the `milestone-tests` tier.

`INTERSECT_EVAL_PACK` must point to a pack containing the standard library
snapshot, built from the same sources as the test binary:

```sh
sysml_pack build . PACK --library
```

A plain pack fails with "no library snapshot". Rebuild the library-containing
pack when its source identities differ from the test binary. Leaving the
environment variable unset skips text tests with an explicit message; a
configured but invalid or incompatible pack fails the test.

## M1 API and tracing

`Evaluator::new(&model, EvalMode::ModelLevel)` resolves the schema and library
index once. `evaluate`, `evaluate_feature`, `is_model_level_evaluable` and
`evaluate_gated` operate on stored relationships without mutating the model.
Each evaluation resets its step/depth limits. Integer arithmetic is checked
`i64` (E3); the Pilot reference describes wrapping Java `int` instead.
`Extended` also inlines user and library function bodies with bound arguments.
M4/M5 provide the extended function table, metadata reflection,
and condition/bound queries. Their identity-paired corpus checks and required
user deviation review are complete. `StringFunctions::Substring` uses
one-based inclusive UTF-16 bounds. A slice that splits a surrogate pair returns
`EvalError`, as approved, because `Value::Str` cannot hold a lone surrogate.

`evaluate_observed(..., &mut Trace::default())` records expression entry/exit,
library application, lazy skips and context lookups. An observer may stop at
an entry step. Function application reports already-bound arguments where
available; direct invocation operands are pending when the function is entered.
The ordinary path does not allocate trace events or argument snapshots.

Supply source text with `Trace::default().with_source(uri, text)` before
evaluation to render line/column positions with `trace_text`. The frozen model
does not retain text. Missing host text is rendered as URI plus byte offsets.
The optional `SourceMap` remaps wrapper spans to the user's expression.

`format::pilot_text` and `format::outcome_json` format results. JSON distinguishes
not-evaluable from empty values and retains exact real bits and reason identities.
Text real formatting uses Rust formatting primitives with Java 19+ decimal
selection and layout, including the closest one- or two-digit decimal when a
one-digit decimal round-trips. Java/Pilot comparison remains M3 work.

## M2 sites and expression sources

`sites(&model, resources)` enumerates feature values, result expressions and
multiplicity bounds in requested-resource then containment order, suppressing
sites nested inside another site's expression. Its targets follow the rules in
M2.1 of the plan. Library resources are included only when explicitly named.

`eval_source(target, expression, n)` returns `Result<(Source, SourceMap), EvalError>`.
The host loads the source together with user files on its prepared library, then
calls `wrapper_expression(&model, &source.uri)` to get the result expression.
Invalid qualified target names are rejected before constructing the import.
The library performs no parsing, file I/O or pack loading in these APIs.

`Evaluator::qualified_name` and `resolve_target` share the escaped-name queries
with the CLI. Target lookup requires a unique fully qualified name and reports
missing or ambiguous targets as errors.

## M5 metadata, conditions and bounds

Metadata access returns attached metadata features followed by
`Value::Metaclass(element)`. Metaclass contexts reflect stored attributes and
references without creating model objects. Derived fields remain explicitly
unsupported. Enumeration reflection returns an explicit Unsupported reason:
the Pilot returns a null item, which `Value` cannot represent. This limitation
was approved and is distinct from an empty result list.
Annotation target lookup preserves a pinned Pilot quirk: a metadata feature
contained by an `Annotation` does not see that containing relationship as an
annotation target. Its own annotation relationships and implicit namespace
target are handled normally. The independent in-memory Pilot probe documents
this behavior; the corpus comparison and required user deviation review are complete.

`check_condition` accepts only a single Boolean `true`. `check_conditions_on`
requires every condition to match some metadata candidate (different conditions
may match different candidates). These APIs always use model-level semantics,
including when called on an Extended evaluator. They do not alter loading,
scoping, validation, or import filtering.

`multiplicity_bound` returns `Some(n)` for a nonnegative integer, `Some(-1)`
for `*`, and `None` for invalid or non-model-level-evaluable results; evaluation
errors propagate. The range's owning namespace supplies its context.

## Corpus boundary (2026-11-09)

The evaluator implements M0–M5. The identity-paired corpus covers 357 sites in
47 admitted files in three modes: 1,066 exact observations of 1,071. Five
recursion-error observations at four sites are covered by the explicit user
review approved on 2026-11-09; GE is complete under plan §8. The 110 excluded
input files and all per-file counts are recorded in
[the result report](../../tools/pilot-oracle/results/eval-2026-11-09.json).
The optional library evaluation-site corpus was not run; the full standard
library was loaded as context. This does not qualify all possible models or
change the release gates. Java is used only by independent verification tools.

Final native checks: 203 evaluator quick tests passed (7 milestone tests
ignored); the full evaluator/CLI milestone suite passed 220 tests with none
ignored. Formatting, strict clippy and the evaluator WASM build passed.
