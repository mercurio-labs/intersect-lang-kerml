"""Inventory of the SysML v2 Pilot's validation rules (KerML and SysML).

Handwritten verification tooling (never production code). The inventory below
was read from the Pilot sources at the pinned commit
692170b71867353b8f90341e61556f49a5beb0e5:

- org.omg.kerml.xtext/src/org/omg/kerml/xtext/validation/KerMLValidator.xtend
- org.omg.sysml.xtext/src/org/omg/sysml/xtext/validation/SysMLValidator.xtend
  (extends KerMLValidator; SysML files get both rule sets)
- the utilities they call in org.omg.sysml.logic/src/main/java/org/omg/sysml/util

Codes and messages are not retyped here: each rule names the Xtend constants,
and ``build`` resolves them from the sources, so a wrong constant name fails.
``build`` also checks that every ``@Check`` method of both validators is
inventoried and lists constants that are declared but never reported.

Usage: python -B validation_rules.py UPSTREAM_TESTS OUT.json
       python -B validation_rules.py --markdown RULES.json [RULES_REPORT.json]
(UPSTREAM_TESTS is the sparse checkout described in README.md.)

Needs (what an implementation must compute before the rule can be decided):

- owned: owned relationships, stored attributes, metaclass tests on resolved
  targets, owner navigation (owningType, owningNamespace, owningMembership).
- defaults: construction defaults the Pilot sets when parsing (isComposite,
  direction, isPortion, isVariation ...); Intersect has these as `bind default`.
- implicit: implicit relationships or elements (implicit specializations,
  redefinitions, binding connectors, conjugated port definitions).
- derived: derived properties computed over the model (featuringType, type and
  getAllTypesOf, multiplicity, endFeature, inputParameters, inherited
  memberships, isReference, specializes/conformsTo, isFeaturedWithin ...).
- library: a library element looked up by qualified name.
- names: member names and inherited memberships (distinguishability).
- typing: the static result type of an expression (Boolean, Integer, ...).
- evaluation: model-level evaluability or evaluating an expression.
- resource: a fact about the resource (is it a library resource).

The category is the strongest need: evaluation > names > derived properties
(derived, implicit, library, typing) > purely structural (owned, defaults).
"""

import json
import re
import sys
from collections import Counter
from pathlib import Path

KERML = "org.omg.kerml.xtext/src/org/omg/kerml/xtext/validation/KerMLValidator.xtend"
SYSML = "org.omg.sysml.xtext/src/org/omg/sysml/xtext/validation/SysMLValidator.xtend"

BATCHES = {
    "B1": "KerML structure: owned relationships, flags and metaclass of resolved targets",
    "B2": "SysML structure: memberships, owning types, variations, referential flags, reference subsettings",
    "B3": "Effective typing: 'must be typed by' rules over all types of a feature (getAllTypesOf)",
    "B4": "Derived feature semantics: featuring types, multiplicities, ends, cross features, "
          "redefinition/subsetting conformance, connectors, inherited parameters and memberships, library specialization",
    "B5": "Names: namespace distinguishability (member names, alias names, inherited memberships)",
    "B6": "Expression typing and evaluation: Boolean/Integer/Duration/Time results, model-level evaluability, "
          "cast and binding type conformance, implicit binding connectors",
}

EMPTY_CHECKS = [
    ("SysML", "checkOccurrenceDefinition", "OccurrenceDefinition"),
    ("SysML", "checkDecisionNode", "DecisionNode"),
    ("SysML", "checkForkNode", "ForkNode"),
    ("SysML", "checkJoinNode", "JoinNode"),
    ("SysML", "checkMergeNode", "MergeNode"),
]

# (language, method, metaclass, code constant, message constants, severity,
#  target, condition, needs, area, batch, notes)
# A code constant written as "MSG:NAME" means the Pilot passes that message
# constant as the issue code (a Pilot quirk recorded verbatim).
R = []


def rule(lang, method, metaclass, code, msgs, severity, target, condition, needs, area, batch, notes=""):
    R.append(dict(language=lang, method=method, metaclass=metaclass, code_constant=code,
                  message_constants=msgs, severity=severity, target=target, condition=condition,
                  needs=needs, area=area, batch=batch, notes=notes))


K, S = "KerML", "SysML"
# ---- KerML: root -------------------------------------------------------------
rule(K, "checkElement", "Element", "INVALID_ELEMENT_IS_IMPLIED_INCLUDED", ["INVALID_ELEMENT_IS_IMPLIED_INCLUDED_MSG"], "error",
     "the element", "isImpliedIncluded is false but the element owns a relationship with isImplied = true",
     ["owned"], "Root", "B1", "Parsed models have no implied owned relationships; practically unreachable from text.")
rule(K, "checkAnnotation", "Annotation", "INVALID_ANNOTATION_ANNOTATING_ELEMENT", ["INVALID_ANNOTATION_ANNOTATING_ELEMENT_MSG"], "error",
     "the annotation", "the annotation has neither, or both, an owned annotating element and an owning annotating element",
     ["owned"], "Root", "B1")
rule(K, "checkAnnotation", "Annotation", "INVALID_ANNOTATION_ANNOTATED_ELEMNT_OWNERSHIP",
     ["INVALID_ANNOTATION_ANNOTATED_ELEMNT_OWNERSHIP_MSG_1", "INVALID_ANNOTATION_ANNOTATED_ELEMNT_OWNERSHIP_MSG_2"], "error",
     "the annotation", "owned by its annotated element without owning its annotating element (msg 1), or owning its annotating element without being owned by the annotated element (msg 2)",
     ["owned"], "Root", "B1")
rule(K, "checkNamespace", "Namespace", "INVALID_NAMESPACE_DISTINGUISHABILITY",
     ["INVALID_NAMESPACE_DISTINGUISHABILITY_MSG", "INVALID_NAMESPACE_DISTINGUISHABILITY_MSG_0",
      "INVALID_NAMESPACE_DISTINGUISHABILITY_MSG_1", "INVALID_NAMESPACE_DISTINGUISHABILITY_MSG_2"], "warning",
     "the owned member element (declaredName/declaredShortName) for owning memberships, the membership (memberName/memberShortName) for aliases, the namespace for two clashing inherited memberships",
     "two owned memberships, an alias and an owned membership, two aliases, an owned and an inherited membership (types only), or two inherited memberships (types only) have the same name or short name but different member elements. Skipped for invocation, feature reference, literal and null expressions and binding connectors. The message gets \" '<name>' from <sorted owning namespace names>\" appended when the duplicates come from other namespaces",
     ["names", "derived", "implicit"], "Root", "B5",
     "The most frequent Pilot diagnostic (inherited 'self', 'start', 'done' ... through implicit library supertypes). Imported memberships are not checked.")
rule(K, "checkImport", "Import", "INVALID_IMPORT_TOP_LEVEL_VISIBILITY", ["INVALID_IMPORT_TOP_LEVEL_VISIBILITY_MSG"], "error",
     "the import", "an import owned by a root namespace is not private", ["owned"], "Root", "B1")
# ---- KerML: core ---------------------------------------------------------------
rule(K, "checkSpecialization", "Specialization", "INVALID_SPECIALIZATION_SPECIFIC_NOT_CONJUGATED", ["INVALID_SPECIALIZATION_SPECIFIC_NOT_CONJUGATED_MSG"], "error",
     "the specialization, feature specific", "the specific type is conjugated (owns a conjugator)", ["owned"], "Core types", "B1")
rule(K, "checkType", "Type", "INVALID_TYPE_AT_MOST_ONE_CONJUGATOR", ["INVALID_TYPE_AT_MOST_ONE_CONJUGATOR_MSG"], "error",
     "the type", "the type owns more than one Conjugation", ["owned"], "Core types", "B1")
for kind in ("DIFFERENCING", "INTERSECTING", "UNIONING"):
    rule(K, "checkType", "Type", f"INVALID_TYPE_OWNED_{kind}_NOT_ONE", [f"INVALID_TYPE_OWNED_{kind}_NOT_ONE_MSG"], "error",
         f"the only owned {kind.lower()} relationship", f"the type owns exactly one {kind.capitalize()} relationship", ["owned"], "Core types", "B1")
    rule(K, "checkType", "Type", f"INVALID_TYPE_{kind}_TYPES_NOT_SELF", [f"INVALID_TYPE_{kind}_TYPES_NOT_SELF_MSG"], "error",
         f"the offending {kind.capitalize()}", f"an owned {kind.capitalize()} relationship targets the type itself", ["owned"], "Core types", "B1")
rule(K, "checkType", "Type", "INVALID_TYPE_OWNED_MULTIPLICITY", ["INVALID_TYPE_OWNED_MULTIPLICITY_MSG"], "error",
     "the second and later memberships, feature memberElement", "the type owns more than one membership whose member is a Multiplicity", ["owned"], "Core types", "B1")
rule(K, "checkClassifier", "Classifier", "INVALID_CLASSIFIER_DEFAULT_SUPERTYPE", ["INVALID_CLASSIFIER_DEFAULT_SUPERTYPE_MSG"], "error",
     "the classifier, feature ownedSubclassification", "the classifier does not (transitively) specialize the library default supertype of its metaclass (ImplicitGeneralizationMap 'base', e.g. Class: Occurrences::Occurrence, DataType: Base::DataValue); {supertype} is that qualified name",
     ["derived", "implicit", "library"], "Core types", "B4", "Not in the spec as one constraint. Fires when an explicit specialization leads away from the default (no implicit default is added then).")
rule(K, "checkClassifier", "Classifier", "INVALID_CLASSIFIER_MULTIPLICITY_DOMAIN", ["INVALID_CLASSIFIER_MULTIPLICITY_DOMAIN_MSG"], "error",
     "the classifier, feature multiplicity", "the classifier's multiplicity has featuring types", ["derived"], "Core types", "B4")
rule(K, "checkEndFeatureMembership", "EndFeatureMembership", "INVALID_END_FEATURE_MEMBERSHIP_IS_END", ["INVALID_END_FEATURE_MEMBERSHIP_IS_END_MSG"], "error",
     "the owned member feature", "the owned member feature is not an end feature", ["owned", "defaults"], "Core features", "B1", "Code spelled 'validateEndFeatureMembershpIsEnd'.")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_HAS_TYPE", ["INVALID_FEATURE_HAS_TYPE_MSG"], "error",
     "the feature, feature type", "FeatureUtil.getAllTypesOf(feature) is empty (no own, inherited, implicit or chained type)",
     ["derived", "implicit", "library"], "Core features", "B3", "Not in the spec. Fires on library-less loads (28 times on Base, ScalarValues, Links alone).")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_CHAINING_FEATURE_NOT_ONE", ["INVALID_FEATURE_CHAINING_FEATURE_NOT_ONE_MSG"], "error",
     "the only owned FeatureChaining", "the feature owns exactly one FeatureChaining", ["owned"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_CHAINING_FEATURES_NOT_SELF", ["INVALID_FEATURE_CHAINING_FEATURES_NOT_SELF_MSG"], "error",
     "the offending FeatureChaining", "an owned FeatureChaining has the feature itself as chaining feature", ["owned"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_OWNED_REFERENCE_SUBSETTING", ["INVALID_FEATURE_OWNED_REFERENCE_SUBSETTING_MSG"], "error",
     "the second and later ReferenceSubsettings", "the feature owns more than one ReferenceSubsetting", ["owned"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_MULTIPLICITY_DOMAIN", ["INVALID_FEATURE_MULTIPLICITY_DOMAIN_MSG"], "error",
     "the feature, feature multiplicity", "the feature's multiplicity has a different set of featuring types than the feature", ["derived"], "Core features", "B4")
rule(K, "checkFeature", "Feature", "INVALID_REDEFINITION_DIRECTION_CONFORMANCE", ["INVALID_REDEFINITION_DIRECTION_CONFORMANCE_MSG"], "error",
     "the feature", "for an implicitly redefined feature: as checkRedefinition's direction rule", ["derived", "implicit", "defaults"], "Core features", "B4",
     "Same code as the explicit Redefinition rule; here the target is the feature itself.")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_CROSS_FEATURE_SPECIALIZATION", ["INVALID_FEATURE_CROSS_FEATURE_SPECIALIZATION_MSG"], "error",
     "the owned cross feature, or the owned CrossSubsetting (feature crossedFeature)", "the feature has a cross feature that does not specialize the cross feature of a feature it redefines (including computed redefinitions)",
     ["derived", "implicit"], "Core features", "B4")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_CROSS_FEATURE_TYPE", ["INVALID_FEATURE_CROSS_FEATURE_TYPE_MSG"], "error",
     "the owned cross feature, or the owned CrossSubsetting (feature crossedFeature)", "the cross feature's types differ (as a set) from the feature's types", ["derived"], "Core features", "B4")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_OWNED_CROSS_SUBSETTING", ["INVALID_FEATURE_OWNED_CROSS_SUBSETTING_MSG"], "error",
     "the second and later CrossSubsettings", "the feature owns more than one CrossSubsetting", ["owned"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_END_FEATURE_MULTIPLICITY", ["INVALID_FEATURE_END_FEATURE_MULTIPLICITY_MSG"], "warning",
     "the feature", "an end feature none of whose multiplicities is a range with bounds exactly 1..1",
     ["derived", "evaluation"], "Core features", "B6", "Bounds are compared through MultiplicityRange.hasBounds, which evaluates the bound expressions.")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_CROSSING_SPECIALIZATION", ["INVALID_FEATURE_CROSSING_SPECIALIZATION_MSG"], "error",
     "the owned cross feature", "the feature owns a cross feature that is not its computed cross feature", ["derived"], "Core features", "B4")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_CONSTANT_IS_VARIABLE", ["INVALID_FEATURE_CONSTANT_IS_VARIABLE_MSG"], "error",
     "the feature", "isConstant and not isVariable", ["owned", "defaults"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_END_NO_DIRECTION", ["INVALID_FEATURE_END_NO_DIRECTION_MSG"], "error",
     "the feature", "an end feature with a direction", ["owned", "defaults"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_END_NOT_DERIVED_ABSTRACT_COMPOSITE_OR_PORTION", ["INVALID_FEATURE_END_NOT_DERIVED_ABSTRACT_COMPOSITE_OR_PORTION_MSG"], "error",
     "the feature", "an end feature that is derived, abstract, composite or portion", ["owned", "defaults"], "Core features", "B1")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_IS_VARIABLE", ["INVALID_FEATURE_IS_VARIABLE_MSG"], "error",
     "the feature", "a variable feature without an owning type, or whose owning type does not specialize Occurrences::Occurrence",
     ["derived", "implicit", "library"], "Core features", "B4")
rule(K, "checkFeature", "Feature", "INVALID_FEATURE_PORTION_NOT_VARIABLE", ["INVALID_FEATURE_PORTION_NOT_VARIABLE_MSG"], "error",
     "the feature", "isPortion and isVariable", ["owned", "defaults"], "Core features", "B1")
rule(K, "checkFeatureChaining", "FeatureChaining", "INVALID_FEATURE_CHAINING_FEATURE_CONFORMANCE", ["INVALID_FEATURE_CHAINING__FEATURE_CONFORMANCE_MSG"], "error",
     "the FeatureChaining, feature chainingFeature", "a chaining feature after the first is not featured within the previous chaining feature (isFeaturedWithin)",
     ["derived", "implicit"], "Core features", "B4")
rule(K, "checkRedefinition", "Redefinition", "INVALID_REDEFINITION_DIRECTION_CONFORMANCE", ["INVALID_REDEFINITION_DIRECTION_CONFORMANCE_MSG"], "error",
     "the Redefinition, feature redefinedFeature", "for some effective featuring type T of the redefining feature, the redefined feature's direction relative to T (directionOf, which accounts for conjugation) is in or out and the redefining direction differs, or is inout and the redefining feature has no direction",
     ["derived", "defaults"], "Core features", "B4")
rule(K, "checkRedefinition", "Redefinition", "INVALID_REDEFINITION_FEATURING_TYPES",
     ["INVALID_REDEFINITION_FEATURING_TYPES_MSG_1", "INVALID_REDEFINITION_FEATURING_TYPES_MSG_2"], "error",
     "the Redefinition, feature redefinedFeature", "the redefined feature is not owned by the redefinition and both features have the same set of effective featuring types (owning type for variable features, else featuringType); msg 1 when that set is empty (package level)",
     ["derived", "implicit"], "Core features", "B4")
rule(K, "checkRedefinition", "Redefinition", "INVALID_REDEFINITION_END_CONFORMANCE", ["INVALID_REDEFINITION_END_CONFORMANCE_MSG"], "error",
     "the Redefinition, feature redefinedFeature", "the redefined feature is an end, the redefining one is not, and the redefining feature's owning type is an Association or Connector",
     ["owned", "defaults"], "Core features", "B1")
rule(K, "checkSubsetting", "Subsetting", "INVALID_REDEFINITION_MULTIPLICITY_CONFORMANCE", ["INVALID_REDEFINITION_MULTIPLICITY_CONFORMANCE_MSG"], "warning",
     "the Subsetting, feature redefiningFeature", "a Redefinition between non-end features whose multiplicity ranges differ: the redefining lower bound (or upper when no lower) is a smaller LiteralInteger, or * against a positive integer",
     ["derived"], "Core features", "B4", "Compares literal bounds only (LiteralInteger, LiteralInfinity); computed bounds are ignored. Multiplicities may be inherited.")
rule(K, "checkSubsetting", "Subsetting", "INVALID_SUBSETTING_MULTIPLICITY_CONFORMANCE", ["INVALID_SUBSETTING_MULTIPLICITY_CONFORMANCE_MSG"], "warning",
     "the Subsetting, feature subsettingFeature", "both features end or both not end, different multiplicity ranges, and the subsetting upper bound is * against a finite one, or a larger LiteralInteger",
     ["derived"], "Core features", "B4", "Literal bounds only, as above.")
rule(K, "checkSubsetting", "Subsetting", "INVALID_SUBSETTING_UNIQUENESS_CONFORMANCE", ["INVALID_SUBSETTING_UNIQUENESS_CONFORMANCE_MSG"], "error",
     "the Subsetting, feature subsettingFeature", "the subsetted feature is unique and the subsetting feature is nonunique", ["owned"], "Core features", "B1")
rule(K, "checkSubsetting", "Subsetting", "INVALID_SUBSETTING_CONSTANT_CONFORMANCE", ["INVALID_SUBSETTING_CONSTANT_CONFORMANCE_MSG"], "error",
     "the Subsetting, feature subsettedFeature", "the subsetted feature is constant and the subsetting feature is variable but not constant", ["owned", "defaults"], "Core features", "B1",
     "No null check on subsettedFeature: an unresolved target throws inside the Pilot's validator.")
rule(K, "checkSubsetting", "Subsetting", "INVALID_SUBSETTING_FEATURING_TYPES", ["INVALID_SUBSETTING_FEATURING_TYPES_MSG"], "error",
     "the Subsetting, feature subsettedFeature", "the subsetted feature has featuring types and is not accessible from the subsetting feature (FeatureUtil.canAccess: featured within one of its featuring types, recursively through featuring types that are features)",
     ["derived", "implicit"], "Core features", "B4", "Also covers Redefinition and ReferenceSubsetting.")
rule(K, "checkCrossSubsetting", "CrossSubsetting", "INVALID_CROSS_SUBSETTING_CROSSED_FEATURE", ["INVALID_CROSS_SUBSETTING_CROSSED_FEATURE_MSG"], "error",
     "the CrossSubsetting, feature crossedFeature", "the crossing feature is an end with an owning type, and the crossed feature is not a two-feature chain, or (two ends) its first chaining feature is not the other end",
     ["derived"], "Core features", "B4")
rule(K, "checkCrossSubsetting", "CrossSubsetting", "INVALID_CROSS_SUBSETTING_CROSSING_FEATURE", ["INVALID_CROSS_SUBSETTING_CROSSING_FEATURE_MSG"], "error",
     "the CrossSubsetting", "the crossing feature is not an end, has no owning type, or its owning type has fewer than two end features", ["derived"], "Core features", "B4")
# ---- KerML: kernel classifiers ------------------------------------------------------
rule(K, "checkDataType", "DataType", "INVALID_DATA_TYPE_SPECIALIZATION", ["INVALID_DATA_TYPE_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "an owned specialization's general is a Class or an Association", ["owned"], "Kernel classifiers", "B1",
     "SysML overrides the message (INVALID_ATTRIBUTE_DEFINITION_SPECIALIZATION_MSG), same code.")
rule(K, "checkClass", "Class", "INVALID_CLASS_SPECIALIZATION", ["INVALID_CLASS_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "an owned specialization's general is a DataType, or an Association while the class is not one", ["owned"], "Kernel classifiers", "B1",
     "SysML overrides the message (INVALID_ITEM_DEFINITION_SPECIALIZATION_MSG), same code.")
rule(K, "checkStructure", "Structure", "INVALID_STRUCTURE_SPECIALIZATION", ["INVALID_STRUCTURE_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "an owned specialization's general is a Behavior", ["owned"], "Kernel classifiers", "B1")
rule(K, "checkBehavior", "Behavior", "INVALID_BEHAVIOR_SPECIALIZATION", ["INVALID_BEHAVIOR_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "an owned specialization's general is a Structure", ["owned"], "Kernel classifiers", "B1")
rule(K, "checkAssociation", "Association", "INVALID_ASSOCIATION_BINARY_SPECIALIZATION", ["INVALID_ASSOCIATION_BINARY_SPECIALIZATION_MSG"], "error",
     "the third and later owned end features", "more than two owned end features and the association conforms to Links::BinaryLink",
     ["derived", "implicit", "library"], "Kernel classifiers", "B4")
rule(K, "checkAssociation", "Association", "INVALID_ASSOCIATION_RELATED_TYPES", ["INVALID_ASSOCIATION_RELATED_TYPES_MSG"], "error",
     "the association, feature relatedElement", "a non-abstract association with fewer than two related types (types of its end features, inherited included)", ["derived"], "Kernel classifiers", "B4")
rule(K, "checkAssociation", "Association", "INVALID_ASSOCIATION_END_TYPES", ["INVALID_ASSOCIATION_END_TYPES_MSG"], "error",
     "the owned end feature", "an owned end feature does not have exactly one type (Feature.type, inherited types included)", ["derived"], "Kernel classifiers", "B3")
# ---- KerML: connectors -----------------------------------------------------------------
rule(K, "checkBindingConnector", "BindingConnector", "INVALID_BINDING_CONNECTOR_IS_BINARY", ["INVALID_BINDING_CONNECTOR_IS_BINARY_MSG"], "error",
     "the binding connector", "the binding connector does not have exactly two related features", ["derived"], "Kernel connectors", "B4")
rule(K, "checkBindingConnector", "BindingConnector", "INVALID_BINDING_CONNECTOR_TYPE_CONFORMANCE", ["INVALID_BINDING_CONNECTOR_TYPE_CONFORMANCE_MSG"], "warning",
     "the binding connector (explicit), or the type owning an implicit one; feature endFeature", "no type of one related feature conforms to a type of the other, except that a Boolean-valued expression result conforms to Performances::BooleanEvaluation",
     ["derived", "implicit", "library", "typing"], "Kernel connectors", "B6",
     "checkImplicitBindingConnectors(Type) applies the same check to implicit binding connectors (feature values, expression results ...) with at least two related features.")
rule(K, "checkConnector", "Connector", "INVALID_CONNECTOR_RELATED_FEATURES", ["INVALID_CONNECTOR_RELATED_FEATURES_MSG"], "error",
     "the connector, feature relatedElement", "a non-abstract connector with fewer than two related features", ["derived"], "Kernel connectors", "B4")
rule(K, "checkConnector", "Connector", "INVALID_CONNECTOR_BINARY_SPECIALIZATION", ["INVALID_CONNECTOR_BINARY_SPECIALIZATION_MSG"], "error",
     "the connector when it owns at most two ends, else its third and later ends", "more than two end features (inherited included) and the connector conforms to Links::BinaryLink",
     ["derived", "implicit", "library"], "Kernel connectors", "B4")
rule(K, "checkConnector", "Connector", "INVALID_CONNECTOR_TYPE_FEATURING", ["INVALID_CONNECTOR_TYPE_FEATURING_MSG"], "error",
     "the connector end (explicit connector) or the feature reference expression (implicit binding)", "a related feature is not featured within every featuring type of the connector (or within none, for a connector without featuring types); skipped when canAccess already fails (reported by validateSubsettingFeaturingTypes), for chain/reference expression results, and for flows owned by a feature without owning type",
     ["derived", "implicit"], "Kernel connectors", "B4", "Also run from checkImplicitBindingConnectors for FeatureReferenceExpressions.")
rule(K, "checkImplicitBindingConnectors", "Type", "INVALID_CONNECTOR_TYPE_FEATURING", ["INVALID_CONNECTOR_TYPE_FEATURING_MSG"], "error",
     "the feature reference expression", "for each implicit binding connector of a FeatureReferenceExpression (TypeUtil.forEachImplicitBindingConnectorOf): a related feature is not featured within the connector's featuring types plus the expression",
     ["derived", "implicit"], "Kernel connectors", "B4")
rule(K, "checkImplicitBindingConnectors", "Type", "INVALID_BINDING_CONNECTOR_TYPE_CONFORMANCE", ["INVALID_BINDING_CONNECTOR_TYPE_CONFORMANCE_MSG"], "warning",
     "the type owning the implicit binding connector, feature endFeature", "for each implicit binding connector (feature values, expression results, ...) with at least two related features: the binding type conformance rule above",
     ["derived", "implicit", "library", "typing"], "Kernel connectors", "B6")
# ---- KerML: behaviors, functions, expressions ---------------------------------------------
rule(K, "checkParameterMembership", "ParameterMembership", "INVALID_PARAMETER_MEMBERSHIP_OWNING_TYPE", ["INVALID_PARAMETER_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership, feature ownedMemberParameter", "a (non-return) parameter membership whose owning type is not a Behavior, a Step, or a constructor expression's result", ["owned"], "Kernel behaviors", "B1")
rule(K, "checkParameterMembership", "ParameterMembership", "INVALID_PARAMETER_MEMBERSHIP_DIRECTION", ["INVALID_PARAMETER_MEMBERSHIP_DIRECTION_MSG"], "error",
     "the parameter", "the parameter's direction differs from the membership's parameterDirection (in; out for return parameters); {direction} is that direction in lower case",
     ["owned", "defaults"], "Kernel behaviors", "B1")
rule(K, "checkExpression", "Expression", "INVALID_EXPRESSION_RESULT_PARAMETER_MEMBERSHIP", ["INVALID_EXPRESSION_RESULT_PARAMETER_MEMBERSHIP_MSG"], "error",
     "the second and later return parameter memberships, feature ownedMemberParameter", "the expression owns more than one ReturnParameterMembership", ["owned"], "Kernel behaviors", "B1")
rule(K, "checkExpression", "Expression", "INVALID_EXPRESSION_RESULT_EXPRESSION_MEMBERSHIP", ["INVALID_EXPRESSION_RESULT_EXPRESSION_MEMBERSHIP_MSG"], "error",
     "an owned result expression membership (feature ownedResultExpression), else the expression", "more than one result expression membership, owned or inherited from supertypes",
     ["derived", "implicit"], "Kernel behaviors", "B4", "checkFunction reports the same code for functions (the FUNCTION_RESULT_EXPRESSION constant is never used).")
rule(K, "checkFunction", "Function", "INVALID_FUNCTION_RESULT_PARAMETER_MEMBERSHIP", ["INVALID_FUNCTION_RESULT_PARAMETER_MEMBERSHIP_MSG"], "error",
     "the second and later return parameter memberships, feature ownedMemberParameter", "the function owns more than one ReturnParameterMembership", ["owned"], "Kernel behaviors", "B1")
rule(K, "checkFunction", "Function", "INVALID_EXPRESSION_RESULT_EXPRESSION_MEMBERSHIP", ["INVALID_EXPRESSION_RESULT_EXPRESSION_MEMBERSHIP_MSG"], "error",
     "an owned result expression membership, else the function", "more than one result expression membership, owned or inherited", ["derived", "implicit"], "Kernel behaviors", "B4",
     "Reported with the Expression code and message.")
rule(K, "checkReturnParameterMembership", "ReturnParameterMembership", "INVALID_RETURN_PARAMETER_MEMBERSHIP_OWNING_TYPE", ["INVALID_RETURN_PARAMETER_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership, feature ownedMemberParameter", "the owning type is not a Function or an Expression", ["owned"], "Kernel behaviors", "B1")
rule(K, "checkResultExpressionMembership", "ResultExpressionMembership", "INVALID_RESULT_EXPRESSION_MEMBERSHIP_OWNING_TYPE", ["INVALID_RESULT_EXPRESSION_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership (the Pilot passes ParameterMembership.ownedMemberParameter, not a feature of this class)", "the owning type is not a Function or an Expression", ["owned"], "Kernel behaviors", "B1")
rule(K, "checkCollectExpression", "CollectExpression", "INVALID_COLLECT_EXPRESSION_OPERATOR", ["INVALID_COLLECT_EXPRESSION_OPERATOR_MSG"], "error",
     "the expression", "operator is not 'collect'", ["owned"], "Kernel expressions", "B1", "The Pilot derives the operator; it holds by construction.")
rule(K, "checkFeatureChainExpression", "FeatureChainExpression", "INVALID_FEATURE_CHAIN_EXPRESSION_FEATURE_CONFORMANCE", ["INVALID_FEATURE_CHAIN_EXPRESSION_FEATURE_CONFORMANCE_MSG"], "error",
     "the second owned membership, feature memberElement", "the target feature is not a Feature, or the relative namespace (the source's result) is a Type and the target is not featured within it",
     ["derived", "names"], "Kernel expressions", "B4", "The relative namespace is the scope the target name resolves in; the rule itself only uses resolved references.")
rule(K, "checkFeatureChainExpression", "FeatureChainExpression", "INVALID_FEATURE_CHAIN_EXPRESSION_OPERATOR", ["INVALID_FEATURE_CHAIN_EXPRESSION_OPERATOR_MSG"], "error",
     "the expression", "operator is not '.'", ["owned"], "Kernel expressions", "B1")
rule(K, "checkFeatureReferenceExpression", "FeatureReferenceExpression", "INVALID_FEATURE_REFERENCE_EXPRESSION_REFERENT_IS_FEATURE", ["INVALID_FEATURE_REFERENCE_EXPRESSION_REFERENT_IS_FEATURE_MSG"], "error",
     "the expression", "the referent is not a Feature", ["owned"], "Kernel expressions", "B1")
rule(K, "checkFeatureReferenceExpression", "FeatureReferenceExpression", "INVALID_FEATURE_REFERENCE_EXPRESSION_RESULT", ["INVALID_FEATURE_REFERENCE_EXPRESSION_RESULT"], "error",
     "the expression", "no owned result parameter", ["owned", "implicit"], "Kernel expressions", "B1",
     "The message is the code ('validateFeatureReferenceExpressionResult'): the Pilot passes the code constant as message. The result is implicit (constructed).")
rule(K, "checkInstantiationExpression", "InstantiationExpression", "INVALID_INSTANTIATION_EXPRESSION_INSTANTIATED_TYPE", ["INVALID_INSTANTIATION_EXPRESSION_INSTANTIATED_TYPE_MSG"], "error",
     "the expression", "no instantiated type", ["owned"], "Kernel expressions", "B1")
rule(K, "checkInstantiationExpression", "InstantiationExpression", "INVALID_INSTANTIATION_EXPRESSION_RESULT", ["INVALID_INSTANTIATION_EXPRESSION_RESULT"], "error",
     "the expression", "no owned result parameter", ["owned", "implicit"], "Kernel expressions", "B1", "The message is the code, as above.")
rule(K, "checkConstructionExpression", "ConstructorExpression", "INVALID_CONSTRUCTOR_EXPRESSION_RESULT_FEATURE_REDEFINITION", ["INVALID_CONSTRUCTOR_EXPRESSION_RESULT_FEATURE_REDEFINITION_MSG"], "error",
     "the result's feature", "an input-directed feature of the result does not redefine exactly one public feature (inherited included) of the instantiated type",
     ["derived", "implicit"], "Kernel expressions", "B4")
rule(K, "checkConstructionExpression", "ConstructorExpression", "INVALID_CONSTRUCTOR_EXPRESSION_NO_DUPLICATE_FEATURE_REDEFINITION", ["INVALID_CONSTRUCTOR_EXPRESSION_NO_DUPLICATE_FEATURE_REDEFINITION_MSG"], "error",
     "the result's feature", "two input-directed result features redefine the same feature of the instantiated type", ["derived", "implicit"], "Kernel expressions", "B4")
rule(K, "checkConstructionExpression", "ConstructorExpression", "INVALID_CONSTRUCTOR_EXPRESSION_OWNED_FEATURES", ["INVALID_CONSTRUCTOR_EXPRESSION_OWNED_FEATURES_MSG"], "error",
     "the owned feature", "the constructor expression owns a feature other than its result", ["owned"], "Kernel expressions", "B1")
rule(K, "checkInvocationExpression", "InvocationExpression", "INVALID_INVOCATION_EXPRESSION_INSTANTIATED_TYPE", ["INVALID_INVOCATION_EXPRESSION_INSTANTIATED_TYPE_MSG"], "error",
     "the expression", "the instantiated type is neither a Behavior nor a Feature typed by exactly one Behavior", ["derived"], "Kernel expressions", "B4",
     "When it fires, the two parameter rules and the owned-feature rule below are skipped.")
rule(K, "checkInvocationExpression", "InvocationExpression", "INVALID_INVOCATION_EXPRESSION_OWNED_FEATURES", ["INVALID_INVOCATION_EXPRESSION_OWNED_FEATURES_MSG"], "error",
     "the owned feature", "an owned feature other than the result whose direction is not in", ["owned", "defaults"], "Kernel expressions", "B1")
rule(K, "checkInvocationExpression", "InvocationExpression", "INVALID_INVOCATION_EXPRESSION_PARAMETER_REDEFINITION", ["INVALID_INVOCATION_EXPRESSION_PARAMETER_REDEFINITION_MSG"], "error",
     "the argument feature", "an input-directed owned feature does not redefine exactly one input parameter (inherited included) of the invoked type",
     ["derived", "implicit"], "Kernel expressions", "B4")
rule(K, "checkInvocationExpression", "InvocationExpression", "INVALID_INVOCATION_EXPRESSION_NO_DUPLICATE_PARAMETER_REDEFINITION", ["INVALID_INVOCATION_EXPRESSION_NO_DUPLICATE_PARAMETER_REDEFINITION_MSG"], "error",
     "the argument feature", "two input-directed owned features redefine the same input parameter", ["derived", "implicit"], "Kernel expressions", "B4")
rule(K, "checkOperatorExpression", "OperatorExpression", "INVALID_OPERATOR_EXPRESSION_CAST_CONFORMANCE_TYPE", ["INVALID_OPERATOR_EXPRESSION_CAST_CONFORMANCE_MSG"], "warning",
     "the expression", "an 'as' cast whose argument result types and target types do not conform either way",
     ["derived", "typing"], "Kernel expressions", "B6")
rule(K, "checkOperatorExpression", "OperatorExpression", "INVALID_OPERATOR_EXPRESSION_BRACKET_OPERATOR", ["INVALID_OPERATOR_EXPRESSION_BRACKET_OPERATOR_MSG"], "warning",
     "the expression", "the operator is '[' (KerML only: the SysML override replaces this with the quantity check)", ["owned"], "Kernel expressions", "B1", "Not in the spec.")
rule(K, "checkSelectExpression", "SelectExpression", "INVALID_SELECT_EXPRESSION_OPERATOR", ["INVALID_SELECT_EXPRESSION_OPERATOR_MSG"], "error",
     "the expression", "operator is not 'select'", ["owned"], "Kernel expressions", "B1")
rule(K, "checkIndexExpression", "IndexExpression", "INVALID_INDEX_EXPRESSION_OPERATOR", ["INVALID_INDEX_EXPRESSION_OPERATOR_MSG"], "error",
     "the expression", "operator is not '#'", ["owned"], "Kernel expressions", "B1")
# ---- KerML: flows ---------------------------------------------------------------------------
rule(K, "checkFlow", "Flow", "INVALID_FLOW_ITEM_FEATURE", ["INVALID_FLOW_ITEM_FEATURE_MSG"], "error",
     "the second and later payload features", "the flow owns more than one PayloadFeature", ["owned"], "Kernel flows", "B1")
rule(K, "checkFlowEnd", "FlowEnd", "INVALID_FLOW_END_IS_END", ["INVALID_FLOW_END_IS_END_MSG"], "error",
     "the flow end", "the flow end is not an end feature", ["owned", "defaults"], "Kernel flows", "B1")
rule(K, "checkFlowEnd", "FlowEnd", "INVALID_FLOW_END_NESTED_FEATURE", ["INVALID_FLOW_END_NESTED_FEATURE_MSG"], "error",
     "the flow end", "the flow end does not own exactly one feature", ["owned"], "Kernel flows", "B1")
rule(K, "checkFlowEnd", "FlowEnd", "INVALID_FLOW_END_OWNING_TYPE", ["INVALID_FLOW_END_OWNING_TYPE_MSG"], "error",
     "the flow end", "the owning type is not a Flow", ["owned"], "Kernel flows", "B1")
rule(K, "checkFlowEnd", "FlowEnd", "INVALID_FLOW_END_SUBSETTING", ["INVALID_FLOW_END_SUBSETTING_MSG"], "error",
     "the flow end", "the flow end subsets (other than by redefinition, implicit included) no feature", ["derived", "implicit"], "Kernel flows", "B4")
rule(K, "checkFlowEnd", "FlowEnd", "INVALID_FLOW_END_IMPLICIT_SUBSETTING", ["INVALID_FLOW_END_IMPLICIT_SUBSETTING_MSG"], "warning",
     "the flow end", "it subsets something but owns no Subsetting, and its first owned feature owns a Redefinition (the old 'end redefines' notation)", ["derived", "implicit"], "Kernel flows", "B4")
# ---- KerML: values, multiplicities, metadata, filters, library packages ---------------------
rule(K, "checkFeatureValue", "FeatureValue", "INVALID_FEATURE_VALUE_OVERRIDING", ["INVALID_FEATURE_VALUE_OVERRIDING_MSG"], "error",
     "the feature value", "a feature the valued feature (transitively) redefines has a different, non-default (binding) feature value", ["derived", "implicit"], "Kernel values", "B4")
rule(K, "checkFeatureValue", "FeatureValue", "INVALID_FEATURE_VALUE_IS_INITIAL", ["INVALID_FEATURE_VALUE_IS_INITIAL_MSG"], "error",
     "the feature value", "an initial value (:=) on a feature that is not variable", ["owned"], "Kernel values", "B1", "No null check on the feature.")
rule(K, "checkMultiplicityRange", "MultiplicityRange", "INVALID_MULTIPLICITY_RANGE_BOUND_RESULT_TYPES", ["INVALID_MULTIPLICITY_RANGE_BOUND_RESULT_TYPES_MSG"], "error",
     "the bound expression", "a model-level evaluable bound evaluates to no natural (valueOf = -2), or a non-evaluable bound is not Integer-typed (literal, result specializing ScalarValues::Integer, or an integer operator over Integer arguments)",
     ["evaluation", "typing", "library"], "Kernel values", "B6")
rule(K, "checkMultiplicityRange", "MultiplicityRange", "INVALID_MULTIPLICITY_RANGE_BOUNDS", ["INVALID_MULTIPLICITY_RANGE_BOUNDS_MSG"], "error",
     "the multiplicity range", "the bound expressions are not the first one or two owned members (lower, upper)", ["owned"], "Kernel values", "B1")
rule(K, "checkMetadataFeature", "MetadataFeature", "INVALID_METADATA_FEATURE_METACLASS", ["INVALID_METADATA_FEATURE_METACLASS_MSG"], "error",
     "the metadata feature", "it does not have exactly one type that is a Metaclass", ["derived"], "Kernel metadata", "B3", "Code 'validateMetadataFeatureMetadata'.")
rule(K, "checkMetadataFeature", "MetadataFeature", "INVALID_METADATA_FEATURE_METACLASS_NOT_ABSTRACT", ["INVALID_METADATA_FEATURE_METACLASS_NOT_ABSTRACT_MSG"], "error",
     "the metadata feature", "one of its types is abstract", ["derived"], "Kernel metadata", "B3")
rule(K, "checkMetadataFeature", "MetadataFeature", "INVALID_METADATA_FEATURE_ANNOTATED_ELEMENT", ["INVALID_METADATA_FEATURE_ANNOTATED_ELEMENT_MSG"], "error",
     "the metadata feature", "the metaclass of an annotated element (from the KerML/SysML reflective library) does not specialize all types of any (non-abstract, if any) feature of the metadata feature subsetting Metaobjects::Metaobject::annotatedElement; {metaclass} is its declared name",
     ["derived", "implicit", "library"], "Kernel metadata", "B4")
rule(K, "checkMetadataFeature", "MetadataFeature", "INVALID_METADATA_FEATURE_BODY", ["INVALID_METADATA_FEATURE_BODY_MSG_1", "INVALID_METADATA_FEATURE_BODY_MSG_2"], "error",
     "the body feature (msg 1) or its FeatureValue, feature value (msg 2)", "recursively for owned features of the metadata body: a feature that redefines no feature of a supertype of its owner (msg 1), or whose value is not model-level evaluable (msg 2)",
     ["derived", "evaluation"], "Kernel metadata", "B6", "Code 'invalidateMetadataFeatureBody'.")
rule(K, "checkElementFilterMembership", "ElementFilterMembership", "INVALID_ELEMENT_FILTER_MEMBERSHIP_IS_MODEL_LEVEL_EVALUABLE", ["INVALID_ELEMENT_FILTER_MEMBERSHIP_IS_MODEL_LEVEL_EVALUABLE_MSG"], "error",
     "the membership, feature condition", "the filter condition is not model-level evaluable", ["evaluation"], "Kernel metadata", "B6")
rule(K, "checkElementFilterMembership", "ElementFilterMembership", "INVALID_ELEMENT_FILTER_MEMBERSHIP_IS_BOOLEAN", ["INVALID_ELEMENT_FILTER_MEMBERSHIP_IS_BOOLEAN_MSG"], "error",
     "the membership, feature condition", "an evaluable condition that is not Boolean-typed (result specializing ScalarValues::Boolean, LiteralBoolean, or not/xor/&/| over Boolean arguments)",
     ["typing", "library", "evaluation"], "Kernel metadata", "B6")
rule(K, "checkLibraryPackage", "LibraryPackage", "INVALID_LIBRARY_PACKAGE_NOT_STANDARD", ["INVALID_LIBRARY_PACKAGE_NOT_STANDARD_MSG"], "warning",
     "the package, feature isStandard", "a standard library package in a resource that is not a library resource", ["owned", "resource"], "Root", "B1", "Not formalized in the spec.")

# ---- SysML ---------------------------------------------------------------------------------------
rule(S, "checkDefinition", "Definition", "INVALID_DEFINITION_VARIATION_MEMBERSHIP", ["INVALID_DEFINITION_VARIATION_MEMBERSHIP_MSG"], "error",
     "the feature membership", "a variation definition owns a FeatureMembership (owned usages must be variants)", ["owned"], "SysML definitions and usages", "B2")
rule(S, "checkDefinition", "Definition", "INVALID_DEFINITION_VARIATION_SPECIALIZATION", ["INVALID_DEFINITION_VARIATION_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "a variation definition specializes a variation", ["owned"], "SysML definitions and usages", "B2")
rule(S, "checkReferenceUsage", "ReferenceUsage", "INVALID_ATTRIBUTE_USAGE_IS_REFERENTIAL", ["INVALID_ATTRIBUTE_USAGE_IS_REFERENTIAL_MSG"], "error",
     "the usage", "a reference usage that is not referential (isReference = not isComposite)", ["owned", "defaults"], "SysML definitions and usages", "B2",
     "Reported with the attribute-usage code and message ('validateAttributeUsageIsReferential', 'An attribute usage must be referential.').")
rule(S, "checkUsage", "Usage", "INVALID_USAGE_TYPE", ["INVALID_USAGE_TYPE_MSG"], "error",
     "the usage, feature definition", "for usages other than attribute and occurrence usages: a type (getAllTypesOf) is not a Classifier", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkUsage", "Usage", "INVALID_USAGE_IS_REFERENTIAL",
     ["INVALID_USAGE_IS_REFERENTIAL_MSG_1", "INVALID_USAGE_IS_REFERENTIAL_MSG_2", "INVALID_USAGE_IS_REFERENTIAL_MSG_3"], "error",
     "the usage", "a composite usage that is directed (msg 1), an end (msg 2), or has no featuring types (package level, msg 3)", ["derived", "defaults"], "SysML definitions and usages", "B4")
rule(S, "checkUsage", "Usage", "INVALID_USAGE_VARIATION_IS_ABSTRACT", ["INVALID_USAGE_VARIATION_IS_ABSTRACT_MSG"], "error",
     "the usage", "a variation usage that is not abstract", ["owned", "defaults"], "SysML definitions and usages", "B2", "The Pilot makes variations abstract by default.")
rule(S, "checkUsage", "Usage", "INVALID_USAGE_VARIATION_MEMBERSHIP", ["INVALID_USAGE_VARIATION_MEMBERSHIP_MSG"], "error",
     "the feature membership", "a variation usage owns a FeatureMembership other than a parameter or objective membership", ["owned"], "SysML definitions and usages", "B2")
rule(S, "checkUsage", "Usage", "INVALID_USAGE_VARIATION_SPECIALIZATION", ["INVALID_USAGE_VARIATION_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "a variation usage specializes a variation", ["owned"], "SysML definitions and usages", "B2")
rule(S, "checkVariantMembership", "VariantMembership", "INVALID_VARIANT_MEMBERSHIP_OWNING_NAMESPACE", ["INVALID_VARIANT_MEMBERSHIP_OWNING_NAMESPACE_MSG"], "error",
     "the membership", "the owning namespace is not a variation definition or usage", ["owned"], "SysML definitions and usages", "B2", "Code spelled 'validateVariationMembershipOwningNamespace'.")
rule(S, "checkAttributeDefinition", "AttributeDefinition", "INVALID_ATTRIBUTE_DEFINITION_FEATURES", ["INVALID_ATTRIBUTE_DEFINITION_FEATURES_MSG"], "error",
     "the owned feature", "an owned feature is composite", ["owned", "defaults"], "SysML definitions and usages", "B2")
rule(S, "checkAttributeUsage", "AttributeUsage", "INVALID_ATTRIBUTE_USAGE_TYPE", ["INVALID_ATTRIBUTE_USAGE_MSG"], "error",
     "the usage, feature attributeDefinition", "a type is not a DataType", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkAttributeUsage", "AttributeUsage", "INVALID_ATTRIBUTE_USAGE_ENUMERATION_TYPE", ["INVALID_ATTRIBUTE_USAGE_ENUMERATION_TYPE_MSG"], "error",
     "the usage, feature attributeDefinition", "a non-enumeration attribute usage with more than one type, one of them an EnumerationDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkAttributeUsage", "AttributeUsage", "INVALID_REFERENCE_USAGE_IS_REFERENCE", ["INVALID_REFERENCE_USAGE_IS_REFERENCE_MSG"], "error",
     "the usage", "the attribute usage is not referential", ["owned", "defaults"], "SysML definitions and usages", "B2",
     "Reported with the reference-usage code ('validateReferenceUsageIsReference'); the codes of this and checkReferenceUsage are swapped.")
rule(S, "checkAttributeUsage", "AttributeUsage", "INVALID_ATTRIBUTE_USAGE_FEATURES", ["INVALID_ATTRIBUTE_USAGE_FEATURES_MSG"], "error",
     "the owned feature", "an owned feature is composite", ["owned", "defaults"], "SysML definitions and usages", "B2")
rule(S, "checkEnumerationDefinition", "EnumerationDefinition", "INVALID_ENUMERATION_DEFINITION_IS_VARIATION", ["INVALID_ENUMERATION_DEFINITION_IS_VARIATION_MSG"], "error",
     "the definition", "isVariation is false", ["owned", "defaults"], "SysML definitions and usages", "B2", "isVariation defaults to true in EnumerationDefinitionImpl; unreachable from text.")
rule(S, "checkEnumerationUsage", "EnumerationUsage", "INVALID_ENUMERATION_USAGE_TYPE", ["INVALID_ENUMERATION_USAGE_TYPE_MSG"], "error",
     "the usage, feature enumerationDefinition", "not exactly one type, or that type is not an EnumerationDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkEventOccurrenceUsage", "EventOccurrenceUsage", "INVALID_EVENT_OCCURRENCE_USAGE_IS_REFERENCE",
     ["INVALID_EVENT_OCCURRENCE_USAGE_IS_REFERENCE_MSG_0", "INVALID_EVENT_OCCURRENCE_USAGE_IS_REFERENCE_MSG_1",
      "INVALID_EVENT_OCCURRENCE_USAGE_IS_REFERENCE_MSG_2", "INVALID_EVENT_OCCURRENCE_USAGE_IS_REFERENCE_MSG_3"], "error",
     "the usage", "the usage is composite; the message names the kind (event occurrence, exhibit state, include use case, perform action)", ["owned", "defaults"], "SysML definitions and usages", "B2")
rule(S, "checkEventOccurrenceUsage", "EventOccurrenceUsage", "INVALID_EVENT_OCCURRENCE_USAGE_REFERENCE", ["INVALID_EVENT_OCCURRENCE_USAGE_REFERENCE_MSG"], "error",
     "the ReferenceSubsetting", "(not perform action or include use case) the owned reference subsetting's referenced feature (last of a chain) is not an OccurrenceUsage", ["owned"], "SysML definitions and usages", "B2")
rule(S, "checkOccurrenceUsage", "OccurrenceUsage", "INVALID_OCCURRENCE_USAGE_TYPE", ["INVALID_OCCURRENCE_USAGE_TYPE_MSG"], "error",
     "the usage, feature occurrenceDefinition", "(not port, connection, metadata usages or steps) a type is not a Class", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkOccurrenceUsage", "OccurrenceUsage", "MSG:INVALID_OCCURRENCE_USAGE_INDIVIDUAL_DEFINITION_MSG", ["INVALID_OCCURRENCE_USAGE_INDIVIDUAL_DEFINITION_MSG"], "error",
     "the usage", "more than one occurrence definition is an individual definition", ["derived"], "SysML typing", "B3", "The code is the message text.")
rule(S, "checkOccurrenceUsage", "OccurrenceUsage", "INVALID_OCCURRENCE_USAGE_INDIVIDUAL_USAGE", ["INVALID_OCCURRENCE_USAGE_INDIVIDUAL_USAGE_MSG"], "error",
     "the usage, feature occurrenceDefinition", "an individual usage that does not have exactly one individual definition", ["derived"], "SysML typing", "B3")
rule(S, "checkOccurrenceUsage", "OccurrenceUsage", "MSG:INVALID_OCCURRENCE_USAGE_IS_PORTION_MSG", ["INVALID_OCCURRENCE_USAGE_IS_PORTION_MSG"], "error",
     "the usage", "a portion kind without isPortion", ["owned", "defaults"], "SysML definitions and usages", "B2", "The code is the message text.")
rule(S, "checkOccurrenceUsage", "OccurrenceUsage", "MSG:INVALID_OCCURRENCE_USAGE_PORTION_KIND_MSG", ["INVALID_OCCURRENCE_USAGE_PORTION_KIND_MSG"], "error",
     "the usage", "a portion kind on a usage whose owning type is not an occurrence definition or usage", ["owned"], "SysML definitions and usages", "B2", "The code is the message text.")
rule(S, "checkConjugatedPortDefinition", "ConjugatedPortDefinition", "INVALID_CONJUGATED_PORT_DEFINITION_CONJUGATED_PORT_DEFINITION", ["INVALID_CONJUGATED_PORT_DEFINITION_CONJUGATED_PORT_DEFINITION_MSG"], "error",
     "the conjugated port definition", "the conjugated port definition itself has a conjugated port definition", ["owned", "implicit"], "SysML ports and connections", "B2")
rule(S, "checkConjugatedPortDefinition", "ConjugatedPortDefinition", "INVALID_CONJUGATED_PORT_DEFINITION_ORIGINAL_PORT_DEFINITION", ["INVALID_CONJUGATED_PORT_DEFINITION_ORIGINAL_PORT_DEFINITION_MSG"], "error",
     "the conjugated port definition", "its port conjugator's original port definition is not its owning (original) port definition", ["owned", "implicit"], "SysML ports and connections", "B2",
     "Same code as the previous rule ('validateConjugatedPortDefinitionConjugatedPortDefinition').")
rule(S, "checkPortDefinition", "PortDefinition", "INVALID_PORT_DEFINITION_CONJUGATED_PORT_DEFINITION", ["INVALID_PORT_DEFINITION_CONJUGATED_PORT_DEFINITION_MSG"], "error",
     "the port definition", "a (non-conjugated) port definition does not own exactly one ConjugatedPortDefinition", ["owned", "implicit"], "SysML ports and connections", "B2",
     "The conjugated port definition is implicit (constructed), so this holds by construction.")
rule(S, "checkPortDefinition", "PortDefinition", "INVALID_PORT_DEFINITION_OWNED_USAGES_NOT_COMPOSITE", ["INVALID_PORT_DEFINITION_OWNED_USAGES_NOT_COMPOSITE_MSG"], "error",
     "the owned usage", "an owned usage other than a port is composite", ["owned", "defaults"], "SysML ports and connections", "B2")
rule(S, "checkPortUsage", "PortUsage", "INVALID_PORT_USAGE_TYPE", ["INVALID_PORT_USAGE_TYPE_MSG"], "error",
     "the usage, feature portDefinition", "a type is not a PortDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkPortUsage", "PortUsage", "INVALID_PORT_USAGE_IS_REFERENCE", ["INVALID_PORT_USAGE_IS_REFERENCE_MSG"], "error",
     "the usage", "a composite port usage whose owning type is not a port definition or usage", ["owned", "defaults"], "SysML ports and connections", "B2")
rule(S, "checkPortUsage", "PortUsage", "INVALID_PORT_USAGE_NESTED_USAGES_NOT_COMPOSITE", ["INVALID_PORT_USAGE_NESTED_USAGES_NOT_COMPOSITE_MSG"], "error",
     "the nested usage", "a nested usage other than a port is composite", ["owned", "defaults"], "SysML ports and connections", "B2")
rule(S, "checkConnectionUsage", "ConnectionUsage", "INVALID_CONNECTION_USAGE_TYPE", ["INVALID_CONNECTION_USAGE_TYPE_MSG"], "error",
     "the usage, feature connectionDefinition", "(not flow, interface, allocation usages) a type is not an Association", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkFlowDefinition", "FlowDefinition", "INVALID_FLOW_DEFINITION_END", ["INVALID_FLOW_DEFINITION_END_MSG"], "error",
     "the definition when it owns at most two ends, else the third and later ends", "more than two end features (inherited included)", ["derived"], "SysML ports and connections", "B4")
rule(S, "checkFlowUsage", "FlowUsage", "INVALID_FLOW_USAGE_TYPE", ["INVALID_FLOW_USAGE_TYPE_MSG"], "error",
     "the usage, feature flowDefinition", "a type is not an Interaction", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkInterfaceDefinitionEnds", "InterfaceDefinition", "INVALID_INTERFACE_DEFINITION_END", ["INVALID_INTERFACE_DEFINITION_END_MSG"], "error",
     "the end feature", "an owned end feature is not a PortUsage", ["owned"], "SysML ports and connections", "B2")
rule(S, "checkInterfaceUsageEnds", "InterfaceUsage", "INVALID_INTERFACE_USAGE_END", ["INVALID_INTERFACE_USAGE_END_MSG"], "error",
     "the end feature", "an owned end feature is not a PortUsage", ["owned"], "SysML ports and connections", "B2")
rule(S, "checkInterfaceUsage", "InterfaceUsage", "INVALID_INTERFACE_USAGE_TYPE", ["INVALID_INTERFACE_USAGE_TYPE_MSG"], "error",
     "the usage, feature interfaceDefinition", "a type is not an InterfaceDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkAllocationUsage", "AllocationUsage", "INVALID_ALLOCATION_USAGE_TYPE", ["INVALID_ALLOCATION_USAGE_TYPE_MSG"], "error",
     "the usage, feature allocationDefinition", "a type is not an AllocationDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkAcceptActionUsage", "AcceptActionUsage", "INVALID_ACCEPT_ACTION_USAGE_PARAMETERS", ["INVALID_ACCEPT_ACTION_USAGE_PARAMETERS_MSG"], "error",
     "the usage", "no input parameters (inherited included)", ["derived", "implicit"], "SysML actions and states", "B4")
rule(S, "checkActionUsage", "ActionUsage", "INVALID_ACTION_USAGE_TYPE", ["INVALID_ACTION_USAGE_TYPE_MSG"], "error",
     "the usage, feature actionDefinition", "(not state, calculation, flow usages) a type is not a Behavior", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkAssignmentActionUsage", "AssignmentActionUsage", "INVALID_ASSIGNMENT_ACTION_USAGE_REFERENT", ["INVALID_ASSIGNMENT_ACTION_USAGE_REFERENT_MSG"], "error",
     "the second owned relationship", "no owned non-feature membership whose member is a Feature (and not a metadata feature)", ["owned"], "SysML actions and states", "B2")
rule(S, "checkAssignmentActionUsage", "AssignmentActionUsage", "INVALID_ASSIGNMENT_ACTION_USAGE_REFERENT_IS_TIME_VARYING", ["INVALID_ASSIGNMENT_ACTION_USAGE_REFERENT_IS_TIME_VARYING_MSG"], "error",
     "the second owned relationship", "the resolved referent's target feature (last of a chain) is not variable", ["owned"], "SysML actions and states", "B2")
rule(S, "checkTriggerInvocationExpression", "TriggerInvocationExpression", "INVALID_TRIGGER_INVOCATION_EXPRESSION_AFTER_ARGUMENT", ["INVALID_TRIGGER_INVOCATION_EXPRESSION_AFTER_ARGUMENT_MSG"], "error",
     "the expression", "kind 'after' and the argument is not a duration (a [unit] literal with an ISQBase::DurationUnit, a result specializing ISQBase::DurationValue, or a quantity operator over durations/times)",
     ["typing", "library"], "SysML actions and states", "B6")
rule(S, "checkTriggerInvocationExpression", "TriggerInvocationExpression", "INVALID_TRIGGER_INVOCATION_EXPRESSION_AT_ARGUMENT", ["INVALID_TRIGGER_INVOCATION_EXPRESSION_AT_ARGUMENT_MSG"], "error",
     "the expression", "kind 'at' and the argument is not a time instant (result specializing Time::TimeInstantValue, or a quantity operator over durations/times)",
     ["typing", "library"], "SysML actions and states", "B6")
rule(S, "checkTriggerInvocationExpression", "TriggerInvocationExpression", "INVALID_TRIGGER_INVOCATION_EXPRESSION_WHEN_ARGUMENT", ["INVALID_TRIGGER_INVOCATION_EXPRESSION_WHEN_ARGUMENT_MSG"], "error",
     "the expression", "kind 'when' and the argument is not a Boolean expression (result specializing Performances::BooleanEvaluation, or a reference to a Boolean expression)",
     ["typing", "library"], "SysML actions and states", "B6")
rule(S, "checkControlNode", "ControlNode", "INVALID_CONTROL_NODE_OWNING_TYPE", ["INVALID_CONTROL_NODE_OWNING_TYPE_MSG"], "error",
     "the node", "the owning type is not an action usage or definition", ["owned"], "SysML actions and states", "B2")
rule(S, "checkPerformActionUsage", "PerformActionUsage", "INVALID_PERFORM_ACTION_USAGE_REFERENCE", ["INVALID_PERFORM_ACTION_USAGE_REFERENCE_MSG"], "error",
     "the ReferenceSubsetting", "(not exhibit state or include use case) the referenced feature (last of a chain) is not an ActionUsage", ["owned"], "SysML actions and states", "B2")
rule(S, "checkSendActionUsage", "SendActionUsage", "INVALID_SEND_ACTION_USAGE_RECEIVER", ["INVALID_SEND_ACTION_USAGE_RECEIVER_MSG"], "warning",
     "the receiver argument", "the receiver ('to') argument references a port usage directly or as the last feature of a chain", ["owned"], "SysML actions and states", "B2")
rule(S, "checkSendActionUsage", "SendActionUsage", "MSG:INVALID_SEND_ACTION_USAGE_PAYLOAD_ARGUMENT_MSG", ["INVALID_SEND_ACTION_USAGE_PAYLOAD_ARGUMENT_MSG"], "error",
     "the usage", "a send action owned by a state subaction or transition feature membership without a payload argument", ["owned"], "SysML actions and states", "B2", "The code is the message text.")
rule(S, "checkForLoopActionUsage", "ForLoopActionUsage", "INVALID_FOR_LOOP_ACTION_USAGE_LOOP_VARIABLE", ["INVALID_FOR_LOOP_ACTION_USAGE_LOOP_VARIABLE_MSG"], "error",
     "the usage", "the first owned feature is not a ReferenceUsage", ["owned"], "SysML actions and states", "B2")
rule(S, "checkForLoopActionUsage", "ForLoopActionUsage", "INVALID_FOR_LOOP_ACTION_USAGE_PARAMETERS", ["INVALID_FOR_LOOP_ACTION_USAGE_PARAMETERS_MSG"], "error",
     "the usage", "not exactly two input parameters (inherited included)", ["derived", "implicit"], "SysML actions and states", "B4")
rule(S, "checkIfActionUsage", "IfActionUsage", "INVALID_IF_ACTION_USAGE_PARAMETERS", ["INVALID_IF_ACTION_USAGE_PARAMETERS_MSG"], "error",
     "the usage", "fewer than two input parameters", ["derived", "implicit"], "SysML actions and states", "B4")
rule(S, "checkWhileLoopActionUsage", "WhileLoopActionUsage", "INVALID_WHILE_LOOP_ACTION_USAGE_PARAMETERS", ["INVALID_WHILE_LOOP_ACTION_USAGE_PARAMETERS_MSG"], "error",
     "the usage", "fewer than two input parameters", ["derived", "implicit"], "SysML actions and states", "B4")
rule(S, "checkExhibitStateUsage", "ExhibitStateUsage", "INVALID_EXHIBIT_STATE_USAGE_REFERENCE", ["INVALID_EXHIBIT_STATE_USAGE_REFERENCE_MSG"], "error",
     "the ReferenceSubsetting", "the referenced feature (last of a chain) is not a StateUsage", ["owned"], "SysML actions and states", "B2")
rule(S, "checkStateDefinition", "StateDefinition", "INVALID_STATE_DEFINITION_SUBACTION_KIND",
     ["INVALID_STATE_SUBACTION_KIND_ENTRY_MSG", "INVALID_STATE_SUBACTION_KIND_DO_MSG", "INVALID_STATE_SUBACTION_KIND_EXIT_MSG"], "error",
     "the second and later owned subaction memberships of that kind", "more than one owned entry, do or exit subaction membership", ["owned"], "SysML actions and states", "B2")
rule(S, "checkStateSubactionMembership", "StateSubactionMembership", "INVALID_STATE_SUBACTION_MEMBERSHIP_OWNING_TYPE", ["INVALID_STATE_SUBACTION_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership", "the owning type is not a state usage or definition", ["owned"], "SysML actions and states", "B2", "Code spelled 'validateStateSubactionMembershioOwningType'.")
rule(S, "checkStateUsage", "StateUsage", "INVALID_STATE_USAGE_TYPE", ["INVALID_STATE_USAGE_TYPE_MSG"], "error",
     "the usage, feature stateDefinition", "a type is not a Behavior", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkStateUsage", "StateUsage", "INVALID_STATE_USAGE_SUBACTION_KIND",
     ["INVALID_STATE_SUBACTION_KIND_ENTRY_MSG", "INVALID_STATE_SUBACTION_KIND_DO_MSG", "INVALID_STATE_SUBACTION_KIND_EXIT_MSG"], "error",
     "the second and later owned subaction memberships of that kind", "more than one owned entry, do or exit subaction membership", ["owned"], "SysML actions and states", "B2")
rule(S, "checkTransitionFeatureMembership", "TransitionFeatureMembership", "INVALID_TRANSITION_FEATURE_MEMBERSHIP_EFFECT_ACTION", ["INVALID_TRANSITION_FEATURE_MEMBERSHIP_EFFECT_ACTION_MSG"], "error",
     "the membership", "an effect membership whose feature is not an ActionUsage", ["owned"], "SysML actions and states", "B2")
rule(S, "checkTransitionFeatureMembership", "TransitionFeatureMembership", "INVALID_TRANSITION_FEATURE_MEMBERSHIP_GUARD_EXPRESSION", ["INVALID_TRANSITION_FEATURE_MEMBERSHIP_GUARD_EXPRESSION_MSG"], "error",
     "the membership", "a guard membership whose feature is not a Boolean-typed expression (isBoolean)", ["typing", "library"], "SysML actions and states", "B6")
rule(S, "checkTransitionFeatureMembership", "TransitionFeatureMembership", "INVALID_TRANSITION_FEATURE_MEMBERSHIP_TRIGGER_ACTION", ["INVALID_TRANSITION_FEATURE_MEMBERSHIP_TRIGGER_ACTION_MSG"], "error",
     "the membership", "a trigger membership whose feature is not an AcceptActionUsage", ["owned"], "SysML actions and states", "B2")
rule(S, "checkTransitionFeatureMembership", "TransitionFeatureMembership", "INVALID_TRANSITION_FEATURE_MEMBERSHIP_OWNING_TYPE", ["INVALID_TRANSITION_FEATURE_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership", "the owning type is not a TransitionUsage", ["owned"], "SysML actions and states", "B2")
rule(S, "checkTransitionUsage", "TransitionUsage", "MSG:INVALID_STATE_USAGE_PARALLEL_SUBACTIONS_MSG",
     ["INVALID_STATE_DEFINITION_PARALLEL_SUBACTIONS_MSG", "INVALID_STATE_USAGE_PARALLEL_SUBACTIONS_MSG"], "error",
     "the transition", "the owning type is a parallel state definition or usage", ["owned"], "SysML actions and states", "B2",
     "The code is the message text (both constants have the same text).")
rule(S, "checkTransitionUsage", "TransitionUsage", "INVALID_TRANSITION_USAGE_PARAMETERS",
     ["INVALID_TRANSITION_USAGE_PARAMETERS_MSG_1", "INVALID_TRANSITION_USAGE_PARAMETERS_MSG_2"], "error",
     "the transition", "no trigger and no input parameter (msg 1), or a trigger and fewer than two input parameters (msg 2)", ["derived", "implicit"], "SysML actions and states", "B4")
rule(S, "checkTransitionUsage", "TransitionUsage", "INVALID_TRANSITION_USAGE_SUCCESSION", ["INVALID_TRANSITION_USAGE_SUCCESSION_MSG"], "error",
     "the transition", "no owned Succession member, or the first one targets a feature whose basic feature is not an ActionUsage", ["owned", "implicit"], "SysML actions and states", "B2")
rule(S, "checkTransitionUsage", "TransitionUsage", "INVALID_TRANSITION_USAGE_TRIGGER_ACTIONS", ["INVALID_TRANSITION_USAGE_TRIGGER_ACTIONS_MSG"], "error",
     "the trigger membership", "a transition with a trigger whose source is not a StateUsage", ["derived"], "SysML actions and states", "B4")
rule(S, "checkSuccession", "Succession", "MSG:INVALID_STATE_USAGE_PARALLEL_SUBACTIONS_MSG", ["INVALID_STATE_USAGE_PARALLEL_SUBACTIONS_MSG"], "error",
     "the succession", "the owning type is a parallel state definition or usage", ["owned"], "SysML actions and states", "B2", "Same code as the transition variant.")
rule(S, "checkCalculationUsage", "CalculationUsage", "INVALID_CALCULATION_USAGE_TYPE", ["INVALID_CALCULATION_USAGE_TYPE_MSG"], "error",
     "the usage, feature calculationDefinition", "(not case usages) not exactly one type, or it is not a Function", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkAssertConstraintUsage", "AssertConstraintUsage", "INVALID_ASSERT_CONSTRAINT_USAGE_REFERENCE", ["INVALID_ASSERT_CONSTRAINT_USAGE_REFERENCE_MSG"], "error",
     "the ReferenceSubsetting", "the referenced feature (last of a chain) is not a ConstraintUsage", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkConstraintUsage", "ConstraintUsage", "INVALID_CONSTRAINT_USAGE_TYPE", ["INVALID_CONSTRAINT_USAGE_TYPE_MSG"], "error",
     "the usage, feature constraintDefinition", "(not requirement usages) not exactly one type, or it is not a Predicate", ["derived", "implicit"], "SysML typing", "B3",
     "Code 'Invalid Constraint Usage - invalid type'.")
for name, owners in (("ACTOR", "a requirement or case definition or usage"), ("STAKEHOLDER", "a requirement definition or usage"),
                     ("SUBJECT", "a requirement or case definition or usage")):
    rule(S, f"check{name.capitalize()}Membership", f"{name.capitalize()}Membership", f"INVALID_{name}_MEMBERSHIP_OWNING_TYPE", [f"INVALID_{name}_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
         "the membership", f"the owning type is not {owners}", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkFramedConcernUsage", "FramedConcernMembership", "INVALID_FRAMED_CONCERN_MEMBERSHIP_CONSTRAINT_KIND", ["INVALID_FRAMED_CONCERN_MEMBERSHIP_CONSTRAINT_KIND_MSG"], "error",
     "the membership", "kind is not requirement", ["owned", "defaults"], "SysML requirements and cases", "B2", "kind defaults to requirement; unreachable from text.")
rule(S, "checkRequirementConstraintMembership", "RequirementConstraintMembership", "INVALID_REQUIREMENT_CONSTRAINT_MEMBERSHIP_IS_COMPOSITE", ["INVALID_REQUIREMENT_CONSTRAINT_MEMBERSHIP_IS_COMPOSITE_MSG"], "error",
     "the membership", "the owned constraint is not composite", ["owned", "defaults"], "SysML requirements and cases", "B2")
rule(S, "checkRequirementConstraintMembership", "RequirementConstraintMembership", "INVALID_REQUIREMENT_CONSTRAINT_MEMBERSHIP_OWNING_TYPE", ["INVALID_REQUIREMENT_CONSTRAINT_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership", "the owning type is not a requirement definition or usage", ["owned"], "SysML requirements and cases", "B2")
for owner, lang_metaclass in (("REQUIREMENT_DEFINITION", "RequirementDefinition"), ("REQUIREMENT_USAGE", "RequirementUsage"),
                              ("CASE_DEFINITION", "CaseDefinition"), ("CASE_USAGE", "CaseUsage")):
    method = "check" + lang_metaclass
    if owner.startswith("CASE"):
        rule(S, method, lang_metaclass, f"INVALID_{owner}_ONLY_ONE_OBJECTIVE", [f"INVALID_{owner}_ONLY_ONE_OBJECTIVE_MSG"], "error",
             "the owned objective memberships (all but the first when all are owned), else the type", "more than one objective membership among all feature memberships (inherited included)",
             ["derived", "implicit"], "SysML requirements and cases", "B4")
    rule(S, method, lang_metaclass, f"INVALID_{owner}_ONLY_ONE_SUBJECT", [f"INVALID_{owner}_ONLY_ONE_SUBJECT_MSG"], "error",
         "the owned subject memberships (all but the first when all are owned), else the type", "more than one subject membership among all feature memberships (inherited included)",
         ["derived", "implicit"], "SysML requirements and cases", "B4")
    rule(S, method, lang_metaclass, f"INVALID_{owner}_SUBJECT_PARAMETER_POSITION", [f"INVALID_{owner}_SUBJECT_PARAMETER_POSITION_MSG"], "error",
         "the owned subject parameter, else the type", "no inputs, or the first input (inherited included) is not a subject parameter",
         ["derived", "implicit"], "SysML requirements and cases", "B4")
rule(S, "checkRequirementUsage", "RequirementUsage", "INVALID_REQUIREMENT_USAGE_TYPE", ["INVALID_REQUIREMENT_USAGE_TYPE_MSG"], "error",
     "the usage, feature requirementDefinition", "not exactly one type, or it is not a RequirementDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkCaseUsage", "CaseUsage", "INVALID_CASE_USAGE_TYPE", ["INVALID_CASE_USAGE_TYPE_MSG"], "error",
     "the usage, feature caseDefinition", "(not analysis, verification or use case usages) not exactly one type, or it is not a CaseDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkSatisfyRequirementUsage", "SatisfyRequirementUsage", "INVALID_SATISFY_REQUIREMENT_USAGE_REFERENCE", ["INVALID_SATISFY_REQUIREMENT_USAGE_REFERENCE_MSG"], "error",
     "the ReferenceSubsetting", "the referenced feature (last of a chain) is not a RequirementUsage", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkObjectiveMembership", "ObjectiveMembership", "INVALID_OBJECTIVE_MEMBERSHIP_IS_COMPOSITE", ["INVALID_OBJECTIVE_MEMBERSHIP_IS_COMPOSITE_MSG"], "error",
     "the membership", "the owned objective requirement is not composite", ["owned", "defaults"], "SysML requirements and cases", "B2")
rule(S, "checkObjectiveMembership", "ObjectiveMembership", "INVALID_OBJECTIVE_MEMBERSHIP_OWNING_TYPE", ["INVALID_OBJECTIVE_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership", "the owning type is not a case definition or usage", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkAnalysisCaseUsage", "AnalysisCaseUsage", "INVALID_ANALYSIS_CASE_USAGE_TYPE", ["INVALID_ANALYSIS_CASE_USAGE_TYPE_MSG"], "error",
     "the usage, feature analysisCaseDefinition", "not exactly one type, or it is not an AnalysisCaseDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkRequirementVerificationMembership", "RequirementVerificationMembership", "INVALID_REQUIREMENT_VERIFICATION_MEMBERSHIP_KIND", ["INVALID_REQUIREMENT_VERIFICATION_MEMBERSHIP_KIND_MSG"], "error",
     "the membership", "kind is not requirement", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkRequirementVerificationMembership", "RequirementVerificationMembership", "INVALID_REQUIREMENT_VERIFICATION_MEMBERSHIP_OWNING_TYPE", ["INVALID_REQUIREMENT_VERIFICATION_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership", "the owning type is not an objective requirement of a verification case definition or usage", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkVerificationCaseUsage", "VerificationCaseUsage", "INVALID_VERIFICATION_CASE_USAGE_TYPE", ["INVALID_VERIFICATION_CASE_USAGE_TYPE_MSG"], "error",
     "the usage, feature verificationCaseDefinition", "not exactly one type, or it is not a VerificationCaseDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkIncludeUseCaseUsage", "IncludeUseCaseUsage", "INVALID_INCLUDE_USE_CASE_USAGE_REFERENCE", ["INVALID_INCLUDE_USE_CASE_USAGE_REFERENCE_MSG"], "error",
     "the ReferenceSubsetting", "the referenced feature (last of a chain) is not a UseCaseUsage", ["owned"], "SysML requirements and cases", "B2")
rule(S, "checkUseCaseUsage", "UseCaseUsage", "INVALID_USE_CASE_USAGE_TYPE", ["INVALID_USE_CASE_USAGE_TYPE_MSG"], "error",
     "the usage, feature useCaseDefinition", "not exactly one type, or it is not a UseCaseDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkExpose", "Expose", "INVALID_EXPOSE_IS_IMPORT_ALL", ["INVALID_EXPOSE_IS_IMPORT_ALL_MSG"], "error",
     "the expose", "isImportAll is false", ["owned", "defaults"], "SysML views and metadata", "B2", "The Pilot sets isImportAll on every expose; unreachable from text.")
rule(S, "checkExpose", "Expose", "INVALID_VARIANT_MEMBERSHIP_OWNING_NAMESPACE", ["INVALID_VARIANT_MEMBERSHIP_OWNING_NAMESPACE_MSG"], "error",
     "the expose", "the importing namespace is not a ViewUsage (validateExposeOwningNamespace)", ["owned"], "SysML views and metadata", "B2",
     "Reported with the variant-membership code and message ('validateVariationMembershipOwningNamespace', 'A variant must be an owned member of a variation.'); the EXPOSE_OWNING_NAMESPACE constants are never used.")
rule(S, "checkRenderingUsage", "RenderingUsage", "INVALID_RENDERING_USAGE_TYPE", ["INVALID_RENDERING_USAGE_TYPE_MSG"], "error",
     "the usage, feature renderingDefinition", "not exactly one type, or it is not a RenderingDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkViewDefinition", "ViewDefinition", "INVALID_VIEW_DEFINITION_ONLY_ONE_VIEW_RENDERING", ["INVALID_VIEW_DEFINITION_ONLY_ONE_VIEW_RENDERING_MSG"], "error",
     "the owned view rendering memberships (all but the first when all are owned), else the type", "more than one view rendering membership (inherited included)",
     ["derived", "implicit"], "SysML views and metadata", "B4", "Code spelled 'validateViewDefinitionOnlyOnvViewRendering'.")
rule(S, "checkViewRenderingMembership", "ViewRenderingMembership", "INVALID_VIEW_RENDERING_MEMBERSHIP_OWNING_TYPE", ["INVALID_VIEW_RENDERING_MEMBERSHIP_OWNING_TYPE_MSG"], "error",
     "the membership", "the owning type is not a view definition or usage", ["owned"], "SysML views and metadata", "B2")
rule(S, "checkViewpointUsage", "ViewpointUsage", "INVALID_VIEWPOINT_USAGE_TYPE", ["INVALID_VIEWPOINT_USAGE_TYPE_MSG"], "error",
     "the usage, feature viewpointDefinition", "not exactly one type, or it is not a ViewpointDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkViewUsage", "ViewUsage", "INVALID_VIEW_USAGE_TYPE", ["INVALID_VIEW_USAGE_TYPE_MSG"], "error",
     "the usage, feature viewDefinition", "not exactly one type, or it is not a ViewDefinition", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkViewUsage", "ViewUsage", "INVALID_VIEW_USAGE_ONLY_ONE_RENDERING", ["INVALID_VIEW_USAGE_ONLY_ONE_RENDERING_MSG"], "error",
     "the owned view rendering memberships, else the type", "more than one view rendering membership (inherited included)", ["derived", "implicit"], "SysML views and metadata", "B4")
rule(S, "checkMetadataUsage", "MetadataUsage", "INVALID_METADATA_USAGE_TYPE", ["INVALID_METADATA_USAGE_TYPE_MSG"], "error",
     "the usage, feature metadataDefinition", "not exactly one type, or it is not a Metaclass", ["derived", "implicit"], "SysML typing", "B3")
rule(S, "checkDataType", "DataType", "INVALID_ATTRIBUTE_DEFINITION_SPECIALIZATION", ["INVALID_ATTRIBUTE_DEFINITION_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "as KerML checkDataType (override with a SysML message)", ["owned"], "Kernel classifiers", "B1",
     "Override: SysML resources report this instead of the KerML message; code 'validateDataTypeSpecialization'.")
rule(S, "checkClass", "Class", "INVALID_ITEM_DEFINITION_SPECIALIZATION", ["INVALID_ITEM_DEFINITION_SPECIALIZATION_MSG"], "error",
     "the Specialization, feature general", "as KerML checkClass (override with a SysML message)", ["owned"], "Kernel classifiers", "B1",
     "Override: code 'validateClassSpecialization'.")
rule(S, "checkOperatorExpression", "OperatorExpression", "INVALID_OPERATOR_EXPRESSION_QUANTITY", ["INVALID_OPERATOR_EXPRESSION_QUANTITY_MSG"], "warning",
     "the second ([unit]) argument", "a '[' operator whose second argument's result does not conform to MeasurementReferences::TensorMeasurementReference (directly, or through an operator expression whose argument conforms)",
     ["typing", "library"], "SysML views and metadata", "B6",
     "Override: other operators fall back to KerML checkOperatorExpression (cast warning); the KerML '[' warning is never reported for SysML.")

OVERRIDES = {"checkDataType", "checkClass", "checkOperatorExpression"}


def constants(text):
    """Xtend `public static val NAME = "value"` constants (value may be another constant)."""
    out = {}
    for m in re.finditer(r"public static val (\w+)\s*=\s*(?:([\"'])(.*?)\2|(\w+))\s*;?\s*$", text, re.M):
        out[m[1]] = m[3] if m[2] else out.get(m[4], m[4])
    return out


def checks(text):
    """@Check methods as (method name, parameter metaclass)."""
    return re.findall(r"@Check\s+(?:def|override)\s+(?:void\s+)?(\w+)\s*\(\s*(?:org\.omg\.sysml\.lang\.sysml\.)?(\w+)", text)


def category(needs):
    if "evaluation" in needs:
        return "expression evaluation"
    if "names" in needs:
        return "name resolution"
    if set(needs) & {"derived", "implicit", "library", "typing"}:
        return "derived properties"
    return "purely structural"


def build(upstream):
    upstream = Path(upstream)
    ktext = (upstream / KERML).read_text(encoding="utf-8")
    stext = (upstream / SYSML).read_text(encoding="utf-8")
    kconst = constants(ktext)
    sconst = {**kconst, **constants(stext)}
    rules = []
    used_codes = set()
    for r in R:
        table = kconst if r["language"] == "KerML" else sconst
        code_name = r["code_constant"]
        quirk = code_name.startswith("MSG:")
        code_name = code_name[4:] if quirk else code_name
        if code_name not in table:
            raise KeyError(f"{r['method']}: unknown constant {code_name}")
        messages = []
        for name in r["message_constants"]:
            if name not in table:
                raise KeyError(f"{r['method']}: unknown constant {name}")
            messages.append(table[name])
        used_codes.add(code_name)
        rule_id = f"{r['language']}.{r['method']}.{code_name}"
        rules.append({"id": rule_id, **{k: v for k, v in r.items() if k != "code_constant"},
                      "code_constant": code_name, "code": table[code_name], "code_is_message": quirk,
                      "messages": messages, "category": category(r["needs"]),
                      "override": r["language"] == "SysML" and r["method"] in OVERRIDES})
    ids = Counter(r["id"] for r in rules)
    duplicates = [i for i, n in ids.items() if n > 1]
    if duplicates:
        raise ValueError(f"duplicate rule ids: {duplicates}")
    inventoried = {(r["language"], r["method"]) for r in rules} | {(l, m) for l, m, _ in EMPTY_CHECKS}
    missing = [("KerML", m) for m, _ in checks(ktext) if ("KerML", m) not in inventoried]
    missing += [("SysML", m) for m, _ in checks(stext) if ("SysML", m) not in inventoried]
    if missing:
        raise ValueError(f"@Check methods not inventoried: {missing}")
    declared_unused = sorted(
        {name: value for name, value in sconst.items()
         if not re.search(r"_MSG(_\d)?$", name) and name not in used_codes}.items())
    return {
        "source": {
            "repository": "https://github.com/Systems-Modeling/SysML-v2-Pilot-Implementation",
            "commit": "692170b71867353b8f90341e61556f49a5beb0e5",
            "files": [KERML, SYSML],
        },
        "check_methods": {"KerML": len(checks(ktext)), "SysML": len(checks(stext))},
        "empty_check_methods": [{"language": l, "method": m, "metaclass": c} for l, m, c in EMPTY_CHECKS],
        "batches": BATCHES,
        "declared_codes_never_reported": [{"constant": n, "code": v} for n, v in declared_unused],
        "rules": rules,
    }


def markdown(data, report=None):
    """The inventory as Markdown tables, one per batch; with a rules report
    (compare_diagnostics.py rules), the corpus count of each rule's code."""
    hits = Counter()
    if report is not None:
        for row in report["codes"]:
            hits[row["code"]] += row["count"]
    lines = []
    for batch, title in data["batches"].items():
        rules = [r for r in data["rules"] if r["batch"] == batch]
        lines += [f"### {batch}: {title}", "", f"{len(rules)} rules.", "",
                  "| Rule (method) | Metaclass | Code | Sev. | Message | Condition | Needs | Corpus |",
                  "|---|---|---|---|---|---|---|--:|"]
        for r in rules:
            def cell(text):
                return str(text).replace("|", "\\|").replace("\n", " ")
            messages = " / ".join(f"\"{m}\"" for m in r["messages"])
            note = f" Note: {r['notes']}" if r["notes"] else ""
            lines.append(f"| {r['language']} `{r['method']}`{' (override)' if r['override'] else ''} | {r['metaclass']} | "
                         f"`{cell(r['code'])}` | {r['severity'][0].upper()} | {cell(messages)} | "
                         f"{cell(r['condition'])}. Target: {cell(r['target'])}.{cell(note)} | "
                         f"{', '.join(r['needs'])} | {hits.get(r['code'], 0) if report else ''} |")
        lines.append("")
    return "\n".join(lines)


def main():
    if len(sys.argv) in (3, 4) and sys.argv[1] == "--markdown":
        with open(sys.argv[2], encoding="utf-8") as stream:
            data = json.load(stream)
        report = None
        if len(sys.argv) == 4:
            with open(sys.argv[3], encoding="utf-8") as stream:
                report = json.load(stream)
        sys.stdout.reconfigure(encoding="utf-8", newline="\n")
        print(markdown(data, report))
        return
    if len(sys.argv) != 3:
        print(__doc__)
        sys.exit(2)
    data = build(sys.argv[1])
    with open(sys.argv[2], "w", encoding="utf-8", newline="\n") as stream:
        json.dump(data, stream, indent=1)
        stream.write("\n")
    rules = data["rules"]
    print(f"{len(rules)} rules from {data['check_methods']} @Check methods; "
          f"{Counter(r['language'] for r in rules)}; {Counter(r['severity'] for r in rules)}; "
          f"{Counter(r['category'] for r in rules)}; {Counter(r['batch'] for r in rules)}; "
          f"{len(data['declared_codes_never_reported'])} declared codes never reported")


if __name__ == "__main__":
    main()
