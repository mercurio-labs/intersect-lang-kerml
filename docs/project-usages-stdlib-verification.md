# Standard-library package verification

Verified 2026-11-11 at core `92aef03c6b49563be3ffd6d21fc49654fc49f447`
and language implementation `459d5bdf5faffc82ac871297d1f414aedcdbf11d`.
The [machine-readable evidence](project-usages-stdlib-verification.json) records
every executed input's SHA-256, the library identity, receipts and timings.

## Checks

| Scope | Result |
| --- | --- |
| Core formatting / strict Clippy / WASM | Passed; Clippy includes `text-compiler,xmi` and all targets |
| Core quick tier | 1,492 passed, 21 ignored; receipt `20261111-100528`, 103.7 s build + 61.9 s tests |
| Language/bridge/CLI formatting / strict Clippy | Passed; all 31 workspace Rust targets formatted, changed crates linted with all targets |
| Language/bridge WASM libraries | Passed |
| Language/bridge/CLI quick tier | 74 passed, 30 ignored; receipt `20261111-101430`, 228.1 s build + 7.3 s tests |
| Fixed-set equivalence | 8 files, 4 folders, all compared successfully; receipt `20261111-105204`, 20.9 s tests |
| Full-corpus equivalence | 157 files, 37 folders, all compared successfully and clean; receipt `20261111-105328`, 662.2 s tests |

All suites ran through `python -B tools/test_report.py run -- ...`. Local CLI
fixtures used workspace-local `TEMP`/`TMP`: the sandbox's `#!sandbox` temp path
exposes a pre-existing legacy archive filename/URI-fragment issue. The new
compatibility test also found an empty schema-revision projection in package
assembly; the core fix and its regression assertion are included in the verified
revision above.

## Native equivalence evidence

The pack was built with `test-fast` and `intersect-kerml/milestone-tests`, matching
the suite's compiler feature context. Its library was parsed, linked, completed,
validated, tagged and saved through `prepare_library`: 109,826 objects and
18,939 of 18,939 references resolved. The fixed and corpus loads both restored
that library from its snapshot. The core pin, library identity and input hashes
are retained in the JSON evidence.

The fixed set contains the six D9 Pilot files plus the checked-in valid and
invalid user files. Its expected negative case retained the same unresolved
reference diagnostic: 329 references, 328 resolved overall; three clean units
and one unit with the intended problem.

The full corpus was run **once**, after the fixed set passed. All 157 local
example files were verified against immutable Git blobs at Pilot
`692170b71867353b8f90341e61556f49a5beb0e5`, accounting explicitly for CRLF checkout
materialization. The actual executed input hashes were then checked against
that verified file set. All 37 units were clean, with 8,581 of 8,581 references
resolved and zero equivalence mismatches.

Each unit executes the new lock/admission path and the pre-slice
`FrozenStore::from_model` plus language-load body. Object data, other resource
metadata, reference counts and located diagnostics must agree exactly. Only the
intentionally added `resource.package` field is normalized. The new path also
checks the implicit direct lock entry and every library/project resource's tag.
This establishes native path equivalence on the pinned inputs. Existing language
qualification remains the governing semantic baseline.

Both real-library controls are modules of the existing suite and milestone-only:
they load the full library and the fixed set itself exceeds the ten-second
quick-test limit. Portable identity, catalog, provenance and archive-bridge
controls remain in the quick tier. Commands and required input variables are in
[the archive plan](kpar-plan.md#slice-3-verification-commands).

## External and deferred items

GitHub did not start the new PR workflows. Core run `43045554229` and language
run `43051392230` have zero executed job steps; their annotations cite failed
account payments or an insufficient spending limit. Remote CI needs a rerun
after that account condition is resolved.

Core PR 5 and language PR 4 merged on 2026-11-11. The follow-up pins every
manifest and lockfile to core develop merge commit
`38176bb0d917a3933c7a096c9ad73b4909142d8b`. Its Git tree is identical to the
verified feature commit `92aef03c6b49563be3ffd6d21fc49654fc49f447`; the evidence
above retains the exact revision used for the original runs. Published OMG
source profiles need their own supported parser/profile evidence. Mounting,
visibility, shared storage, compiled payloads with external package references,
selective loading, API/local-folder acquisition and legacy archive resolver
migration remain deferred.

The merged-pin follow-up passed formatting (31 targets), strict all-target
Clippy for `intersect-kerml`, `intersect-kpar-host` and `intersect-cli`, and WASM
library builds for `intersect-kerml` and `intersect-kpar-host`. Its quick-tier
receipt `20261111-111636` records 74 passed, 30 ignored and no failures or
crashes: 199.5 s build plus 9.2 s tests, with no new failures against
`20261111-101430`. Locked Cargo metadata also validated the workspace and all
five standalone manifests. All 26 core pin lines match the merged revision.
The original corpus run was not repeated because both core revisions have
the identical tree `e9a350ebcc8deb76a4ba72c73d80da02b42a48c5`.
