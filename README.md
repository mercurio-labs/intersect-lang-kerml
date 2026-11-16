# Intersect KerML and SysML

The UI-independent `intersect-kerml-ide` facade serves native and WASM consumers.
See [the bounded visualization and source-command profile](docs/visualization.md).

The KerML and SysML v2 language package for
[Intersect](https://github.com/mercurio-labs/intersect-model): native loading of
KerML and SysML models against the SysML v2 standard library, with language
semantics written in Rcore and run on Intersect's frozen engine, checked
against the Java SysML v2 Pilot Implementation (used only as a test oracle).

This repository starts with a fresh `develop` history on November 16, 2026,
from the previously maintained KerML source at
`b5392d1641af92d4274d583aa80c33972062e2df`. Earlier history, branches and
GitHub discussions are preserved in the read-only
[intersect-lang-kerml-pre-dev](https://github.com/mercurio-labs/intersect-lang-kerml-pre-dev)
repository. See [the reset receipt](docs/repository-reset.json) for provenance.

The language package was originally split from `mercurio-labs/intersect`
after Release 1 (v0.1.0). The
status, performance figures and release notes up to the split are in the core
repository's [README](https://github.com/mercurio-labs/intersect-model-pre-dev/blob/3d7d6a3a8cc219e1b8874aeb1258dae3749d641a/README.md#where-things-stand)
and [release notes](https://github.com/mercurio-labs/intersect/blob/develop/docs/release-notes.md).

## Crates

| Crate | What it is |
| --- | --- |
| `intersect-kerml` | The language package: pinned Pilot grammars, Rcore semantics (`semantics/`), the frozen KerML/SysML load, language packs and the standard-library snapshot. |
| `intersect-kerml-eval` | KerML expression evaluation on the frozen model ([plan](docs/kerml-expression-evaluation-plan.md)). |
| `intersect-kpar` | KerML project interchange archives (`.kpar`): bounded reading, deterministic writing, offline `usage` resolution ([plan](docs/kpar-plan.md)). No I/O; builds for WASM. |
| `intersect-kpar-host` | Package-source and lock-catalog bridge over `intersect-kpar`, using core's project-usage resolver. |
| `intersect-cli` | The `intersect` command: check, evaluate and export `.kerml`/`.sysml` files ([tutorial](docs/cli-tutorial.md)). |
| `intersect-kerml-web` | Core's `intersect-web` browser host plus the `draft/kerml/*` methods, with the same WASM ABI. |

Standalone probe sources remain under `tests/*-only` and
`fixtures/xtext/imported-assignment-v66/full-source-probes/native-*`.
The three historical read/name probes already fail on the unchanged develop
baseline because their language policy exports/controls are absent; they are
retained and are not counted as passing split acceptance. See
[the consumer receipt](docs/repository-split-consumer.json).

## Dependencies on Model and Text

Modeling, execution, project packages and build support come from
[intersect-model](https://github.com/mercurio-labs/intersect-model).
Grammar compilation, text interpretation and Web come from
[intersect-text](https://github.com/mercurio-labs/intersect-text).
This language package owns KerML/SysML semantics and standard libraries.
Neither upstream repository depends on this repository.

Model, Text and build support use the exact crates.io release
`=0.1.0-alpha.1`. Unpublished project-host, visualization and browser-host
adapters use immutable commits from the public Model and Text repositories.
These adapters consume the same released runtimes, so the maintained KerML
workspace has one Rust package identity per core crate and requires no patches
or private repository access. Generic visualization contracts now live in Model.

The workspace requires Rust 1.88 or newer; the development toolchain is pinned
separately. This dependency update does not qualify additional KerML/SysML
semantics or the historical standalone probes. Update maintained standalone
consumers and lockfiles together; historical imported fixture manifests retain
their original provenance. Hosted CI execution remains separate from local
build verification.

### Coordinated local changes

Use an explicit, uncommitted Cargo configuration at this workspace root:

```toml
[patch.crates-io]
intersect-model = { path = "C:/work/intersect-model/crates/intersect-model" }
intersect-build-support = { path = "C:/work/intersect-model/crates/intersect-build-support" }
intersect-text = { path = "C:/work/intersect-text/crates/intersect-text" }
```

Pass it explicitly with `cargo --config path/to/local-upstreams.toml ...`.
Ordinary builds require neither patches nor sibling checkouts. Remove overrides
before qualification and inspect `cargo metadata --locked` and `cargo tree -d`
for duplicate Intersect package identities. Land Model changes first, then Text,
then select the exact tested release pair for this language package.

The shared host-only source identity scanner is the Model-owned
`intersect-build-support` crate. Program compiler components compose installed
Model/Text identities. Historical archive component keys such as
`intersect-xtext` retain their existing wire meaning; the active Rust crate and
imports are `intersect-text` and `intersect_text`.

Hosted CI is explicitly skipped for this split. Local execution receipts retain
their actual platform and source revisions; skipped checks are not passes.

## Native Python SDK

The [Python SDK](python/README.md) builds generated KerML/SysML wrappers and a
native PyO3 wheel alongside the prepared language pack. It supports folder and
in-memory projects, stored-model inspection and validated source transactions.
The Python crate has its own workspace and stays outside the default WASM build.

## Browser Toolkit

The Toolkit lives in the Text repository. To run it with KerML, build this
repository's host and point the Toolkit's WASM build at it:

~~~sh
cargo build -p intersect-kerml-web --release --target wasm32-unknown-unknown --locked
~~~

Then, in the Text checkout, set `INTERSECT_WEB_WASM` to
`target/wasm32-unknown-unknown/release/intersect_kerml_web.wasm` from this
repository and run `npm run build` in `toolkit/`. The KerML draft browser
specs run with `playwright.prepared-draft.config.js` and
`playwright.physical-project.config.js` there.

## Checks

### Test tiers

Run suites through `tools/test_report.py` (every run appears at
http://localhost:8765/).

- **Quick tier**, after every change: the crates you touched, default
  features. It must finish in under 10 minutes including the build.

  ~~~sh
  python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml -p intersect-kerml-eval -p intersect-kerml-web -p intersect-cli -p intersect-kpar -p intersect-kpar-host --lib --bins --tests
  ~~~

- **Milestone tier**, at milestone boundaries only: everything, including
  whole-library loads and language-pack equivalence.

  ~~~sh
  python -B tools/test_report.py run -- --profile test-fast --workspace --locked --no-fail-fast --features intersect-kerml/milestone-tests,intersect-kerml-eval/milestone-tests,intersect-kerml-web/milestone-tests,intersect-cli/milestone-tests
  ~~~

### Rust checks

Rust 1.99.1 is pinned in `rust-toolchain.toml`, matching core; rustup installs
it on first use, and CI names the same release.

~~~sh
python -B tools/format_workspace.py --check
cargo clippy --workspace --all-targets --locked -- -D warnings
rustup target add wasm32-unknown-unknown
cargo build --workspace --lib --target wasm32-unknown-unknown --locked
~~~

## License

MIT; see [LICENSE](LICENSE). Pinned upstream inputs under `fixtures/` keep
their own license notices.
