# Hosted KPAR component prototype

This opt-in native harness opens a retained Text session, downloads a textual
KPAR via HTTP/S, prepares a closed library with the real SysML parser and Rcore
scopes, and mounts its immutable Model component. Project text, saved baselines,
diagnostics, reference targets and the dependency lock belong to the session.
The mounted library is shared; it is never merged into writable project storage.

## Try the prepared worktree

From this worktree, run:

```powershell
./tools/run_hosted_kpar_proof.ps1
./tools/run_hosted_kpar_proof.ps1 -Mode Shell
# Or open your own saved single-file project:
./tools/run_hosted_kpar_proof.ps1 -Mode Shell -Project C:/path/drone.sysml
```

In the shell:

```text
edit package Drone { part engine : Motors::Motor; }
add fixture
refs
inspect Motor
attempt-write Motor
show
usages
quit
```

`add fixture` fetches the checked-in KPAR from a real loopback HTTP server. For
another self-contained textual KPAR, use `add https://host/library.kpar urn:your:library`.
`edit-file C:/path/edited.sysml` replaces only the unsaved buffer. `restore KEY`
mounts an exact locally prepared artifact without network access. There is no
save command: adding a usage cannot save your project as a side effect.

The automated demo exits nonzero on any failed control and writes `report.json`
to the displayed cache directory. It checks edits made while acquisition is
scheduled and after preparation, stale-candidate refusal, real SysML reference
resolution, read-only mutation, shared arenas, exact locks, bad HTTP/archive
inputs, corrupt cache rejection and offline reopening in a fresh process after
the fixture server stops. `reopen.result.json` records that the child process
restored a snapshot with zero downloads and zero library preparations.

## Build from the integrated checkout

```powershell
./tools/run_hosted_kpar_proof.ps1 -Build `
  -BuildDirectory C:/path/build -Cache C:/path/cache
```

Requires the repository's Rust toolchain and installed `curl` (`curl.exe` on
Windows). Maintained manifests and lockfiles select the committed Model and Text
pair. No sibling checkout or local dependency override is required. The compiled
executable embeds the four pinned native language definitions and the independent
ZIP fixture.

Language-program preparation is a milestone operation and can take several
minutes on the first run. Later runs reuse the exact compatible language pack.
Component cache entries include original archive bytes, provenance, descriptor,
and a resolved model snapshot. Context keys include Model, Text and KerML source
identities, schema layout, archive digest, logical package URI and recipe. A
runtime change correctly invalidates earlier artifacts. Restoring checks the
snapshot and graph again but does not parse, link or complete the library.

## Supported boundary

This prototype supports **self-contained textual KPARs** using the existing
native parser's supported SysML/KerML profile. KPARs declaring other package
usages are explicitly refused. Transitive archive acquisition, OMG repository
API import, arbitrary SysML semantic coverage, authenticated registry flows,
multi-language schema composition, editor UI integration, background cancellation
and project save/undo integration remain future work. The shell opens one project
file; the library session API accepts multiple sources. No large-library
performance qualification is claimed.

Network and disk operations stay in the native host. Model and Text remain
language-neutral and WASM-compatible. The new mounted linker evaluates the same
verified Rcore programs over borrowed arenas, with the native authoritative VM
handling cases outside the direct executor's subset.

The archive fixture `crates/intersect-kpar-host/tests/fixtures/motors.kpar` is a
standard ZIP with `.project.json`, `.meta.json`, and `model/Motors.sysml`, generated
independently by `tools/make_component_kpar_fixture.py`.

## Verified worktree result (November 17)

The original prototype executable contained the fixture and language definitions;
it did not need source files to run. The receipts below describe that earlier
worktree qualification, before integration onto current develop.

The final checks passed: Model **937**, Text **955**, KerML **53**, and KPAR host
**10** tests. Strict all-target lint passed for all four crates. Model, Text and
KerML passed component-enabled WASM checks; Model also passed without the
component feature. Formatting, whitespace and ownership/dependency checks passed.

Both cold and warm demos passed **17 controls**, including navigation through
actual borrowed model reference handles. The fixture produced **5 library objects**
kept outside the **6 writable project objects**. Cold acquisition prepared the
library once. Warm acquisition restored it once with **zero library preparations**.
The fresh-process offline control recorded **zero downloads**, **zero library
preparations**, and one resolved snapshot restore, preserving the unsaved bytes.

The manual shell was also exercised through the launcher against a real project
file. Adding the hosted fixture changed the edited model from an unresolved
reference to a checked model with no diagnostics; inspecting `Motor` reached a
`PartDefinition`. A write returned `Component objects are read-only`. The saved
project file's SHA-256 remained unchanged.

Local receipts: [qualification](../target/component-demo/qualification.json),
[cold report](../target/component-demo/cold-report.json),
[warm report](../target/component-demo/warm-report.json),
[offline report](../target/component-demo/offline-report.json), and
[manual transcript](../target/component-demo/manual.log).
These are prototype worktree qualifications, not published registry or release
artifacts. Historical development reports include failed probes and a deliberately
stopped default-profile regression run; the qualification receipt selects the
final successful runs.

## Integrated dependency graph

The development source pins select one committed Model and Text pair. A fresh
checkout can run `./tools/run_hosted_kpar_proof.ps1 -Build` without sibling
checkouts or path patches. `-BuildDirectory` and `-Cache` relocate host artifacts;
they do not change the dependency graph. The components APIs remain opt-in and are not yet a registry release.

## Prepared standard-library component pack

Component-enabled `sysml_pack build --library` adds the optional
`library/sysml-component` part. The native builder reads the exact selected
library snapshot, preserves physical slots and assignment evidence, prepares
the closed component, then checks its saved snapshot with full revalidation.
The part key binds the runtime/compiler and exact original library payload
digest. Replacing or removing the library also removes this dependent cache.
Incremental builds reuse a compatible, checksum-checked cache.

The required section of this optional part contains a bounded versioned header,
the component descriptor and its native snapshot. It is host-built output, with
the same trust convention as the executable language pack. It is not a KPAR
interchange format or a cryptographic signature. The component snapshot is
stored alongside the ordinary snapshot; this prototype trades additional pack
bytes for removing runtime preparation. Loading a project retains the cache
bytes without decoding the extra model. The first hosted usage restores the
component from the verified section before publishing its handle. It never
materializes a stdlib graph, reparses or relinks it, or reruns its admission.
Authored project checks and ordinary preparation of downloaded KPARs remain.

A classic-only pack can still open ordinary projects. If it contains a stdlib
but lacks its component cache, hosted usage preparation reports a rebuild
requirement before adoption. It does not fall back to slow runtime preparation.

## Retained explorer projection

The IDE facade prepares each closed component's explorer nodes once, using the
ordinary native presentation projection and the image's actual qualified IDs and
source correspondence. Later analyses project only editable objects and merge
these cached nodes. Reciprocal overlays do not change component names, spans or
containment. Scope, completion, semantic checks and inspection continue to read
the current mounted model. A focused comparison checks the merged projection
against a full mounted traversal before and after project edits and overlays,
including unnamed containment wrappers and two mounts with identical local IDs.
