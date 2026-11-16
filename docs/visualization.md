# Bounded IDE and visualization provider

`intersect-kerml-ide` is the UI-independent host formerly prototyped in Mercurio.
Native and WASM hosts use the same implementation. It owns pack initialization,
project batch analysis, offline KPAR input resolution, named containment, stored
property inspection, LSP document synchronization and lexical completion, and a
bounded SysML visualization/source-command provider. It owns no filesystem,
threads, clocks, renderer or document persistence.

`Host::view_catalog`, `view_scene`, `prepare_view_command` and
`commit_view_command` expose the same implementations as JSON dispatch methods
`views/catalog`, `views/scene`, `views/prepareCommand`, `views/commitCommand`.
DTOs come from core `intersect-views`; `Source` is re-exported so applications
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
rendering declarations are retained; arbitrary rendering functions are not run.

This is not full SysML view evaluation. Filters, unsupported exposure forms,
ambiguous identity paths, unsupported endpoint chains and creation owners are
explicit errors or scene problems. Unknown palette commands have disabled
capabilities. Completion provides lexical keywords/snippets only.

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

Pack binding identity follows the existing sysml_pack recipe. Unifying this with
the CLI's independently selected host key remains follow-up. KPAR input resolution
is preserved; migration to newer catalog/project-host APIs is separate work.

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
