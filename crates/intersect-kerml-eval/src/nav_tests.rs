//! Handwritten unit coverage of every stored navigation query.
use super::*;
use crate::test_fixture::FixtureBuilder;

#[test]
fn operands_and_feature_value_follow_the_parameter_membership_path() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let one = b.int(1);
    let two = b.int(2);
    let sum = b.op("+", &[one, two]);
    let a = b.attribute(p, "a", Some(sum));
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.operands(sum)?, [one, two]);
    assert_eq!(nav.arguments(sum)?, [one, two]);
    assert_eq!(nav.feature_value(a)?, Some(sum));
    assert_eq!(nav.feature_value(p)?, None);
    Ok(())
}
#[test]
fn named_arguments_follow_parameter_order_and_preserve_missing_slots() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.function(p, "f", &["a", "b"]);
    let a = b.named("P::f::a");
    let formal_b = b.named("P::f::b");
    let inv = b.invoke("P::f", &[]);
    let two = b.int(2);
    let bp = b.operand(inv, two);
    b.redefines(bp, formal_b);
    let one = b.int(1);
    let ap = b.operand(inv, one);
    b.redefines(ap, a);
    let missing = b.invoke("P::f", &[]);
    let v = b.int(3);
    let bp = b.operand(missing, v);
    b.redefines(bp, formal_b);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.instantiated_type(inv)?, Some(f));
    assert_eq!(nav.operands(inv)?, [two, one]);
    assert_eq!(nav.arguments(inv)?, [one, two]);
    assert_eq!(nav.argument_slots(missing)?, [None, Some(v)]);
    Ok(())
}
#[test]
fn referents_use_nonowning_memberships() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let a = b.attribute(p, "a", None);
    let e = b.feature_ref("P::a");
    let absent = b.object("FeatureReferenceExpression");
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.referent(e)?, Some(a));
    assert_eq!(nav.referent(absent)?, None);
    Ok(())
}
#[test]
fn owning_member_can_refer_to_a_feature_chain() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let e = b.object("FeatureReferenceExpression");
    let chain = b.object("Feature");
    b.member(e, "OwningMembership", chain);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.referent(e)?, Some(chain));
    Ok(())
}

#[test]
fn owned_expression_referent_skips_implicit_return_parameters() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let reference = b.object("FeatureReferenceExpression");
    let result = b.parameter("result");
    b.member(reference, "ReturnParameterMembership", result);
    let body = b.int(1);
    b.member(reference, "FeatureMembership", body);
    let empty = b.object("FeatureReferenceExpression");
    let result = b.parameter("result");
    b.member(empty, "ReturnParameterMembership", result);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.referent(reference)?, Some(body));
    assert_eq!(nav.referent(empty)?, None);
    Ok(())
}
#[test]
fn redefinition_closure_hides_inherited_features_and_terminates_cycles() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let u = b.part_def(p, "U");
    b.subclassifies(u, t);
    let a = b.attribute(t, "a", None);
    let c = b.attribute(u, "c", None);
    let d = b.attribute(u, "d", None);
    b.redefines(c, a);
    b.redefines(d, c);
    b.redefines(a, d);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.redefined_closure(d)?, [d, c, a]);
    assert_eq!(nav.all_features(u)?, [c]);
    assert_eq!(nav.all_features(u)?, [c]);
    Ok(())
}
#[test]
fn all_features_is_owned_then_depth_first_in_specialization_order() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let root = b.part_def(p, "R");
    let a = b.part_def(p, "A");
    let c = b.part_def(p, "C");
    let d = b.part_def(p, "D");
    b.subclassifies(a, root);
    b.subclassifies(c, root);
    b.subclassifies(d, a);
    b.subclassifies(d, c);
    let rf = b.attribute(root, "r", None);
    let af = b.attribute(a, "a", None);
    let cf = b.attribute(c, "c", None);
    let df = b.attribute(d, "d", None);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.owned_features(d)?, [df]);
    assert_eq!(nav.all_features(d)?, [df, af, rf, cf]);
    assert!(nav.specializes(d, root)?);
    assert!(!nav.specializes(root, d)?);
    assert!(nav.specializes(d, d)?);
    Ok(())
}
#[test]
fn types_are_inherited_through_redefinitions() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let a = b.attribute(p, "a", None);
    let c = b.attribute(p, "c", None);
    let integer = b.named("ScalarValues::Integer");
    let real = b.named("ScalarValues::Real");
    b.typed(a, integer);
    b.redefines(c, a);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.types(c)?, [integer]);
    assert!(nav.specializes(c, real)?);
    Ok(())
}
#[test]
fn result_expressions_and_return_parameter_values_are_found() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.function(p, "f", &["a"]);
    let result = b.named("P::f::result");
    let a = b.named("P::f::a");
    let one = b.int(1);
    b.value(result, one);
    let two = b.int(2);
    let body = b.body(&[], two);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.parameters(f)?, [a, result]);
    assert_eq!(nav.direction(a)?, Some("in"));
    assert!(nav.is_return(result)?);
    assert_eq!(nav.result_expression(f)?, Some(one));
    assert_eq!(nav.result_expression(body)?, Some(two));
    Ok(())
}
#[test]
fn feature_chaining_preserves_relationship_order() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let a = b.attribute(p, "a", None);
    let c = b.attribute(p, "c", None);
    let f = b.object("Feature");
    for target in [a, c] {
        let r = b.object("FeatureChaining");
        b.reference(f, "ownedRelationship", r);
        b.reference(r, "chainingFeature", target);
    }
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert_eq!(nav.chaining_features(f)?, [a, c]);
    Ok(())
}
#[test]
fn qualified_names_escape_symbols_but_not_identifiers() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let space = b.function(p, "a b", &[]);
    let plus = b.named("DataFunctions::+");
    let not = b.named("DataFunctions::not");
    let equal = b.named("BaseFunctions::==");
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    for (id, name) in [
        (plus, "DataFunctions::'+'"),
        (not, "DataFunctions::not"),
        (equal, "BaseFunctions::'=='"),
        (space, "P::'a b'"),
    ] {
        assert_eq!(nav.qualified_name(id)?.as_deref(), Some(name));
    }
    assert_eq!(escape("a'b"), "'a\\'b'");
    Ok(())
}
#[test]
fn schema_and_malformed_stored_paths_return_errors() -> Result<(), EvalError> {
    use intersect_model::definitions::dynamic::SchemaBuilder;
    let mut s = SchemaBuilder::new("empty", "urn:empty");
    s.class("Other").map_err(EvalError::Schema)?;
    let s = s.build().map_err(EvalError::Schema)?;
    let mm = intersect_model::core::frozen::Metamodel::freeze(s.as_schema())
        .map_err(|e| EvalError::Schema(e.to_string()))?;
    assert!(matches!(EvalSchema::new(&mm), Err(EvalError::Schema(_))));
    let mut b = FixtureBuilder::new();
    let f = b.object("Feature");
    let r = b.object("FeatureValue");
    let child = b.object("Feature");
    b.reference(f, "ownedRelationship", r);
    b.reference(r, "memberElement", child);
    let model = b.build();
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(&model, &schema);
    assert!(matches!(nav.feature_value(f), Err(EvalError::Model { .. })));
    Ok(())
}
