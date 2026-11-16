//! Handwritten contextual features, redefinition and chain regressions.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, Outcome, Reason, Value, value::equal_seq,
};
#[test]
fn fix_1_inherited_redefinition_wins_in_both_general_orders() -> Result<(), EvalError> {
    for typing_first in [true, false] {
        let mut b = FixtureBuilder::new();
        let pkg = b.package("Review");
        let p_type = b.part_def(pkg, "P");
        let five = b.int(5);
        let m = b.attribute(p_type, "m", Some(five));
        let q_type = b.part_def(pkg, "Q");
        b.subclassifies(q_type, p_type);
        let p = b.part(p_type, "p", p_type);
        let three = b.int(3);
        let redef = b.attribute(p, "m", Some(three));
        b.redefines(redef, m);
        let q = b.object("PartUsage");
        if typing_first {
            b.typed(q, q_type);
            b.redefines(q, p);
        } else {
            b.redefines(q, p);
            b.typed(q, q_type);
        }
        let e = b.feature_ref_to(m);
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
        for _ in 0..2 {
            assert!(
                matches!(ev.evaluate(e,Some(q))?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(3)]))
            );
        }
    }
    Ok(())
}
#[test]
fn fix_1_diamond_filters_a_base_reached_before_its_redefinition() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let pkg = b.package("Review");
    let a = b.part_def(pkg, "A");
    let five = b.int(5);
    let m = b.attribute(a, "m", Some(five));
    let sub = b.part_def(pkg, "B");
    b.subclassifies(sub, a);
    let seven = b.int(7);
    let redef = b.attribute(sub, "m", Some(seven));
    b.redefines(redef, m);
    let c = b.part_def(pkg, "C");
    b.subclassifies(c, a);
    b.subclassifies(c, sub);
    let e = b.feature_ref_to(m);
    let model = b.build();
    assert!(
        matches!(Evaluator::new(&model,EvalMode::ModelLevel)?.evaluate(e,Some(c))?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(7)]))
    );
    Ok(())
}
#[test]
fn feature_values_and_package_target() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let one = b.int(1);
    b.attribute(p, "a", Some(one));
    let a = b.feature_ref("P::a");
    let one = b.int(1);
    let sum = b.op("+", &[a, one]);
    let f = b.attribute(p, "b", Some(sum));
    let reference = b.feature_ref_to(f);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for target in [None, Some(p)] {
        assert!(
            matches!(ev.evaluate(reference,target)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(2)]))
        );
    }
    Ok(())
}
#[test]
fn subtype_redefinition_value_wins_and_unbound_stops_lookup() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let pkg = b.package("P");
    let p = b.part_def(pkg, "Base");
    let five = b.int(5);
    let f = b.attribute(p, "m", Some(five));
    let q = b.part_def(pkg, "Q");
    b.subclassifies(q, p);
    let seven = b.int(7);
    let g = b.attribute(q, "m", Some(seven));
    b.redefines(g, f);
    let r = b.part_def(pkg, "R");
    b.subclassifies(r, q);
    let h = b.attribute(r, "m", None);
    b.redefines(h, g);
    let e = b.feature_ref_to(f);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (t, n) in [(p, 5), (q, 7)] {
        assert!(
            matches!(ev.evaluate(e,Some(t))?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(n)]))
        );
    }
    assert!(
        matches!(ev.evaluate(e,Some(r))?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason:Reason::NoValue{feature,frame:Some(t)}}] if *at==h && *feature==h && *t==r))
    );
    Ok(())
}
#[test]
fn default_and_assignment_values_use_the_same_stored_path() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.attribute(p, "a", None);
    let seven = b.int(7);
    let value = b.value(f, seven);
    b.set(value, "isDefault", true.into());
    let m = b.build();
    assert!(
        matches!(Evaluator::new(&m,EvalMode::ModelLevel)?.evaluate_feature(f,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(7)]))
    );
    Ok(())
}
#[test]
fn self_and_library_pi() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let self_e = b.feature_ref("Base::Anything::self");
    let pi = b.feature_ref("TrigFunctions::pi");
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(
        matches!(ev.evaluate(self_e,Some(t))?,Outcome::Values(v) if equal_seq(&v,&[Value::Element(t)]))
    );
    assert!(
        matches!(ev.evaluate(pi,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Real(std::f64::consts::PI)]))
    );
    Ok(())
}
#[test]
fn three_level_chain_and_multiple_sources() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let q = b.part(t, "q", t);
    let v = b.int(9);
    b.attribute(q, "r", Some(v));
    let root = b.part(t, "p", t);
    let source = b.feature_ref_to(root);
    let mid = b.chain(source, "P::T::q");
    let e = b.chain(mid, "P::T::q::r");
    let x = b.feature_ref_to(root);
    let y = b.feature_ref_to(root);
    let list = b.op(",", &[x, y]);
    let multi = b.chain(list, "P::T::q");
    let multi = b.chain(multi, "P::T::q::r");
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(matches!(ev.evaluate(e,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(9)])));
    assert!(
        matches!(ev.evaluate(multi,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(9),Value::Int(9)]))
    );
    Ok(())
}
#[test]
fn missing_reference_and_unbound_feature_have_distinct_reasons() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.attribute(p, "x", None);
    let e = b.feature_ref_to(f);
    let missing = b.object("MetadataAccessExpression");
    let one = b.int(1);
    let eq = b.op("==", &[e, one]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(
        matches!(ev.evaluate_feature(f,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason:Reason::NoValue{feature,frame:None}}] if *at==f && *feature==f))
    );
    assert!(
        matches!(ev.evaluate(missing,None)?,Outcome::NotEvaluable(Reason::NoReferent{expression}) if expression==missing)
    );
    assert!(
        matches!(ev.evaluate(eq,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Bool(false)]))
    );
    Ok(())
}
#[test]
fn implicit_self_without_the_library_reports_no_referent() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::empty();
    let reference = b.object("FeatureReferenceExpression");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(matches!(ev.evaluate(reference, None)?,
        Outcome::NotEvaluable(Reason::NoReferent { expression }) if expression == reference));
    Ok(())
}
#[test]
fn cyclic_feature_values_are_depth_bounded() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.attribute(p, "x", None);
    let reference = b.feature_ref_to(f);
    b.value(f, reference);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        depth: 20,
        ..EvalLimits::default()
    });
    assert_eq!(
        ev.evaluate_feature(f, None).err(),
        Some(EvalError::DepthLimit(20))
    );
    Ok(())
}
