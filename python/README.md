# Native KerML and SysML Python SDK

This package adds native model opening, complete stored-model inspection and
validated source-based creation. It uses the language package's Rust engine and
prepared standard library. The workbench and project server are independent.

## Build a local wheel

Requires conventional CPython 3.11 or later, Rust, the Windows MSVC build tools,
Git access to the pinned private core, and `maturin==1.13.3`. The core revision
must be available in the configured Git source; development commits can be
seeded in the local Cargo cache before their upstream publication.

From the language repository root:

```powershell
python -m pip install maturin==1.13.3
cargo run --release --locked -p intersect-kerml --example sysml_pack -- build . target/python-release/sysml.pack --library --python-out target/python-release/python --wheel
python -m venv target/python-smoke
target/python-smoke/Scripts/python -m pip install --no-index --find-links target/python-release/python/wheels intersect-kerml
target/python-smoke/Scripts/python -B -m unittest discover -s python/tests -v
```

Use a fresh `--python-out` directory for each build. The command creates the
pack, wrappers, typed modules, compatibility manifest, native extension and
wheel. The ordinary pack-only command still works without Python or maturin.
The native crate is excluded from the default Rust/WASM workspace. Its separate
Cargo.lock is committed, and its dependencies do not enter the workbench build.

The first pack build compiles the grammars, language bindings and entire
standard library. It is a milestone operation and can take tens of minutes.
Pass `--previous PATH` to reuse parts whose identities still match. Generated
artifacts are staged before the standalone pack is written; the Python package
and its bundled pack are published together by renaming their staging directory.
A failed wheel build leaves the staging directory for inspection and preserves
the previously published output. Avoid reusing a failed staging directory.
The completed language build is also saved as `sysml.build-cache.pack` before
Python packaging. Use it with `--previous` to retry packaging without rebuilding
compatible language parts. Wrapper-name checks run before semantic compilation.

The SDK code uses the repository's MIT license. Bundled Pilot inputs retain their
EPL-2.0 notice; both notices are included under the installed package's resources.

## Open and inspect

```python
from intersect_kerml import Engine
from intersect_kerml.sysml import PartDefinition

with Engine() as engine:
    project = engine.open("models/vehicle")
    analysis = project.analyze()
    for diagnostic in analysis.diagnostics:
        print(diagnostic.severity, diagnostic.message, diagnostic.location)
    with analysis.require_model() as model:
        for part in model.elements(PartDefinition):
            print(part.declared_name, part.id, part.source_location)
            print(part.stored_properties(raw=True))
```

`kerml` and `sysml` expose the same combined upstream metamodel and share class
identities. Element queries include subtypes; standard-library and dependency
elements are available with `include_libraries=True`. Unnamed elements and
relationship wrappers remain accessible. `container`, `children`, `get`, and
`is_instance` use native containment, reflection and subtype services.

Read-only generated accessors currently support strings, booleans, integers,
enums and references, including collections. Unsupported codecs and derived or
volatile members have explicit reasons in `bindings.json` and raise
`UnsupportedError`. `get(feature, raw=True)` and `stored_properties(raw=True)`
expose other stored nominal values as immutable native JSON data. This preserves
their information without claiming an implemented Python scalar codec.
Python keywords and names reserved by the convenience API receive a trailing
underscore. For example, `MetadataFeature.metaclass_` addresses the native
`metaclass` feature; the wrapper's `metaclass` property reports its element type.

Snapshots retain their own native model after later analyses. IDs are scoped
to a snapshot, not durable OMG identities. Closing a model or engine makes its
handles fail explicitly. Close unused models to release native memory; the
engine accepts at most 64 retained snapshots. Source positions use UTF-8 byte
offsets with an exclusive end.

## Create and edit

```python
from intersect_kerml import Engine

with Engine() as engine:
    project = engine.new_project()
    with project.transaction() as transaction:
        transaction.put_source("vehicle.sysml", """
            package Vehicles {
                part def Vehicle;
                part vehicle : Vehicle;
            }
        """)
        transaction.validate().require_model()
        transaction.commit()
    project.save_as("models/vehicle")
```

Transactions add, replace or delete sources and compile a complete candidate
through the language package. Invalid candidates return diagnostics; commit
rejects them. Stale commits fail without changing the project. Exiting without
commit aborts. Source text is authoritative; general structured builders,
renames, model-to-text round trips, derived queries and evaluation are deferred.

Opening a folder reads `.project.json` when present and accepts folders without
one. KPAR usage resolves offline through `intersect-kpar`. No network fetch,
Rust build or cache regeneration occurs during import or model analysis.
Archive version constraints use the upstream resolver. Standard-library version
constraints remain unverified because the prepared pack has no project version.
The initial host limits folders to 4,096 entries, source inputs to 256 files and
8 MiB, dependency archives to 64 files and 8 MiB combined, and descriptors to
1 MiB. The bounded native JSON transport also has a 16 MiB request limit.

`save()` checks loaded inputs for external changes. Each replacement is atomic,
but a whole-folder save is not. Interrupted saves retain `.intersect-save-*`
with `journal.json` and `old/` copies; reconcile those files before retrying.
`save_as()` requires a new destination and copies documents and sidecars while
excluding tooling folders and symbolic links.

## Compatibility and trust

The package checks schema fingerprint, generator/runtime API, native compiler
identities, pack recipe/parts and digest before exposing a model. The pack
builder and extension use the same pinned engine and features. Changing either
can require rebuilding the pack. Windows x64 with conventional CPython is the
initial target; other platforms and free-threaded interpreters are unqualified.

This implementation supports locally built wheels and packs from a trusted
matching toolchain. `Engine()` trusts the pack installed with that locally built
wheel. An external path requires `Engine(pack=path, trusted=True)` and still must
match the generated artifact manifest. Checksums detect mismatches; they do not
authenticate an artifact producer. Automated network pack installation and
signed public release distribution are not implemented.

Language state is constructed and destroyed on a dedicated native thread. Python
calls release the GIL while waiting; calls on one engine are serialized through
a bounded queue. Engine parse/link/validation budgets still apply. Hard process
memory/time isolation and cancellation of an in-flight native request are not
provided by this initial host.

See [verification.md](verification.md) for the qualified artifact and checks.
