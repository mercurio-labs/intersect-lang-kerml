//! Handwritten REF 3 table and top-level gating tests.
use super::{base::seq, fixture::FixtureBuilder};
use intersect_kerml_eval::{
    EvalError, EvalMode, Evaluator, Outcome, Reason, Value, value::equal_seq,
};

#[test]
fn m3_repeated_feature_references_are_not_reference_cycles() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let one = b.int(1);
    let x = b.attribute(p, "x", Some(one));
    let a = b.feature_ref_to(x);
    let z = b.feature_ref_to(x);
    let sum = b.op("+", &[a, z]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(ev.is_model_level_evaluable(sum)?);
    assert!(
        matches!(ev.evaluate_gated(sum,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(2)]))
    );
    Ok(())
}

#[test]
fn m3_expression_referent_failure_still_checks_its_metadata_owner() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let owner = b.object("MetadataFeature");
    b.member(p, "OwningMembership", owner);
    let calc = b.object("CalculationUsage");
    b.member(owner, "FeatureMembership", calc);
    let reference = b.feature_ref_to(calc);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(!ev.is_model_level_evaluable(calc)?);
    assert!(ev.is_model_level_evaluable(reference)?);
    Ok(())
}

#[test]
fn m3_directed_feature_memberships_participate_in_evaluability() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let expression = b.object("Expression");
    let output = b.object("Feature");
    b.enumeration(output, "direction", "out");
    b.member(expression, "FeatureMembership", output);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(!ev.is_model_level_evaluable(expression)?);
    Ok(())
}
#[test]
fn fix_9_chained_referent_uses_only_its_first_features_featuring_type() -> Result<(), EvalError> {
    fn chain(
        b: &mut FixtureBuilder,
        parts: &[intersect_kerml_eval::ObjId],
    ) -> intersect_kerml_eval::ObjId {
        let f = b.object("Feature");
        for target in parts {
            let r = b.object("FeatureChaining");
            b.reference(f, "ownedRelationship", r);
            b.reference(r, "chainingFeature", *target);
        }
        f
    }
    let mut b = FixtureBuilder::new();
    let pkg = b.package("Review");
    let t = b.part_def(pkg, "T");
    let p = b.part(t, "p", t);
    let q = b.attribute(t, "q", None);
    let unfeatured = b.attribute(pkg, "global", None);
    let pq = chain(&mut b, &[p, q]);
    let nested = chain(&mut b, &[pq, q]);
    let global_q = chain(&mut b, &[unfeatured, q]);
    let bad = b.feature_ref_to(pq);
    let nested = b.feature_ref_to(nested);
    let good = b.feature_ref_to(global_q);
    let cycle = b.object("Feature");
    let rel = b.object("FeatureChaining");
    b.reference(cycle, "ownedRelationship", rel);
    b.reference(rel, "chainingFeature", cycle);
    let cyclic = b.feature_ref_to(cycle);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(!ev.is_model_level_evaluable(bad)?);
    assert!(!ev.is_model_level_evaluable(nested)?);
    assert!(ev.is_model_level_evaluable(good)?);
    assert!(matches!(
        ev.is_model_level_evaluable(cyclic),
        Err(EvalError::Model { .. })
    ));
    Ok(())
}
#[test]
fn fix_8_gated_default_requires_result_membership_not_return_parameter_value()
-> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let e = b.object("Expression");
    let result = b.object("Feature");
    b.enumeration(result, "direction", "out");
    b.member(e, "ReturnParameterMembership", result);
    let seven = b.int(7);
    b.value(result, seven);
    let outer = b.body(&[], e);
    let both = b.object("Expression");
    let ret = b.object("Feature");
    b.enumeration(ret, "direction", "out");
    b.member(both, "ReturnParameterMembership", ret);
    let nine = b.int(9);
    b.value(ret, nine);
    let one = b.int(1);
    b.member(both, "ResultExpressionMembership", one);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    for expr in [e, outer] {
        assert!(
            matches!(ev.evaluate(expr,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(7)]))
        );
        assert!(
            matches!(ev.evaluate_gated(expr,None)?,Outcome::NotEvaluable(Reason::NoResultExpression{expression}) if expression==e)
        );
    }
    for result in [ev.evaluate(both, None)?, ev.evaluate_gated(both, None)?] {
        assert!(matches!(result,Outcome::Values(v) if equal_seq(&v,&[Value::Int(1)])));
    }
    assert!(
        matches!(Evaluator::new(&model,EvalMode::Extended)?.evaluate_gated(e,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(7)]))
    );
    Ok(())
}
#[test]
fn fix_3_metadata_type_operators_remain_evaluable_after_m5_execution_support()
-> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("Review");
    let t = b.part_def(p, "T");
    let mut cases = Vec::new();
    for op in ["@@", "meta"] {
        let metadata = b.metadata_access("Review");
        cases.push((op, b.type_test(op, metadata, t)));
    }
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    for (op, e) in cases {
        assert!(ev.is_model_level_evaluable(e)?);
        for outcome in [ev.evaluate(e, None)?, ev.evaluate_gated(e, None)?] {
            let expected = if op == "@@" {
                vec![Value::Bool(false)]
            } else {
                vec![]
            };
            assert!(matches!(outcome,Outcome::Values(v) if equal_seq(&v,&expected)));
        }
    }
    Ok(())
}
#[test]
fn literals_null_metadata_and_arithmetic_are_evaluable() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let mut cases = vec![
        b.int(1),
        b.real(2.0),
        b.bool(true),
        b.string("s"),
        b.infinity(),
        b.null(),
        b.metadata_access("P"),
    ];
    let a = b.int(1);
    let c = b.int(2);
    cases.push(b.op("+", &[a, c]));
    let s = seq(&mut b, &[1, 2]);
    cases.push(s);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for e in cases {
        assert!(ev.is_model_level_evaluable(e)?);
    }
    assert!(matches!(
        ev.is_model_level_evaluable(p),
        Err(EvalError::NotAnExpression(_))
    ));
    Ok(())
}
#[test]
fn library_extended_and_user_invocations_are_not_model_level() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    b.function(p, "f", &["x"]);
    let mut cases = Vec::new();
    for name in [
        "SequenceFunctions::size",
        "StringFunctions::Length",
        "NumericalFunctions::sum",
        "P::f",
    ] {
        let a = b.int(3);
        let c = b.int(4);
        let sum = b.op("+", &[a, c]);
        cases.push(b.invoke(name, &[sum]));
    }
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for e in cases {
        assert!(!ev.is_model_level_evaluable(e)?);
        assert!(
            matches!(ev.evaluate_gated(e,None)?,Outcome::NotEvaluable(Reason::NotModelLevelEvaluable{expression}) if expression==e)
        );
    }
    Ok(())
}
#[test]
fn constructors_ungated_return_parameter_but_top_level_gated_null() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let one = b.int(1);
    let e = b.constructor(t, &[one]);
    let missing = b.object("ConstructorExpression");
    let body = b.body(&[], e);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(ev.is_model_level_evaluable(e)?);
    assert!(
        matches!(ev.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Element(_)]))
    );
    assert!(
        matches!(ev.evaluate(body,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Element(_)]))
    );
    assert!(matches!(
        ev.evaluate_gated(e, None)?,
        Outcome::NotEvaluable(_)
    ));
    assert!(
        matches!(ev.evaluate(missing,None)?,Outcome::NotEvaluable(Reason::NoResultExpression{expression}) if expression==missing)
    );
    assert!(matches!(
        Evaluator::new(&m, EvalMode::Extended)?.evaluate_gated(e, None)?,
        Outcome::Values(_)
    ));
    Ok(())
}
#[test]
fn calc_constraint_and_default_expression_gating() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let calc = b.object("CalculationUsage");
    let constraint = b.object("ConstraintUsage");
    let empty = b.object("Expression");
    let one = b.int(1);
    let body = b.body(&[], one);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for e in [calc, constraint] {
        assert!(!ev.is_model_level_evaluable(e)?);
        assert!(matches!(ev.evaluate_gated(e,None)?,Outcome::Values(v) if v.is_empty()));
    }
    assert!(ev.is_model_level_evaluable(empty)?);
    assert!(matches!(
        ev.evaluate_gated(empty, None)?,
        Outcome::NotEvaluable(Reason::NoResultExpression { .. })
    ));
    assert!(
        matches!(ev.evaluate_gated(body,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(1)]))
    );
    Ok(())
}
#[test]
fn default_parameter_rules() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let mut cases = Vec::new();
    for (direction, value, owned, want) in [
        ("in", false, false, true),
        ("inout", false, false, false),
        ("out", false, false, false),
        ("in", true, false, false),
        ("in", false, true, false),
    ] {
        let p = b.parameter("x");
        b.enumeration(p, "direction", direction);
        if value {
            let n = b.int(1);
            b.value(p, n);
        }
        if owned {
            b.attribute(p, "nested", None);
        }
        let n = b.int(1);
        cases.push((b.body(&[p], n), want));
    }
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, want) in cases {
        assert_eq!(ev.is_model_level_evaluable(e)?, want);
    }
    Ok(())
}
#[test]
fn feature_reference_table_rows_and_cycle() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let unbound = b.attribute(p, "unbound", None);
    let typed = b.attribute(t, "member", None);
    let meta = b.object("Metaclass");
    let mf = b.attribute(meta, "x", None);
    let annotation = b.object("MetadataFeature");
    let af = b.attribute(annotation, "x", None);
    let cycle = b.attribute(p, "cycle", None);
    let recur = b.feature_ref_to(cycle);
    b.value(cycle, recur);
    let expression = b.int(3);
    let mut cases = vec![
        (b.object("FeatureReferenceExpression"), true),
        (b.feature_ref("Base::Anything::self"), true),
    ];
    for (f, want) in [
        (unbound, true),
        (typed, false),
        (mf, true),
        (af, true),
        (cycle, false),
        (expression, true),
    ] {
        cases.push((b.feature_ref_to(f), want));
    }
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, want) in cases {
        assert_eq!(ev.is_model_level_evaluable(e)?, want, "{e:?}");
    }
    Ok(())
}
#[test]
fn body_parameter_reference_is_not_evaluable_but_direct_collect_runs() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.parameter("x");
    let x = b.feature_ref_to(p);
    let two = b.int(2);
    let times = b.op("*", &[x, two]);
    let body = b.body(&[p], times);
    let source = seq(&mut b, &[1, 2, 3]);
    let e = b.collect(source, body);
    let empty = b.null();
    let p = b.parameter("x");
    let one = b.int(1);
    let constant = b.body(&[p], one);
    let empty_collect = b.collect(empty, constant);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(!ev.is_model_level_evaluable(e)?);
    assert!(
        matches!(ev.evaluate(e,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(2),Value::Int(4),Value::Int(6)]))
    );
    assert!(matches!(
        ev.evaluate_gated(e, None)?,
        Outcome::NotEvaluable(Reason::NotModelLevelEvaluable { .. })
    ));
    assert!(ev.is_model_level_evaluable(empty_collect)?);
    Ok(())
}
