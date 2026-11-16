# KerML language policies

This package owns KerML/Pilot domain policies. It depends on the
shared language runtime, core store and definitions; none depend on it.

The [source-defined integration batch](../../../docs/rcore-xtext-bindings.md)
starts moving those policies into authored Rcore. `semantics/package-name.rcore`
supplies the bounded Package.name getter. `semantics/base-name.rcore` and
`semantics/membership-label.rcore` supply checked base and owning label projections;
`semantics/local-feature-typing-scope.rcore` defines an explicit local scope over
the original nominal metamodel. Generic Rust adapters execute the existing Rcore
IR and use shared checked reads/linking. Most native policies still remain in
Rust; these components do not establish full KerML scope or replace the frozen
native language recipes.

`OperandSettings` implements the append-only pinned Pilot operand adapter.
Its source-bound contract selects one exact receiver class and schema revision.
It creates private ParameterMembership, in Feature and public FeatureValue
wrappers through shared checked setting plans and containment opposites. Reads
of the operand facade return an empty array, as in Pilot. This is not normative
argument evaluation, full KerML or a production web language installation.

The implementation migrates the existing Mercurio append algorithm through
Intersect's setting interfaces. Exact upstream/Mercurio observations and source
hashes are retained in fixtures/xtext/pinned-kerml-operands, with license notices.
The raw SysML.ecore fixture is unchanged. All seven concrete oracle receiver
classes are compared using two distinct equal-valued operands.

The oracle starts Feature objects with null direction (pinned generated Java
FeatureImpl.DIRECTION_EDEFAULT). Tests explicitly set that initial state.
Generic enum defaults remain unchanged. A source-bound language initializer is
still needed for automatic text-created object parity. The provider does not
rewrite existing operand defaults or implement every Pilot constructor effect.

Verification: `cargo test -p intersect-kerml --locked` and
`node tools/test_kerml_operands_wasm.mjs`. The latter executes the same assertions
in real WASM, including the actual pinned schema import. No Java is required.
Java21+ is needed only to freshly replay the independent retained Pilot oracle.


Use the host-only `inspect_pinned_grammar_admission` example for current ordinary
KerML or SysML source admission. It requires an explicit profile, verifies raw
source pins, migrates Ecore, checks exact operand contracts and emits one JSON
observation with policies, dependencies and the first blocker. SysML and KerML
independently extend KerMLExpressions. See the [commands, source pins and next
acceptance targets](../../../docs/pinned-grammar-admission.md). Full grammar
activation and domain semantics remain unqualified.

`inspect_pinned_kerml_with_settings` retains the historical lower-level compiler
route and NativeV42 default. Its results do not describe the ordinary frontend.
