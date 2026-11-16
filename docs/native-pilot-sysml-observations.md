# Native Pilot SysML observations

The language-owned [observation specification](../crates/intersect-kerml/observations/pilot-sysml-v1.json)
records 22 selected observations and 53 phase-specific checks for the unchanged
Pilot PartTest, ConnectionTest, RequirementTest and ConstraintTest examples at
commit 692170b71867353b8f90341e61556f49a5beb0e5. These are source-grounded
expectations, with every native result pending. No Java or duplicate corpus
qualification was run.

Each example has its original URI, byte count, SHA-256 and unique UTF-8 anchor
ranges. Five real library target sources also have exact pins and declaration
anchors. A receipt must record actual object IDs and compare actual target
identities. Qualified labels in the specification do not invent UUIDs, duplicate
inherited objects or equate two declarations by leaf name.

| Observations | Selected graph facts |
| --- | --- |
| P01–P04 | Part/port ownership and protected visibility; derived/constant/ref flags through the actual Usage.isReference provider; A.z reaches genuine ScalarValues::Integer. |
| P05–P07 | Canonical timeslice enum identity; conjugated port typing tied to original C; authored p1/p2/p3 cycle and p4 self-subsetting links remain intact. |
| C01–C03 | Chained p1.x.x1 endpoint lookup, binding endpoints and succession endpoint order use their actual target identities. |
| C04–C05 | Four bus endpoint declarations coexist with the typed three-end C; AB.a retains end/multiplicity/type and its original @M metadata usage. |
| R01–R02 | Private recursive import of q and assumption membership with its canonical RequirementConstraintKind value. |
| R03–R05 | Negated satisfaction keeps ReferenceSubsetting and SubjectMembership/FeatureValue structure; framed concerns and nested subject ownership remain explicit. |
| K01–K02 | ISQ::MassValue reaches ISQBase::MassValue through ISQ's public import. SI::kg identifies SI::kilogram by its declared short name. Component masses retain their real quantity type and multiplicity. |
| K03–K05 | NumericalFunctions::sum has its actual ScalarValue collection parameter and ScalarValue result declaration; vehicle feature ownership and negated assertion structure remain separate from evaluation. |

The optional `assert` token in SatisfyRequirementUsage does not justify inventing
an `isAsserted` stored field. The negated form has the real inherited isNegated
flag. Requirement satisfaction, constraint truth, quantity arithmetic and metadata
application remain pending semantic checks. An abstract sum signature and source
construction do not supply numeric values for unassigned component masses.

The package-level P01 `part f` is owned through `OwningMembership`: the original
SysML.xtext `PackageMember` rule (line 220) returns that class for both definitions
and usages. The `FeatureMembership` rules (534/538) apply inside definition
bodies. Connector definitions likewise own authored `end item` usages through
`FeatureMembership`; `EndFeatureMembership` belongs to the explicit connector
endpoint grammar. These are source expectations, with native results pending.

The standalone native host helper
`crates/intersect-kerml/examples/native_pilot_checks/sysml.rs` checks seven direct
stored typing bindings against six exact source/resource pins on the admitted
ResourceSet's actual `MemoryStore`. It returns actual owner, membership, typing
and target IDs. The genuine ScalarValues and ISQBase roots are `LibraryPackage`
objects, not exact-class `Package` rows. It checks stored containment and links;
it does not compute general Feature.type or certify whole-example semantics.

## Upstream diagnostic evidence

All four corresponding upstream Xpect fixtures explicitly declare `noErrors`.
Their byte pins and differences are recorded in the JSON. None is the exact
original example body:

- PartTest retains the same cycles/self-subsetting but omits original
  multiplicities, ScalarValues typing, timeslices, action and state bodies.
- ConnectionTest omits the original metadata use and metadata definition M.
- RequirementTest omits the original private recursive import q::**.
- ConstraintTest changes import visibility and expands inherited redefinitions.

The fixtures use SysMLTests, which extends KerMLXtextTests and external
XtextTests; the inspected driver registers real derived-setting and operation
delegates. This records upstream test intent, without claiming an executed oracle
or exact-example diagnostic baseline. In particular, cycles, the four-end bus,
negated satisfaction and absent numeric values are not classified here as
intentional semantic errors.

A native harness should report each observation as passed, refused, failed or
not reached, with a phase and receipt. Keep definition import, source-model
construction, linking, declared-signature reads, validation and evaluation
separate. The specification requests neither whole-example zero diagnostics nor
whole-language validation/publication permission. Preserve the original texts
and source/resource identities when a later check is refused.

R05 follows the implemented original Requirement identifier carrier: nested
`RequirementTest::R::A` stores `reqId="1"`; its raw `declaredShortName` is absent.
The full-mode observer must call the SAME scoped StructuralValueSelector on the
canonical Element.declaredShortName member and observe effective `"1"`. Its
OwningMembership and SubjectMembership/ReferenceUsage containment and resolved
handles are checked against the original source resource. This is a selected
effective identifier result, not Requirement satisfaction. The observer now
accepts the canonical schema and scoped selector explicitly; original full-
corpus execution of this updated R05 observation remains pending.

C01 preserves the original owned chain object. ReferenceSubsetting's parser
postprocessor sets referencedFeature to its last ownedRelatedElement, which is
the actual Feature helper for `p1.x.x1`. The three FeatureChaining references
resolve in order to `P::p1`, `p::x` and `p::x::x1`. ConnectorUtil returns that
helper as the first related Feature, so effective source is `[helper]`, not the
terminal x1 ID; target is `[P::y]`. The observer separately checks both identities
through existing stored ownership/reference and scoped selector APIs. No host
resolver or chain evaluator is introduced. Original full-corpus execution of
this updated C01 observation remains pending.
