# Bounded IDE and visualization provider

`intersect-kerml-ide` is the UI-independent host formerly prototyped in Mercurio.
Native and WASM hosts use the same implementation. It owns pack initialization,
project batch analysis, offline KPAR input resolution, named containment, stored
property inspection, LSP document synchronization, source navigation and completion,
and a bounded SysML visualization/source-command provider. It owns no filesystem,
threads, clocks, renderer or document persistence.

`Host::view_catalog`, `view_scene`, `prepare_view_command` and
`commit_view_command` expose the same implementations as JSON dispatch methods
`views/catalog`, `views/scene`, `views/prepareCommand`, `views/commitCommand`.
DTOs come from the standalone [Intersect Views](https://github.com/mercurio-labs/intersect-views) contracts; `Source` is re-exported so applications
need no direct core dependency. `views/locate` maps source selections to bindings.

`load_pack_for_analysis` skips the optional library presentation tree while
retaining the same prepared model. `workspace/analyze` accepts `projectNodesOnly`
to restrict its named-element presentation projection to project resources.
Neither option changes parsing, linking or library visibility. Prepared command
results preserve this projection option. Default workbench behavior is unchanged.

## Supported profile

The real compiled fixture uses native SysML view definitions/usages, viewpoint
definitions/usages, standard GeneralView/InterconnectionView inheritance,
explicit exposes and standard rendering names. General scenes project
definitions, usages, stored typing and specialization. Interconnection scenes
project concrete part contexts, inherited owned parts/ports and binary resolved
part/port connections. Viewpoint references are source-linked details. Standard
rendering declarations are retained. M14 also supports bounded read-only action
flow and state-transition projections. Arbitrary rendering functions are not run.

Enable `intersect-kerml-ide/components` to use the same catalog, scene,
navigation and checked source-transaction APIs on projects with mounted KPARs.
The accepted project supplies one checked read view for both project source and
mounted elements. Mounted source may be revealed. Source edits require a
writable, supported project owner and are validated before commit.

This is not full SysML view evaluation. Filters, unsupported exposure forms,
ambiguous identity paths, unsupported endpoint chains and creation owners are
explicit errors or scene problems. Unknown palette commands have disabled
capabilities. [Editor services](editor-services.md) describes the ordinary
project's contextual completion. The experimental mounted component session
currently supplies lexical completion and reference navigation.

Named declaration keys use resource URI and named ownership path. They survive
source offset changes; rename, move and duplicate paths need a future durable
identity contract. Occurrence keys include view and usage context. Unnamed
relationship bindings include source offsets and are snapshot-local; parallel
connections get distinct identities. Never persist model element IDs.

Creation supports simple named part/port declarations under supported writable
owners. Preparation anchors patches with the core lexer, validates a candidate
through the native loader and retains it privately. Only commit with its token,
base revision and exact complete editable-buffer receipt publishes the candidate
model/source epoch. Reanalysis invalidates pending commands. Clients own applying
patches through undo and recovering buffers on failure. Source undo followed by
analysis removes created elements.

Pack binding identity follows the existing sysml_pack recipe. See the
[hosted KPAR profile](hosted-kpar-prototype.md) for archive admission and the
[Rust API reference](../README.md#rust-api-reference) for the host entry points.

## Checks

Quick tests cover UTF-16 coordinates, descriptor failures and incomplete-buffer
editing. Compiled qualification is milestone-only because it loads the prepared
library and analyzes several model epochs:

```sh
python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml-ide --lib --test all
# Set INTERSECT_IDE_TEST_PACK to a trusted pack built with default language features.
python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml-ide --features milestone-tests --test all
cargo clippy -p intersect-kerml-ide --all-targets --locked -- -D warnings
cargo check -p intersect-kerml-ide --target wasm32-unknown-unknown --locked
```

The fixture checks both profiles, source/viewpoint bindings, typed/JSON
equivalence, patches, candidate isolation, exact receipts, stale revisions,
occurrence stability and source undo. Core separately qualifies wire structure
against shared Rust/TypeScript fixtures. Passing these checks qualifies this
bounded profile only, not arbitrary Cameo diagrams or OMG model publication.
