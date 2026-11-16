# KerML/SysML expression evaluation: implementation plan

Status: **M0–M5 complete; expression corpus gate GE satisfied with
user-reviewed deviations**, 2026-11-09.
Originally written on `claude/rcore-frozen-engine` for an autonomous coding
agent (OpenAI Codex) to execute task by task. Verification and approved
amendments are recorded in §11. Choices that belong to the user are listed
under [Open decisions](#open-decisions-for-the-user); each has a default the
plan uses until the user decides. Expression evaluation is **not** a Release 1
gate (see [release plan](release-plan.md)); this plan does not change any gate.

Companion document: [Pilot expression evaluator reference](pilot-expression-evaluator-reference.md)
(the Pilot's behaviour, read from its Java source at the pinned commit). Cited
below as **REF §n**. Read it before task M1.1.

## 1. Goal

Evaluate KerML/SysML expressions natively in Intersect, on the frozen engine,
with results that agree with the Java SysML v2 Pilot Implementation on a
corpus of real expressions, and expose that as:

- a library API: given a loaded, validated `intersect_model::core::frozen::Model`
  (library plus user files), evaluate an expression element in a context, ask
  whether it is model-level evaluable, evaluate a feature's value, and check a
  condition;
- a CLI command: `intersect eval` evaluates an expression text, or every
  feature value of the loaded files, against a language pack's prepared
  library, printing results as text or JSON;
- a Pilot comparison: a repeatable harness that evaluates the same sites with
  the Pilot (Java only as an oracle) and with Intersect and reports agreement
  per site, with every difference fixed or listed with a reason.

### User-facing outcome

```text
$ intersect eval --pack sysml.pack --expr "1 + 2 * 3"
LiteralInteger 7
$ intersect eval --pack sysml.pack --target Vehicles::Car --expr "mass / 2" Vehicles.sysml
LiteralRational 750.0
$ intersect eval --pack sysml.pack --all --json Vehicles.sysml > values.jsonl
```

### Scope

- The Pilot's **model-level** evaluator (REF §1–§3): literals, `null`,
  sequences, arithmetic, comparison, Boolean and conditional operators,
  `??`, `..`, `#`, `.` feature chains, `collect`/`select`, `istype`,
  `hastype`, `@`, `as`, equality, feature references resolved through the
  context, constructor results, metadata access, `@@`, `meta`, reflective
  metaclass access, `isModelLevelEvaluable`, `checkCondition`, multiplicity
  bound values.
- The Pilot's **extended** evaluator (REF §1.7, §2.2), used by `%eval`:
  sequence, string, trig, numerical, `forAll`/`exists`/`reduce`/... functions,
  and inlining of user and library function bodies.
- The evaluation API, the `intersect eval` CLI command, the Pilot comparison
  harness and its deviations file, and the docs for all of these.

### Non-goals

- Units and quantities beyond the Pilot: `5 [kg]` (`BaseFunctions::'['`),
  `ISQ`/`SI` conversions, quantity arithmetic. The Pilot leaves them
  unevaluated (REF §5); so does this plan (decision E6).
- Behaviour execution: actions, states, transitions, `TriggerInvocationExpression`,
  assignment actions, time. Constraint *checking* at validation time (the Pilot
  has none, REF §4).
- Wiring the evaluator into loading: filtered imports (`import P::*[@Safety]`)
  in name resolution, view filters, the multiplicity-bound validation
  diagnostic. They need the evaluator inside linking and validation; that is a
  separate design (decision E5). This plan provides the functions they would call.
- Changing Release 1 gates, the roadmap's decisions D1–D10, the KerML/SysML
  grammars, the Rcore name-resolution rules, or the language-pack recipe.
- Solvers, symbolic algebra, optimisation (`maximize`/`minimize` beyond the
  Pilot's fold).

## 2. What exists today (facts the plan builds on)

All verified on 2026-11-08 at `b1ff97b5` plus the uncommitted CLI work.

- **Loaded model.** `intersect_kerml::sysml_host::SysmlHost::load_on_library(&library, &sources, observe)`
  returns `frozen_load::FrozenLoad { model: Option<frozen::Model>, diagnostics, references, resolved }`.
  The `Model` is immutable and validated; it holds the library and the user
  files. `Model::obj(ObjId) -> Option<Obj>`, `Model::lookup(identity)`,
  `Model::inverse()`, `Obj::{class, container, get(FeatureId) -> Option<ValueRef>, refs, str, identity, resource}`,
  `Metamodel::{class(name), feature(class, name), is_a(actual, expected), feature_info}`
  (`crates/intersect-model/src/core/frozen/model.rs`, `metamodel.rs`).
- **Derived features are not stored.** `Expression.function`, `result`,
  `InvocationExpression.operand`, `InstantiationExpression.argument`,
  `instantiatedType`, `FeatureReferenceExpression.referent`, `FeatureValue.value`
  read as `None`. The evaluator navigates stored relationships:
  - operands: expression `ownedRelationship` → `ParameterMembership` →
    owned `Feature` (direction `in`) → its `FeatureValue` → `ownedRelatedElement[0]`
    (the operand expression); built by `intersect_kerml::OperandSettings`
    (`crates/intersect-kerml/src/lib.rs` l.134–247), read the same way by
    `first_operand` in `semantics/kerml-name-resolution.rcore`;
  - referent / instantiated type: the first `Membership` in `ownedRelationship`
    that is not an `OwningMembership` (its `memberElement`), or for a chain the
    `OwningMembership`'s owned feature chain (grammar `InstantiatedTypeMember`,
    `KerMLExpressions.xtext` l.436; `referenced_members` in the Rcore rules).
    M2 text observation: `OwnedExpressionReference` also stores a referent
    through `FeatureMembership` (e.g. a lazy conditional branch); accept that
    owned feature, while excluding parameter memberships and implicit returns;
  - a feature's value: its owned `FeatureValue` relationship → `ownedRelatedElement[0]`;
  - result expression of a `Function`/`Expression`/calc: the
    `ResultExpressionMembership` in `ownedRelationship` → its owned expression.
- **No function typing for operators.** Nothing stores an implicit
  `FeatureTyping` from an `OperatorExpression` to `DataFunctions::'+'`. The
  stored `operator` string is set from the text, or by `bind default
  implied_operator` (`kerml-construction.rcore` l.48, l.167) to `.`, `#`,
  `collect`, `select` for chain, index, collect and select expressions. The
  evaluator maps the operator string to the library function itself (REF §1.5).
- **Implicit specializations are not stored** (`implicit-generalizations.rcore`
  is used only inside name resolution). Type tests against implicit supertypes
  (`Base::Anything`, `Performances::evaluations`) therefore cannot be decided
  from stored relationships (risk R4).
- **Values.** `LiteralInteger.value` is `EInt`, stored as `Slot::Int(i64)`.
  There is no `LiteralReal` class: reals parse to `LiteralRational`, whose
  `value` is `EDouble`, stored as `ValueRef::Json({"kind":"float64","bits":"<16 hex>"})`
  (`definitions/scalars.rs` l.164, `definitions/scalar_float.rs`); decode with
  `u64::from_str_radix(bits, 16)` then `f64::from_bits`. `LiteralBoolean`
  (`EBoolean`), `LiteralString` (`EString`), `LiteralInfinity` (no value).
- **Rcore executors.** The direct executor (`crates/intersect-model/src/rcore_fast.rs`)
  supports only `Bool`, `Int(i64)`, `Str`, objects, options, sequences and
  closures; no `Float64`, `Decimal` or `BigInt`, and `EDouble` reads are refused
  at preparation. `FastRead` is implemented only for `LinkView`, which exists
  only during linking. The VM supports `Float64` and `Decimal`, but is the
  fallback with a 16 MiB retained-storage limit that SysML already exceeds
  (roadmap D8). A new Rcore binding costs about 10–15 s to compile cold.
- **Compiler identity.** `intersect_kerml::SOURCE_IDENTITY` hashes every file
  under `crates/intersect-kerml/src/` (and what they embed); language-pack
  parts and binding caches are keyed on it (`language_pack.rs` l.118, l.170).
  Any edit there invalidates every prepared pack and forces a binding
  recompile (minutes).
- **CLI.** `crates/intersect-cli` (`intersect check|export|info`) is being
  written in another session and is not committed yet at the time of writing.
  `host::run` reads the pack, opens `NativePilotSession::from_pack`, loads the
  combined bindings, enters `with_sysml_host`, prepares the library and calls
  `host.load_on_library`.
- **Tests.** Integration tests of a crate live in one suite binary
  (`tests/all.rs` with `#[path]` modules). Every test that parses KerML/SysML
  text on the frozen path is milestone-tier today (grammar and binding
  preparation take minutes cold). Quick-tier evaluator tests must therefore
  build models without the parser (task M0.2).
- **Oracle.** `tools/pilot-oracle/PilotOracle.java` loads files with the Pilot
  jar (`target/pilot-oracle/kernel/sysml/jupyter-sysml-kernel-0.62.0-all.jar`,
  Java 21) and dumps every object; `compare.py` pairs Pilot and Intersect
  objects by walking containment and compares fields; `deviations.json` lists
  allowed differences. Example corpus: `target/pilot-oracle/upstream`
  (99 `.sysml`, 58 `.kerml`; 67 files contain value expressions), a sparse
  checkout at the pinned commit; the D9 subset is in `fixtures/native-pilot/v1/sources/`.

## 3. Architecture decision

### Alternatives considered

**A. Native Rust interpreter over the frozen model, in a language-package
crate (recommended).** A tree-walking evaluator reads the validated `Model`
through `Obj`/`ValueRef`, dispatches on metaclass like the Pilot (REF §1.4),
and implements the library functions as a Rust table keyed by the function's
escaped qualified name (REF §1.5). Values are a small Rust enum with `f64`
reals. Evaluation contexts and argument bindings are environment frames, not
new model objects (the Pilot creates unowned EMF objects; the frozen model is
immutable).

- For: matches the Pilot's structure one-to-one, so each REF row maps to code
  and a test; reals work today; no per-expression compile cost (expressions
  are model *data*, written by users, unlike scoping rules); deterministic;
  `wasm32-unknown-unknown`-compatible; the standard-library function semantics
  belong to the language package (AGENTS.md).
- Against: it is Rust, not Rcore. Decision D7 (compile language semantics to IR
  and execute) is about language *rules*; this evaluator implements the
  KerML *function library*, which AGENTS.md assigns to the language package.
  The structural queries it needs (all features of a type, redefinition
  closure, specialization test) overlap with Rcore name-resolution logic
  (risk R3, decision E8).

**B. Compile each expression to Rcore IR and run it on the direct executor.**
- Against: the direct executor has no `Float64`; adding it touches the IR
  verifier, both executors and the core crate. Each user expression would be a
  new program (about 10–15 s cold compile per binding today), or one generic
  Rcore interpreter written in Rcore over heterogeneous sequences, which the
  statically typed Rcore does not express. The VM fallback already exceeds its
  limits on SysML. Large core change for a language-package feature.

**C. Author the evaluator as Rcore functions (`bind` a generic `evaluate`).**
- Against: same `Float64` gap; values of mixed kind (Integer, Real, String,
  Boolean, elements) in one sequence need a sum type Rcore lacks; no binding
  kind exists for "call this function from a host"; results are not model
  objects, so `bind default`/`bind implicit` do not fit.

**D. Call the Pilot (Java) at run time.** Forbidden: no Java in production.

### Recommendation

Alternative A, in a **new crate `crates/intersect-kerml-eval`** (package
`intersect-kerml-eval`, library name `intersect_kerml_eval`), part of the
KerML language package, depending on `intersect-model` and `intersect-kerml`.
Dependency direction: `intersect-cli` → `intersect-kerml-eval` →
`intersect-kerml` → `intersect-model`; neither core nor definitions depends on
it. A separate crate keeps `intersect_kerml::SOURCE_IDENTITY` unchanged, so
evaluator edits never invalidate language packs or binding caches (decision E1
covers putting it inside `intersect-kerml` instead).

Design rules:

1. **Pilot-compatible semantics by default**, including its quirks (REF §7),
   because the Pilot is the agreed oracle; each quirk is a named test. Where
   Intersect deliberately differs (integer width, decision E3), the difference
   is listed in the deviations file with a reason.
2. **Two modes**, `EvalMode::ModelLevel` (REF §2.1, the EMF `evaluate`
   semantics) and `EvalMode::Extended` (REF §1.7, §2.2, the `%eval` semantics).
   One evaluator; the mode selects the function table and the two behaviours
   that differ (non-library invocation, unbound input parameter).
3. **No model mutation, no synthetic objects.** Where the Pilot creates EMF
   objects, Intersect uses values: new literals are scalar `Value`s; the
   Pilot's chained "target feature" path is a context stack; the Pilot's
   instantiated invocation is an argument frame; a metaclass feature is
   `Value::Metaclass(ObjId)`.
4. **Deterministic and bounded.** No clocks, I/O, threads or hash-order
   iteration. Every evaluation runs under `EvalLimits { steps, depth }`;
   exceeding a limit is an `EvalError`, never a panic or a silent result.
5. **No `unwrap`/`expect`/`panic!`/indexing that can panic in library code.**
   Malformed models produce `EvalError::Model(..)` with the object identity.
6. **Every metaclass and feature is looked up by name once**, at
   `Evaluator::new`, into an `EvalSchema` of `ClassId`/`FeatureId`s; a missing
   name is `EvalError::Schema(name)`. This also lets quick-tier tests use a
   reduced metamodel with the same names if needed (task M0.2).

## 4. Public API (target state)

Module layout of `crates/intersect-kerml-eval/src/` (all handwritten; each file
starts with `//! Handwritten ...`):

| File | Contents |
| --- | --- |
| `lib.rs` | Re-exports; crate docs pointing at this plan and REF |
| `value.rs` | `Value`, `Outcome`, equality and `value_of` (REF §2.3) |
| `error.rs` | `EvalError` |
| `schema.rs` | `EvalSchema`: class and feature ids by name |
| `nav.rs` | Stored-relationship navigation: operands, arguments, referent, instantiated type, feature value, result expression, parameters, owned/inherited features, redefinition and specialization closures, qualified names |
| `library.rs` | `LibraryIndex`: escaped qualified name ↔ `ObjId` for the library elements the evaluator needs; operator → function name table |
| `context.rs` | Context stack frames and argument bindings |
| `eval.rs` | `Evaluator`: metaclass dispatch, `evaluate_feature`, `evaluate_feature_chain`, `evaluate_expression` (REF §1.4, §1.6) |
| `functions/mod.rs` | `LibraryFunction` trait, model-level and extended tables |
| `functions/{base,data,bool,control,sequence,string,numerical,trig}.rs` | Function implementations |
| `evaluable.rs` | `is_model_level_evaluable` (REF §3) |
| `condition.rs` | `check_condition`, `check_conditions_on`, `multiplicity_bound` |
| `format.rs` | Pilot-style text (`LiteralInteger 7`), Java `Double.toString` formatting, JSON |
| `sites.rs` | Enumerate evaluation sites in resources (for `--all` and the oracle) |

Signatures (Codex implements exactly these; additional private items are free):

```rust
pub use intersect_model::core::frozen::{Model, ObjId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalMode { ModelLevel, Extended }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalLimits { pub steps: u64, pub depth: u32 }
impl Default for EvalLimits { /* steps: 1_000_000, depth: 256 */ }

/// One value of a result sequence.
#[derive(Clone, Debug)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(std::rc::Rc<str>),
    /// `*` (LiteralInfinity); `value_of` is null (REF §1.3).
    Infinity,
    /// A model element as a value (a feature, an enumeration literal, a
    /// constructor result, a metadata feature, ...).
    Element(ObjId),
    /// The Pilot's in-band "unevaluated" marker: the expression or feature
    /// that could not be reduced (REF §1.3), with why. Compares by identity
    /// of `at` only; `reason` never affects equality.
    Unevaluated { at: ObjId, reason: Reason },
    /// The metaclass feature of an element (REF §1.4 item 4, §1.6).
    Metaclass(ObjId),
}

/// The result of an evaluation.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// The Pilot's Java `null`: not evaluable or gated (distinct from `[]`).
    NotEvaluable(Reason),
    /// A sequence; empty means KerML null.
    Values(Vec<Value>),
}

/// Why a result is unevaluated or not evaluable, recorded where the
/// decision is made (debugging, decision 2026-11-08). Each variant names the
/// element it concerns where there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    UnknownFunction { invocation: ObjId },              // not in this mode's table
    NotModelLevelEvaluable { expression: ObjId },       // gated (evaluate_gated)
    OperandNotSingle { invocation: ObjId, operand: usize, count: usize },
    OperandKinds { invocation: ObjId, kinds: Vec<&'static str> }, // e.g. ["String","Integer"]
    OperationUndefined { invocation: ObjId },           // e.g. "a" - "b"
    NoValue { feature: ObjId, frame: Option<ObjId> },   // feature has no value in that context
    NoReferent { expression: ObjId },
    NoResultExpression { expression: ObjId },
    Overflow { invocation: ObjId },                     // decision E3
    BadIndex { invocation: ObjId },
    TypeArgumentMissing { invocation: ObjId },
    Unsupported { element: ObjId, what: &'static str }, // e.g. "quantity '['", "trigger invocation"
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    Schema(String),                       // a metaclass/feature name missing in the metamodel
    Model { object: String, message: String }, // malformed stored model (identity of the object)
    NotAnExpression(String),              // identity of the argument
    StepLimit(u64),
    DepthLimit(u32),
    Stopped(u64),                         // an observer returned Control::Stop at this step
}

pub struct Evaluator<'m> { /* model, schema, library index, mode, limits */ }

impl<'m> Evaluator<'m> {
    pub fn new(model: &'m Model, mode: EvalMode) -> Result<Self, EvalError>;
    pub fn with_limits(self, limits: EvalLimits) -> Self;
    pub fn mode(&self) -> EvalMode;
    /// M2 host queries reuse the evaluator's escaped-name navigation.
    pub fn qualified_name(&self, element: ObjId) -> Result<Option<String>, EvalError>;
    pub fn resolve_target(&self, name: &str) -> Result<ObjId, EvalError>;
    /// Pilot `evaluate(expression, target)`, ungated (REF §1.4).
    pub fn evaluate(&self, expression: ObjId, target: Option<ObjId>) -> Result<Outcome, EvalError>;
    /// Pilot EMF `Expression::evaluate(target)`: gated at invocations by
    /// `is_model_level_evaluable` (REF §3, "Gating"). ModelLevel mode only;
    /// in Extended mode it equals `evaluate`.
    pub fn evaluate_gated(&self, expression: ObjId, target: Option<ObjId>) -> Result<Outcome, EvalError>;
    pub fn evaluate_feature(&self, feature: ObjId, target: Option<ObjId>) -> Result<Outcome, EvalError>;
    pub fn is_model_level_evaluable(&self, expression: ObjId) -> Result<bool, EvalError>;
    /// Pilot `checkCondition` (REF §3).
    pub fn check_condition(&self, condition: ObjId, target: Option<ObjId>) -> Result<bool, EvalError>;
    /// Pilot `checkConditionsOn` for filters (REF §3).
    pub fn check_conditions_on(&self, element: ObjId, conditions: &[ObjId]) -> Result<bool, EvalError>;
    /// Pilot `MultiplicityRange::valueOf`: Some(-1) for `*`, Some(n) for n >= 0, None otherwise.
    pub fn multiplicity_bound(&self, bound: ObjId) -> Result<Option<i64>, EvalError>;
    /// `evaluate`/`evaluate_gated` reporting every step to `observer` (task M1.9).
    pub fn evaluate_observed(
        &self,
        expression: ObjId,
        target: Option<ObjId>,
        gated: bool,
        observer: &mut dyn EvalObserver,
    ) -> Result<Outcome, EvalError>;
}

/// Debugging hook (task M1.9). Every method has an empty default body, so an
/// observer implements only what it needs. Calls arrive in evaluation order
/// and are identical on every run for the same model and arguments.
pub trait EvalObserver {
    /// About to evaluate `expression` (step number `step`, nesting `depth`).
    /// Returning `Control::Stop` ends the evaluation with `EvalError::Stopped`.
    fn enter(&mut self, step: u64, depth: u32, expression: ObjId, context: &ContextView<'_>) -> Control { Control::Continue }
    fn exit(&mut self, step: u64, expression: ObjId, outcome: &Outcome) {}
    /// A library function is applied to the arguments evaluated so far.
    fn apply(&mut self, invocation: ObjId, function: &'static str, arguments: &[Option<&Outcome>]) {}
    /// A lazy operand (`and`, `or`, `implies`, `if`, `??`, collect/select bodies) was not evaluated.
    fn skipped(&mut self, invocation: ObjId, operand: usize) {}
    /// One context frame was consulted for `feature` (REF §1.6 loop).
    fn lookup(&mut self, feature: ObjId, frame: &FrameView, found: Lookup) {}
}
pub enum Control { Continue, Stop }
pub enum Lookup { Value(ObjId /*value expression*/), Unbound(ObjId /*redefining feature*/), Bound, NotHere }
/// Read-only view of the context stack, innermost last.
pub struct ContextView<'c> { /* frames */ }
impl ContextView<'_> { pub fn frames(&self) -> impl Iterator<Item = FrameView> + '_; }
/// A printable frame: `Type { ty }` or `Arguments { expression, bound: Vec<(parameter, Vec<Value>)> }`.
pub enum FrameView { Type { ty: ObjId }, Arguments { expression: ObjId, bound: Vec<(ObjId, Vec<Value>)> } }

/// Ready-made observer that records every event (for `--trace` and tests).
pub struct Trace { pub events: Vec<TraceEvent> }
impl EvalObserver for Trace { /* records */ }
// User-approved M1.9 amendment: the frozen Model has no source text.
impl Trace { pub fn with_source(self, uri: impl Into<String>, text: impl Into<String>) -> Self; }
pub fn trace_text(model: &Model, trace: &Trace, source: Option<&SourceMap>) -> String;

/// Maps positions in the `--expr` wrapper source back to the user's text (task M2.2).
pub struct SourceMap { pub uri: String, pub prefix_bytes: usize, pub expression_len: usize }
impl SourceMap { pub fn to_user(&self, span: (usize, usize)) -> Option<(usize, usize)>; }

/// An expression whose value the corpus evaluates: a FeatureValue's
/// expression, a ResultExpressionMembership's expression, or a multiplicity bound.
pub struct Site { pub expression: ObjId, pub target: Option<ObjId>, pub kind: SiteKind }
pub enum SiteKind { FeatureValue, ResultExpression, MultiplicityBound }
pub fn sites(model: &Model, resources: &[&str]) -> Result<Vec<Site>, EvalError>;

pub mod format {
    pub fn pilot_text(model: &Model, value: &Value) -> String;   // "LiteralInteger 7"
    pub fn java_double(x: f64) -> String;                        // Java Double.toString
    pub fn outcome_json(model: &Model, outcome: &Outcome) -> serde_json::Value;
}
```

Value JSON (used by the CLI `--json` and the oracle comparison):
`{"kind":"bool","value":true}`, `{"kind":"int","value":7}`,
`{"kind":"real","bits":"400c000000000000","text":"3.5"}`,
`{"kind":"string","value":"a"}`, `{"kind":"infinity"}`,
`{"kind":"element","id":"<identity>","class":"AttributeUsage"}`,
`{"kind":"unevaluated","id":"<identity>","class":"OperatorExpression","reason":{...}}`,
`{"kind":"metaclass","of":"<identity>","class":"PartUsage"}`. An outcome is
`{"outcome":"not-evaluable","reason":{...}}` or `{"outcome":"values","values":[...]}`.
A reason is `{"reason":"<variant in kebab-case>", ...fields, elements as identities}`.
The oracle comparison ignores `reason` (the Pilot has none).

Notation in the tasks below: `Unevaluated(x)` means `Value::Unevaluated { at: x, .. }`
and `NotEvaluable` means `Outcome::NotEvaluable(_)`; each task's tests also
assert the `Reason` variant.

## 5. Working rules for every task

- Branch: work on the branch the user names (default: a new branch from
  `develop`, per AGENTS.md; if the user says so, on `claude/rcore-frozen-engine`).
  One commit per task, message `eval: <task id> <summary>`, ending with the
  attribution line the session requires. Never push unless told.
- Touch only `crates/intersect-kerml-eval/`, the root `Cargo.toml` workspace
  member list, `tools/pilot-oracle/` (new eval files only),
  `crates/intersect-cli/` (task M2.3 only), and docs named in a task.
  User-approved amendment (2026-11-09): M3.2 may update the CLI to retain
  per-site evaluation errors in `--all --json`; M4.2 may set the CLI default
  to Extended. M3.3 may add an optional export of `compare.py`'s existing
  containment pairing map, preserving its algorithm and default output.
- Run after every task, from the repository root (all must pass):

  ```sh
  python -B tools/format_workspace.py --check
  cargo clippy -p intersect-kerml-eval --all-targets --locked -- -D warnings
  python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml-eval --lib --bins --tests --features intersect-model/text-compiler
  cargo build -p intersect-kerml-eval --lib --target wasm32-unknown-unknown --locked
  ```

  Add `-p intersect-cli` to the clippy and test commands in tasks that change
  the CLI (and drop the WASM build for the CLI; it is a native host).
- Quick tier: each new test under 10 s; the crate's quick run under 10 minutes
  including the build. A test that needs a language pack, the parser or the
  Pilot is milestone-tier: `#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]`.
  Integration tests go in `crates/intersect-kerml-eval/tests/all.rs` as
  `#[path]` modules, never as new top-level test files.
- Never start long runs in the background and leave them. Milestone runs only
  at the milestone boundaries marked below, or when the user asks.

## 6. Milestones and tasks

Each task lists files, behaviour, tests and acceptance. "REF §x row y" means
the semantics are exactly that row of the reference unless this plan says
otherwise.

### M0 Foundations

**M0.1 Crate skeleton.**
- Create `crates/intersect-kerml-eval/{Cargo.toml,src/lib.rs,src/value.rs,src/error.rs,tests/all.rs}`;
  add the crate to `members` in the root `Cargo.toml` (edit only that list;
  another session may have unrelated edits there: keep them).
- `Cargo.toml`: workspace version/edition/license; dependencies
  `intersect-model.workspace = true`, `intersect-kerml = { path = "../intersect-kerml" }`,
  `serde_json.workspace = true`; `[features] milestone-tests = []`; one
  `[[test]] name = "all"`.
- Implement `Value`, `Outcome`, `EvalError` (with `Display` and
  `std::error::Error`), and `value::equal(a: &Value, b: &Value) -> bool` and
  `value::equal_seq(a: &[Value], b: &[Value]) -> bool` per REF §2.3:
  `Int(1)` vs `Real(1.0)` false; `Real` equality is Java `Double.equals`
  (compare `to_bits` after mapping every NaN to the canonical NaN; `0.0` ≠
  `-0.0`); `Infinity == Infinity`; `Element`/`Unevaluated`/`Metaclass` by
  variant-independent `ObjId` identity (an `Element(x)` equals `Unevaluated(x)`);
  sequences equal iff same length and pairwise equal.
- Tests (unit, in `value.rs`): one per rule above.
- Accept: the four commands pass; `cargo tree -p intersect-kerml-eval` shows no
  `intersect-xtext`-only or Java dependency beyond what `intersect-kerml` brings.

**M0.2 Quick-tier model fixtures (spike, then decide).** The evaluator tests
need small KerML models with library elements, without the parser.
1. Measure, in a throwaway test, the time to compile the stored SysML structure
   `crates/intersect-kerml/metamodel/SysML.ecore.rcore` with
   `intersect_model::definitions::compiler::compile_structure_bundle_with_limits`
   and `Metamodel::freeze(&schema)` (user-approved amendment, 2026-11-08:
   `compile_rcore_at` rejects this source at its hardcoded 256,000-byte limit).
   The bundle contains exactly one `DefinitionSource` of kind `Rcore`, using
   `STRUCTURE_URI` and the unchanged stored text. Select the stored SysML
   namespace and set `ParseLimits::source_bytes` to the text's byte length;
   this is the same limit-aware structure compiler the SysML loader uses.
   (`language_pack_library_tests.rs` demonstrates freezing a smaller source.)
   `STRUCTURE_URI` is `intersect_kerml::kerml_bindings::STRUCTURE_URI`;
   read the text with `include_str!("../../intersect-kerml/metamodel/SysML.ecore.rcore")`.
2. Measure building a 30-object expression model with `ModelBuilder`
   (`core/frozen/builder.rs`), linking with the `ById` resolver and validating
   it on that metamodel.
3. Decide by rule (no user decision needed):
   - if (1) + (2) < 5 s: write `tests/support/fixture.rs` with a cached
     (`std::sync::OnceLock`) full SysML metamodel and a `FixtureBuilder`;
   - else: write a reduced Rcore metamodel `tests/support/mini-sysml.rcore`
     with the same class and feature names, containment and multiplicities as
     SysML.ecore for exactly the classes the evaluator reads (listed in
     `EvalSchema`), plus a quick-tier test that checks every class/feature of
     the reduced metamodel exists in the full one with the same kind,
     multiplicity and containment (parse the stored structure text only, no
     compile, if compiling it is the slow part).
- `FixtureBuilder` API (test support, not library code; `expect` allowed only
  in test support): `package(name)`, `function(pkg, name, params)` (creates a
  library-shaped `Function` with `in` parameters and a `return` parameter, so
  `BaseFunctions::'=='`, `DataFunctions::'+'`, `ControlFunctions::'.'` etc.
  exist), `attribute(owner, name, value: Option<Expr>)`, `part_def`, `part`,
  `redefines(f, g)`, `subclassifies(a, b)`, `typed(f, t)`, and expression
  constructors `int(i64)`, `real(f64)`, `string(&str)`, `bool(bool)`,
  `infinity()`, `null()`, `op(operator, operands)`, `invoke(function_path, args)`,
  `feature_ref(path)`, `chain(source, path)`, `index(seq, i)`,
  `collect(seq, body)`, `select(seq, body)`, `body(params, expr)`,
  `constructor(type, args)`, `metadata_access(path)`, `build() -> Model`.
  Operand structure must be exactly what `OperandSettings` creates (§2).
  It also builds the scalar types `ScalarValues::{Boolean,String,Rational,Real,Integer,Natural,Positive,NumericalValue,ScalarValue}`
  with their library specializations, `Base::Anything::self`,
  `Collections::{Collection,OrderedCollection,Array}` with `elements` and `dimensions`.
- Record the measurements and the choice in the crate README section of this
  plan's log (§11).
- Accept: a quick test builds a fixture with `1 + 2` as a feature value, and
  the model validates, in under 2 s on a warm build.

**M0.3 Model-built-from-text fixtures (milestone tier).** Add
`tests/text.rs` (module of `all.rs`) with a helper that loads SysML text on top
of a language pack's prepared library, the same way `intersect-cli` does
(pack path from env `INTERSECT_EVAL_PACK`, skip with a clear message when unset;
milestone-tier). Later milestones add text cases here. Accept: one test
loads `package P { attribute a = 1 + 2; }` and finds the `OperatorExpression`.

### M1 Model-level evaluator

**M1.1 Schema, navigation, library index.**
- `schema.rs`: `EvalSchema::new(&Metamodel) -> Result<Self, EvalError>`
  resolves every class and feature the evaluator reads (at least: Element,
  Namespace, Membership, OwningMembership, FeatureMembership,
  ParameterMembership, ReturnParameterMembership, ResultExpressionMembership,
  Relationship, Type, Classifier, Feature, FeatureValue, FeatureTyping,
  Subsetting, Redefinition, Subclassification, FeatureChaining, Expression,
  Function, BooleanExpression, Predicate, InvocationExpression,
  OperatorExpression, FeatureChainExpression, IndexExpression,
  SelectExpression, CollectExpression, FeatureReferenceExpression,
  MetadataAccessExpression, ConstructorExpression, NullExpression,
  LiteralExpression, LiteralBoolean, LiteralInteger, LiteralRational,
  LiteralString, LiteralInfinity, MetadataFeature, Metaclass, MultiplicityRange,
  CalculationUsage, ConstraintUsage; features `ownedRelationship`,
  `ownedRelatedElement`, `memberElement`, `declaredName`, `declaredShortName`,
  `operator`, `value`, `direction`, `type`, `general`, `specific`,
  `subsettedFeature`, `redefinedFeature`, `chainingFeature`, `annotatedElement`
  (check the exact names in `SysML.ecore.rcore`; `memberElement` and similar
  references are stored, others may be derived: use only stored ones, and
  record which in a doc comment).
- `nav.rs` functions (each `-> Result<_, EvalError>`), all deterministic, in
  stored order: `operands(expr) -> Vec<ObjId>` (§2 path), `arguments(inv)`
  (operands; for named arguments, the parameter they redefine decides the
  position: REF §1.2 uses `getArgument()`, which is in parameter order),
  `referent(fre) -> Option<ObjId>`, `instantiated_type(inv) -> Option<ObjId>`,
  `feature_value(feature) -> Option<ObjId>` (first owned `FeatureValue`'s
  expression; `=`, `:=` and `default` alike, REF §1.6 note), `result_expression(t)`,
  `parameters(t) -> Vec<ObjId>` (owned then inherited, `in`/`inout` and result),
  `owned_features(t)`, `all_features(t)` (owned in order, then each general's
  in specialization order, depth-first, skipping features redefined by one
  already listed, visited set), `redefined_closure(f)`, `specializes(t, general)`
  (through FeatureTyping, Subclassification, Subsetting, Redefinition,
  Conjugation if stored; visited set), `types(f) -> Vec<ObjId>` (FeatureTyping
  targets, then inherited through subsetting/redefinition), `qualified_name(e)`
  (escaped per REF §1.5: quote a segment unless it matches `[a-zA-Z_]\w*`,
  joined with `::`), `chaining_features(f)`.
- `library.rs`: `LibraryIndex::new(model, schema)` walks root namespaces and
  maps escaped qualified names to `ObjId` for: every function of REF §2.1 and
  §2.2, `Base::Anything::self`, `Metaobjects::Metaobject::annotatedElement`,
  `Collections::{Collection,OrderedCollection,Array}`
  and their `elements`/`dimensions`, the `ScalarValues` types, `TrigFunctions::pi`,
  and the `KerML`/`SysML` metaclass packages. Missing entries are allowed (a
  user model may load without part of the library); lookups return `None`.
  `operator_function(op: &str) -> Option<&'static str>` is a static table:
  the first of `BaseFunctions::'op'`, `DataFunctions::'op'`,
  `ControlFunctions::'op'` that the pinned library declares (REF §1.5). Build
  the table from the library sources once by hand (functions listed in
  `fixtures/native-pilot/v1/sources/sysml.library/Kernel Libraries/Kernel Function Library/{Base,Data,Control}Functions.kerml`)
  and add a milestone test (M0.3 helper) that every table entry resolves in
  the prepared library and that no earlier package declares the operator.
- Tests: quick-tier unit tests of each `nav` function on M0.2 fixtures
  (operands of `1 + 2`; referent of `a`; redefinition closure across two
  levels; `all_features` hiding a redefined feature; qualified-name escaping
  of `'+'`, `not`, `'=='`, `a b`).
- Accept: commands pass.

**M1.2 Core dispatch and literals.** `eval.rs`: `Evaluator::new`,
`evaluate` with the dispatch order of REF §1.4 (items 1–7), step and depth
accounting (each `evaluate` call and each function application is one step;
feature lookup itself costs no steps; nesting is depth). **M1 review fix 5
amendment:** range materialization additionally charges one step per generated
literal, checking the entire remaining budget before allocating the result.
This bounds memory and work for a huge range that otherwise needs only four
evaluation/application steps. Feature recursion remains depth-bounded.
Literals: `LiteralInteger` → `Int`, `LiteralRational` →
`Real` (decode float64 bits), `LiteralBoolean`, `LiteralString`,
`LiteralInfinity` → `Infinity`; `NullExpression` → `Values([])`. Invocation of
an unknown function → `Values([Unevaluated(inv)])`. Function lookup: for an
`OperatorExpression`, `operator_function(operator)`; otherwise the first
`Function` among the expression's types, where an invocation's types are its
instantiated type if it is a Function, else that feature's `types` (REF §1.5).
- Tests: each literal kind; null; unknown function unevaluated; step limit
  (`EvalLimits { steps: 3, .. }` on a deep expression → `StepLimit`);
  depth limit.

**M1.3 Data functions.** `functions/data.rs`, `functions/bool.rs`, the
`LibraryFunction` trait:

```rust
pub(crate) trait LibraryFunction {
    fn names(&self) -> &'static [&'static str];         // escaped qualified names
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError>;
}
```

plus helpers `evaluate_argument`, `argument_value`, `expression_value`,
`boolean_value`, `boolean_expression_value` with REF §1.2 semantics
(`argument_value` is `None` unless exactly one value). Implement REF §2.1 rows
`'+'`, `'-'`, `'*'`, `'/'`, `'**'`, `'^'`, `'%'`, `'<'`, `'<='`, `'>'`, `'>='`,
`'..'`, `not`, `'&'`, `'|'`, `xor`, with the ArithmeticFunction dispatch of
REF §2 (unary on non-number → `[]`; a missing operation → `NotEvaluable`;
mixed int/real promotes; `/` and `**` always real; zero divisor → `[]`; string
`+` and string comparison by UTF-16 code units: compare `a.encode_utf16()`
sequences, not Rust `str` order). Integer overflow: per decision E3 default,
checked `i64` arithmetic; an overflow gives `Values([Unevaluated(inv)])`.
`%` on integers truncates (Rust `%` on `i64` matches Java), `%` on reals is
Rust `%` on `f64` (fmod, matches Java). `**`: `f64::powf` (check `0**-1` =
`inf`, `(-8)**(1/3)` NaN).
- Tests (quick, one per REF §7 items 1, 2, 4, 5, 6 plus): `7/2 = 3.5`,
  `4/2 = Real 2.0`, `2/0 = []`, `-7 % 3 = -1`, `7.5 % 2 = 1.5`, `2**3 = Real 8.0`,
  `"a" + "b" = "ab"`, `"a" + 1` unevaluated, `"a" - "b"` NotEvaluable, `-"a" = []`,
  `"é" < "z"` per UTF-16, `1 < 2.5`, `true < false` unevaluated, `not 1 = true`,
  `not null = true`, `1 & true = false`, `5..3 = []`, `1..3 = [1,2,3]`,
  `i64::MAX + 1` unevaluated, `(1,2) + 1` unevaluated.

**M1.4 Base functions.** `functions/base.rs`: `'=='`, `'!='`, `'==='`,
`'!=='` (REF §2.1, including the collection `elements` rule and "an
unevaluated operand compares by identity", REF §7 item 3, 8), `','` (concat,
flattening), `'#'` (1-based; ordered collections and arrays with row-major
flat index; out of range `[]`; REF §2.1 row `'#'`), `istype`, `hastype`, `'@'`,
`as` (REF §2.1, §2.4: literal types `ScalarValues::{Integer,Rational,Boolean,String,Positive}`;
features by `types`; `istype` on `[]` true, `@` on `[]` false; type argument is
the first type of the invocation's second owned feature).
- Tests: `1 == 1.0` false; `null == null` true; `* == *` true; `x == 1` with
  `x` unbound false; `===` same as `==`; `(1,(2,3))` = `[1,2,3]`;
  `(10,20,30)#(2) = 20`; `#(4)` → `[]`; `#(0)` → `[]`; array `#(2,1)` on a
  2×2 fixture; `1 istype Real` true; `1 istype Natural` false; `() istype Integer` true;
  `() @ Integer` false; `(1, "a") as Integer = [1]`.

**M1.5 Features, context and chains.** `context.rs` and the feature
evaluation of REF §1.6:

```rust
pub(crate) enum Frame {
    /// A type the evaluation is in (the Pilot's chained target path).
    Type(ObjId),
    /// Arguments bound to an expression's parameters (the Pilot's
    /// instantiated invocation).
    Arguments { expression: ObjId, bound: Vec<(ObjId /*parameter*/, Vec<Value>)> },
}
pub(crate) struct Context { frames: Vec<Frame> }  // last = innermost
```

`target: Option<ObjId>` becomes the initial context: a `Type` target is
`[Type(t)]`; a `Feature` target with chaining features expands to one frame per
chaining feature; any other element or `None` is the empty context.
`evaluate_feature(feature, cx)` follows REF §1.6 steps 1–6 exactly: library
feature (`TrigFunctions::pi`), `self` (feature in `redefined_closure`/specializes
of `Base::Anything::self` → the context's innermost frame as a value:
`Element(t)`), owned feature chaining → `evaluate_feature_chain`, then the
frame loop from innermost outwards (metadata frames are M3; an `Arguments`
frame answers when the feature is or redefines a bound parameter; a `Type`
frame uses `type_feature_for(feature, t)` = first of `all_features(t)` whose
`redefined_closure` contains `feature`: its value if that evaluates non-null,
else `Values([Unevaluated(type_feature)])` **without** looking further out),
then the feature's own value evaluated in the original context, else
`Values([Unevaluated(feature)])`. `evaluate_feature_chain` per REF §1.6
(each feature value pushes a `Type` frame for the next segment; non-type values
end the chain with their own value). `FeatureReferenceExpression` →
`evaluate_feature(referent)`, referent missing → `NotEvaluable`.
`'.'` (`ControlFunctions::'.'`, REF §2.1 row `'.'`): non-feature elements of
the source list are skipped silently.
- Tests: `a = 1; b = a + 1` → 2; part def `P { attribute m = 5; }` evaluated
  with target `P`; redefinition in a subtype wins (`Q :> P { attribute :>> m = 7; }`,
  target `Q` → 7); redefinition without value → `Unevaluated(redefining)`;
  `default` value counts; `self` returns the target; `p.q.r` three-level chain;
  `x.f` where `x` has two values; unbound feature → `Unevaluated(feature)`.

**M1.6 Control functions and bodies.** `ControlFunctions::{if, and, or,
implies, '??', collect, select}` (REF §2.1; lazy operands via
`expression_value`: the operand is a `FeatureReferenceExpression` to an owned
expression whose evaluation yields `[Unevaluated(expr)]`/`[Element(expr)]`, which is
then evaluated). `evaluate_expression(expr, cx, args)` (REF §1.6 last
paragraph): if `expr`'s function is a library function, apply it with the
arguments as an `Arguments` frame; otherwise evaluate its result expression
in `cx` + `Arguments` frame; a `NotEvaluable` result becomes
`[Unevaluated(result_expression)]`; no result expression → `[Unevaluated(expr)]`.
Only `Feature`-like argument values are bound (REF §1.6: non-Feature
arguments are silently not bound; in Intersect every `Value` except
`Metaclass` counts as feature-like, because Pilot literals are Features — check
against the oracle and record).
- Tests: `if 1 < 2 ? "y" else "n"`; `if null ? 1 else 2` unevaluated;
  `false and <unevaluable>` = false, second never evaluated (assert with a step
  count or a body that would hit the step limit); `true or x` = true;
  `false implies x` = true; `null ?? 3 = 3`; `x ?? 3` with `x` unbound =
  `[Unevaluated(x)]`; `(1,2,3)->collect {in v; v * 2}` = `[2,4,6]`;
  `(1,2,3).?{in v; v > 1}` = `[2,3]`; `->reduce` not in model-level table →
  unevaluated; `null.{in x; 1}` = `[]`.

**M1.7 Constructors and `isModelLevelEvaluable`.** `ConstructorExpression`
→ `[Element(result parameter)]` or `NotEvaluable` (REF §1.4 item 6).
`evaluable.rs` implements the REF §3 table exactly, with a visited set; and
`evaluate_gated` (REF §3 "Gating": only invocations are gated; constructors
return `NotEvaluable` at top level; calc and constraint usages return `[]`).
- Tests: one per REF §3 table row and each bullet of "Consequences".

**M1.8 Format.** `format.rs`: `pilot_text` = REF §6.1 `formatElement`
without the element id (`LiteralInteger 7`, `LiteralRational 0.6666666666666666`,
`LiteralString b`, `LiteralBoolean true`, `LiteralInfinity *`,
`<Class> <name>` for elements, `MetadataFeature <AnnotatedClass> <name>` for
metaclass values, operator text for an unevaluated `OperatorExpression`).
`java_double`: Java `Double.toString` layout over Rust's shortest round-trip
digits: NaN → `NaN`, ±inf → `Infinity`/`-Infinity`, `0.0`/`-0.0`; if
`1e-3 <= |x| < 1e7` plain decimal with at least one fractional digit
(`4.0`, `0.001`), else `d.dddE<exp>` with at least one fractional digit
(`1.0E10`, `1.0E-4`, `6.02E23`). Derive digits and exponent from
`format!("{:e}", x)`. **M1 review fix 7:** when that shortest decimal has one
significant digit, select the nearest one- or two-digit decimal (ties to even),
as specified by JDK 19+ `Double.toString`; for example `2^-1074` becomes
`4.9E-324`. `outcome_json` per §4.
- Tests: the examples above plus `0.1+0.2` → `0.30000000000000004`, `1e7` →
  `1.0E7`, `9999999.0` → `9999999.0`, `-0.0` → `-0.0`, `1e-3` → `0.001`,
  `9.99e-4` → `9.99E-4`.

**M1.9 Debugging hooks.** Implement `EvalObserver`, `Control`, `Lookup`,
`ContextView`, `FrameView`, `Trace`, `trace_text` and `evaluate_observed` (§4).
`evaluate` and `evaluate_gated` call the same internal path with a no-op
observer (one implementation; no second evaluator). Rules:
- `enter`/`exit` bracket every evaluated expression, including operands,
  lazy branches that run, bodies per element in `collect`/`select`, and
  inlined function bodies (M4); `step` is the same counter as `EvalLimits::steps`.
- `skipped` fires for every operand that laziness leaves unevaluated, so a
  trace shows each operand once, run or skipped.
- `lookup` fires once per frame consulted in REF §1.6 step 4, plus once for
  the feature's own value (frame `NotHere` → own value).
- `apply` fires before a library function computes its result.
- Every `Reason` (§4) is produced at the point that decides it; no function
  returns an unevaluated or not-evaluable result without one.
- `Control::Stop` is how breakpoints and single-stepping are built on top
  (a host stops at a chosen step or expression and reruns to resume or step
  back; evaluation is deterministic, so a rerun reaches the same state).
- `trace_text` prints one line per event, indented by depth:
  `[12] enter OperatorExpression '+' at Vehicles.sysml:14:9  in Vehicle`,
  `[13] lookup mass in Vehicle -> no value (Unbound mass)`,
  `[14] exit '+' = unevaluated (operand 1: no value for mass in Vehicle)`.
  Positions come from `Obj::span` (byte offsets → line:column with
  `sysml_host::line_column`); with a `SourceMap`, positions in the `--expr`
  wrapper are shown relative to the user's text.
  **User-approved amendment, 2026-11-08:** source text is supplied by the host
  through `Trace::with_source(uri, text)`, since `Model` stores only resource
  identities and byte spans. Without host text, print explicit byte offsets;
  never invent line/column positions or read files. `SourceMap`'s span mapping
  is provided here for the trace signature; no M2 wrapper/loading API is added.
- The no-op path must cost nothing measurable: a quick test evaluates a
  10,000-step expression with and without a `Trace` and checks the outcomes
  are equal (no timing assertion).
- Tests: the trace of `if a > 1 ? a else b` with `a = 2` (enter/exit order,
  `b` skipped); `false and x` (operand 2 skipped); a lookup that stops at a
  redefinition without value (REF §7 item 9) shows `Unbound`; `Stop` at step 3
  returns `Stopped(3)`; every `Reason` variant is produced by at least one test;
  identical traces on two runs.

**Milestone M1 boundary:** run the M0.3 text tests (milestone tier) with a
pack, update §11 of this plan with results, commit.

### M2 API, sites and CLI

**M2.1 Sites.** `sites.rs`: `sites(model, resources)` lists, in resource then
containment pre-order, every expression that is (a) the owned expression of a
`FeatureValue`, (b) the owned expression of a `ResultExpressionMembership`, or
(c) a `MultiplicityRange` bound, in the named resources (by resource URI;
library resources are excluded unless named). Target rule (identical on the
Pilot side): (a) the nearest container of the owning feature that is a `Type`
(skipping the feature itself), else `None`; (b) the owning `Type` of the
membership; (c) the multiplicity's owning namespace (REF §4 row
`MultiplicityRange_valueOf`). Expressions nested inside another site's
expression are not separate sites.
- Tests: a fixture with one site of each kind; nested `1 + (2 * 3)` gives one site.

**M2.2 Expression text.** `pub fn eval_source(target: Option<&str>, expression: &str, n: usize) -> Result<(intersect_xtext::Source, SourceMap), EvalError>`
builds the Pilot's wrapper (REF §6.1): URI `memory:/intersect-eval/<n>.sysml`,
text `calc {\n<expr>}` or `calc {private import <target>::*;\n<expr>}`; and
`pub fn wrapper_expression(model, uri) -> Result<ObjId, EvalError>` finds the
calc's result expression. `eval_source` returns the `SourceMap` (§4) of
the user's text inside the wrapper. The user-approved M2.2 `Result` amendment
allows invalid target names to return an error without panicking.
Parse diagnostics and trace
positions in the wrapper are reported against the user's expression. The host loads it with `load_on_library` together
with the user files (a fresh load per invocation: the library is a snapshot,
user files are small). Reject `target` text that is not a qualified name
(`[A-Za-z_'][\w' ]*(::...)*` per the escaped-name rule) before building source.
- Tests (quick): wrapper text for both forms; rejection of `a; b`.
  (milestone, M0.3 helper): `1 + 2` → `[Int(3)]`; target `P` and `m * 2`.

**M2.3 `intersect eval` command.** Precondition: `crates/intersect-cli` is
committed on the branch (`git log -- crates/intersect-cli` non-empty). If not,
stop and report; do not write the CLI. In `crates/intersect-cli/src/host.rs`:
add `Command::Eval`, usage lines

```text
intersect eval --pack PACK [--target QNAME] [--mode model-level|extended] [--gated] [--json] [--trace] [--explain] (--expr TEXT | --all) [FILE...]
```

`--explain` prints the `Reason` of each unevaluated or not-evaluable result
in words; `--trace` prints `trace_text` to stderr (JSON lines of trace events
with `--json`).

`--expr` evaluates one expression (files optional); `--all` evaluates every
site of the given files (files required) and prints one line per site:
`<resource>:<line>:<col> <feature qualified name> = <values>` (text) or JSON
lines `{"site": {...}, "evaluable": bool, "outcome": {...}}`. Text values are
`pilot_text` joined by `, `; `NotEvaluable` prints `(not evaluable)`; `[]`
prints `()`. Default mode per decision E4 (default `extended` once M4 lands;
`model-level` until then). With `--all`, an `EvalError` at one site is
reported for that site and the remaining sites are still evaluated; the exit
status is then 2 (amended 2026-11-09). Exit status 0 when every evaluation ran (whatever
the values), 1 on load diagnostics, 2 on usage/host errors or `EvalError`.
Add `intersect-kerml-eval` to the CLI's dependencies. Update
`docs/cli-tutorial.md` with an "Evaluate expressions" section (three examples
from the corpus with their actual output).
- Tests: CLI argument parsing unit tests (quick); one milestone test running
  `--expr "1 + 2"` against the pack.
- Accept: the commands with `-p intersect-cli` added; manual run on
  `kerml/src/examples/Simple Tests/Expressions.kerml` recorded in §11.

### M3 Pilot comparison (gate GE, see §8)

**M3.1 Pilot side.** `tools/pilot-oracle/PilotEvalOracle.java` (handwritten,
header as `PilotOracle.java`: "independent verification oracle, never
production"). Usage: `PilotEvalOracle OUT.json LIBRARY_ROOT ROOT FILE...`.
It loads the library and the files exactly like `PilotOracle.load`, then for
every site of §M2.1 in the given files (same rules; implement them in Java over
EMF: `FeatureValue.getValue()`, `ResultExpressionMembership.getOwnedResultExpression()`,
`MultiplicityRange.getLowerBound()/getUpperBound()`), records:
`resource`, EMF fragment of the expression, target fragment, `evaluable`
(`isModelLevelEvaluable()`), `model_level` (`ModelLevelExpressionEvaluator.INSTANCE.evaluate(e, target)`),
`gated` (`e.evaluate(target)`), `extended`
(`org.omg.sysml.execution.expressions.ExpressionEvaluator.INSTANCE.evaluate(e, target)`),
each normalised to the §4 value JSON: literals by value (`LiteralRational`
with `Double.doubleToLongBits` hex), elements contained in a resource by
`{resource, fragment, class}`, a metaclass feature as `metaclass` of its
annotated element, any other unowned element as `{"kind":"synthetic","class":...}`,
Java `null` as `not-evaluable`. Catch exceptions per site and record
`{"outcome":"exception","message":...}`. Commands are documented in
`tools/pilot-oracle/README.md` (new section "Expression evaluation"), using
the existing download and JDK 21 instructions.

**M3.2 Intersect side.** The `intersect eval --all --json` output (M2.3), run
with `--mode model-level`, `--mode model-level --gated` and `--mode extended`,
plus the `FROZEN_KERML_LINKED_DUMP`-style model export the existing G5 flow
already produces for pairing (or `intersect export --format json`).

**M3.3 Comparison.** `tools/pilot-oracle/compare_eval.py` (+ unit tests in
`tools/pilot-oracle/test_compare_eval.py`, run with
`python -B -m unittest discover -s tools/pilot-oracle -p "test_*.py"`): import
the containment pairing from `compare.py` (reuse, do not copy), map each
Pilot site and each element value to the Intersect identity, and compare per
site and per mode: `evaluable` equal; outcomes equal under the value rules of
§4 (reals by bits, NaN canonical; `element` and `unevaluated` both compare by
paired identity, since the Pilot does not distinguish them; `synthetic` on
the Pilot side matches `element`/`metaclass`/`unevaluated` of the same class
only if listed in the deviations file). Output JSON: per file
`{sites, agree, differ, pilot_only, intersect_only}`, per mode totals, and
every difference with both values and source location. Allowed differences:
`tools/pilot-oracle/eval-deviations.json`, entries
`{"mode": "...", "function"|"class"|"sites": ..., "reason": "..."}`
(same style as `deviations.json`; never a blanket entry).

**M3.4 Corpus run (milestone).** Corpus: all files of
`target/pilot-oracle/upstream` that load in Intersect without diagnostics
(the list comes from the G4/G5 corpus run of the CLI session; files that do
not load are reported as excluded, not counted as agreeing), plus the D9
files. Run both sides, compare, fix Intersect or add justified deviation
entries until the gate of §8 holds. Record the table in §11 and in
`tools/pilot-oracle/README.md`.

### M4 Extended evaluator

**M4.1 Extended mode switch.** In `Extended` mode: an invocation of a
non-library function inlines its result expression (REF §1.7: arguments are
evaluated **eagerly in the caller's context**, bound as an `Arguments` frame;
the instantiated type, if a Feature, is pushed as a `Type` frame); an unbound
`in`/`inout` parameter evaluates to `[]` (REF §1.7 `evaluateFeature` override).
Library functions with bodies that are not in the table also inline.
Recursion is bounded by `EvalLimits::depth`.
- Tests: user `calc def Add { in a; in b; return a + b; }`, `Add(1,2)` = 3;
  named arguments; `Fact` recursion to 10; recursion without base case →
  `DepthLimit`; unbound parameter → `[]`; constraint invocation through a feature.

**M4.2 Extended function table.** REF §2.2 rows: `ControlFunctions::{forAll,
exists, reject, selectOne, reduce, maximize, minimize}`, `DataFunctions::{max,
min}`, `NumericalFunctions::{sum, product}`, `SequenceFunctions::{size,
isEmpty, notEmpty, includes, includesOnly, excludes, equals, same, including,
union, includingAt, excluding, intersection, excludingAt, subsequence, head,
last, tail}`, `StringFunctions::{Length, Substring}` (UTF-16 units),
`TrigFunctions::{sin, cos, tan, cot, arcsin, arccos, arctan, deg, rad}`.
Reproduce the `excludingAt` precedence quirk only if the oracle shows it
matters; otherwise implement the evident intent and list a deviation.
Trig: use `f64` methods (`sin`, `cos`, `tan`, `1.0/tan`, `asin`, `acos`,
`atan`, `to_degrees`, `to_radians`); Java `Math` and Rust may differ in the
last ulp: compare such results with a 1-ulp tolerance in `compare_eval.py`
only for these functions, and list that tolerance in the deviations file.
- Tests: one per function including the empty and `[inv]` cases of REF §2.2.

**Milestone M4 boundary:** rerun M3.4 for all three modes.

### M5 Metadata, reflection, conditions

**M5.1 Metadata access.** `MetadataAccessExpression` → metadata features of
the referenced element (owned annotations whose annotating element is a
`MetadataFeature`, plus owned-member `MetadataFeature`s annotating it), then
`Metaclass(element)` (REF §1.4 item 4). `'@@'`, `meta` (REF §2.1). Metadata
frames in the context loop (REF §1.6 step 4 first two bullets): a
`MetadataFeature` frame answers `annotatedElement`; a `Metaclass` frame reads
the element's stored feature with the feature's declared name through
`Metamodel::feature` (attributes become scalar values, references
`Metaclass(target)` per REF §1.6 `results`; enumeration values: decision
recorded after the oracle run, since the Pilot adds a null element there).
Type tests on `Metaclass(e)`: its type is the library metaclass named by `e`'s
metaclass in the `KerML`/`SysML` library packages.
- Tests: `P.metadata` order; `@SysML::PartUsage` on a part's metaclass;
  `(P meta SysML::PartDefinition).isAbstract`; `Safety::isMandatory` through a
  metadata usage.

**M5.2 Conditions and bounds.** `check_condition`, `check_conditions_on`,
`multiplicity_bound` per REF §3 and §4 (candidates = metadata features plus
metaclass; every condition must hold for some candidate; no candidates →
`check_condition(c, None)`). These are API only (decision E5).
- Tests: filter `@Safety` true/false; `[0..*]` → `Some(0)`, `Some(-1)`;
  `[1.5]` → `None`; unevaluable bound → `None`.

**Milestone M5 boundary:** rerun M3.4; update the coverage table (§7) status
column and §11.

## 7. Operator and function coverage

Phase = the task that implements it. "Pilot mode" = the Pilot evaluator that
evaluates it (ML model-level, X extended only, — neither). Semantics: REF row.
Native test coverage does not mean that the corpus exercises every function.
Qualification and any allowed differences are reported separately under §8/§11.

| Syntax / function (escaped qualified name) | Phase | Pilot mode | Notes | Native status |
| --- | --- | --- | --- | --- |
| Literals `1`, `1.5`, `"s"`, `true`, `*` | M1.2 | ML | Real = `LiteralRational`, f64 | Implemented; native tests pass |
| `null`, `()` | M1.2 | ML | `[]` | Implemented; native tests pass |
| Feature reference `a`, `P::a` | M1.5 | ML | context lookup REF §1.6 | Implemented; native tests pass |
| `self` | M1.5 | ML | the innermost context | Implemented; native tests pass |
| `BaseFunctions::','` `(a, b)` | M1.4 | ML | flattens | Implemented; native tests pass |
| `BaseFunctions::'=='`, `'!='`, `'==='`, `'!=='` | M1.4 | ML | `===` = `==` (Pilot) | Implemented; native tests pass |
| `BaseFunctions::'#'` `s#(i)`, `a#(i,j)` | M1.4 | ML | 1-based, row-major arrays | Implemented; native tests pass |
| `BaseFunctions::istype`, `hastype`, `'@'`, `as` | M1.4 | ML | literal types REF §2.4 | Implemented; native tests pass |
| `BaseFunctions::'@@'`, `meta` | M5.1 | ML | metadata lists | Implemented; native tests pass |
| `BaseFunctions::'['` (`5 [kg]`) | — | — | unevaluated (E6) | Intentionally unmapped |
| `BaseFunctions::all`, `ToString` | — | — | unevaluated | Intentionally unmapped |
| `DataFunctions::'+'`, `'-'`, `'*'`, `'/'`, `'**'`, `'^'`, `'%'` | M1.3 | ML | `/`, `**` real; `/0` `[]`; E3 | Implemented; native tests pass |
| `DataFunctions::'<'`, `'<='`, `'>'`, `'>='` | M1.3 | ML | strings by UTF-16 | Implemented; native tests pass |
| `DataFunctions::'..'` | M1.3 | ML | inclusive | Implemented; native tests pass |
| `DataFunctions::not`, `'&'`, `'\|'`, `xor` | M1.3 | ML | non-Boolean = false | Implemented; native tests pass |
| `DataFunctions::'~'` | — | — | unevaluated | Intentionally unmapped |
| `DataFunctions::max`, `min` | M4.2 | X | | Implemented; native tests pass |
| `ControlFunctions::'.'` (`x.f`) | M1.5 | ML | non-features skipped | Implemented; native tests pass |
| `ControlFunctions::if`, `and`, `or`, `implies`, `'??'` | M1.6 | ML | lazy | Implemented; native tests pass |
| `ControlFunctions::collect` (`.{}` `->collect`), `select` (`.?{}`) | M1.6 | ML | | Implemented; native tests pass |
| `ControlFunctions::reject`, `selectOne`, `reduce`, `forAll`, `exists`, `maximize`, `minimize` | M4.2 | X | | Implemented; native tests pass |
| `ControlFunctions::allTrue`, `anyTrue` | M4.1 | X | inlined library bodies | Implemented; native tests pass |
| `SequenceFunctions::*` (18 functions) | M4.2 | X | | Implemented; native tests pass |
| `CollectionFunctions::size`, `contains`, `'=='`, `'#'` | M4.1 | X | inlined bodies | Implemented; native tests pass |
| `StringFunctions::Length`, `Substring` | M4.2 | X | UTF-16 units | Implemented; native tests pass |
| `StringFunctions::'+'` etc., `ScalarFunctions::*`, `RealFunctions::*`, `IntegerFunctions::*`, `NaturalFunctions::*`, `RationalFunctions::*`, `ComplexFunctions::*`, `BooleanFunctions::*`, `VectorFunctions::*` | — | — | not matched by name in the Pilot (REF §1.5); unevaluated, or inlined in X when the library gives a body | Intentionally unmapped |
| `NumericalFunctions::sum`, `product` | M4.2 | X | | Implemented; native tests pass |
| `TrigFunctions::pi` | M1.5 | ML | library feature | Implemented; native tests pass |
| `TrigFunctions::sin` … `rad` | M4.2 | X | 1-ulp tolerance | Implemented; native tests pass |
| User `calc def`/`function` invocation | M4.1 | X | ML: unevaluated | Implemented; native tests pass |
| Constructor `new T(...)` | M1.7 | ML | result parameter element | Implemented; native tests pass |
| `X.metadata`, metaclass reflection | M5.1 | ML | | Implemented; native tests pass |
| `isModelLevelEvaluable` | M1.7 | ML | REF §3 | Implemented; native tests pass |
| `checkCondition`, filters, multiplicity bounds | M5.2 | ML | API only (E5) | Implemented; native tests pass |
| Trigger invocations (`at`, `after`, `when`) | — | — | unevaluated | Intentionally unmapped |

## 8. Pilot comparison gate (GE, not a release gate)

- **Corpus:** the evaluation sites (§M2.1) of every corpus file that loads in
  Intersect without diagnostics: the D9 files and the `target/pilot-oracle/upstream`
  examples (157 files, 67 with value expressions). Library sites are a second,
  optional corpus (run once at M5 and reported, not gated).
- **Metric:** per site and per mode (model-level ungated, model-level gated,
  extended): `evaluable` and outcome equal under §M3.3 rules. Report counts
  per file and totals; never a percentage without its denominator.
- **Pass:** every paired site agrees or is covered by an
  `eval-deviations.json` entry with a specific reason; zero unpaired sites
  except files excluded for load failures, which are listed by name. The
  deviations file is reviewed by the user (decision E7 decides whether GE
  becomes part of a release).
- **Evidence:** the compare report JSON committed under
  `tools/pilot-oracle/results/eval-<date>.json`, the commands, both revisions
  and the JDK version, summarised in §11.

## 9. Risks

| # | Risk | Mitigation |
| --- | --- | --- |
| R1 | The Pilot creates EMF objects (target features, invocation copies, metaclass features); emulating them with frames may differ in corner cases (lookup order, which element an unevaluated result names) | Follow REF §1.6 step by step; oracle compares element identities; differences become fixes or listed deviations |
| R2 | Pilot quirks (`not null` true, `1 == 1.0` false, int32 wrap, unary `-` on non-number `[]`) look like bugs to users | Pilot-compatible by default (E2) with one named test per quirk; a strict mode can follow if the user wants |
| R3 | `all_features`, redefinition closure and `specializes` re-implement in Rust what Rcore name resolution also computes (AGENTS.md: avoid a second implementation) | Keep them in `nav.rs` only, documented as evaluation queries; E8 decides whether to move them to Rcore once a `FastRead` for a validated `Model` exists |
| R4 | Implicit specializations are not stored, so `istype Anything`, `self` through implicit `Occurrence::self` subsetting, and inherited library features may differ | Measure in M3.4; list as deviations; the fix (storing implicit specializations) belongs to the G5 work, not here |
| R5 | Corpus files that do not load (name resolution still being fixed in the CLI session) shrink the corpus | Report excluded files; rerun M3.4 when the CLI session lands its fixes |
| R6 | The oracle jar was built 2026-09-11; REF was read from the commit | The oracle run is the authority; REF is corrected when they disagree |
| R7 | f64 differences: Java `Math` vs Rust libm in trig/pow; `Double.toString` vs Rust formatting | Bit-exact comparison except listed trig functions (1 ulp); `java_double` tested against Java outputs |
| R8 | Per-call indexes (`all_features`, library index) are slow on the 110k-object library | Build `LibraryIndex` once per `Evaluator`; memoise `all_features`/closures per evaluator in `RefCell<BTreeMap>`; measure `--all` on the corpus in M3.4 and record |
| R9 | The CLI is being written concurrently; editing `host.rs` conflicts | M2.3 waits until the CLI is committed and touches only the eval additions |
| R10 | Evaluating text needs a fresh load (parse + link + validate of the whole model, about 1.5 s) per `--expr` | Acceptable for a CLI; an incremental load is frozen-engine edits work (roadmap 8), out of scope |

## 10. Open decisions for the user

**E1 — Where the evaluator lives.** Default: new crate `intersect-kerml-eval`
(keeps `intersect_kerml::SOURCE_IDENTITY` and therefore packs and binding caches
stable). Alternative: a module in `intersect-kerml` (one crate fewer; every
evaluator edit invalidates packs). Also confirms that a native Rust
interpreter for the KerML function library is compatible with D7 (rules in
Rcore; library function implementations in the language package).

**E2 — Pilot-compatible or specification semantics.** Default: reproduce the
Pilot, quirks included (REF §7), since it is the oracle. Alternative: follow
the KerML specification where the Pilot visibly diverges (`===` identity,
`1 == 1.0`, `not null`), with those cases as deviations.

**E3 — Integer width.** Default: `i64` with checked arithmetic; overflow is
unevaluated; any corpus site where the Pilot's 32-bit wrap differs is a
deviation. Alternatives: Java `int` wrapping (exact Pilot), or arbitrary
precision (`BigInt`, closer to KerML `Integer`).

**E4 — Default mode of `intersect eval`.** Default: `extended` (what `%eval`
does) once M4 lands, `model-level` before; `--mode` selects.

**E5 — Using the evaluator inside loading.** Filtered imports in name
resolution, view filters, the multiplicity-bound diagnostic
(`validateMultiplicityRangeBoundResultTypes`) and the model-level-evaluable
checks on metadata bodies and filter conditions. Default: not in this plan;
a follow-up design once M5 passes, because it couples Rcore scopes to the
evaluator and changes G5 diagnostics.

**E6 — Units and quantities.** Default: none beyond the Pilot (unevaluated).
Alternative: a later plan for `'['`, `ISQ`/`SI` and quantity arithmetic,
which the Pilot cannot verify.

**E7 — Status of gate GE.** Default: a workstream gate recorded in this plan,
not a Release 1 gate. Only the user may add it to `release-plan.md`.

**E8 — Structural queries in Rust or Rcore.** Default: Rust in `nav.rs`
(R3). Alternative: implement `FastRead` for a validated `Model` in
`intersect-model` and run Rcore helper functions; needs a host API for calling
non-binding Rcore functions.

**E9 — Library imports in the `--expr` wrapper.** The wrapper adds no
imports, so `(1..5)->collect {in x; x * x}` fails to link `collect`; REF does
not say what the Pilot's `%eval` imports. Options: (a) no imports, the user
writes qualified library names (`->ControlFunctions::collect`); (b) the wrapper
adds `private import ControlFunctions::*;` and the other standard function
packages; (c) match the Pilot once its `%eval` scope is observed. Default:
(a), with a hint on unresolved names inside the expression (2026-11-09).

## 11. Log

- 2026-11-09, M2 fix round (applied after the M0–M5 merge, on
  `claude/eval-m2-fixes`):
  1. `--all` reports every site in text and JSON; an `EvalError` at one site
     prints `<path>:<line>:<col> <name> = (error: <message>)` (JSON: an
     `exception` outcome row) and evaluation continues. The command exits 2 at
     the end, naming how many sites failed (M2.3 amended; previously text
     output stopped at the first such site and JSON exited 0).
  2. `trace_text` names elements by their Pilot-style text instead of
     internal identities, prints values with `values_text` (and
     `(not evaluable: <reason>)`), drops the empty ` in ` context suffix, and
     no longer prints `pending` operands. `apply` is still reported before
     the operands are evaluated (operand evaluation is part of each function,
     and lazy functions evaluate only some operands); the operand values are
     on the nested `exit` lines (M1.9's `apply ... to (2, 3)` example is
     superseded). JSON traces are unchanged. `reason_text` and `values_text`
     moved from the CLI to `format` so the CLI and traces share them.
  3. `--all` text labels mark non-value sites: `[multiplicity]`, `[result]`.
  4. `--all` text shows the file path as given (`Simple Tests/...`), not its
     percent-encoded URI; JSON sites add `path` beside `resource`.
  5. E9 recorded; unresolved names inside the `--expr` wrapper get a hint to
     use qualified library names.
  6. Specialized calculations: an inherited input parameter is bound to the
     calling type's owned parameter at the same position when that parameter
     declares no redefinitions (KerML implicit parameter redefinition, which
     the frozen model does not record). This fixes `calc def D :> B { in a; }`
     with B's inherited result `a`: `D(3)` was `()`, the Pilot gives 3, now 3.
     It applies to inlined calls (Extended mode) only.

- 2026-11-09, M5.2 implementation and verification: added API-only
  `check_condition`, `check_conditions_on` and `multiplicity_bound` (E5).
  Conditions always use the model-level gate, including on Extended
  evaluators; only a single true Boolean satisfies a condition. Filter
  candidates share one step budget. Multiplicity accepts natural integers
  and infinity, using the range's owning namespace. Seven quick cases and
  a separate parsed metadata-filter/bounds test cover these APIs. All four
  task checks passed: 203 quick tests passed, 7 ignored
  (`20261109-083825`). The final standard-library-backed milestone suite
  passed 220 tests with none ignored (`20261109-083917`): 210 evaluator and
  10 CLI tests, including parsed metadata/filter/bounds and extended cases.
  The coverage table separates native implementation/testing from corpus
  qualification. GE satisfies §8 with the user-reviewed deviations; M6 and
  release-gate integration are outside this work.

- 2026-11-09, M5.1 implementation and verification: implemented metadata
  access, `@@`, `meta`, virtual metaclass contexts, stored-field reflection,
  annotatedElement lookup and metaclass type tests without synthetic objects.
  Present reflected enums return Unsupported (approved); unset fields return
  an empty sequence. Derived fields remain explicit Unsupported results.
  Observed Pilot clarifications relative to REF: metaclass names use the
  nested KerML::Root/Core/Kernel and SysML::Systems packages; the pinned
  inverse navigation omits an Annotation containing its own annotating
  metadata element. Owned annotation relationships and implicit namespace
  targets work normally. The implicit self referent is derived from the
  library when no explicit reference is stored. Eighteen independent Pilot
  assertions and native metadata tests cover these behaviors. All four
  checks passed on the isolated task: 196 quick tests passed, 6 ignored
  (`20261109-083528`). Parsed metadata reflection passed separately and is
  included again in the final M5 boundary suite.

- 2026-11-09, M4.2 implementation and verification: added all 40 mapped
  Extended functions in REF §2.2, with 47 quick function tests plus a parsed
  text test. CLI default is Extended, under the approved scope amendment.
  UTF-16 length/substrings preserve valid surrogate pairs; a split surrogate
  returns EvalError under the approved representation limit. The independent
  Pilot probe confirms the `excludingAt` precedence/omitted-end behavior and
  the distinct empty-string/empty-sequence cases (6 assertions). Strict
  clippy, formatting, quick tier and WASM passed on the isolated task:
  191 passed, 6 ignored, including 9 CLI tests (`20261109-083411`). The M4
  parsed boundary run before the final binding fix passed 195 tests; the M5
  final suite reruns these cases. Corpus numbers and approved GE result are
  recorded in the M3.4 entry and final JSON report.

- 2026-11-09, M3.4 final corpus run (GE satisfied; user review approved): compared 357 sites
  in 47 admitted files (21 independent groups), in all three modes. All
  producers exited successfully with zero load diagnostics and zero unpaired
  sites. The 110 files excluded by the earlier G4 load report are listed by
  name and reason in `tools/pilot-oracle/results/eval-2026-11-09.json`; they are
  not counted as agreeing. Raw ModelLevel: 356 exact + 1 approved exception
  allowance; gated: 357 exact; Extended: 353 exact + 4 approved exception
  allowances. Across modes: 1,066 exact observations of 1,071. The four sites
  are Expressions::z, MassRollup_1/2::MassedThing::totalMass, and
  MassRollup::filteredMassThing::totalMass. Each allowance matches only Pilot
  `java.lang.StackOverflowError` versus native depth limit 256, at its exact
  source identity and mode. Nine named trig 1-ULP tolerances have zero corpus
  hits. The user approved the final list on 2026-11-09 under §8.
  Corpus-discovered fixes: active-path evaluability visitation, Expression
  referent fallback, direction-based input parameters, owned lazy bodies, and
  re-evaluation of bound Feature references in the body/callee context. The
  last fix reproduces REF §1.6/1.7 instead of freezing an unevaluated argument;
  two focused native tests and two independent Pilot assertions cover it.
  All four checks passed on the isolated task: 135 quick tests passed,
  4 milestone tests ignored (`20261109-083103`). Python oracle/comparator
  protocol tests: 57 passed. Oracle harness: 49 assertions passed. JDK:
  Temurin 21.0.6+7; Pilot revision `692170b71867353b8f90341e61556f49a5beb0e5`.
  The final corpus uses the cumulative M5 implementation; the report records
  its exact source hashes and independently tested task snapshots. This is
  expression qualification, not a new whole-model G5 or release gate.
  Native `--all` timing samples before the final binding fix: D9 65 sites,
  13.815 s; association examples 67 sites, 10.377 s; vehicle decomposition
  43 sites, 9.643 s. These include pack loading, parsing/linking, evaluator
  indexing and output, not just evaluation. The optional library-site corpus
  was not run; the complete standard library supplied evaluation context.

- 2026-11-09, corpus collection strategy correction: a proposed shared-library
  batch passed an exact fresh-load comparison for its first (D9) group, but
  the next Address Book group acquired 49 diagnostics absent from its fresh
  load. The batch's equivalence guard stopped it. An unchanged library-export
  digest was insufficient to establish safe reuse; those reused-load results
  are discarded. The replacement `collect_eval.py` uses an independent JVM,
  parser, resource set, full library resolution and validation for every group,
  with at most three workers and all children reaped. `--model-out` invokes
  the unchanged G5 exporter on that same fresh load inside observation rollback.
  The valid first D9 report exactly matches the earlier isolated fresh report;
  Address Book uses its earlier fresh report/export. This is verification
  harness state contamination, not a native semantic discrepancy.

- 2026-11-09, M3.3 comparison tooling completed: added the identity-based
  three-mode comparator and exact deviation selectors. The approved
  `include_pairs=True` export reuses `compare.py`'s existing traversal and
  preserves its default report. **49 Python tests pass**. The oracle restores
  source-model mutations after each query/mode and retains pre-evaluation
  identities; reports without that isolation marker are rejected. **49 Java
  assertions pass**, including reparenting recovery and reuse of the unchanged
  G5 exporter through a verification-only reflective adapter. No protected
  PilotOracle source was changed or export algorithm copied.
  First comparisons against the current native draft: D9 has 64 exact sites
  across all modes and one recursive site differing only in raw model-level
  and Extended exception messages (native DepthLimit vs Pilot StackOverflowError).
  All 65 gated D9 sites agree; Address Book's one site agrees in all modes.
  No sites are missing in these subsets. No depth-limit waiver was silently
  added, and GE remains incomplete pending the full corpus and deviation review.
  All four checks pass: format (155 targets), strict Clippy (incremental cache
  disabled to avoid filesystem hard-link warnings), WASM, and **201 evaluator
  quick tests passed, 6 ignored**, report `20261109-064620`.

- 2026-11-09, M3.2 collection completed: the approved CLI change records
  evaluation exceptions as JSON rows in `--all --json` and continues to later
  sites. Refreshed all 21 admitted groups (47 files), all three modes:
  **63 commands succeeded**, **357 sites per mode**. Model-level has 356 value
  outcomes and one DepthLimit; gated has 282 values and 75 not-evaluable;
  Extended has 354 values and three DepthLimits. D9 retains 61 rows after its
  recursive site; the two Mass Roll-up errors retain 18 and 17 later rows.
  Per-group model exports and full output/provenance are retained under
  `target/eval-m3-d9-model/`, `target/eval-m3-examples/` and
  `target/eval-m3-approved/native/`. This establishes native collection, not GE.
  All four required checks passed with CLI included: format (155 targets),
  strict Clippy, WASM, **210 quick tests passed, 7 ignored** (201 evaluator,
  9 CLI; report `20261109-063250`). Clippy's cached filesystem hard-link
  fallback warnings are build-cache diagnostics, not lint failures (exit 0).

- 2026-11-09: the user approved all three pending amendments: the prepared
  CLI/comparator patch, `EvalError` for Substring results containing a lone
  UTF-16 surrogate, and explicit Unsupported for reflected enum values that
  the Pilot converts to `[Java null]`. The public `Value` representation stays
  unchanged. These representation limitations must remain explicit; they are
  not exact Pilot agreement. No other protected-file edit was authorized.

- 2026-11-08, additional M3 corpus preparation (no agreement claim):
  isolated folder-01 and folder-02 Pilot evaluation runs completed with **1
  and 4 sites**, respectively, in **3 files**, with zero load diagnostics.
  All five sites produced value outcomes in all three modes. Together with
  isolated D9, the available Pilot observations cover 70 of the native
  corpus's 357 sites; remaining groups have not been verified. The original,
  unchanged PilotOracle completed folder-01's full model export (95 resources,
  109,847 objects; cold and warm loads; 3359.2 s). The unchanged `compare.py`
  pairs all **21 user objects**, with **0 unpaired** on either side, against
  the CLI export. Its three reference differences are library targets outside
  the selected user-resource pairing scope, not resolved expression differences.
  This preflight does not extract the private map or compare expression results.
  Reports, hashes and commands are under
  `target/pilot-oracle/eval-isolated-examples/`, including `batch-01.json` and
  `folder-01-pairing-preflight.json`. All task-owned processes are terminal.
  Prepared the unapplied `target/eval-scope-amendment-proposed.patch`; its
  `git apply --check` passes. Protected files remain unchanged. The repeated
  CLI/comparator and representation approval requests remain unanswered;
  M3.2–M3.4, M4.2 and M5 cannot be completed or committed under the current
  restrictions. No new task commit or push was made.

- 2026-11-08, M5 focused oracle follow-up (draft; GE still incomplete):
  added `PilotEvalMetadataTest.java`, with **18 passing assertions** for
  metadata order, stored Boolean/string reflection, conditions and bounds.
  The in-memory fixture provides one library metaclass via Pilot's library
  provider adapter; it uses the actual evaluator and does not load a pack or
  parser. Its initial run expected a containing Annotation to contribute its
  target; after verifying both endpoints and containment, the pinned Pilot
  instead returned an empty target list. The bytecode confirms that
  `owningAnnotatingRelationship` uses the Relationship container delegate.
  E2 therefore makes the Rust draft omit that containing annotation too;
  metadata-owned annotations and implicit namespace targets still work.
  This is a Pilot detail missing from REF's metadata summary, not a new
  specification-semantics choice. A later fixture failure was caused by its
  absent library metaclass; providing that identity fixed the fixture.
  All four checks pass after the native correction: format (155 targets),
  strict Clippy, WASM and **197 quick tests passed, 6 ignored** (report
  `20261108-222822`; unit 9.43 s, integration 8.69 s). The focused pack-backed
  metadata run passed **10 tests, 0 ignored** in 32.30 s (report
  `20261108-222925`). No task commit or milestone completion is claimed;
  scope/representation amendments and identity-paired corpus comparison remain
  pending. The pre-correction native corpus outputs retain their source hash
  and must be refreshed after the remaining implementation changes.

- 2026-11-08, M3 corpus preparation and oracle isolation correction (draft):
  built the unchanged CLI against the current evaluator and collected all
  three modes over the 20 admitted example groups (41 files). Raw model-level:
  **292 value outcomes**, all 20 groups complete. Gated: **246 value outcomes,
  46 not-evaluable**, all 20 groups complete. Extended: **273 value outcomes**,
  19 groups complete; folder-04 stops after 2 of its 21 sites with DepthLimit
  at MassRollup_1::MassedThing::totalMass. Its remaining 19 sites are missing,
  not agreeing. These counts include unevaluated elements inside value outcomes.
  Artifacts and evaluator-source hash are in
  `target/eval-m3-examples/native-current/summary.json` and `counts.json`.
  The current D9 gated run still has 65 sites: 36 values and 29 not-evaluable.
  Across D9 and all admitted example groups, gated evaluation covers 357
  distinct sites: 242 primitive-only results, 39 unevaluated elements, one
  model element and 75 not-evaluable results. Twelve admitted files have no
  expression sites. These are native coverage counts, not Pilot agreement.
  The aggregate is `target/eval-m3-examples/native-current/gated-corpus-counts.json`.
  No identity-paired agreement counts were obtained.

  Investigating the old D9 test2 evaluability difference exposed oracle
  contamination: Pilot argument binding reparents source Expressions into
  temporary FeatureValues. The focused edge suite now has **6 passing
  assertions**, including this mutation. Earlier sites/modes can therefore
  alter later observations in the old sequential reports. Stopped the three
  newly launched, obsolete example-oracle processes; none is left running.
  Added per-query/per-mode EMF ChangeRecorder rollback and captured source
  identities to PilotEvalOracle (no protected PilotOracle.java change).
  The oracle regression suite passes **44 assertions**, including restored
  source containment/evaluability after normal return and StackOverflowError.
  Its initial rollback fixture used bare Feature resource roots; switching
  to the normal namespace-owned model shape made the fixture representative.
  Existing reports without the new observation_isolation marker are historical
  diagnostic evidence only and need reruns before qualification. The comparator
  rejects them; **48 Python tests pass**. The replacement isolated D9 run
  completed with **65 sites in 6 files, 0 load diagnostics**. Raw model-level
  and Extended each produced **64 value outcomes and 1 exception**; gated
  produced **36 value outcomes and 29 not-evaluable**. Isolation changed four
  sites versus the old report (Expressions.kerml lines 17, 18, 19 and 66).
  In particular, test2 at line 66 is now not model-level evaluable, resolving
  the apparent evaluability discrepancy without a native semantic change.
  The source-expression reparenting probe and isolated rerun establish this
  as oracle cross-observation contamination. No identity-based native/Pilot
  agreement count is claimed. Reports are
  `target/pilot-oracle/eval-m3-d9-pilot-isolated.json` and
  `eval-m3-d9-isolation-changes.json`. All task-owned processes are terminal.
  This is an oracle harness fix, not a native semantic deviation. The scope
  and representation questions recorded below remain open.

- 2026-11-08, M4.2/M5 implementation checkpoint (uncommitted; GE incomplete):
  Extended dispatch covers 39 of the 40 requested functions. Substring remains
  pending the requested UTF-16 representation decision: the focused pinned
  Pilot probe returned a lone high surrogate for Substring("😀",1,1), which
  `Value::Str(Rc<str>)` cannot encode. The same probe confirmed the excludingAt
  precedence quirk, now reproduced with an EvalError instead of a Java cast
  exception on its invalid-index path. Five Java edge assertions passed.
  Proposed one-ulp trig tolerances are restricted to the nine named Extended
  trig functions; the comparison unit suite passes 47 tests. These are not
  user-reviewed corpus deviations or an identity-paired agreement claim.

  M5 draft code implements ordered metadata access, @@/meta, virtual metaclass
  frames, annotatedElement, stored scalar/reference reflection, metaclass
  typing, model-level condition APIs and multiplicity bounds. Enum reflection
  remains unfinished pending the requested decision: the Pilot probe returns
  a list containing Java null, for which Value has no item representation.
  No loader, scoping, grammar, model-store or CLI integration was added.

  Prepared-library testing exposed two navigation details absent from the
  constructed fixtures. ElementUtil.getMetaclassOf searches KerML::Root,
  KerML::Core, KerML::Kernel and SysML::Systems in that order; the shorter text
  names use public imports. An empty FeatureReferenceExpression (the grammar's
  implicit self in @Safety) derives Base::Anything::self in the Pilot setting
  delegate. Both are now implemented without synthetic objects or parser
  changes, with quick regressions and parsed acceptance coverage. The latter
  clarifies REF's referent-null case: with the self library present this
  reference has a derived self referent, rather than a null referent.

  The M3.4 draft also fixes active-path evaluability visitation, Expression
  referent fallthrough, and directed-feature parameter discovery. Quick
  fixtures now own lazy/control bodies through FeatureMembership, matching
  OwnedExpressionReference, instead of referring to detached bodies.

  All four current-work checks passed: formatting (155 targets), strict locked
  clippy, quick tier **197 passed, 6 ignored, 0 failures** (`20261108-205258`),
  and locked WASM build. The full evaluator milestone run with the compatible
  `target/eval-m2-current.pack` passed **203 tests, 0 ignored, 0 failures**
  (`20261108-205136`). It includes parsed metadata reflection, @Safety matching
  and rejection, [0..*] bounds, extended standard-library allTrue/anyTrue bodies,
  selectOne, numeric reductions, sequence size, UTF-16 Length and trig. These
  are native tests, not Pilot comparisons. Initial parsed-test failures and a
  stale no-referent trace expectation were corrected before these final runs.

  M3.2–M3.4, M4.2 and the M5 boundary remain incomplete. The restricted CLI
  error-collection/default-mode changes and compare.py pairing-map export
  still await the scoped amendment requested earlier. Substring and reflected
  enums await their representation decisions. Full three-mode identity-paired
  corpus counts, deviation review, coverage qualification, per-task commits
  and push remain outstanding; no milestone completion is claimed.

- 2026-11-08, M4.1 library implementation (M3 gate still incomplete): under
  the user's goal to finish M5, implemented independent Extended-mode calls
  while the restricted M3 CLI/pairing amendments await approval. Calls bind
  eagerly evaluated arguments in owned order, preserve formal positions for
  named arguments, use inherited result bodies, and distinguish recursive
  call frames so omitted inputs cannot capture an outer call's bindings.
  Defaults, unencodable actuals, unbound in/inout, feature call contexts and
  bounded recursion have focused coverage. The internal Call frame is exposed
  to observers through the existing Arguments view; no synthetic objects.
  Model-level dispatch/evaluability and the CLI default are unchanged.

  All four task checks passed: format (155 Rust targets), strict locked
  clippy, quick tier **130 passed, 4 ignored, 0 failures** (report
  `20261108-200715`, unit/integration binaries 1.83/1.69 s), and locked WASM
  build. Thirteen new quick tests pass. The focused milestone run with
  `INTERSECT_EVAL_PACK=target/eval-m2-current.pack` passed all **14** extended
  tests (`20261108-200757`), including parsed Add(1,2)=3, named Add=3,
  Fact(10)=3628800, Pair(1)=[1], and a constraint invoked through a feature.
  Initial text tests exposed that directed SysML ReferenceUsages have ordinary
  FeatureMemberships; input discovery now uses direction, consistent with
  Pilot TypeUtil.getAllParametersOf's directed-feature query. A constructed
  caller-context fixture also needed explicit null direction on its ordinary
  attributes instead of the frozen schema's default enum value.

  No new identity-paired Pilot agreement is claimed. M3.2–M3.4, the M4 CLI
  default switch and the M4 corpus boundary remain outstanding. Read-only
  Pilot bytecode inspection shows a qualification to REF's caller-context
  wording: Feature callees are pushed before instantiateInvocation evaluates
  arguments. This implementation follows the plan's explicit caller-context
  rule; a paired oracle case is still needed before proposing a deviation.

- 2026-11-08, M3.1: added the independent `PilotEvalOracle`, a milestone-only
  Java assertion runner and a five-site text smoke fixture. The oracle follows
  PilotOracle's setup, resolve-all and per-language ALL-validation sequence,
  visits only selected resources in containment order, and records all three
  modes independently. Results preserve canonical real bits, contained-element
  identities, metaclass/synthetic distinctions, null versus empty sequences,
  diagnostics and per-mode exceptions (including stack overflow). The report
  also lists selected resources so files with zero sites remain visible.
  The pinned 2026-08 archive checksum was verified; Java is Temurin 21.0.6+7,
  and the referenced source revision is
  `692170b71867353b8f90341e61556f49a5beb0e5`. The Java runner passed **24
  assertions**, including calls to all three real evaluator entry points.
  These tests require Java and remain outside the Rust quick tier. The first
  text smoke recorded five sites and continued past recursive evaluation,
  but its temporary input mistakenly used the reserved name `after`, producing
  one syntax diagnostic. That run is not clean-load evidence; the committed
  smoke uses `recovered` instead. No Intersect/Pilot agreement is claimed by
  this task; identity pairing, mode comparisons and GE remain M3.2–M3.4.
  All four §5 checks passed: formatting checked 155 Rust targets, strict
  clippy and evaluator WASM build passed, and the quick tier had **117 passed,
  3 ignored, 0 failures** (19 unit and 98 integration; report
  `20261108-185845`). No Rust production code or dependency manifests changed.
  The corrected full-library smoke completed with **5 sites, 0 load
  diagnostics** (`target/pilot-oracle/eval-smoke-pilot-clean.json`). Each mode
  produced four values: integer 7, real 2/3 (bits `3fe5555555555555`), infinity,
  and integer 9 after the recursive site. Model-level and extended each recorded
  one StackOverflowError; gated recorded one not-evaluable result. All 15
  mode outcomes were retained. No REF discrepancy was observed in this bounded
  smoke; it did not exercise metaclass normalization or compare native results.
  M3.2 is blocked by the current CLI stopping at its first evaluation error
  (for example Expressions.kerml's recursive `z`), while §5 permits CLI edits
  only for M2.3. M3.3 requires importing a pairing map that is private inside
  `compare()`, while §12 prohibits modifying compare.py. Scoped amendments
  were requested for opt-in CLI continuation with explicit error rows and an
  optional export of the existing pairing map. Neither restricted file was
  changed, and M3.2–M3.4 have not been completed.

- 2026-11-08, M2.3 and M2 boundary: CLI prerequisite was satisfied by
  `7383d501` (existing check/export/info command). Added `eval`, the specified
  options, source/target selection, text and JSON results, text explanations,
  and stderr traces. Default stays model-level (E4); extended coverage stays
  M4. The host loads wrapper and files together on the library snapshot and
  maps wrapper diagnostics/trace positions to expression text. Shared public
  `Evaluator::qualified_name` / `resolve_target` forward to the existing Rust
  navigation; target names are canonicalized, missing/ambiguous matches fail,
  and qualified feature names use effective names. Explicit `--target` with
  `--all` overrides site contexts. Root Cargo.toml was untouched; Cargo.lock
  gained only the CLI's `intersect-kerml-eval` dependency edge, with no version
  changes. Corrected the M2.1 function fixture to use OwningMembership.

  All four §5 commands pass (CLI included in clippy and tests): quick tier
  **126 passed, 4 ignored, 0 failures**, report `20261108-163950` (19 evaluator
  unit, 98 evaluator integration, 9 CLI tests). Every quick test is under 10 s.
  Format checked 155 Rust targets; clippy used `-D warnings`; evaluator WASM
  build passed. With INTERSECT_EVAL_PACK set for the run, both crates' milestone
  features enabled: **130 passed, 0 ignored, 0 failures**, report
  `20261108-164027`. This includes the four pack-backed tests: library operator
  table, text smoke, wrapper arithmetic/conditional/target, and CLI `--expr`.
  The report runner strips interior `--` separators, so an initial attempt
  with `--ignored` was rejected before tests ran; the feature-enabled run is
  the passing milestone result.

  Local pack: `target/eval-m2-current.pack`, generated from the pinned sources
  and final lockfile with the test-fast build and intersect-model/text-compiler.
  An initial `--library` pack became stale when the CLI dependency edge changed:
  compiler identities hash the entire workspace lockfile. Rebuilt with
  `sysml_pack build . target/eval-m2-current.pack --previous target/eval-m2.pack`,
  then populated the library via the CLI's existing `--library
  fixtures/native-pilot/v1/sources/sysml.library` path (94 files, about 79.8 s).
  No pack, dump or generated report is committed.

  Manual native acceptance used `target/test-fast/intersect.exe eval --pack
  target/eval-m2-current.pack --read-only` and the pinned file
  `fixtures/native-pilot/v1/sources/kerml/src/examples/Simple Tests/Expressions.kerml`.
  `--expr Expressions::C::count` => `LiteralInteger 0`; `--expr Expressions::test1`
  => `LiteralBoolean false`; `--expr Expressions::test2` => `LiteralBoolean true`;
  each exited 0. `--target Expressions::C --expr count --trace` returned 0 with
  expression and corpus line/column locations. `--expr '1 +' --json` returned a
  parse diagnostic at `<expression>:1:4` and exited 1; invalid target `a; b`
  exited 2 before pack access. The tutorial records the three corpus outputs.

  REF discrepancy found and corrected during native trace acceptance:
  `if true? 1 else 2` initially returned NotEvaluable/OperandKinds because the
  branch's FeatureReferenceExpression had NoReferent. The parsed grammar's
  OwnedExpressionReference owns its expression through FeatureMembership,
  which M1 navigation excluded. Navigation now accepts that stored path while
  excluding parameter memberships/implicit returns. A parser-free regression
  and the pack-backed wrapper conditional pass; JSON trace/explain now reports
  Int(1), the skipped branch and mapped locations. No parser, grammar, core or
  Rcore rule changes were made.

  Final corpus `--all --gated` reports **25 sites** (22 feature values, two
  results, one bound; 21 NotEvaluable and four Values outcomes), exit 0. Ungated `--all
  --json` emits the first two site outcomes, then recursive `Expressions::z`
  reaches DepthLimit(256), exit 2. Complete ungated results for this recursive
  file cannot be produced under the stated default; the limit was not raised.
  The earlier 25-site ungated run preceded the owned-expression fix and is
  superseded. No Java/Pilot runs, no M3 work, and no Pilot-agreement claim.

- 2026-11-08, M2.2: added `eval_source` and `wrapper_expression`, escaped
  qualified-name validation and byte-exact SourceMap offsets. The user approved
  returning `Result<(Source, SourceMap), EvalError>` to reject invalid targets.
  Moved the already-present intersect-xtext dependency from dev to normal for
  Source; no lockfile change. Three new quick tests pass; the new pack-backed
  arithmetic/target test is milestone-only. All four §5 checks pass: 115 passed,
  3 ignored, 0 failures (`20261108-160011`). Pack-backed checks are pending the
  M2 boundary; INTERSECT_EVAL_PACK is currently unset. No Pilot comparison.

- 2026-11-08, M2.1: added deterministic resource/containment-preorder site
  enumeration, the three specified target rules, and pruning below each site's
  expression. Three new quick tests cover all site kinds, nesting, resource
  order and explicit library selection. All four §5 checks pass; quick tier:
  112 passed, 2 ignored, 0 failures (report `20261108-155804`). An initial
  fixture omitted the required FeatureValue.memberElement reference; corrected
  before the passing run. No parser or Pilot run; no REF disagreement observed.

- 2026-11-08: **M1 independent-review fix round (items 1–10), complete.**
  Added 12 focused quick-tier tests in existing `tests/all.rs` modules (two
  each for items 1 and 7; one for each other item). Final §5 checks all passed:
  formatting (155 workspace Rust targets), evaluator Clippy with `-D warnings`
  and `--locked`, the reported quick tier (**109 passed, 2 ignored**; unit
  binary 1.95 s, integration binary 2.17 s), and the locked wasm32 library
  build. Both final test binaries finished below 10 s, including fixture setup.
  Initial isolated item-2 runs had noisy setup times of 21.52/10.49/13.43 s;
  the combined items 1–4 rerun took 7.02 s before the final full run above.
  Resolutions:
  1. Collect inherited features before filtering every inherited feature
     redefined by any candidate. Both typing/redefinition orders and the
     diamond case pass. Full-suite validation caught a regression in the
     existing cyclic owned-feature case; the final validation repair preserves
     its first-owned-feature rule without weakening inherited filtering.
  2. The `Element(T)` representation of a non-Feature Type context is treated
     as typed by T for `istype`, `hastype`, `@` and `as`; no synthetic model
     objects or public Value variants were introduced. Feature targets retain
     their ordinary declared/inherited types.
  3. `BaseFunctions::'@@'` and `meta` are admitted by model-level evaluability.
     Their execution still returns the existing unevaluated UnknownFunction
     result; metadata implementation remains M5.1.
  4. Integer remainder by -1 returns zero, including arithmetic-built i64::MIN.
     Checked i64 arithmetic otherwise remains unchanged (E3).
  5. Feature lookups no longer consume steps. Range materialization retains
     one step per generated literal, with a pre-allocation budget check;
     M1.2 now states this allocation-protection exception. Trace/limit tests
     were adjusted and still exercise exactly 10,000 steps.
  6. Text uses effective names (and short names) inherited through redefinition,
     with declared names taking precedence and cycles terminating.
  7. Decimal formatting follows the JDK closest one-/two-digit selection rule
     when a one-digit decimal round-trips, fixing `4.9E-324` and related small
     subnormals. Tests cover signed small subnormals, normal/subnormal and
     maximum-finite boundaries, and 1,024 signed subnormal round trips.
     Reference: [JDK Double.toString specification](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/Double.html#toString(double)).
     **The M3 oracle JVM must be JDK 19 or later** for shortest-digit
     Double.toString behavior. Expected values were derived without Java runs.
  8. Gated default evaluation reads only ResultExpressionMembership; a return
     parameter value alone is NotEvaluable. Ungated fallback is unchanged.
  9. Chained referents inherit their featuring type from the first chaining
     feature, including nested chains. Cyclic first-segment paths return an
     explicit model error instead of recursing indefinitely.
  10. Text-fixture and crate documentation now require a standard-library pack
      (`sysml_pack build . PACK --library`) built from the same sources as the
      test binary, and explain the plain-pack "no library snapshot" failure.
      The focused documentation test uses no pack or parser.
  Nothing from these ten items remains unimplemented. The two milestone text
  tests were not run in this quick-tier round; no Java/Pilot runs or agreement
  claims were made. M2 was not started. Changes are limited to the evaluator
  crate and this plan; other crates, root Cargo.toml and Cargo.lock are untouched.

- 2026-11-08: M1.9 observer hooks and trace rendering completed; all four
  checks passed, **97 passed, 2 ignored** (unit 2.52 s, integration 2.30 s).
  Tests verify expression entry/exit order, every Reason variant, lazy skips,
  contextual Unbound/Bound lookups, Stop at step 3, identical repeated traces,
  and equal observed/unobserved outcomes at exactly 10,000 steps. Host-supplied
  text (`Trace::with_source`) supplies line/column positions; this user-approved
  API amendment is necessary because frozen models contain no source text.
  Missing host text has an explicit URI/byte-span fallback. SourceMap supplies
  only the mapping needed by tracing; M2 wrapper creation, sites and CLI work
  were not started. Trace regressions exposed and corrected an interim scalar
  chain behavior: literals must continue as Features/Types, rather than end
  the chain early (REF §1.6). The final scalar/self chain tests pass.
- 2026-11-08: **M1 boundary:** `INTERSECT_EVAL_PACK` is unset, so the two
  milestone-tier text tests were **not run**. No parser, pack or Java work
  entered the quick tier. No Pilot agreement is claimed; comparison is M3.
  The intentional numeric difference from REF remains E3: checked i64
  arithmetic rather than wrapping Java int. REF-directed fixtures pass, but
  pack-backed text, implicit-specialization behavior and Pilot agreement
  remain unverified. Root Cargo.toml changes only its member list; Cargo.lock
  only registers the evaluator and preserves existing dependency versions.

- 2026-11-08: M1.8 Pilot-style formatting and lossless JSON completed; four
  checks passed, **89 passed, 2 ignored** (unit 1.20 s, integration 1.22 s).
  Java-style layout follows the prescribed Rust shortest-round-trip digits;
  no Java formatting comparison or Pilot agreement claim was made.

- 2026-11-08: M1.7 constructors, evaluability and gating completed; four
  checks passed, **86 passed, 2 ignored** (unit 3.12 s, integration 3.56 s).
  The initial output-parameter test exposed that M1.1's input/result query
  excludes ordinary output parameters; evaluability now uses a separate
  stored all-parameter query, as REF requires. No core changes.

- 2026-11-08: M1.6 lazy controls and argument-bound bodies completed; four
  checks passed, **79 passed, 2 ignored** (unit 1.33 s, integration 1.23 s).
  Short-circuit tests use a three-step budget and a skipped range that would
  exceed it. Collect/select cover binding, flattening and null body results.

- 2026-11-08: M1.5 feature/context lookup and chains completed; four checks
  passed, **70 passed, 2 ignored** (unit 2.59 s, integration 2.62 s).
  Tests include redefinition shadowing without a value, cyclic value limits,
  default values, self, pi, multi-source and three-level chains.

- 2026-11-08: M1.4 base functions completed; four checks passed, **63 passed,
  2 ignored** (unit 2.37 s, integration 1.60 s). Covers collection elements,
  row-major array indexing (including nonpositive-index behavior), primitive
  type specialization, and the literal comma operand-shape case from M1.3.

- 2026-11-08: M1.3 arithmetic and eager Boolean functions: quick tier **55
  passed, 2 ignored** (unit 6.00 s, integration 6.23 s). Checked i64 overflow
  uses arithmetic-built wide values. Range materialization charges one step
  per generated literal and checks the remaining budget before allocation.
  The two-value operand shape is tested using a range; the literal comma
  spelling is added with comma support in M1.4. No Pilot comparison run.

- 2026-11-08: M1.2 bounded dispatch and literals completed. All four checks
  passed; quick tier **30 passed, 2 ignored** (unit 1.75 s, integration 1.89 s).
  Observed storage constraint: although frozen integer slots hold `i64`, the
  full SysML `EInt` validator admits only 32-bit literal values, matching REF.
  Literal tests therefore use valid EInt boundaries; E3's checked `i64`
  arithmetic/overflow tests must construct wider values through arithmetic.
  The failed out-of-range literal fixture was corrected without modifying the
  schema or changing E3. Limits reset on each public evaluation call.
- 2026-11-08: M1.1 schema, navigation and library index completed. All four
  checks passed; quick tier **20 passed, 2 ignored** (unit binary 3.01 s,
  integration 2.33 s). Navigation tests cover stored operand paths, named
  argument ordering/holes, referents, redefinition cycles, inherited features,
  types, result expressions, chaining and escaped names. The pinned operator
  table's prepared-library check is milestone-only and was not run without a
  pack. No Pilot agreement claim.
- 2026-11-08: M0.3 added the pack-backed text helper and operator smoke test,
  as a milestone-only module of `tests/all.rs`. All four task checks passed:
  quick tier **8 passed, 1 ignored** (integration 3.47 s). The text test was
  not run: `INTERSECT_EVAL_PACK` is unset. `intersect-xtext` is a dev-dependency
  solely to construct `Source` inputs. Observed plan qualification: the shared
  source-identity build script hashes the root `Cargo.toml` and `Cargo.lock`,
  so registering the new crate and its dependencies changes compiler identities
  once. Subsequent evaluator-source-only edits remain outside those hashes.
- 2026-11-08: M0.1 completed on `codex/kerml-expression-eval` in `770a3507`.
  The required base (`0a5bb1ee`) did not compile: `owner_id` was declared in
  `load_kerml` but used only in `load_frozen`. The user authorized the narrow
  correction, committed separately as `9c9231f2`. The user also authorized
  the new crate's minimal `Cargo.lock` entry; existing dependency versions
  were preserved. Formatting (155 Rust targets), strict evaluator Clippy,
  the quick tier (6 evaluator tests; additionally 47 KerML tests passed,
  17 ignored), and the evaluator WASM build passed. No Pilot comparison run.
- 2026-11-08: M0.2 measurement attempt blocked before schema freezing:
  `compile_rcore_at(STRUCTURE_URI, include_str!(...))` returned
  `Rcore compilation source budget exceeded`. The stored source is 485,745
  bytes; `rcore.rs` rejects each source above 256,000 bytes, with no budget
  override on this API. The throwaway test failed (0 passed, 1 failed;
  harness runtime 0.08 s). This is not a compile/freeze timing measurement:
  no 30-object timing was obtained and no fixture approach was selected.
  The user subsequently approved using the existing structure compiler,
  retaining the original timing rule and making no core changes. Its
  convenience wrapper has the same source-size limit, so the measurement
  uses `compiler::compile_structure_bundle_with_limits`, the entry point
  already used by the SysML loader, with `source_bytes = text.len()`.
- 2026-11-08: M0.2 successful measurement (`test-fast`, Windows/MSVC): full
  structure compilation plus freezing **4.181910 s**; construction, `ById`
  linking and validation of **30 objects: 0.000807 s**; total **4.182717 s**.
  The total is below 5 s, so the selected fixture approach is the full SysML
  metamodel cached in `OnceLock`, not a reduced metamodel. The preceding
  validation attempt exposed the full schema's required stored
  `Membership.memberElement`; fixtures populate it alongside the exact
  `OperandSettings` containment path. The successful throwaway test was
  removed. Timing evidence is also recorded in the evaluator crate README.
  All four task checks passed; the final quick tier passed 8 tests (6 unit,
  2 fixture; the integration binary took 4.57 s). An earlier passing run took
  51.24 s; isolated repeats took 3.40 s and 2.76 s including initialization,
  and the full repeat was below 10 s. No parser/pack/Java tests were added to
  the quick tier. No REF/Pilot behavior comparison was performed.
- 2026-11-08: plan written from the Pilot source (REF) and a survey of the
  frozen engine, Rcore executors, CLI and oracle. No code yet.
- 2026-11-08: the user wants expression debugging eventually. Added the
  foundations to the plan: `Reason` on every unevaluated/not-evaluable result,
  the `EvalObserver` hook with `evaluate_observed` and `Trace` (task M1.9),
  the `--expr` `SourceMap` (M2.2), and `--trace`/`--explain` (M2.3). An
  interactive debugger (breakpoints, stepping UI, browser panel) is later work
  built on these; it is not planned here.

## 12. What Codex must not do

- No `unwrap`, `expect`, `panic!`, `unreachable!`, unchecked indexing or
  `as` casts that can truncate in library code; return `EvalError`.
- No Java, JVM or Pilot call in production code or in the quick tier; Java
  only in `tools/pilot-oracle/`.
- No clocks, randomness, file or network I/O, threads or hash-order iteration
  in `intersect-kerml-eval`; I/O stays in the CLI host.
- Do not change `docs/release-plan.md` gates, roadmap decisions, the grammars,
  `crates/intersect-kerml/semantics/*.rcore`, `crates/intersect-kerml/src/**`,
  `crates/intersect-model/**`, `crates/intersect-xtext/**`, the language-pack
  recipe, or `tools/pilot-oracle/{PilotOracle.java,compare.py,deviations.json}`
  (reuse them; add new files beside them).
- Do not take any open decision (§10) beyond its stated default; if a task
  cannot proceed under the default, stop and report.
- Do not describe a phase as complete without its tests and, for M3+, the
  oracle numbers; imported, evaluated and Pilot-verified are separate claims.
- Do not add top-level integration test binaries; do not put tests of 10 s
  or more in the quick tier; do not leave long runs going in the background.
- Do not commit generated reports other than the named result files; do not
  push.
