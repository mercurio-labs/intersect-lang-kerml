//! Handwritten laziness, body binding and collection cases.
use super::{base::seq, data::check, fixture::FixtureBuilder};
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, Outcome, Reason, Value, value::equal_seq,
};
#[test]
fn m3_body_feature_arguments_are_referenced_in_the_body_context() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let inner = b.part_def(p, "Inner");
    let base = b.attribute(inner, "x", None);
    b.set(base, "direction", serde_json::Value::Null);
    let outer = b.attribute(p, "outer", None);
    let three = b.int(3);
    let redefinition = b.attribute(outer, "x", Some(three));
    b.redefines(redefinition, base);
    let child = b.attribute(outer, "child", None);
    b.typed(child, inner);
    b.set(child, "direction", serde_json::Value::Null);
    let child = b.feature_ref_to(child);
    let input = b.chain(child, "P::Inner::x");
    let parameter = b.parameter("item");
    let reference = b.feature_ref_to(parameter);
    let body = b.body(&[parameter], reference);
    let collect = b.collect(input, body);
    let model = b.build();
    for mode in [EvalMode::ModelLevel, EvalMode::Extended] {
        let ev = Evaluator::new(&model, mode)?;
        // child.x is unbound in Inner, but the Pilot encodes that Feature as
        // a reference when binding item, then evaluates it in outer's context.
        let result = ev.evaluate(collect, Some(outer))?;
        assert!(
            matches!(&result, Outcome::Values(v) if equal_seq(v, &[Value::Int(3)])),
            "{mode:?}: {result:?}"
        );
    }
    Ok(())
}
#[test]
fn conditional_selects_expression_branch() -> Result<(), EvalError> {
    check(
        |b| {
            let a = b.int(1);
            let c = b.int(2);
            let test = b.op("<", &[a, c]);
            let yes = b.string("y");
            let no = b.string("n");
            b.op("if", &[test, yes, no])
        },
        &[Value::Str("y".into())],
    )
}
#[test]
fn conditional_null_is_unevaluated() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let test = b.null();
    let a = b.int(1);
    let c = b.int(2);
    let e = b.op("if", &[test, a, c]);
    let m = b.build();
    assert!(
        matches!(Evaluator::new(&m,EvalMode::ModelLevel)?.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{reason:Reason::OperandKinds{invocation,..},..}] if *invocation==e))
    );
    Ok(())
}
#[test]
fn short_circuit_does_not_enter_expensive_branch() -> Result<(), EvalError> {
    for (op, test, want) in [
        ("and", false, false),
        ("or", true, true),
        ("implies", false, true),
    ] {
        let mut b = FixtureBuilder::new();
        let test = b.bool(test);
        let a = b.int(1);
        let c = b.int(2000000000);
        let expensive = b.op("..", &[a, c]);
        let e = b.op(op, &[test, expensive]);
        let m = b.build();
        let ev = Evaluator::new(&m, EvalMode::ModelLevel)?.with_limits(EvalLimits {
            steps: 3,
            ..EvalLimits::default()
        });
        assert!(
            matches!(ev.evaluate(e,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Bool(want)]))
        );
    }
    Ok(())
}
#[test]
fn null_coalesces_but_unbound_feature_does_not() -> Result<(), EvalError> {
    check(
        |b| {
            let n = b.null();
            let three = b.int(3);
            b.op("??", &[n, three])
        },
        &[Value::Int(3)],
    )?;
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let x = b.attribute(p, "x", None);
    let r = b.feature_ref_to(x);
    let three = b.int(3);
    let e = b.op("??", &[r, three]);
    let m = b.build();
    assert!(
        matches!(Evaluator::new(&m,EvalMode::ModelLevel)?.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason:Reason::NoValue{..}}] if *at==x))
    );
    Ok(())
}
#[test]
fn collect_binds_each_argument_and_flattens_results() -> Result<(), EvalError> {
    check(
        |b| {
            let source = seq(b, &[1, 2, 3]);
            let p = b.parameter("v");
            let v = b.feature_ref_to(p);
            let two = b.int(2);
            let result = b.op("*", &[v, two]);
            let body = b.body(&[p], result);
            b.collect(source, body)
        },
        &[Value::Int(2), Value::Int(4), Value::Int(6)],
    )
}
#[test]
fn select_keeps_values_with_true_body() -> Result<(), EvalError> {
    check(
        |b| {
            let source = seq(b, &[1, 2, 3]);
            let p = b.parameter("v");
            let v = b.feature_ref_to(p);
            let one = b.int(1);
            let result = b.op(">", &[v, one]);
            let body = b.body(&[p], result);
            b.select(source, body)
        },
        &[Value::Int(2), Value::Int(3)],
    )
}
#[test]
fn empty_collect_and_unmapped_reduce() -> Result<(), EvalError> {
    check(
        |b| {
            let source = b.null();
            let p = b.parameter("v");
            let one = b.int(1);
            let body = b.body(&[p], one);
            b.collect(source, body)
        },
        &[],
    )?;
    let mut b = FixtureBuilder::new();
    let n = b.null();
    let e = b.invoke("ControlFunctions::reduce", &[n]);
    let m = b.build();
    assert!(
        matches!(Evaluator::new(&m,EvalMode::ModelLevel)?.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{reason:Reason::UnknownFunction{..},..}]))
    );
    Ok(())
}
#[test]
fn body_typed_by_library_function_uses_bound_arguments() -> Result<(), EvalError> {
    check(
        |b| {
            let source = seq(b, &[1, 2, 3]);
            let body = b.object("Expression");
            let plus = b.named("DataFunctions::'+'");
            b.typed(body, plus);
            b.collect(source, body)
        },
        &[Value::Int(1), Value::Int(2), Value::Int(3)],
    )
}
#[test]
fn null_body_result_becomes_unevaluated_result_expression() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let a = b.string("a");
    let c = b.string("b");
    let bad = b.op("-", &[a, c]);
    let body = b.body(&[], bad);
    let source = seq(&mut b, &[1]);
    let e = b.collect(source, body);
    let m = b.build();
    assert!(
        matches!(Evaluator::new(&m,EvalMode::ModelLevel)?.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason:Reason::OperationUndefined{invocation}}] if *at==bad && *invocation==bad))
    );
    Ok(())
}
