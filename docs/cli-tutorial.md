# The `intersect` command: a short tutorial

`intersect` checks KerML and SysML v2 files against the SysML v2 standard
library and exports them as JSON or XMI. It runs natively, with no Java. The
languages, their semantics and the prepared standard library come from one
file, a **language pack**.

This tutorial builds a pack, checks one of the Pilot example models and
exports it. Commands run from the repository root.

## 1. Build the command and a language pack

```sh
cargo build --release -p intersect-cli
cargo build --release -p intersect-kerml --example sysml_pack
target/release/examples/sysml_pack build . sysml.pack
```

`sysml_pack build` compiles the pinned Pilot grammars, migrates the SysML
metamodel and compiles the Rcore bindings (name resolution, defaults and
implicit elements) into `sysml.pack`. This takes 15 to 25 minutes on a
16-core machine, almost all of it compiling the bindings, and is needed only
once per Intersect build.

A pack is tied to the binaries that built it: build `intersect` and
`sysml_pack` from the same sources (two `cargo build` commands; `--example`
restricts a build to examples, so it cannot build both at once), and after rebuilding Intersect from
changed sources, `intersect` refuses an old pack ("compiler identity
differs"). Rebuild it with `sysml_pack build . sysml.pack --previous
sysml.pack`, which reuses the parts that did not change.

## 2. Prepare the standard library (once)

The first check parses the 94 files of the standard library and saves a
snapshot of the result into the pack, so later runs load it in a fraction of
a second:

```sh
target/release/intersect check --pack sysml.pack \
  --library fixtures/native-pilot/v1/sources/sysml.library \
  "fixtures/native-pilot/v1/sources/sysml/src/examples/Simple Tests/PartTest.sysml"
```

This prints `intersect: saved the library snapshot into sysml.pack`; it took
under 3 minutes on a busy 16-core machine. Later checks of a few files take
a few seconds: the snapshot itself loads in about 0.2 s, binding the languages
about 1 s.

`--library` is needed only while the pack holds no library snapshot. From
then on the pack also stores the library sources, so a stale snapshot (after
a pack rebuild) is re-prepared without `--library`.

## 3. Check models

```sh
target/release/intersect check --pack sysml.pack model.sysml other.kerml
```

Files with the `.kerml` extension are read as KerML, the rest as SysML. All
files load together, on top of the standard library, so they may reference
each other and the library. Each problem is printed as

```text
ActionTest.sysml:33:25: error[link]: redefinedFeature: no element named 'receiver'
demo.sysml:5:5: error[parse]: No viable predicted alternative
2 file(s) checked: 2 problem(s)
```

with the phase that found it: `parse`, `link` (name resolution),
`implicit`, `defaults`, `validate` (structural validation) or `check` (the
language's validation rules, `bind validate` in Rcore). A validation rule
may also report a warning or a note, printed as `warning[check]` or
`info[check]`; a problem with a code ends with it in parentheses:

```text
Imports.kerml:4:1: error[check]: Top level import must be private (validateImportTopLevelVisibility)
```

Rules ported from the Pilot use the Pilot's issue code verbatim; a
structural problem's code is its kind (`Multiplicity`, `TargetType`, ...).
With `--json`, each problem is one JSON object per line (`severity`
(`error`, `warning` or `info`), `phase`, `message`, and `code`, `rule`,
`feature`, `file`, `line`, `column`, `element` when known).

Exit status: 0 when no problem is an error (warnings and notes do not
count), 1 when one is, 2 for a usage or host error (a missing file, a
refused pack).

## 4. Export

```sh
target/release/intersect export --pack sysml.pack --format xmi --out out model.sysml
target/release/intersect export --pack sysml.pack --format json --out out model.sysml
```

XMI writes one document per input file (`out/model.sysml.xmi`); references
into the standard library are references to the library resources. JSON
writes `out/model.json`, the Intersect model of the input files. For
PartTest, the XMI document is 219 KB and the JSON model 327 KB.

## 5. Inspect a pack

```sh
target/release/intersect info --pack sysml.pack
```

lists the pack's parts and its library snapshot key, and whether each
compiler component of the pack matches this `intersect` build (the
components that differ are why a pack is refused).

## Evaluate expressions

```text
intersect eval --pack PACK [--target QNAME] [--mode model-level|extended] [--gated] [--json] [--trace] [--explain] (--expr TEXT | --all) [FILE...]
```

`--expr` evaluates an expression, optionally in the context of a fully qualified
`--target` from the input files or library. Files and the expression wrapper load
together on the prepared library. `--all` requires files and evaluates their
feature values, result expressions and multiplicity bounds in containment order;
nested expressions are included in their enclosing site.
An explicit `--target` overrides each site's default evaluation context.

The default is `extended`: the model-level functions plus the extended
library functions (sequences, iteration, control functions) and inlining of
user `calc`/`function` bodies; metadata access (`@@`, `meta`, metaclass
reflection) is included. `model-level` restricts evaluation to the functions
the Pilot's model-level evaluator supports. `--gated` applies the model-level
`Expression::evaluate` gate. Unsupported or unbound expressions
remain visible in the result; `--explain` describes the reason.

Results go to stdout. Text uses Pilot-style value names, with `()` for an empty
sequence and `(not evaluable)` for an absent result. `--json` writes one JSON
object per site with `site`, `evaluable` and `outcome` fields. `--trace` writes
evaluation events to stderr, as JSON lines when combined with `--json`.
Wrapper diagnostics and trace locations use the user's expression positions.

Exit status is 0 when all evaluations ran, including unevaluated results;
1 for load diagnostics; 2 for usage, host or evaluator errors (with `--all`,
after every site has been reported).
Build a pack containing the standard library with
`sysml_pack build . PACK --library`, using the same sources and Cargo features
as the command. Milestone tests use `INTERSECT_EVAL_PACK` to select that pack.

Three examples from the pinned `Expressions.kerml` corpus (stdout shown;
each returned status 0):

```sh
FILE="fixtures/native-pilot/v1/sources/kerml/src/examples/Simple Tests/Expressions.kerml"
target/release/intersect eval --pack sysml.pack --expr "Expressions::C::count" "$FILE"
# LiteralInteger 0
target/release/intersect eval --pack sysml.pack --expr "Expressions::test1" "$FILE"
# LiteralBoolean false
target/release/intersect eval --pack sysml.pack --expr "Expressions::test2" "$FILE"
# LiteralBoolean true
```

To evaluate every site with the model-level gate and functions:

```sh
target/release/intersect eval --pack sysml.pack --all --gated --mode model-level "$FILE"
```

This reports 25 sites, including results marked `(not evaluable)`. In the
default `extended` mode the recursive `Expressions::z` value reaches the
default depth limit of 256: that site is reported as `(error: evaluation
depth limit exceeded: 256)`, the other sites are still evaluated (all 25),
and the command exits 2. Sites that are not feature values are marked
`[result]` or `[multiplicity]`. These are native evaluation observations, not
Pilot comparison results.

Inside `--expr`, library functions are not imported: write
`->ControlFunctions::collect {...}` or `SequenceFunctions::size(...)`, or
evaluate against a `--target` whose file imports them.

## Run a SysML analysis case

An execution profile selects a versioned analysis engine for the linked SysML
model. In the [cistern example](examples/sysml-execution-profile/cistern.sysml),
the analysis usage binds the subject, the case defines an asserted equality,
and its return parameter declares the quantity type. KerML evaluation computes
the numeric values of the model-bound inputs, including the authored
`(2 + 4) [L]` capacity. The shared affine solver solves the equation for the
return parameter. The profile reports volume flow rates in litres per second.

The [authoring manifest](examples/sysml-execution-profile/language.toml)
supplies the default profile.

```toml
[execution]
default = "sizing"

[execution.profiles.sizing]
engine = "intersect.scalar-affine@1"
```

Build the CLI and prepare an identity-matched SysML pack using the steps above.
The manifest is supplied alongside the pack.

```sh
python3.13 -B tools/compiler_build.py -- build --release --locked -p intersect-cli
FIXTURE=docs/examples/sysml-execution-profile
target/release/intersect analyze --pack sysml.pack --read-only \
  --execution-profile "$FIXTURE/language.toml" --case CisternAnalysis::sizeInlet \
  --inputs "$FIXTURE/four-seconds.json" "$FIXTURE/cistern.sysml"
# CisternAnalysis::sizeInlet [sizing] flow = 1.25 L/s
```

The input JSON is an array of named quantities. It binds parameters the
model leaves open. A caller can select another declared profile with
`--profile NAME`. With `--json`, the result includes the profile and engine,
the linked case, subject, input, result, and equation locations, and digests
for the manifest, pack, inputs, and model sources. Load and analysis diagnostics
exit with status 1. Invalid configuration and host errors exit with status 2.

This profile admits one affine asserted equality and one unbound scalar return.
It supports dimensionless scalars and uses litres, seconds, and litres per
second for volume, duration, and volume flow rate.
Explicit objectives, foreign bodies, and additional analysis steps receive
unsupported diagnostics. The evaluator library builds for
`wasm32-unknown-unknown`.

Cargo pins the shared execution engines to Model commit
`33102b3707fb68b5377a901548311ddb258a22d8`.

The qualification script runs the checked fixture, a model-bound duration, a
unit conversion, and the rejection controls against an already prepared pack.

```sh
python3.13 -B tools/verify_sysml_execution_profile.py target/release/intersect sysml.pack
```

### Run state-driven sizing

The [stateful cistern](examples/sysml-execution-profile/stateful-cistern.sysml)
adds a two-mode controller to the tank. Select the `stateful` profile to run
the linked state machine and scalar analysis in the same native command.
The profile binds the active state's numeric `durationSeconds` to the
analysis input `fillTime` in seconds. The scenario supplies qualified event
types in millisecond order. Events at the same tick run in file order.

```sh
FIXTURE=docs/examples/sysml-execution-profile
target/release/intersect analyze --pack sysml.pack --read-only \
  --execution-profile "$FIXTURE/language.toml" --profile stateful \
  --case StatefulCistern::sizeInlet --events "$FIXTURE/cistern-events.json" \
  "$FIXTURE/stateful-cistern.sysml"
```

Start, `Boost`, and `Reset` give `0.5`, `1.25`, and `0.5 L/s`. Events sent in
the wrong state produce rejected observations. Add `--json` for the linked
state, transition, parameter, and equation locations, along with file digests.
This profile supports one flat controller with unguarded item-triggered
transitions and one binding to an open scalar analysis input.

```sh
python3.13 -B tools/verify_sysml_stateful_cistern.py target/release/intersect sysml.pack
```

### Run a thermostat-controlled enclosure

The [thermostat](examples/sysml-execution-profile/thermostat.sysml) links a
heater state machine and an affine heat-balance analysis. The
`intersect.coupled-ode@1` profile feeds the current temperature and active
state's heater power into that analysis to compute the slope at each RK4
stage. The model supplies the initial temperature and switch thresholds.
This profile integrates one scalar, with temperatures expressed as numeric
degrees Celsius and the derivative as degrees Celsius per second.

```sh
FIXTURE=docs/examples/sysml-execution-profile
target/release/intersect analyze --pack sysml.pack --read-only --json \
  --execution-profile "$FIXTURE/language.toml" --profile thermostat \
  --case Thermostat::temperatureRate "$FIXTURE/thermostat.sysml"
```

The trace contains 121 regular temperature samples and the intervening
threshold crossings over 30 minutes. A crossing at a sample tick applies its
linked transition before the sample is recorded. At 120 W the first shutoff
is at 430.023 seconds. Changing the authored heating value to 250 W moves
the first shutoff to 122.599 seconds.

```sh
python3.13 -B tools/verify_sysml_thermostat.py target/release/intersect sysml.pack
```

Use `--reference` with the Rcore thermostat trace JSON to compare sample and
crossing times, modes, and temperatures across the two language adapters.

## Project archives (`.kpar`)

A KerML project interchange archive (`.kpar`, a ZIP file with
`.project.json` and `.meta.json`) can be given wherever a file can:

```sh
target/release/intersect check --pack sysml.pack vehicles.kpar
target/release/intersect check --pack sysml.pack --project-path deps vehicles.kpar
```

The project is where the archive's one `.project.json` is: its root or a
folder (the OMG library archives use one folder). Its `.kerml` and
`.sysml` files load under `<archive>!/<path in the archive>`, and problems name them that way
(`vehicles.kpar!/model/Car.sysml:12:5: ...`). Reading checks the metadata,
verifies SHA224/256/384/512 checksums, and refuses what is not loaded
rather than skipping it: JSON or XMI model files, nested archives,
`includesDerived`/`includesImplied` set to true, a metamodel other than
OMG KerML or SysML. Each `.meta.json` index entry must name a top-level
member of its file. Problems and notes from archives print with the phase
`archive`.

Dependencies (`usage`) resolve offline. The OMG standard library archives
(`https://www.omg.org/spec/{KerML,SysML}/<release>/<Library>.kpar`) are
provided by the pack's library; their version constraints are noted, not
checked. Any other usage resolves to the one archive whose file name is
the IRI's last segment, among the inputs and the `.kpar` files in each
`--project-path` folder, and its `versionConstraint` (Semantic Versioning,
`1.0.0` meaning `^1.0.0`) must match that archive's `version`.

The standard library can come from the OMG archives too: repeat
`--library` for each `.kpar`. Their files load as `<archive>!/<file>`, so
the pack prepares a separate snapshot for them once.

To write the input files as an archive:

```sh
target/release/intersect export --pack sysml.pack --format kpar --out out   --project-name Vehicles --project-version 1.0.0 model.sysml parts.sysml
```

writes `out/Vehicles.kpar`: the files' text unchanged, an index of their
top-level members, SHA256 checksums, and a `usage` of each OMG standard
library archive they reference, always the Semantic Library and, for SysML
files, the Systems Library (plus the usages of input archives).
Dependencies loaded from the project path are not written. `created` is
the current time, or `SOURCE_DATE_EPOCH` when set; equal inputs and time
give equal bytes. The format is described in
[the format reference](kpar-format-reference.md).

## Options

| Option | Meaning |
|---|---|
| `--library DIR` | Standard library sources, when the pack has no usable snapshot; or `--library FILE.kpar`, repeated, for library archives |
| `--project-path DIR` | Folder of `.kpar` files that archive inputs' `usage` resolves against (repeatable) |
| `--project-name`, `--project-version` | Project name and version of `export --format kpar` |
| `--read-only` | Never write the library snapshot into the pack |
| `--no-library-cache` | Parse the library even when the pack holds a snapshot (debug) |
| `--json` | Problems as JSON lines |
| `--timing` | Phase times on stderr |
