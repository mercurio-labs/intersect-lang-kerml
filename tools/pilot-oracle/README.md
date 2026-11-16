# Pilot oracle

Independent verification of the frozen-engine KerML load against the SysML v2
Pilot Implementation. Java is used only here, as an oracle; nothing in the
production build or the normal test run needs it.

## Provenance

| Item | Value |
|---|---|
| Pilot release | `2026-08`, tag commit `692170b71867353b8f90341e61556f49a5beb0e5` (the commit `fixtures/native-pilot/v1/manifest.json` pins) |
| Download | `https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/releases/download/2026-08/jupyter-sysml-kernel-0.62.0.zip` |
| sha256 of the zip | `5a015ed3f2b3d2dae14e9c3fb9602d9e1fef5f9ce33cf396c422329405cc3cb2` |
| Runtime | `sysml/jupyter-sysml-kernel-0.62.0-all.jar` from that zip (Pilot, EMF, Xtext and Gson in one jar) |
| JDK | Temurin 21.0.6 |

Downloads stay under `target/pilot-oracle/` (ignored by Git).

## Run

```sh
mkdir -p target/pilot-oracle/download target/pilot-oracle/classes
curl -sSL -o target/pilot-oracle/download/jupyter-sysml-kernel-0.62.0.zip \
  https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/releases/download/2026-08/jupyter-sysml-kernel-0.62.0.zip
unzip -q target/pilot-oracle/download/jupyter-sysml-kernel-0.62.0.zip -d target/pilot-oracle/kernel
JAR=target/pilot-oracle/kernel/sysml/jupyter-sysml-kernel-0.62.0-all.jar
javac -cp "$JAR" -d target/pilot-oracle/classes tools/pilot-oracle/PilotOracle.java
java -cp "$JAR;target/pilot-oracle/classes" PilotOracle target/pilot-oracle/bsl.json \
  fixtures/native-pilot/v1/sources \
  "sysml.library/Kernel Libraries/Kernel Semantic Library/Base.kerml" \
  "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml" \
  "sysml.library/Kernel Libraries/Kernel Semantic Library/Links.kerml"
FROZEN_KERML_LINKED_DUMP=target/pilot-oracle/bsl-frozen.json \
  cargo run --release -p intersect-kerml --example frozen_kerml_load -- .
python tools/pilot-oracle/compare.py target/pilot-oracle/bsl.json \
  target/pilot-oracle/bsl-frozen.json target/pilot-oracle/bsl-compare.json
```

(On Linux and macOS the classpath separator is `:`.)

`PilotOracle` loads the files into one Xtext resource set, calls
`EcoreUtil.resolveAll`, then runs the injected `IResourceValidator` with
`CheckMode.ALL` on every resource (including resources loaded during validation).
It writes every object's class, node span, stored
non-derived values, children and reference targets (as resource and EMF
fragment), with setup, parse, resolve and validation times for a cold load
and a warm repeat in the same JVM. Each resource also has a `diagnostics`
array from the cold load: severity, code, message, target resource/EMF
fragment, target node
`start`/`end`, and the issue's `offset`/`length`. Null means unavailable.
Pilot offsets use Java/Xtext UTF-16 units; Intersect spans use byte offsets.
Spans are retained as evidence and are not matching keys.

`compare.py` pairs objects by walking both containment trees in step from
each resource root, then compares class, attributes and references through
the pairing. It skips `elementId` (the Pilot draws random UUIDs) and parent
pointers (container features, which the Pilot export omits), decodes enum
literals, and treats a value the Pilot leaves unset as equal to an unset
value or the metamodel default on our side.

## Diagnostic comparison

The linked dump adds top-level `diagnostics` and `graph_stage` fields without
changing the shared model format. Each Intersect violation has severity `error`,
its frozen `Violation` variant as code, its message, target element ID and
available resource/node span. These are structural violations from the frozen
validator, not a claim that all Pilot semantic checks exist in Intersect.

`compare.py` maps the Pilot target through the existing containment pairing.
It matches a multiset by target, severity and exact nonempty code. If either
side has no code, it requires an exact nonempty message instead; different
nonempty codes remain disagreements even if messages agree. Exact code matches
are reserved before message fallback. No rule-name aliases or message rewriting
are inferred. Missing/unpaired targets stay unmatched. The report's
`diagnostics` section counts `matched`, `pilot_only` and `intersect_only` and
retains every match and unmatched diagnostic, including source locations.
Exports without a diagnostics field are rejected rather than treated as zero;
older `library_probe` dumps must be extended before diagnostic comparison.

On validation failure the consuming frozen API does not expose the completed
graph. The host exports the original construction graph with
`graph_stage: "construction"` and the actual validation violations. Explicit
target IDs still pair; implicit targets absent from construction cannot pair.
On success it exports the completed graph with `graph_stage: "validated"`.
Link/implicit/default failures still stop before validation and produce no dump.
Use a fresh output path so a failed load cannot be mistaken for an older dump.

Run the fast inline fixtures (no JVM or Rust build required):

```sh
python -B -m unittest discover -s tools/pilot-oracle -p "test_*.py"
```

## BSL diagnostics measurement (2026-11-06)

Run from the `codex/pilot-diagnostics` worktree based on `develop` at
`774132a4f`, with the pinned runtime checksum verified and JDK 21.0.6 at
`D:/dev/jdks/jdk21`. The machine was running other builds; this is a
conformance observation, not an idle-machine performance comparison.

Measured diagnostic comparison:

| Category | Count |
|---|--:|
| Matched | 0 |
| Pilot-only | 28 |
| Intersect-only | 0 |

All 266 objects pair, all 2673 of
2673 fields agree, and all 28
Pilot diagnostic targets map through that pairing. The Intersect dump has
`graph_stage: "validated"` and zero structural validation violations.

Disagreements left unfixed: all 28 Pilot-only findings have severity
`error`, code `validateFeatureHasType_`, and message
"Features must have at least one type":

| Resource | Pilot-only findings |
|---|--:|
| Base.kerml | 16 |
| ScalarValues.kerml | 0 |
| Links.kerml | 12 |

The generated [full comparison report](results/bsl-2026-11-06.json) lists every
finding's resource, EMF fragment, node span and issue location. No semantic
changes were made.

PowerShell commands used after downloading/unpacking the runtime above:

```powershell
$jar = 'target/pilot-oracle/kernel/sysml/jupyter-sysml-kernel-0.62.0-all.jar'
& D:/dev/jdks/jdk21/bin/javac.exe -cp $jar -d target/pilot-oracle/classes tools/pilot-oracle/PilotOracle.java
& D:/dev/jdks/jdk21/bin/java.exe -cp "$jar;target/pilot-oracle/classes" PilotOracle target/pilot-oracle/bsl.json fixtures/native-pilot/v1/sources `
  'sysml.library/Kernel Libraries/Kernel Semantic Library/Base.kerml' `
  'sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml' `
  'sysml.library/Kernel Libraries/Kernel Semantic Library/Links.kerml'
$env:CARGO_TARGET_DIR = 'D:/dev/worktrees/intersect/rcore-frozen-engine/target'
$env:KERML_BINDING_CACHE = 'target/pilot-oracle/kerml-bindings.json'
Remove-Item Env:CARGO_PROFILE_TEST_FAST_OPT_LEVEL -ErrorAction SilentlyContinue
$env:CARGO_BUILD_JOBS = '1'
$env:FROZEN_KERML_LINKED_DUMP = 'target/pilot-oracle/bsl-frozen.json'
cargo build --profile test-fast --config 'profile.test-fast.package.intersect-kerml.opt-level=0' -p intersect-kerml --example frozen_kerml_load
python -B tools/pilot-oracle/set_exe_stack.py "$env:CARGO_TARGET_DIR/test-fast/examples/frozen_kerml_load.exe" target/pilot-oracle/frozen_kerml_load.exe
$env:RUST_MIN_STACK = '67108864'
& ./target/pilot-oracle/frozen_kerml_load.exe .
python -B tools/pilot-oracle/compare.py target/pilot-oracle/bsl.json target/pilot-oracle/bsl-frozen.json target/pilot-oracle/bsl-compare.json
New-Item -ItemType Directory -Force tools/pilot-oracle/results | Out-Null
Copy-Item target/pilot-oracle/bsl-compare.json tools/pilot-oracle/results/bsl-2026-11-06.json
```

`CARGO_TARGET_DIR` reuses this machine's existing Cargo artifacts; omit it on
another machine and use `target/test-fast/examples/frozen_kerml_load.exe` as
the copy source. The binding cache stays inside the worktree. A prior shared
binding cache was rejected as belonging to another compiler. For the BSL host,
only `intersect-kerml` optimization is disabled; model/Xtext dependencies retain
the normal `test-fast` optimization. This is not a performance comparison.

The unoptimized Windows `cargo run` initially overflowed its main-thread
stack after definition preparation. `RUST_MIN_STACK` controls spawned threads,
not the executable's main thread. `set_exe_stack.py` copies the x64 PE host into
the ignored oracle output directory and raises only that copy's stack reserve
to at least 64 MiB. This verification-only launch workaround does not modify
the cached executable or introduce a production build requirement.

Verification commands (same Cargo cache and one build job; the Rust test
retry disabled optimization for all workspace packages):

```powershell
python -B -m unittest discover -s tools/pilot-oracle -p 'test_*.py'
$env:CARGO_PROFILE_TEST_FAST_OPT_LEVEL = '0'
python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml --lib --bins --tests
Remove-Item Env:CARGO_PROFILE_TEST_FAST_OPT_LEVEL
cargo fmt -- --check
cargo clippy -p intersect-kerml --all-targets -- -D warnings
cargo build -p intersect-kerml --lib --target wasm32-unknown-unknown --locked
```

The normal-profile quick-tier attempt was stopped after **2,022.8 s** of
compilation under concurrent builds, with no tests executed; it does not meet
the ten-minute quick-tier limit.

Pairing fixtures: **10 passed**. Rust retry: **183 passed, 0 failed, 128 ignored**;
build **616.2 s**, test phase **222.6 s** (including a Cargo lock wait). This
retry also exceeds the quick-tier budget; no normal-profile passing receipt is
claimed. Formatting, strict clippy and the scoped WASM build passed.
See [the test receipt](http://localhost:8765/pilot-diagnostics/20261106-121616/).

## Results (2026-11-02, measured; the machine was not idle)


### Per-file comparison (2026-11-06)

Re-running `compare.py` against the existing `pilot-oracle-kerml.json` and
`kerml-frozen-fix5.json` dumps measures 25,162 paired objects, 288,010 fields
compared, and 287,880 fields equal. The report reads
[`deviations.json`](deviations.json): each entry has an exact class, feature,
and justification. The 107 `FeatureChainExpression.operator` and 23
`IndexExpression.operator` differences are allow-listed; they are compared
but are not counted as equal. Unpaired objects, class differences, containment
count differences and all other field differences remain unexplained.

The console prints the per-resource table and totals; the optional JSON report
contains the same counts in `resources` plus global totals, difference kinds
and examples. References agreeing counts reference-valued fields, including
containment; a list agrees only when every target agrees in order, and counts
once. This is distinct from the loader's count of references requiring resolution.

```sh
python -B -m unittest tools/pilot-oracle/test_compare.py
python -B tools/pilot-oracle/compare.py PATH/pilot-oracle-kerml.json \
  PATH/kerml-frozen-fix5.json PATH/compare.json
```

All files below are under `sysml.library/Kernel Libraries/`; file names are
unique in this measured cohort.

| Resource file | Objects paired | Fields compared | Fields equal | References agreeing | Allow-listed |
|---|---:|---:|---:|---:|---:|
| Collections.kerml | 377 | 4,179 | 4,177 | 334/334 | 2 |
| ScalarValues.kerml | 39 | 309 | 309 | 49/49 | 0 |
| VectorValues.kerml | 60 | 554 | 554 | 63/63 | 0 |
| BaseFunctions.kerml | 398 | 4,550 | 4,550 | 368/368 | 0 |
| BooleanFunctions.kerml | 156 | 1,730 | 1,730 | 159/159 | 0 |
| CollectionFunctions.kerml | 872 | 10,403 | 10,381 | 760/760 | 22 |
| ComplexFunctions.kerml | 584 | 6,742 | 6,742 | 557/557 | 0 |
| ControlFunctions.kerml | 830 | 9,694 | 9,693 | 742/742 | 1 |
| DataFunctions.kerml | 511 | 5,797 | 5,797 | 502/502 | 0 |
| IntegerFunctions.kerml | 512 | 5,771 | 5,771 | 517/517 | 0 |
| NaturalFunctions.kerml | 309 | 3,457 | 3,457 | 317/317 | 0 |
| NumericalFunctions.kerml | 545 | 6,193 | 6,193 | 541/541 | 0 |
| OccurrenceFunctions.kerml | 467 | 5,357 | 5,354 | 428/428 | 3 |
| RationalFunctions.kerml | 596 | 6,735 | 6,735 | 598/598 | 0 |
| RealFunctions.kerml | 578 | 6,504 | 6,504 | 584/584 | 0 |
| ScalarFunctions.kerml | 454 | 5,085 | 5,085 | 469/469 | 0 |
| SequenceFunctions.kerml | 1,648 | 19,595 | 19,588 | 1,441/1,441 | 7 |
| StringFunctions.kerml | 222 | 2,486 | 2,486 | 225/225 | 0 |
| TrigFunctions.kerml | 340 | 3,985 | 3,985 | 311/311 | 0 |
| VectorFunctions.kerml | 2,254 | 26,863 | 26,842 | 1,978/1,978 | 21 |
| Base.kerml | 106 | 1,099 | 1,099 | 89/89 | 0 |
| Clocks.kerml | 494 | 5,710 | 5,702 | 438/438 | 8 |
| ControlPerformances.kerml | 470 | 5,385 | 5,385 | 432/432 | 0 |
| FeatureReferencingPerformances.kerml | 570 | 6,353 | 6,352 | 577/577 | 1 |
| KerML.kerml | 2,380 | 26,658 | 26,658 | 2,434/2,434 | 0 |
| Links.kerml | 121 | 1,265 | 1,265 | 131/131 | 0 |
| Metaobjects.kerml | 65 | 652 | 652 | 62/62 | 0 |
| Objects.kerml | 720 | 8,117 | 8,115 | 682/682 | 2 |
| Observation.kerml | 406 | 4,612 | 4,609 | 379/379 | 3 |
| Occurrences.kerml | 3,580 | 40,446 | 40,423 | 3,516/3,516 | 23 |
| Performances.kerml | 515 | 5,506 | 5,504 | 503/503 | 2 |
| SpatialFrames.kerml | 955 | 11,191 | 11,175 | 847/847 | 16 |
| StatePerformances.kerml | 1,315 | 15,597 | 15,581 | 1,181/1,181 | 16 |
| Transfers.kerml | 851 | 9,499 | 9,499 | 841/841 | 0 |
| TransitionPerformances.kerml | 495 | 5,800 | 5,799 | 447/447 | 1 |
| Triggers.kerml | 367 | 4,131 | 4,129 | 322/322 | 2 |

```text
allow-listed differences: 130
unexplained differences: 0
```

### Earlier measurements

Base, ScalarValues and Links:

| | Pilot | Intersect |
|---|--:|--:|
| Objects | 266 | 266, all paired |
| Fields compared | | 2,673; 2,653 equal |
| Differences | | `Documentation.body` (20): the Pilot stores the comment text without `/* */`, leading `*` and with `\n` line ends; Intersect stores the raw comment |

All classes, reference targets and other attributes agree.

KerML library (all 36 `.kerml` files). The Pilot loads 25,162 objects with
no resource errors (setup 6.7 s, cold parse 2.3 s, `resolveAll` 123 s; warm
parse 0.24 s, `resolveAll` 68 s; `resolveAll` also visits the Pilot's derived
references, which it computes on demand, so it is not only proxy
resolution). Intersect's linked library comes from
`LIBRARY_PROBE_DUMP=PATH library_probe --skip-sysml .` (the 29 unresolved
references and two values without a construction rule are not bound-checked
in that export):

| | Result |
|---|--:|
| Intersect objects paired with Pilot objects | 22,416 of 22,416; 25,162 of 25,162 after the fixes below |
| Pilot objects without a counterpart | 2,746; none after the fixes below |
| Fields compared | 255,058; 288,010 with the implicit elements |
| Fields equal | 252,038 (98.8%); 287,723 (99.90%) after the fixes below |

Differences, by cause:

- **Implicit elements** (all 2,746 objects and the containment and expression
  `ownedRelationship` differences): fixed. The Pilot adds a
  `ReturnParameterMembership` owning an `out` result `Feature` to each feature
  reference or instantiation expression without a declared result (1,266),
  and an unnamed `Feature` under the source parameter of each feature chain
  expression (107). `bind implicit` with Rcore `new` in
  `kerml-construction.rcore` adds the same objects, marked implicit; every
  object now pairs.
- **Parameter direction** (626): fixed. A parameter without a declared
  direction gets `in`, or `out` for the result a `ReturnParameterMembership`
  owns (`bind default k.Feature.direction` in `kerml-construction.rcore`); all
  626 now agree, and 253,398 of 255,058 fields (99.35%) are equal.
- **Comment bodies** (365): fixed. Languages can convert terminal values
  (`StringRuleConversion::convert_terminal`, the counterpart of Xtext terminal
  value converters); KerML converts `REGULAR_COMMENT` to its body as the
  Pilot stores it, and all 365 now agree.
- **Quoted declared names** (162, for example `'=='`): fixed. The open
  capability now takes the language's EString rule converters
  (`StringRuleConversion`); KerML supplies its Name conversion
  (`kerml_string_rules`), so `'=='` is stored as `==`.
- **`operator` of chain and index expressions** (130): the Pilot computes it
  and does not store it; Intersect's construction default stores it.
- **`isEnd`** (180): fixed. A construction default makes a feature an
  `EndFeatureMembership` owns an end feature.
- Number literal encoding (26, a comparison artifact, fixed in the tool) and
  about 150 reference differences that follow the 29 unresolved references
  (effective names of unnamed redefining features).

Update 2026-11-06 (`0eaa45cc`, KerML gate G2 closed): all 4,050 references
resolve; after the expression-`self` and positional-parameter fixes,
287,880 of 288,010 fields equal (99.95%) and every reference target agrees.
The only differences are the 130 `operator` values above (a justified
deviation); details in [library-probe.md](../../docs/library-probe.md).

## Whole library, KerML and SysML (2026-11-07)

`PilotOracle` now registers the SysML language as well as KerML and
validates each resource with its own language's validator, so it loads the
whole standard library (36 `.kerml` and 58 `.sysml` files under
`fixtures/native-pilot/v1/sources/sysml.library`, in sorted order):

```sh
java -Xmx12g -Xss64m -cp "$JAR;target/pilot-oracle/classes" PilotOracle   target/pilot-oracle-library.json fixtures/native-pilot/v1/sources FILE...
LIBRARY_PROBE_SYSML_DUMP=target/library-frozen.json   target/release/examples/library_probe --sysml-phase2 .
python tools/pilot-oracle/compare.py target/pilot-oracle-library.json   target/library-frozen.json target/library-compare.json
```

Pilot (Java 21.0.6, machine not idle): setup 3.3 s; cold parse 2.8 s,
`resolveAll` 317 s, validate 13.8 s; warm parse 0.8 s, `resolveAll` 303 s,
validate 13.2 s. Intersect at `dbc3aa03`: 109,196 objects, link 20.6 s.

First comparison: 109,196 of our objects pair with the Pilot's 109,826;
1,258,456 of 1,260,343 fields equal (99.85%); 104,627 of 105,029 reference
targets agree; 1,044 allow-listed `operator` values, 1,882 unexplained
differences. By cause: SysML implicit elements Intersect does not construct
(630 unpaired Pilot objects, with their containment and `ownedRelationship`
differences); SysML construction defaults (`isComposite` about 300,
`isConstant` 76, `EnumerationDefinition.isAbstract` 10); `LiteralString.value`
keeping its quotes in SysML (61); reference targets (`memberElement` 64,
`redefinedFeature` 20, others 4); a transition payload's `declaredName` (1).
A further 94 "root count" differences were an export bug, fixed the same day
(`Model::to_model` now takes resource roots from the validated containers).

## SysML construction rules (2026-11-07, gate G5)

The SysML construction differences of the whole-library comparison, fixed in
`sysml-construction.rcore` and the KerML string conversion. Each rule was
read from the Pilot's SysML parser post-processors
(`org.omg.sysml.xtext.postprocessing`) and adapters
(`org.omg.sysml.adapter`) in the pinned kernel jar, and checked against the
SysML metamodel constraints (`isReference = not isComposite`,
`isVariation implies isAbstract`, `EnumerationDefinition: isVariation`,
`mayTimeVary`):

- **Implicit multiplicities** (630 unpaired Pilot objects, 315 containment
  counts, about 320 `ownedRelationship` references): fixed. A usage without
  an owned multiplicity gets an empty `Multiplicity` in an
  `OwningMembership` (`bind implicit k.Usage.ownedRelationship`) when it is
  an end, or, for attribute, item and port usages other than connection
  usages, when it has an owning type and no owned subsetting targets a
  feature that has one (`UsageAdapter.isAddDefaultMultiplicity`).
- **Transition link** (1 unpaired `ReferenceUsage`): fixed. A transition
  with a succession and no transition link feature owns an implicit
  `ReferenceUsage` in a `FeatureMembership`.
- **`isComposite`**: a usage that declares neither `ref` nor a composite
  keyword is composite unless its metaclass is referential, it is directed,
  an end, featured by an attribute, or has no expected featuring type; a
  port not featured by a port is not composite
  (`UsageParserPostProcessor`, `PortUsageParserPostProcessor`).
- **`isConstant`**: an end usage whose owning type specializes
  `Occurrences::Occurrence` (and that is not a portion, self link, happens
  link or composite action) is constant (`UsageAdapter.doTransform`). The
  specialization walk includes each type's metaclass library default, as
  `specializesFromLibrary` requires (`connection : MatesWith` is still a
  `Connections::connections`).
- **`isAbstract` / `isVariation`**: a variation definition or usage is
  abstract; an enumeration definition is a variation. The Pilot omits
  `EnumerationDefinition.isVariation` because `EnumerationDefinitionImpl`
  initializes its default to true; `compare.py` now uses that instance
  default.
- **Transition payload name**: the Pilot stores the accepter payload's name
  as the payload parameter's `declaredName` (`ReferenceUsageAdapter`); a
  `bind default k.Element.declaredName` stores it after linking, and
  `transition_payload` still derives it while names resolve.
- **`LiteralString.value`** (61): fixed. `KerMLStringRules` (shared by KerML
  and SysML) converts the `STRING_VALUE` terminal as the Pilot's
  `ElementUtil.unescapeString`: quotes and escapes removed. The KerML
  library has no string literals, so only SysML showed it.

| | Before (`3985ee3a`) | After |
|---|--:|--:|
| Objects (ours / Pilot) | 109,196 / 109,826 | 109,852 / 109,826 |
| Unpaired Pilot / ours | 630 / 0 | 0 / 26 |
| Fields equal | 1,258,456 of 1,260,343 | 1,266,532 of 1,267,904 |
| References agreeing | 104,627 of 105,029 | 105,244 of 105,344 |
| Allow-listed (`operator`) | 1,044 | 1,044 |
| Unexplained | 1,788 | 367 |

The 367 left:

- 228 `isComposite` of usages declared `ref` (ItemUsage 189, ...): the
  probe's phase 1 saves each construction by element export and phase 2
  imports it. `ref` assigns `isComposite = false` (the `isReference`
  setting), but export records an assignment only for optional attributes
  (`interop::derived_assignment`), so the import makes the required
  `isComposite` unassigned again and the default makes it composite. A
  parse linked in memory keeps the assignment. The fix is an engine change
  (record assigned required attributes equal to their default), not made
  here.
- 87 reference targets (`memberElement` 64, `redefinedFeature` 20, others
  3), name resolution, handled separately.
- 52 that follow 13 of those `redefinedFeature` targets: an attribute usage
  that redefines a package-level feature instead of the Pilot's nested one
  gets an implicit multiplicity (13 unpaired memberships and 13
  multiplicities, 13 containment counts, 13 `ownedRelationship` lists).

Reference targets, the same day (details in
[library-probe.md](../../docs/library-probe.md), "SysML reference targets
against the Pilot"): the 87 references with a different target (64
`memberElement`, 20 `redefinedFeature`, 2 `referencedFeature`, 1
`chainingFeature`, measured at `3985ee3a`) now agree, except 2 `redefinedFeature` targets in
`Items::Item::boundingShapes` that need the SysML `subitems` implicit
subsetting. Those 2 are allow-listed for their two objects only: a
deviation may carry `objects`, a list of Pilot `[resource, fragment]` pairs,
and then covers its class and feature on those objects alone. Reference
targets agreeing: 104,712 of 105,029 (104,627 before); unexplained
differences 1,701 (1,788 before).

## Whole library after the construction and reference rules (2026-11-07, `09bfae34`)

Both rule sets merged, with `FrozenStore` marking every assigned single
attribute equal to its default (so `ref`'s `isComposite = false` survives the
probe's save and reload):

| | Result |
|---|--:|
| Objects, ours / Pilot | 109,826 / 109,826, all paired |
| Fields equal | 1,266,858 of 1,267,904 |
| Reference targets agreeing | 105,342 of 105,344 |
| Allow-listed | 1,044 stored `operator` values, 2 `Item::boundingShapes` redefinitions |
| Unexplained differences | 0 (1,882 at the first comparison) |

KerML alone is unchanged (23,824 of 23,824 references, 0 unexplained).
Not compared yet: diagnostics (the probe's export carries none), and the
Pilot example corpus.

## Pilot example corpus (2026-11-07, milestone 2)

The D9 corpus (KerML `Types` and `Expressions`; SysML `PartTest`,
`ConnectionTest`, `RequirementTest` and `ConstraintTest`, under
`fixtures/native-pilot/v1/sources/{kerml,sysml}/src/examples/Simple Tests/`)
loaded with the whole library: `LIBRARY_PROBE_EXAMPLES=1` adds the six files
to the probe, and the Pilot ran over the same 100 files.

| | Result |
|---|--:|
| Objects, ours / Pilot | 112,174 / 112,174, all paired |
| Fields equal | 1,294,645 of 1,295,710 |
| Reference targets agreeing | 107,465 of 107,467 (the 2 `boundingShapes` redefinitions) |
| Allow-listed | 1,063 stored `operator` values (now also `collect` and `select`), 2 redefinitions |
| Unexplained differences | 0 |
| Intersect | KerML 4,175 of 4,175 references; all 19,266 resolve; valid |
| Pilot | setup 3.3 s, cold parse 2.5 s, `resolveAll` 329 s, validate 17.1 s |

What the corpus needed, all measured against the Pilot:
- Scopes for plain `Specialization.general`/`specific` and
  `Conjugation.originalType`/`conjugatedType` (`specialization Gen subtype A
  specializes B;`), and a default `Specialization.specific` = the owning type.
- `port x : ~C`: `ConjugatedPortTyping.conjugatedPortDefinition` resolves `C`
  and takes its implicit conjugated port definition
  (`sysml-name-resolution.rcore`).
- A parameter inherits from the general's parameter it redefines by position
  (`in onOccurrence = w::self { feature redefines startingAt }`), and a
  feature chain from its last chaining feature, with that feature's library
  default when it has no general (`to a1.aa.receiver`).
- SysML defaults from the Pilot's adapters: `isPortion` with a portion kind,
  an abstract message (`FlowUsageAdapter.makeMessageAbstract`), a flow end's
  feature taking the direction of the feature it redefines (`FlowEndAdapter`).
- `FrozenStore` writes a redefined attribute to its redefining one
  (`declaredShortName` to `RequirementDefinition.reqId`, as reads already did),
  and an absent value no longer replaces a written one in the same slot.
- `compare.py`: `FramedConcernMembership.kind` defaults to `requirement` in the
  Pilot's implementation class.

Not compared yet: diagnostics.

## Example models (2026-11-08)

`PilotOracle --examples OUT.json MANIFEST.json` (in `ExampleRun.java`) runs the
Pilot over the Pilot example folders and records what the Pilot itself says
about each file. The manifest names the library root and files and the
example root and folders (each folder's files, as
`crates/intersect-kerml/examples/pilot_examples.rs` loads them). The library
is loaded once; each folder's files are then added to the same resource set,
each folder resource is resolved (`EcoreUtil.resolveAll` per resource; library
proxies resolve on demand), every non-library resource is validated with
`CheckMode.ALL` by its own language, and the folder's resources (and any
loaded on demand) are unloaded and removed before the next folder. Per file it
writes the object count, the resource errors and warnings (for the linker's
"Couldn't resolve reference to ..." errors also the reference feature, owner
class and link text from the node model) and the validator issues (severity,
code, message, line), with per-folder parse, resolve and validate times.

`examples_report.py` compares that with Intersect's `pilot_examples` report
and classifies Intersect's link problems: (a) the Pilot also reports an
unresolved reference at the same spelling in the same file; (b) only
Intersect reports it (grouped by the reason after `scope:` and the feature,
with a representative use line found by searching the spelling); (c) the
Pilot reports something Intersect does not. The summary is in
[docs/pilot-examples-oracle.md](../../docs/pilot-examples-oracle.md).

```sh
JAR=target/pilot-oracle/kernel/sysml/jupyter-sysml-kernel-0.62.0-all.jar
javac -cp "$JAR" -d target/pilot-oracle/classes tools/pilot-oracle/PilotOracle.java tools/pilot-oracle/ExampleRun.java
java -Xmx8g -Xss64m -cp "$JAR;target/pilot-oracle/classes" PilotOracle --examples \
  target/pilot-oracle/examples-pilot.json target/pilot-oracle/examples-manifest.json
python -B tools/pilot-oracle/examples_report.py target/pilot-oracle/examples-pilot.json \
  target/pilot-examples-run1.json target/pilot-oracle/upstream target/pilot-oracle/examples-report.json
```

To iterate on Intersect's side, `pilot_examples` takes `--only TEXT`
(repeatable; folders whose path contains TEXT) and `--locate` (each problem
with file, line, column, line text and the containment path of the element
it names; `PILOT_EXAMPLES_DUMP=1` also prints the stored values of the
element and its ancestors). The examples root may be any directory with
`kerml/src/examples` and `sysml/src/examples`, so a small model can be checked
on its own; with `--snapshot`, a run that does not change the bindings takes
under a minute.

## Validation diagnostics (2026-11-08, gate G5)

Preparation for comparing validation diagnostics; no rule is implemented in
Intersect yet. The inventory, corpus, results and commands are in
[docs/pilot-validation-rules.md](../../docs/pilot-validation-rules.md).

- `validation_rules.py` holds the inventory of the Pilot's validation rules
  (`KerMLValidator.xtend`, `SysMLValidator.xtend` at the pinned commit): 209
  rules from 113 `@Check` methods, each with code, severity, messages, target,
  condition, needs and batch. It resolves codes and messages from the sources,
  checks that every `@Check` method is inventoried, and writes
  `validation-rules.json` (and, with `--markdown`, the document's tables).
- `validation_corpus.py` turns the Pilot's Xpect tests (a sparse checkout under
  `target/pilot-oracle/upstream-tests`) into a negative corpus: 179 units,
  242 files, 624 expected diagnostics, and a `PilotOracle --examples` manifest
  with `"export_objects": true`.
- `ExampleRun` now also writes each validator issue's target resource and node
  span (UTF-16, like the issue offset) and, with `export_objects`, each file's
  object export (`object_export`) for containment pairing.
  `compare.pair_objects` is the containment pairing `compare.py` uses.
- `compare_diagnostics.py` compares the Pilot with the Xpect expectations
  (616 of 624 agree), counts the Pilot's diagnostics per rule and batch (356
  validator diagnostics, 104 codes), and compares Pilot and Intersect
  diagnostics (`intersect-diagnostics/1`) by resource, severity, code and
  target, paired by containment or by line and column.

## Expression evaluation

`PilotEvalOracle.java` is an independent verification oracle, never production.
It uses the pinned download/checksum above and JDK 21 (minimum JDK 19, whose
shortest-digit `Double.toString` selection is required). All Java runs here,
including the factory-built assertions, are milestone-only.

Compile beside the existing oracle; `ExampleRun.java` is a dependency of
`PilotOracle.java`:

```powershell
$jar = 'target/pilot-oracle/kernel/sysml/jupyter-sysml-kernel-0.62.0-all.jar'
$jdk = 'D:/dev/jdks/jdk21/bin'
& "$jdk/javac.exe" -cp $jar -d target/pilot-oracle/classes `
  tools/pilot-oracle/PilotOracle.java tools/pilot-oracle/ExampleRun.java `
  tools/pilot-oracle/PilotEvalOracle.java tools/pilot-oracle/PilotEvalModelExport.java `
  tools/pilot-oracle/PilotEvalOracleTest.java
& "$jdk/java.exe" -cp "$jar;target/pilot-oracle/classes" PilotEvalOracleTest
& "$jdk/java.exe" -Xmx6g -cp "$jar;target/pilot-oracle/classes" PilotEvalOracle `
  target/pilot-oracle/eval-smoke-pilot.json `
  fixtures/native-pilot/v1/sources/sysml.library tools/pilot-oracle eval-smoke.sysml
& "$jdk/java.exe" -Xmx6g -cp "$jar;target/pilot-oracle/classes" PilotEvalOracle `
  target/pilot-oracle/expressions-eval-pilot.json `
  fixtures/native-pilot/v1/sources/sysml.library fixtures/native-pilot/v1/sources `
  'kerml/src/examples/Simple Tests/Expressions.kerml'
```

Usage: `PilotEvalOracle OUT.json LIBRARY_ROOT ROOT FILE...`. `LIBRARY_ROOT` is
the directory containing the standard-library `.kerml`/`.sysml` files;
`FILE` paths are relative to `ROOT`. Only explicitly named resources contribute
sites. The oracle reuses `PilotOracle.setup` and follows its load, resolve-all,
and language-specific `CheckMode.ALL` validation sequence. Library resources
are named `sysml.library/<relative path>`; selected resources retain the given
file names with forward slashes. Diagnostics are recorded, not silently treated
as successful loads.

Each `sites` row records resource, expression fragment/class, target identity
and fragment, site kind, available UTF-16 node span/line, `evaluable`, and
`model_level`, `gated`, and `extended` outcomes. Sites follow resource then
containment order, with nested expressions suppressed. Literals use value JSON;
reals retain canonical Java bits and text, including NaN and signed zero.
Contained elements use resource/fragment/class identities; metaclass features
identify their annotated element; other unowned elements are explicit
`synthetic` values. Java null lists and empty lists remain distinct.

Exceptions (including StackOverflowError) are recorded separately for each
mode, allowing later modes and sites to run. An evaluability exception has
`evaluable: null` and `evaluable_error`; it is not reported as false. A null item
inside a non-null result sequence is an exception, not an empty result. Fatal
VM failures such as out-of-memory errors are not converted into successful
oracle output. This tool alone does not establish Intersect/Pilot agreement;
pairing, comparison and the full corpus gate are subsequent M3 tasks.

Each function/evaluability query and each evaluation mode runs inside an EMF
`ChangeRecorder` observation. Its changes are undone before the next query,
including after an exception. A rollback failure aborts the run. This is
necessary because Pilot `EvaluationUtil.instantiateArguments` reparents an
existing Expression argument into a temporary invocation, removing it from its
source FeatureValue. Without isolation, an earlier collect/call can change a
later site's evaluability. The oracle preserves the Pilot's behavior within
each evaluation; it restores the loaded model between observations.

Original source-object identities are captured before evaluation. A source
Expression moved during a call retains its original resource/fragment in the
normalized result; only newly created unowned objects are `synthetic`. The
report records `observation_isolation`. Earlier reports without that marker
must be rerun before use as corpus qualification evidence. The factory suite
now passes **44 assertions**, including source containment and derived-query
restoration after success and StackOverflowError; the separate edge probe
passes **6 assertions**, including the Pilot's source-expression reparenting.
The comparator rejects reports without the isolation marker; its test suite
now has **48 passing cases**.

The isolated D9 rerun recorded 65 sites in six files with zero load diagnostics.
Model-level and Extended each returned 64 value outcomes and one exception;
gated returned 36 value outcomes and 29 not-evaluable. Four sites changed from
the old sequential report. Expressions.kerml's `test2` (line 66) now correctly
remains not model-level evaluable after earlier sites run: its source feature
value is restored between observations. This resolves the apparent native
evaluability discrepancy but does not establish identity-paired agreement.

Historical M3.1 smoke result (before observation isolation; rerun required for
qualification, 2026-11-08, Temurin 21.0.6+7): the 24 factory-built
assertions passed. The committed `eval-smoke.sysml`, loaded with the complete
standard library, produced five sites and zero load diagnostics:

| Mode | Value outcomes | Not evaluable | Exceptions |
|---|---:|---:|---:|
| Model-level | 4 | 0 | 1 |
| Gated | 4 | 1 | 0 |
| Extended | 4 | 0 | 1 |

The values are 7, 2/3 (real bits `3fe5555555555555`), infinity and 9. The
recursive site overflows in both ungated modes; the subsequent value 9 is
still recorded. The gated entry point rejects that recursive expression.
The initial temporary smoke accidentally used the reserved name `after` and
reported one syntax diagnostic; the committed input uses `recovered` and the
clean rerun is the evidence above. Metaclass normalization is implemented but
not exercised by this smoke. These are oracle-tool checks, not a native/Pilot
comparison or a GE result.

The M3.3 comparison core is implemented in `compare_eval.py`, with quick
protocol tests in `test_compare_eval.py`. It accepts the existing G5
containment pairing and never substitutes source locations, names or site
order for identity. Real text and native reasons are informational; real bits
(canonical NaN, distinct signed zero) and paired element identities determine
value equality. Exact agreement, allowed differences and missing sites have
separate counts. Every difference retains both results and source locations.
`eval-deviations.json` currently lists the nine M4.2 trig tolerances; entries need one exact mode, one function,
class or explicit site selector, and a reason. Missing sites and target/site-kind
mismatches cannot be hidden by value deviations. Synthetic values require the
same class and an explicit deviation. An exception message difference remains
a difference, so a native depth limit is not silently equated to stack overflow.

The user-approved command-line bridge uses `compare(..., include_pairs=True)`
to export the existing local containment map as
`pairs: [{"pilot": {"resource": ..., "fragment": ...}, "intersect": "..."}]`.
The default comparison report is unchanged. No second traversal is added.
Invoke it with:

```sh
python -B tools/pilot-oracle/compare_eval.py --pilot-eval PILOT_EVAL.json \
  --pilot-model PILOT_MODEL.json --native-model NATIVE_MODEL.json \
  --model-level MODEL_LEVEL.jsonl --gated GATED.jsonl --extended EXTENDED.jsonl \
  --out COMPARISON.json
```

All three native inputs are required. `--excluded FILE.json` supplies an explicit
array of `{resource, reason}` entries for native load failures. Producer exit
codes and corpus coverage must also be recorded by the corpus run; an empty
JSONL file alone cannot prove that a zero-site resource loaded. The report's
`comparison_satisfied` is a comparison result, **not** gate GE approval: corpus
coverage and the user's deviation review are still required. Function selectors
use the oracle's newly recorded `function` qualified name; older records without
that field cannot match a function selector.

M3.3 verification: **49 Python tests passed**, including an actual containment
pair export with an unchanged default report, and **49 milestone Java
assertions passed**, including observation isolation and the model-export
adapter. The adapter calls the pinned, unchanged private `PilotOracle.export`
through reflection; an API-shape mismatch fails explicitly. It reuses the
export algorithm on an already loaded model and copies no export logic.
The first identity comparisons have no missing sites: D9 has 64 exact sites,
with its recursive site differing in raw model-level/Extended exception text
(native DepthLimit vs Pilot StackOverflowError); all 65 gated sites agree.
Address Book's one site agrees in all three modes. These subset results do
not establish corpus gate GE; remaining groups and deviations need review.

The `max_real_ulp: 1` entries apply only to the named extended trig function.
They allow one finite ulp on same-sign real values and never waive changed
outcome shapes, identities, evaluability, NaN/infinity or signed-zero changes.
These allowances remain separate from exact agreement. The user approved the
final list on 2026-11-09; the corpus has no trig-tolerance hits.

The recursion entries use an additional `exception_messages` constraint with
exact `pilot` and `intersect` strings and exact site selectors. They apply only
when that exception-message difference is the site's sole difference. A changed
value, evaluability result, error message, target, kind or identity remains a
failure. The three observed recursive sites hit native DepthLimit(256) versus
Pilot StackOverflowError; their allowances require user review under plan §8.
The protocol suite now has **57 passing tests**, including these narrow
allowances and fresh-process collection cleanup/equivalence checks.

`PilotEvalEdgeTest.java` is an additional milestone-only probe using constructed
Pilot objects and the actual library-function implementations. Compile it with
the pinned jar and `target/pilot-oracle/classes` on the classpath, then run
`PilotEvalEdgeTest` on that same classpath. Its six assertions passed with
Temurin 21.0.6+7: Substring retains a lone U+D83D when splitting a surrogate
pair; reflective enum conversion returns `[Java null]`; excludingAt admits a
string start with an integer end and throws ClassCastException; an omitted end
removes just the starting position; argument binding reparents an existing
expression out of its source FeatureValue. These are focused Pilot observations, not
identity-paired native agreement or gate GE evidence.

`PilotEvalMetadataTest.java` adds 18 passing milestone-only assertions on
metadata order, stored Boolean/string reflection, conditions and multiplicity
bounds. Compile and run it on the same classpath. Its in-memory fixture supplies
one `SysML::Systems::PartDefinition` library metaclass through the Pilot's
library-provider adapter; it does not parse or load the standard library.
Evaluation and metadata navigation use the pinned Pilot implementations.
The probe verifies both endpoints and containment of an owned Annotation,
then observes that its contained metadata has no annotated targets: the pinned
`owningAnnotatingRelationship` delegate uses the Relationship container
delegate and omits that containing Annotation. The Rust draft matches this
quirk under E2; metadata-owned annotations and implicit namespace annotation
remain supported. This detail is not described in REF's metadata summary.
These focused observations do not substitute for the identity-paired corpus gate.

Additional isolated corpus runs for `folder-01` and `folder-02` recorded five
sites in three files, with zero load diagnostics and value outcomes in all
three modes. Including D9, available Pilot observations cover 70 of 357 native
corpus sites. The unchanged full model exporter completed `folder-01`; a
user-resource-only containment preflight through unchanged `compare.py` paired
all 21 objects with zero unpaired. Three references point outside that subset
to library objects and remain unpaired references. No expression outcomes were
compared. Artifacts and command/hash provenance are in
`target/pilot-oracle/eval-isolated-examples/`. No verification process is left
running at that checkpoint. The user approved the scope and representation
amendments on 2026-11-09; the CLI exception rows and comparator export are now
committed. Substring rejects lone-surrogate results with EvalError; reflected
present enum values remain explicitly unsupported (unset values yield `[]`).

For fresh corpus collection, `PilotEvalOracle` also accepts
`--model-out MODEL.json` before its positional arguments. It runs the existing
G5 exporter inside observation rollback after the same full fresh load, then
evaluates the selected sites. `collect_eval.py` accepts a JSON manifest with
`groups: [{"id": "folder-02", "files": ["relative/file.sysml", ...]}]`:

```sh
python -B tools/pilot-oracle/collect_eval.py --java JAVA \
  --classpath "PINNED_JAR;CLASSES" --library LIBRARY_ROOT --root INPUT_ROOT \
  --manifest INPUTS.json --out NEW_OUTPUT_DIRECTORY --jobs 3
```

Use absolute classpath entries (a colon separator on Unix). Every group gets
an independent JVM and full library resolution/validation. All workers are
waited for and reaped; only complete successful reports enter the manifest.
An optional `fresh_eval` path checks exact resources, diagnostics, sites and
isolation metadata against an earlier fresh report. Existing outputs are never
overwritten. Exit failures and collection failures remain explicit.

The rejected shared-library experiment is not qualification evidence: although
its first D9 report exactly reproduced a fresh load, the next Address Book
load acquired 49 diagnostics absent from its fresh report. The equivalence
guard stopped the batch. Even an unchanged exported-library digest did not
establish safe reuse, so subsequent groups use fresh JVMs. The first D9
report/export and the earlier fresh Address Book report/export remain valid.

### M3.4 / M5 boundary comparison (2026-11-09)

[The recorded result](results/eval-2026-11-09.json) covers all 47 files admitted
by the recorded G4 load report plus the D9 selection (357 sites, 21 groups).
The other 110 original input files remain explicitly excluded by name/reason.
All producer commands completed, with zero load diagnostics and zero unpaired
expression sites. Fresh JVMs were used for each library/user-file load.

| Mode | Sites | Exact | Approved recursion allowances | Unexplained |
| --- | ---: | ---: | ---: | ---: |
| Raw ModelLevel | 357 | 356 | 1 | 0 |
| Gated ModelLevel | 357 | 357 | 0 | 0 |
| Extended | 357 | 353 | 4 | 0 |

The approved allowances compare only StackOverflowError to DepthLimit(256)
at exact sites; they never waive changed values, evaluability, identities,
targets or missing sites. Five mode observations involve four distinct sites.
The nine named Extended trig tolerances have zero corpus hits. The final
list was approved by the user on 2026-11-09 as required by plan §8;
GE is satisfied for this admitted expression corpus. The raw corpus source hashes, pinned Pilot revision, JDK 21,
commands, excluded files and per-file counts are in the report. The optional
library-site corpus was not run; library loading as context is not library
site qualification. This is not a new structural G5 comparison.

Corpus fixes include re-evaluating Feature arguments in body/callee contexts,
matching REF §1.6/1.7. `PilotEvalBindingTest` provides two independent body
binding assertions. Comparator/collector tests pass 57 cases; the main oracle
harness passes 49 assertions. `PilotEvalEdgeTest` passes six representation/
precedence checks and `PilotEvalMetadataTest` passes 18 metadata/reflection
checks. All Java probes are milestone-only and are never production dependencies.

Final scoped milestone suite: 220 passed, 0 ignored, with the local compatible
standard-library pack. All four plan checks passed independently for each task.
