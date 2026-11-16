# Native Pilot KerML observations

The language-owned [observation specification](../crates/intersect-kerml/observations/pilot-kerml-v1.json)
defines 14 selected observations and 35 phase-specific checks for the unchanged
Pilot Types and Expressions examples at commit
692170b71867353b8f90341e61556f49a5beb0e5. They are source-grounded expectations,
not recorded native passes. No Java was executed.

The specification pins original byte counts, SHA-256 identities and unique UTF-8
anchor ranges. Its observation IDs are labels. A native receipt must capture the
actual object IDs and compare resolved target identities; it must not fabricate
UUIDs or equate two targets just because their leaf names match.

## First useful graph checks

| Observation | Concrete result to inspect |
| --- | --- |
| T01 | Types::A is an abstract Type whose explicit general reference reaches the genuine Base::Anything object. |
| T03 | Types::Super::P retains private visibility; protected feature f resolves P::Sub to Types::Super::P::Sub. |
| T04–T05 | Named specialization Gen preserves A/B endpoint direction; named conjugations c1/c2 preserve original/conjugated endpoint direction. |
| T06 | Unioning, Intersecting, Differencing and Disjoining references preserve their actual stored target roles. Derived inverse aliases must not be fabricated as stored fields. |
| E02 | Expressions::a and aa reach the actual ScalarValues::Integer and Boolean declarations through ScalarFunctions' public import. |
| E03–E04 | ToString invocation binds BaseFunctions::ToString; explicitly qualified NumericalFunctions::'+' binds that function. Its identity differs from the DataFunctions function selected by ordinary operator syntax. |
| E05 | Compact collect/select forms construct CollectExpression.operator=collect and SelectExpression.operator=select. Explicit arrow invocations reach the real ControlFunctions functions. |
| E06 | C::count preserves variable/initial-value state and its explicit ScalarValues::Integer reference. |
| E07–E08 | c#(1).count preserves IndexExpression plus FeatureChainExpression structure; new L() preserves a ConstructorExpression and its actual L reference. Linking these requires their real semantic providers. |

The exact inputs are
[Types.kerml](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/kerml/src/examples/Simple%20Tests/Types.kerml)
and
[Expressions.kerml](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/kerml/src/examples/Simple%20Tests/Expressions.kerml).
The first has 999 bytes; the second has 1,883 bytes in the pinned Windows checkout.
The JSON carries their full hashes.

## Diagnostic and evaluation boundaries

The adjacent upstream
[ParsingTests_Expressions fixture](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/org.omg.kerml.xpect.tests/src/org/omg/kerml/xpect/tests/parsing/ParsingTests_Expressions.kerml.xt)
contains an explicit XPECT noErrors expectation. It is a near-copy, with public
imports instead of private imports and different C.count typing. It uses
KerMLParsingTest through KerMLXtextTests and external XtextTests. This is evidence
of upstream test intent for that fixture, not a frozen diagnostic baseline for
the exact example or proof that Intersect has reproduced those checks.

x is valued by ToString, whose
[declared return is String](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/sysml.library/Kernel%20Libraries/Kernel%20Function%20Library/BaseFunctions.kerml#L28).
Later arithmetic/comparison uses of x are type-review candidates. No inspected
upstream expectation establishes them as intentional semantic errors. The
recursive TotalMass body, cyclic value paths and unset input features also do
not justify expecting a concrete value for every expression. Recursion alone is
not classified here as a language error.

The independent upstream
[ExpressionUtilTest](https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation/blob/692170b71867353b8f90341e61556f49a5beb0e5/org.omg.sysml.logic/src/test/java/org/omg/sysml/logic/ExpressionUtilTest.java)
asserts that arguments come from valued features redefining instantiated
features, and checks incomplete instantiations. These are useful future
argument-provider controls. They do not supply numeric results for Expressions.

## Recording native results

A harness should report each observation as passed, refused, failed or not
reached, with its phase and receipt. Keep definition import, constructor
admission, source-model construction, linking, declared-signature reads,
validation and evaluation distinct. In particular, a 167-class constructor
catalog does not prove these 35 checks.

The specification does not request whole-example zero diagnostics, successful
evaluation of all expressions, language validation or publication permission.
Known provider frontiers should remain explicit. Whole original texts and
source/resource identities must remain intact when a later observation fails.

