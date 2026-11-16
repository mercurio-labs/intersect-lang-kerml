# KerML project archives (`.kpar`): implementation plan

Status: **K0.1, K0.2, K1, K2.1–K2.3, K2.5, K3 and K4.1–K4.2 implemented**,
2026-11-10; K0.3, K2.4, K4.3 and K5 open. What was built, the amendments
to this plan and the open items are in [§10](#10-implementation-record).
Written for an autonomous coding agent to execute task by task. Choices that
belong to the user are listed under [Open decisions](#8-open-decisions-for-the-user);
each has a default the plan uses until the user decides.

## Standard library as a model package (slice 3)

The prepared `library/sysml` model is one package, with one immutable identity.
`standard_library::StdlibCatalog` serves the exact library IRIs below as aliases
of that identity. This is option (a): it preserves the existing combined snapshot
and its cost of loading all library records. It does not split the library into
parts, scope names by package or share records between views. Dependencies among
library parts are internal to this prepared payload, so its own dependency lock
is empty. Core still has one lock entry per requested URI and requires just one
payload for equal identities.

### Verified addresses and versions

The normative machine-readable listings are [KerML 1.0](https://www.omg.org/spec/KerML/1.0)
and [SysML 2.0](https://www.omg.org/spec/SysML/2.0). Direct inspection of all ten
published archives, with SHA-256 matching `fixtures/kpar/omg-20250201.json`, finds
project versions **1.0.0** for the three KerML libraries and **2.0.0** for the seven
SysML libraries. These are archive project versions, separate from the dated
IRI component and the language metamodel version.

The actual source pin in this repository is Pilot
`692170b71867353b8f90341e61556f49a5beb0e5`. Its [Kernel Semantic project](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/sysml.library/Kernel%20Libraries/Kernel%20Semantic%20Library/.project.json)
declares **1.1.0-dev.20260801**; its [Systems project](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/sysml.library/Systems%20Library/.project.json)
declares **2.1.0-dev.20260801**. All ten pinned project files were checked; their
individual source links and versions are recorded in
[`fixtures/stdlib/pilot-project-versions.json`](../fixtures/stdlib/pilot-project-versions.json).
The same OMG IRIs appear in their usages. The adapter advertises these pinned
Pilot versions only when all source digests, byte lengths and parse languages
match the embedded corpus manifest. Logical URI prefixes may differ; the exact
URIs are still hashed into package content identity.

| Prefix | Archive names | Pinned source version |
| --- | --- | --- |
| `https://www.omg.org/spec/KerML/20250201/` | `Semantic-Library.kpar`, `Data-Type-Library.kpar`, `Function-Library.kpar` | `1.1.0-dev.20260801` |
| `https://www.omg.org/spec/SysML/20250201/` | `Systems-Library.kpar`, `Analysis-Domain-Library.kpar`, `Cause-and-Effect-Domain-Library.kpar`, `Geometry-Domain-Library.kpar`, `Metadata-Domain-Library.kpar`, `Quantities-and-Units-Domain-Library.kpar`, `Requirement-Derivation-Domain-Library.kpar` | `2.1.0-dev.20260801` |

The published Analysis archive mistakenly uses
`https://www.omg.org/spec/KerML/20250201/Quantities-and-Units-Library.kpar`;
the Geometry archive uses
`https://www.omg.org/spec/SysML/20250201/Quantities-and-Units-Library.kpar`.
Both spellings are explicit aliases of the Quantities and Units part (the SysML
version policy), not evidence that arbitrary releases or language prefixes work.
Other dated IRIs are unsatisfied.

Matching uses the existing Cargo-style SemVer policy, including prerelease
exclusion. A requirement for published `1.0.0`/`2.0.0` does **not** silently match
the pinned prerelease. Supporting those published source profiles or another
Pilot pin needs explicit profile evidence. Custom or partial source sets remain
available for unconstrained usages with version `unversioned`; a constrained
usage cannot claim a release for them. Malformed constraints are `Constraint`
errors before lookup.

### Identity and cache validity

`library_package_identity` partitions every prior library-key input:

- content is the canonical `LibraryManifest::digest` (logical URI, parse
  language, SHA-256 and byte length for every source);
- engine hashes both program part keys, bindings identity, program compiler
  components, recipe, library part name/version, payload formats and domain tags;
- schema is the exact `Metamodel::digest` in hexadecimal;
- dependency lock is `DependencyLock::empty().digest()`.

`library_key` is now the SHA-256 of canonical JSON of this complete identity.
There is one derivation, so package identity and cache validity cannot protect
different inputs. The library part version is bumped to 2; old cache keys
rebuild. Unit controls change every former key input separately. Catalog
construction also rejects a `PreparedLibrary` whose manifest, tags, empty lock
or cache key disagrees with its identity.

Both snapshot and source fallback preparation tag every resource, including
empty resources, with this identity. Object identities, values, order and
construction evidence retain the same representation. `StdlibCatalog` provenance
names `urn:intersect:prepared-library` and the manifest digest; this identifies
the logical prepared content, not a physical pack-file acquisition digest.

### Host path and compatibility

`SysmlHost::load_project` resolves explicit `ProjectDependencies` plus an
implicit unconstrained Systems Library root, derives `ResourceContext` from that
lock, and calls core's `load_against_lock`. The existing language parser,
linker, completions and validation then run on that construction store. Resource
tags are checked again before returning `ProjectLoad { load, lock, context }`.
The CLI uses this entry point. User resources carry no package identity.

`load_on_library(&Model, ...)` remains a compatibility facade over the same
path. Tagged models retain their identity. A legacy untagged model has no source
manifest: its exact snapshot bytes supply content identity, with current engine
and schema inputs and an empty lock. It is tagged and admitted by the same
entry point, and cannot claim a release version. This compatibility identity is
separate from a source-manifest identity; callers wanting the latter use
`prepare_library` and `load_project`.

`CombinedCatalog` composes catalogs with the bridge's compatible SemVer policy.
An archive can resolve its own manifest against it using
`SourcePackage::manifest`, then publish that manifest with
`KparCatalog::register_manifest`. The independently ZIP-written
`stdlib-usage.kpar` fixture exercises this flow. This does not load archive
models. The CLI's legacy archive usage resolution, filename matching and cycle
handling remain unchanged; only its prepared-library load uses the new path.

Mounting, visibility/scoping, shared storage, compiled packages with external
references, API/local-folder acquisition and selective loading remain open.

## 1. Goal

### Project-usage resolver bridge

`intersect-kpar-host` adds `KparSource` and `KparCatalog` over the existing
archive reader. The shared `intersect-project-host::PackageSource` contract
returns a source package with separate logical and acquisition addresses,
authored usages, source digest, and a payload. The archive payload retains the
checked `ProjectArchive`, its warnings and notes, and the exact original bytes.
The source and provenance digests are SHA-256 of those bytes.

Register dependencies before their consumers, supply exact engine and schema
identities, and complete the catalog before resolving a project's usages.
Registration calls the shared core resolver and records each candidate's full
own dependency lock as well as its digest. Transitive requests use the exact
identities in that recorded lock, so adding catalog versions preserves existing
consumers. Candidate lookup matches the complete logical IRI
and applies the existing Cargo-style SemVer requirements. In particular,
`1.0.0` is a caret requirement and `=1.0.0` is exact. The host selects the
logical IRI explicitly. The new resolver reports package dependency cycles
as unsupported under its current contract. Root constraint syntax is validated
before candidate lookup and produces a dedicated constraint error.

The core resolver distinguishes missing root candidates, ambiguous root
requests, incompatible identities, and recorded-lock mismatches. Its default
10,000-path budget covers the shared root walk and the recorded package paths
validated against it. Callers can configure the limit through
`ResolutionLimits`. Projection verification reuses the shared walk.
Re-zipping identical model files changes the archive bytes and its content
identity.

The bridge delegates archive decoding, metadata handling, checksums, limits,
wrapper-directory compatibility, and archive-qualified source identities to
`intersect-kpar`. Its fixture suite includes the existing Python-produced ZIP
fixture and §10.3 metadata, plus a catalog diamond and conflicting candidates.
Its qualification covers acquisition and manifest/lock resolution. Model
compilation, mounting, visibility, and scoping require later work.

The next slice should retire the separate offline resolver in
`intersect-kpar` and the CLI through compatibility facades over the core
resolver. It must preserve filename matching, prepared-library aliases,
archive-qualified source identities, and diagnostics. The current legacy
support for mutual archive usages needs an explicit compatibility policy
before migration to the core resolver's current unsupported-cycle contract.

Let Intersect consume and produce KerML project interchange archives
(`.kpar`, KerML 1.0 model interchange clause) natively, with no Java, in the
CLI and the browser host:

- `intersect check|export|eval model.kpar` loads the archive's models as user
  files, on top of the pack's prepared standard library;
- `--library sysml.library.kpar` accepts the standard library as an archive;
- archive dependencies (`usage`) resolve against the pack's library and an
  explicit list of local archives, never the network;
- `intersect export --format kpar` writes a deterministic archive that
  Intersect, and other conforming tools, read back to the same model.

### User-facing outcome

```text
$ intersect check --pack sysml.pack vehicles.kpar
vehicles.kpar!/Vehicles/Car.sysml:12:5: error[link]: typedBy: no element named 'Engin'
3 file(s) checked: 1 problem(s)
$ intersect check --pack sysml.pack --project-path deps/ vehicles.kpar
$ intersect export --pack sysml.pack --format kpar --out out vehicles.sysml parts.sysml
```

### Accomplishments tracked separately (AGENTS.md)

| Accomplishment | Meaning here |
| --- | --- |
| Imported | The archive is read, validated against its metadata and its sources handed to the load, with source identities preserved |
| Consumed | The loaded models check, export and evaluate exactly as the same files loaded loose |
| Semantically verified | Agreement with an independent reader/writer (§6) on real archives |

A successful unzip is not support. Every content kind the archive may hold
that this plan does not load is refused explicitly (§3.4).

## 2. Background

### 2.1 The format (to be confirmed in task K0.1)

Summarized from memory of the KerML 1.0 specification; K0.1 checks every
field against the specification text before any code depends on it.

A `.kpar` file is a ZIP archive holding:

- `.project.json`, an *interchange project*: `name` (required),
  `description`, `version`, `license`, `maintainer`, `website`, `topic`, and
  `usage`, a list of `{ resource: IRI, versionConstraint }` naming the
  projects it depends on;
- `.meta.json`, *interchange project metadata*: `index` (top-level member
  name → archive path), `created` (timestamp), `metamodel` (IRI),
  `includesDerived`, `includesImplied`, and `checksum` (path →
  `{ value, algorithm }`);
- model files, normally KerML/SysML textual notation (`.kerml`, `.sysml`).
  The specification also permits other serializations.

### 2.2 Current state of this repository

- No archive support. No Rust ZIP dependency in either workspace; the core
  Toolkit unzips XMI ZIPs in JavaScript (fflate).
- `intersect-cli` (`crates/intersect-cli/src/host.rs`) reads user files with
  `read_user_files` (only `.kerml`/`.sysml` paths; URI = the path with `/`
  separators and `%20` for spaces) and library sources with
  `library_sources(dir)` (a folder walk; URIs relative to the folder's parent,
  e.g. `sysml.library/Kernel%20Libraries/Base.kerml`). The pack's library
  snapshot is keyed on those sources.
- Core's `intersect_model::core::interchange` passes codecs bytes
  (`Resource { uri, bytes }`) and never touches the filesystem. Archive
  decoding produces the same shape.
- The pinned Pilot corpus (`fixtures/native-pilot/v1`, Pilot commit
  `692170b7`) contains grammars, the metamodel, the library and examples, but
  no archive-handling Java sources and no `.project.json`/`.meta.json` files.

## 3. Design

### 3.1 Placement and dependency direction

| Piece | Where | Why |
| --- | --- | --- |
| Bounded ZIP reader/writer: bytes ⇄ ordered `(path, bytes)` entries | New module `intersect-kerml/src/archive.rs` (default; see D1) | Language-neutral, but keeping it here avoids a core change and a rev bump; promote to core when a second user appears |
| KPAR model: `.project.json`/`.meta.json` types, validation, dependency resolution, URI mapping | New module `intersect-kerml/src/project_archive.rs` | Language packages own language-specific interpretation |
| CLI options and file handling | `intersect-cli/src/host.rs` | Host concern: filesystem, paths |
| Browser method | `intersect-kerml-web` | Decode in Rust so CLI and browser share one implementation |

Neither module touches the filesystem: they take and return bytes, so they
build for `wasm32-unknown-unknown` and stay deterministic.

Dependency: the `zip` crate with deflate only, through `miniz_oxide` (pure
Rust), `default-features = false`. K0.2 confirms the wasm build and licence;
if `zip` does not fit, use `miniz_oxide` directly with a handwritten
central-directory reader (a small, well-specified format). Mark any
handwritten ZIP code as such.

### 3.2 Reading

`project_archive::read(archive_uri: &str, bytes: &[u8], limits: &ArchiveLimits)
-> Result<ProjectArchive, ArchiveError>` where `ProjectArchive` holds the
parsed project, the metadata and the model sources in archive order sorted by
path.

Steps, each with its own `ArchiveError` variant and message:

1. Extract within limits (§3.5).
2. Require `.project.json` and `.meta.json` at the archive root; parse them
   with `serde`, refusing unknown required-field types; unknown optional
   fields are kept as raw JSON (preserve imported information) and reported
   once as notes.
3. Verify every `checksum` entry (supported algorithms: those named by the
   specification that `sha2` covers; others refused, see D4).
4. Check `metamodel` against the pack's language (KerML or SysML metamodel
   IRI). Absent: accept, as a note.
5. Check `index`: every path exists in the archive; every listed name is
   declared at top level by the file it points to (checked after load, as a
   `check`-phase problem, so the index is verified, not trusted).
6. Classify every other entry (§3.4).

### 3.3 Source identity

Each model source keeps its archive and entry path:
`<archive uri>!/<entry path>`, with the existing `resource_uri` escaping
(e.g. `vehicles.kpar!/Vehicles/Car.sysml`). Diagnostics, JSON output and XMI
export use this URI. Two inputs that map to the same URI are refused, as
`read_user_files` does today.

For a library archive the URIs must match what the pack's snapshot is keyed
on, or the snapshot is (correctly) re-prepared. Default (D3): when the
archive's model files sit under a single root folder, map them to
`<root folder>/<path>`, i.e. the same URIs as `--library <folder>`; an
archive of the pinned library then reuses the existing snapshot. Test this
explicitly.

### 3.4 Content the plan does not load

Refuse explicitly, naming the entry: model files in non-textual
serializations (JSON, XMI), `includesDerived: true` or `includesImplied: true`
(textual sources do not carry derived/implied content; see D5), nested
archives, and entries whose extension is not `.kerml`/`.sysml` and that are
not the two metadata files. Other files (README, licence) are listed as
ignored notes, not silently dropped.

### 3.5 Limits

`ArchiveLimits` with defaults: at most 10 000 entries, 512 MiB total
uncompressed, 64 MiB per entry, compression ratio at most 200:1, nesting depth
0. Reject absolute paths, `..` segments, backslashes, duplicate entry names
(including case-only duplicates), encrypted entries, and compression methods
other than stored and deflate. CLI flags may raise the size limits; nothing
disables the path checks.

### 3.6 Dependencies (`usage`)

Resolution is explicit and offline:

1. A `usage` whose `resource` names the standard library (the IRIs to match
   are taken from the real library archives in K0.1) resolves to the pack's
   prepared library; a `versionConstraint` the pack's library does not
   satisfy is an error.
2. Any other `usage` resolves against archives found in `--project-path DIR`
   (repeatable; the archives' `.project.json` names and versions, not file
   names). Resolution is transitive, cycle-checked, and picks exactly one
   version per project; ambiguity is an error.
3. Unresolved dependencies are errors, listed together.

Resolved dependency archives load as user files before the root archive,
with their own source URIs. Version-constraint syntax is whatever K0.1 finds
the specification defines; implement only that, and refuse anything else.

### 3.7 Writing

`project_archive::write(project: &ProjectInfo, sources: &[Source]) ->
Result<Vec<u8>, ArchiveError>`:

- entries sorted by path, fixed DOS timestamp (1980-01-01), deflate at a
  fixed level, no extra fields, so equal inputs give equal bytes;
- `.meta.json` with `index` computed from the loaded model's top-level
  members, `created` supplied by the host (the CLI's clock, behind the host
  adapter; tests pass a fixed value), `metamodel` from the pack, `includesDerived`
  and `includesImplied` false, and SHA-256 checksums;
- `.project.json` from CLI options (`--project-name`, `--project-version`);
  `usage` includes the standard library when any source references it.

Export writes the user's source text unchanged; it does not regenerate text
from the model (source spelling and comments are preserved by construction).

## 4. Milestones and tasks

Each task lands with focused tests in the crate's suite binary (`tests/all.rs`
modules or unit tests), quick tier only unless stated.

### K0. Ground truth

- **K0.1** Read the KerML 1.0 model interchange clause and record the exact
  `.project.json`/`.meta.json` schema, version-constraint syntax, checksum
  algorithms and permitted content in `docs/kpar-format-reference.md`, with
  clause references. Obtain real archives: the OMG/Pilot-published standard
  library archives and at least one other tool's export; pin them under
  `fixtures/kpar/` with a manifest (source URL, sha256, licence), in the style
  of `fixtures/native-pilot/v1/manifest.json`.
- **K0.2** Confirm the ZIP dependency builds for `wasm32-unknown-unknown` and
  record the choice in this plan (§3.1).
- **K0.3** Determine whether a Pilot release reads/writes `.kpar` (its
  library project files and any publishing command). Record the result: it
  decides the oracle in §6.

Gate: the reference doc exists; every later task cites it, not §2.1.

### K1. Read (imported)

- **K1.1** `archive.rs`: bounded extraction (§3.5). Tests: stored and
  deflated entries, every refused path form, zip bomb by ratio and by total
  size, duplicate and case-duplicate names, truncated and corrupt archives,
  encrypted entry.
- **K1.2** `project_archive.rs`: metadata types, parse, checksum, metamodel
  and classification (§3.2, §3.4). Tests: each `ArchiveError` variant; unknown
  optional fields preserved; every pinned K0.1 archive reads.
- **K1.3** URI mapping (§3.3). Tests: user-archive URIs; library-archive URIs
  equal the folder-walk URIs for the pinned library contents.

Gate: all pinned archives read; all negative tests fail with their own error.

### K2. CLI consumption (consumed)

- **K2.1** `check|export|eval` accept `.kpar` inputs alongside loose files.
  `read_user_files` dispatches on extension; archive sources carry their
  archive URI into diagnostics.
- **K2.2** Index verification as a `check`-phase problem (§3.2 step 5).
- **K2.3** `--library` accepts an archive (§3.3). Test: an archive built from
  the pinned library reuses the snapshot key of `--library <folder>`
  (milestone-only if it needs a prepared pack).
- **K2.4** Equivalence: for each pinned archive and for archives built from a
  few Pilot examples, `check --json` and `export --format json` produce the
  same problems and model (modulo resource URIs) as the extracted files loaded
  loose. Needs a pack: milestone-only.
- **K2.5** Update `docs/cli-tutorial.md` and the CLI usage text.

### K3. Dependencies

- **K3.1** `usage` resolution (§3.6) with `--project-path`. Tests:
  standard-library usage, satisfied and unsatisfied constraints, a two-level
  chain, a cycle, two versions of one project, a missing dependency.
- **K3.2** CLI: dependency archives load before the root archive; diagnostics
  in dependencies are reported with their URIs.

### K4. Write

- **K4.1** `project_archive::write` (§3.7). Tests: byte-identical output for
  equal input; read(write(x)) gives back the sources and metadata; checksums
  verify; `created` comes only from the host argument.
- **K4.2** `intersect export --format kpar`. Round-trip test: export → check
  gives the same problems as the loose files.
- **K4.3** Cross-tool check (§6).

### K5. Browser

- **K5.1** `intersect-kerml-web` method `draft/kerml/loadProjectArchive`
  taking base64 archive bytes, returning the same result as loading the
  files; and an export method returning archive bytes. Native/WASM equality
  test on one archive.
- **K5.2** Toolkit (core repository): an open/save control for `.kpar`. Only
  if the user asks (D6).

## 5. Error and determinism rules

- No `unwrap`/`expect` in library code; every refusal is an `ArchiveError`
  variant with the archive URI and entry path in its message.
- Archive reading and writing are pure functions of their inputs; the clock
  (`created`) and the filesystem stay in the host.
- The same archive bytes always produce the same sources in the same order.

## 6. Verification

- **Independent oracle.** If K0.3 finds Pilot archive support, compare: the
  Pilot reads Intersect-written archives and Intersect reads Pilot-written
  ones, through a Java harness that is test-only (as for the expression
  evaluator). If not, use the archives of at least one other published tool
  (K0.1) in both directions, and record that no Pilot comparison exists.
- **Specification examples.** Any example archives or JSON in the
  specification become fixtures.
- **Claims.** The final status lists, per §1 accomplishment, exactly which
  archives and which content kinds are covered. Refused content (§3.4) is
  listed as unsupported, not as conforming.

## 7. Test tiers and checks

Quick tier after each task (README "Test tiers"), for the crates changed:

```sh
python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml -p intersect-cli --lib --bins --tests
```

Tests needing a prepared pack (K2.3, K2.4, K4.3) are milestone-only. Also run
formatting, Clippy and the `wasm32-unknown-unknown` build of
`intersect-kerml-web` as README documents.

## 8. Open decisions for the user

| # | Decision | Default used by this plan |
| --- | --- | --- |
| D1 | Where the ZIP layer lives | In `intersect-kerml`; promote to core when another user appears |
| D2 | Priority: consume or produce | Consume first (K1–K3), then write (K4) |
| D3 | Library-archive URIs | Map to the folder-walk URIs so the existing snapshot is reused |
| D4 | Checksum algorithms beyond SHA-256 | Refuse explicitly until a real archive needs one |
| D5 | Archives with `includesDerived`/`includesImplied` true | Refuse (textual sources do not carry them) |
| D6 | Toolkit UI for archives | Not in scope until asked; K5.1 provides the method |
| D7 | JSON/XMI model content inside archives | Out of scope; refused explicitly |

## 9. Out of scope

Network fetching of dependencies; repository or API (SysML v2 REST API)
integration; JSON or XMI model serializations inside archives; editing
archives in place; any claim of general KerML interchange conformance.

## 10. Implementation record

### What exists

- **`crates/intersect-kpar`** (new crate, no dependency on core or
  `intersect-kerml`; builds for `wasm32-unknown-unknown`):
  - `zip`: handwritten bounded reader (central directory, stored and
    deflate, data descriptors; refuses ZIP64, multi-disk, encryption,
    unsafe and case-duplicate paths, and archives over the §3.5 limits
    before inflating) and deterministic writer. DEFLATE is `miniz_oxide`,
    CRC-32 `crc32fast`.
  - `project`: `.project.json`/`.meta.json` per the OMG schema
    ([format reference](kpar-format-reference.md)), unknown members kept.
  - `archive`: `read` (§3.2, §3.4) and `write` (§3.7).
  - `resolve`: `usage` resolution (§3.6) and the standard library
    resource-to-IRI mapping used by export.
- **`intersect` CLI** (`crates/intersect-cli/src/host_kpar.rs`): `.kpar`
  inputs for `check`, `export` and `eval`; `--project-path`; `--library`
  with library archives; the index check; `export --format kpar`
  (`--project-name`, `--project-version`; `created` from
  `SOURCE_DATE_EPOCH` or the clock). Archive problems and notes print with
  phase `archive`.

### Amendments to this plan

- **Placement (§3.1, D1).** A separate crate instead of modules of
  `intersect-kerml`: `intersect-kerml`'s compiler identity hashes its
  `src/`, so modules there would have changed every pack's identity for
  code the pack does not contain. Adding dependencies changes `Cargo.lock`,
  which the identity also hashes, so packs built before this change must be
  rebuilt once anyway.
- **Library archive URIs (§3.3, D3).** The OMG library archives hold their
  files flat, without the `sysml.library/...` folders, so their files cannot
  map to the folder-walk URIs. They load as `<archive>!/<file>`, and the
  pack's library snapshot is prepared once for them (a different key from
  `--library <folder>`).
- **Matching usages to archives (§3.6).** `.project.json` gives a project
  no IRI of its own, so a non-library `usage` resolves to the one candidate
  archive whose file name is the IRI's last segment (for a URN, the part
  after the last `:`). Unreadable archives on the project path are warnings,
  and matter only when needed.
- **Project folder.** An archive's project is where its single
  `.project.json` is: the archive root or one folder (the OMG archives use
  `Kernel Semantic Library/`). Index and checksum paths are relative to it;
  resource URIs keep the full entry path; files outside it are noted, not
  loaded; several `.project.json` files are refused as ambiguous. Found on
  the real OMG archives: the first implementation required the root.
- **Dangling index entries** are warnings, not refusals, and the index
  check skips them: two of OMG's published index entries name files their
  archives do not hold.
- **Standard library aliases.** The published Analysis archive uses
  `.../KerML/20250201/Quantities-and-Units-Library.kpar`, while Geometry uses
  `.../SysML/20250201/Quantities-and-Units-Library.kpar`. Both refer to the
  Quantities and Units Domain Library (see the slice 3 policy above).
- **Mutual usage.** Cycles are allowed (the OMG Semantic and Function
  libraries use each other); each archive loads once, all in one pass. The
  first implementation refused them.
- **Written metadata (§3.7).** `index` comes from the loaded model's
  top-level names (first file in path order wins a duplicate name). `usage`
  keeps the input archives' usages and adds, without a `versionConstraint`,
  each OMG standard library archive the written files reference through a
  stored reference, plus the Semantic Library always and the Systems
  Library for SysML files (implicit specializations are not all stored
  references: a first end-to-end run left the Systems Library out).
  `metamodel` is not written: whether the pinned library is the 20250201
  release is not established.

### Verification so far

Quick tier: 32 `intersect-kpar` tests (including archives written by
Python's `zipfile`, an independent writer, in seekable and streamed form)
and the CLI's option, resolution and date tests (46 passed). Clippy,
rustfmt and the workspace `wasm32-unknown-unknown` build are clean.

End to end, 2026-11-10, with a pack built from this branch (release
`intersect`; inputs written by Python's `zipfile`, not by this crate):

- `--library` with ten library archives built from the pinned
  `sysml.library` folders (OMG names, flat layout, OMG-style index and
  SHA256 checksums): all 94 files prepared and saved as the pack's snapshot
  (2 min 56 s); an app archive using a `--project-path` dependency
  (`versionConstraint` `1.2` against `1.4.0`) and the Systems Library
  checked clean.
- Without `--project-path`: exit 2, "usage ... is not resolved". A wrong
  index entry: `error[archive]`, exit 1, also as JSON.
- `export --format kpar` twice with the same `SOURCE_DATE_EPOCH`: identical
  bytes; Python's `zipfile.testzip` passes; the archive checks clean.
- K2.4 (partial): six Pilot examples (four SysML, two KerML) loose versus
  exported as an archive: same problems (none). Three handwritten files with
  link errors plus `PartTest.sysml`, loose versus a Python-written archive:
  the same two problems at the same line and column, located as
  `Errs.kpar!/Errs.sysml:3:14`.

OMG archives (K0.1), 2026-11-10: all ten standard library archives
downloaded (the SysML seven later the same day, with the defects listed in
the format reference) (URLs and SHA-256 in `fixtures/kpar/omg-20250201.json`; not
redistributed). `archive_tests::omg_library_archives_read` reads them when
`INTERSECT_KPAR_OMG` names their folder (16, 3 and 17 files, every index
entry present). As `intersect check --library` (three archives): all 36
files prepared with every reference resolved, and `Expressions.kerml` and
`Types.kerml` check clean against them. 8 of the 36 files differ from the
pinned Pilot library ([format reference](kpar-format-reference.md) §4).

### Open

- **K0.3** Pilot `.kpar` support: not determined.
- **The 20250201 library on this pack**: all ten OMG archives read (test
  `omg_library_archives_read`, 94 files), but their text does not load
  completely with the pinned Pilot's languages: 23 of 18,584 references
  unresolved, identically from the archives and from a plain folder. Loading
  the current OMG library needs a newer Pilot pin, not archive work.
- **K2.4** as a repeatable milestone test over a wider corpus, and **K4.3**
  cross-tool checks: need the real archives and a second tool.
- `export` writes nothing when the files have errors (as for JSON and XMI).
- **K5** browser method: not started.

### Slice 3 verification commands

Portable quick tests cover the four identity components, cache-key agreement,
snapshot/fallback tagging, aliases/constraints, legacy model adaptation, and an
independently written archive using the combined catalog. The real fixed set
and full Pilot corpus are suite modules marked milestone-only: AGENTS.md puts
whole-library loads and equivalence in that tier, even when a prepared pack
makes startup fast. They do not pass vacuously when inputs are absent.

Prepare a pack with the same feature profile as the milestone suite:

```sh
cargo run --profile test-fast -p intersect-kerml --features milestone-tests --example sysml_pack -- build . target/stdlib-equivalence.pack --library
```

Set `INTERSECT_PACKAGE_EQUIVALENCE_PACK` to that pack,
`INTERSECT_PILOT_EXAMPLES` to the complete checkout/export of the pinned Pilot,
and `INTERSECT_PACKAGE_EQUIVALENCE_REPORTS` to a writable artifact directory.
Set `RUST_TEST_THREADS=1`. Run the fixed-set control first, then run the full
corpus control once after it passes:

```sh
python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml --test all --features milestone-tests project_package_equivalence::fixed_user_files_and_six_pilot_examples_match_the_pre_slice_load
python -B tools/test_report.py run -- --profile test-fast -p intersect-kerml --test all --features milestone-tests project_package_equivalence::full_pinned_pilot_corpus_matches_the_pre_slice_load
```

The fixed set is the six D9 Pilot files plus the checked-in valid/invalid user
files in `fixtures/project-usages/fixed`. The full control requires all 157
pinned examples. Both compare the new host path with the pre-slice
`FrozenStore::from_model` + language-load body. Only `resource.package` is
normalized; object data, other resource metadata, reference counts and located
diagnostics must match. The candidate must also have the expected implicit lock
entry, every library resource tagged, and every project resource untagged.
Reports carry exact input SHA-256 values, the prepared-library identity and the
number of folders actually compared. The full corpus is run once at the end.

The completed slice 3 run and its input identities are recorded in
[standard-library package verification](project-usages-stdlib-verification.md).
