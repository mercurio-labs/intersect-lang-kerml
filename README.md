# Intersect KerML and SysML

Check KerML and SysML v2 source, evaluate supported expressions, and export
models as JSON or XMI. Use the `intersect` CLI, embed the Rust language package,
or build the Python SDK or a WebAssembly host.

[CLI tutorial](docs/cli-tutorial.md) · [Rust API reference](#rust-api-reference)
· [Python SDK](python/README.md) · [Editor integration](docs/editor-services.md)

This repository owns the KerML and SysML language definitions, Rcore policies,
standard-library preparation, and KPAR project interchange. Intersect Model owns
the shared model and execution runtime. Intersect Text owns grammar compilation
and text interpretation. Java is used only when replaying the independent Pilot
comparison. See [Intersect Model](https://github.com/mercurio-labs/intersect-model)
and [Intersect Text](https://github.com/mercurio-labs/intersect-text).

## Start with the CLI

Clone this repository and install Rust with `rustup`. The checkout selects the
pinned Rust toolchain. Build the CLI from the repository root.

```sh
cargo build --release --locked -p intersect-cli
```

To check your first model, build a language pack from the same checkout and
follow the [CLI tutorial](docs/cli-tutorial.md). A pack contains the prepared
grammar and language rules and can also include the standard library. The first
full pack build can take tens of minutes. Keep your authored `.sysml` and
`.kerml` files so you can rebuild derived packs when the runtime changes.

KerML crates in this workspace are built from source. The previous coordinated
source release is tagged `release-2026-11-16` and uses Model, Text, and
build-support `0.1.0-alpha.1` from crates.io. This development checkout uses
canonical Git revision patches in [Cargo.toml](Cargo.toml) for the unreleased
editor and component APIs. The committed manifests and locks select the same
runtime identities across the adapters and standalone consumers. A fresh
development checkout needs Git access to those revisions and crates.io.

## Rust API reference

Build the reference from the root of this checkout. Rustdoc generates searchable
HTML from the same source as the API you're using.

```sh
cargo doc --locked --no-deps -p intersect-kerml -p intersect-kerml-ide \
  -p intersect-kerml-eval -p intersect-kpar -p intersect-kpar-host
```

| Crate | Use it for | Local rustdoc entry |
| --- | --- | --- |
| `intersect-kerml` | Native language loading, scopes, and language packs | `target/doc/intersect_kerml/index.html` |
| `intersect-kerml-ide` | Source analysis, navigation, completion, and bounded view commands for native or WASM hosts | `target/doc/intersect_kerml_ide/index.html` |
| `intersect-kerml-eval` | Expression evaluation over the stored model | `target/doc/intersect_kerml_eval/index.html` |
| `intersect-kpar` | Reading, writing, and resolving KPAR archives | `target/doc/intersect_kpar/index.html` |
| `intersect-kpar-host` | Host-side archive acquisition and dependency locks | `target/doc/intersect_kpar_host/index.html` |

Repeat the command with `--features intersect-kerml/components,intersect-kerml-ide/components,intersect-kpar-host/components`
to include the experimental component API. For the published upstream APIs,
see the [Model rustdoc](https://docs.rs/intersect-model/latest/intersect_model/)
and [Text rustdoc](https://docs.rs/intersect-text/latest/intersect_text/).

## Integrate with an application

- Build the native Python wheel and inspect model elements or edit through source
  transactions. The Python project has a separate Cargo workspace. See the
  [Python SDK guide](python/README.md).
- Embed the IDE facade for source diagnostics, resolved reference navigation,
  contextual completion, and the supported SysML view and source-edit profile.
  See [editor services](docs/editor-services.md) and
  [visualization](docs/visualization.md).
- Try mounted read-only libraries with the opt-in `components` feature. The
  hosted prototype accepts self-contained textual KPAR archives. Mounted IDE
  sessions currently support inspection and navigation. See the
  [hosted KPAR profile](docs/hosted-kpar-prototype.md) and
  [archive format](docs/kpar-format-reference.md).
- Review measured language coverage before relying on a particular behavior.
  See the [KerML observations](docs/native-pilot-kerml-observations.md),
  [SysML observations](docs/native-pilot-sysml-observations.md), and
  [expression evaluator guide](crates/intersect-kerml-eval/README.md).

### Build the WebAssembly host

Build the host from this checkout.

```sh
cargo build --locked --release -p intersect-kerml-web --target wasm32-unknown-unknown
```

The built module is `target/wasm32-unknown-unknown/release/intersect_kerml_web.wasm`.
In the Examples repository's `toolkit/` directory, set `INTERSECT_WEB_WASM` to its
absolute path and run the full workbench build. The Toolkit keeps its own pins
for fixtures and native oracles. See the [Toolkit build guide](https://github.com/mercurio-labs/intersect-lang-examples/tree/develop/toolkit#full-workbench-development).

## Develop this repository

Use `develop` for new work. Keep the Model, Text, and build-support dependency
sources aligned across the workspace, Python SDK, and maintained standalone
consumers. Check `cargo metadata --locked` and `cargo tree --locked -d` after
updating pins. For a coordinated change, update Model first, then Text, then this
repository. The [editor services guide](docs/editor-services.md) explains why
development needs source pins. The [release receipt](docs/released-upstreams.json)
records the earlier registry pairing.

### Test tiers

Run suites through [`tools/test_report.py`](tools/test_report.py). Reports are
available at [localhost:8765](http://localhost:8765/).

- **Quick tier** covers the crates you changed, with a target of ten minutes
  including compilation. The command below tests the IDE facade and CLI.

  ```sh
  python -B tools/test_report.py run -- --profile test-fast --locked \
    -p intersect-kerml-ide -p intersect-cli --lib --bins --tests
  ```

- **Milestone tier** includes full-library loads and language-pack checks. See
  the [visualization](docs/visualization.md) and [CLI guides](docs/cli-tutorial.md)
  for the matching prepared-pack prerequisites.

  ```sh
  python -B tools/test_report.py run -- --profile test-fast --workspace --locked --no-fail-fast \
    --features intersect-kerml/milestone-tests,intersect-kerml-eval/milestone-tests,intersect-kerml-web/milestone-tests,intersect-cli/milestone-tests,intersect-kerml-ide/milestone-tests
  ```

### Formatting, lint, and WASM

```sh
python -B tools/format_workspace.py --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --lib --target wasm32-unknown-unknown --locked
```

See the [CI workflow](.github/workflows/ci.yml) for the opt-in component lane.
The [repository migration receipt](docs/repository-reset.json) records the earlier
source history. [Standalone probe results](docs/repository-split-consumer.json)
describe separate historical checks.

## License

MIT. See [LICENSE](LICENSE). Pinned upstream inputs under `fixtures/` retain
their own license notices.
