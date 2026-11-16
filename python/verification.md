# Initial Windows qualification — 2026-11-11

The local `codex/python-bindings` implementation was built against exact core
revision `29bd8cf91963f2af4c5c1f67fcd67e7fd1464e9a`. The language branch includes
the existing KPAR prerequisite `ff1196743708a5be0914f29b40a42b95106ac17f`.
At qualification time these commits were local. Installing the completed wheel
requires no Git or Rust toolchain.

The completed artifact is
`intersect_kerml-0.1.0-cp311-abi3-win_amd64.whl`, built with maturin 1.13.3 and
PyO3 0.27.2, then installed into a fresh conventional CPython 3.11 environment.
Tests ran with Rust, Git, Java and Node absent from PATH and PYTHONPATH cleared.
The ABI tag targets CPython 3.11+; only CPython 3.11 on Windows x64 was executed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| Wheel | 19,648,995 | `32664549923a6e4d965937933e6e22ec9073f9d53c0e18c85307a29b98537cc4` |
| Bundled pack | 272,868,549 | `201f19e241cb174b10e8448ec4ebea14aa1f19adbc6ade08802e96dea33d30ca` |

The wheel contains the native extension, Python source and native stub, generated
175 classes and seven enums, KerML/SysML aliases, `py.typed`, compatibility
manifest, full prepared pack and both license notices. No interpreter caches
are included. Library preparation validated 109,826 objects and resolved all
18,939 references. An incremental `--previous` build reused every part and
produced the same pack bytes.

Validation completed:

- Core `intersect-model` library/binary/integration suite: 749 passed, zero
  failed, 19 ignored. Report: [20261111-162347](http://localhost:8765/intersect-python/20261111-162347/).
- Installed-wheel acceptance and filesystem controls: all 10 passed in 70.040s.
  They exercise native inspection, source authoring, invalid and stale candidate
  rejection, multithreaded calls, save/reopen, descriptor/offline KPAR loading,
  path controls and pack compatibility rejection.
- Basedpyright 1.40.2: zero errors, warnings or notes for the installed generated
  module and typed consumer example.
- Strict native clippy checks passed for core, language SDK/pack builder/CLI and
  the separate PyO3 crate. Rust formatting checks passed.
- Core and default language `wasm32-unknown-unknown` compile checks passed.
  PyO3 is excluded from the default language workspace.
- Root/native lockfiles have matching sources for shared package versions.

Reproduce the source build and installed tests using [README.md](README.md).
The final local output directory was `target/python-final/`; generated artifacts
are ignored rather than committed.

This qualification covers local matching-toolchain distribution. Producer
authentication, public release CI, other operating systems/Python versions,
free-threaded CPython, independent generator cache identity, structured semantic
builders and evaluation/derived dispatch remain separate work. The engine's
source identity conservatively invalidates semantic caches after generator
source changes. No remote artifacts or commits were published by this run.

## Local develop integration — 2026-11-12

The Python branches were merged with current `develop` in dependency order.
The language merge pins every core manifest and lockfile to merged core revision
`f676cb5f9b86e0f640bf4c8f8ab272c424344277`. It preserves the newer
standard-library package preparation and pack-host reuse path. Shared
dependencies in the workspace and native lockfiles have matching sources.

The merged core quick suite passed 773 tests, with no failures or crashes:
[merge report](http://localhost:8765/intersect-python/20261112-081857/).
Strict core/native-extension lint, core formatting and core WASM compilation
also passed. The original artifact and installed-wheel qualification above
predate this integration. The merged source requires a fresh matching pack and
wheel; those artifacts were not rebuilt as part of the merge.

The merged language/CLI quick suites passed 66 tests, with no failures or
crashes: [merge report](http://localhost:8765/kerml-python/20261112-082314/).
Strict language/CLI/pack-builder lint, language formatting and the default
language WASM compile check passed. All seven workspace, native and standalone
lockfiles were refreshed and resolved offline against the merged core revision.

These merges remain local pending explicit permission to push shared `develop`.
