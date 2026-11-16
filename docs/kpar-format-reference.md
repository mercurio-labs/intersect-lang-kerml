# KerML project archives: format reference

Task K0.1 of the [kpar plan](kpar-plan.md). What `intersect-kpar` implements,
and where each fact comes from. Read 2026-11-10.

## Sources

| Source | What it settles |
| --- | --- |
| OMG, `KerML-Model-Interchange.json` (ptc/25-04-20), https://www.omg.org/spec/KerML/20250201/KerML-Model-Interchange.json | Members, types, formats and required members of `.project.json` and `.meta.json` (§2) |
| OMG, KerML machine-readable files, https://www.omg.org/spec/KerML/machine-readable | The normative library archives are published as `.kpar` under `https://www.omg.org/spec/KerML/20250201/` |
| The ten OMG library archives themselves (three KerML, seven SysML), downloaded 2026-11-10 (URLs and SHA-256 in `fixtures/kpar/omg-20250201.json`) | Container layout (§1), metadata as published, mutual usage |
| Sensmetry `sysand` (open-source Rust package manager for KerML/SysML projects), `core/src/model.rs` and `core/src/stdlib_assets/20250201/*` at its default branch, read through the GitHub API | The ten standard library archive IRIs, their `.project.json`/`.meta.json` contents (flat file layout, `version` 1.0.0 for KerML and 2.0.0 for SysML libraries), metamodel IRIs, checksum algorithm spellings, and its reading of `versionConstraint` as a Semantic Versioning requirement |

The specification text itself (the KerML 1.0 model interchange clause) was
not read for this reference. Facts below that come only from `sysand` are
marked **(sysand)**; they are one implementation's reading, not the standard.

## 1. Container

A ZIP archive with one `.project.json` and a `.meta.json` beside it, either at
the archive root or in a folder: the OMG archives keep everything in one
folder (`Kernel Semantic Library/.project.json`, `.../Base.kerml`, with a
directory entry for the folder), stored with DEFLATE and data descriptors.
Index and checksum paths are relative to that project folder
(`"Base": "Base.kerml"`). `sysand` likewise locates the project by its
`.project.json`. The OMG archives carry no `checksum`.

Usage may be mutual: the Semantic Library uses the Function Library and
the Function Library uses the Semantic Library.

The SysML archives use bare folders (`Systems Library/`, `Analysis/`,
`Quantities and Units/`). The ten archives hold the same 94 files as the
Pilot's `sysml.library`.

Defects in the published archives (also in `fixtures/kpar/omg-20250201.json`),
and how `intersect-kpar` treats them:

- two index entries name files the archives do not hold (Systems Library:
  `AnalysisCases` → `AnalysisCase.sysml`; Requirement Derivation:
  `RequirementsDerivation` → `RequirementsDerivation.sysml`): a dangling
  index entry is a warning, not a refusal;
- the Analysis archive uses
  `.../KerML/20250201/Quantities-and-Units-Library.kpar`, while Geometry uses
  `.../SysML/20250201/Quantities-and-Units-Library.kpar`, names under which
  nothing is published (the archive is `Quantities-and-Units-Domain-Library.kpar`):
  recognized as an alias of the standard library;
- the Systems Library archive holds macOS metadata (`.DS_Store`,
  `__MACOSX/`): noted, not loaded.

## 2. Metadata (OMG schema)

`.project.json` (`Project`): `name` and `version` required, strings;
`description`, `license` strings; `maintainer`, `topic` arrays of strings;
`website` string, format `iri`; `usage` array of objects with `resource`
(required, format `iri`) and `versionConstraint` (string).

`.meta.json` (`Meta`): `index` (required) object of string values;
`created` (required) string, format `date-time`; `metamodel` string, format
`iri`; `includesDerived`, `includesImplied` booleans; `checksum` object whose
values are objects with required `value` and `algorithm` strings.

The schema has no `additionalProperties: false`: other members are allowed.
`intersect-kpar` keeps them and writes them back. (`sysand` adds, for
example, `publisher` and other `usage` forms.)

## 3. Values

- **metamodel**: `https://www.omg.org/spec/KerML/20250201`,
  `https://www.omg.org/spec/SysML/20250201` **(sysand)**. `intersect-kpar`
  accepts any IRI under `https://www.omg.org/spec/KerML/` or
  `.../SysML/` and refuses others.
- **checksum algorithms**: `sysand` spells `SHA1`, `SHA224`, `SHA256`,
  `SHA-384`, `SHA3-256`, ..., `MD5`, `ADLER32`, and a non-standard `NONE`
  **(sysand)**. `intersect-kpar` verifies SHA224/256/384/512 (with or
  without the hyphen), notes `NONE` as unverified, and refuses the others
  (plan decision D4).
- **versionConstraint**: read as a Semantic Versioning requirement in
  Cargo's syntax (`1.0.0` means `^1.0.0`) **(sysand)**.
- **standard library IRIs**: `https://www.omg.org/spec/KerML/20250201/`
  `Semantic-Library.kpar`, `Data-Type-Library.kpar`, `Function-Library.kpar`;
  `https://www.omg.org/spec/SysML/20250201/` `Systems-Library.kpar` and
  `Analysis-`, `Cause-and-Effect-`, `Geometry-`, `Metadata-`,
  `Quantities-and-Units-`, `Requirement-Derivation-Domain-Library.kpar`
  **(sysand)**. They match the ten folders of the Pilot's `sysml.library`
  (`Kernel Libraries/Kernel Semantic Library`, ...).

## 4. Not settled here

- Whether the specification constrains how `index`, `checksum` and the
  archive's files relate (sysand: it does not). `intersect-kpar` requires
  every index and checksum path to be in the archive, and `intersect check`
  requires each index name to be a top-level member of its file.
- The pinned Pilot (`692170b7`) library is **not** the 20250201 release: 27
  of the 94 files differ (8 of the 36 KerML files) from the OMG archives. The
  KerML files that differ are
  (`Collections`, `VectorFunctions`, `Transfers`, `Observation`, `Objects`,
  `FeatureReferencingPerformances`, `TransitionPerformances`,
  `Occurrences`. Loaded with the pinned Pilot's languages and semantics
  (from the archives or, identically, from a folder), the 20250201 library
  leaves 23 of 18,584 references unresolved (46 problems, the first
  `TradeStudies.sysml`: `no element named 'fn'`, where the pinned text says
  `eval`), so it cannot be a prepared library for this pack. Constraints on standard library usages are therefore
  recorded, not checked, and written archives declare standard library
  usages without a `versionConstraint` and no `metamodel`.
- Pilot support for `.kpar` (task K0.3): not determined.
