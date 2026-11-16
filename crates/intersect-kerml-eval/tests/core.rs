//! Handwritten core evaluation and per-call limit checks.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, Outcome, Reason, Value, value::equal_seq,
};

fn check(
    build: impl FnOnce(&mut FixtureBuilder) -> intersect_kerml_eval::ObjId,
    expected: &[Value],
) -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let expr = build(&mut b);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    let Outcome::Values(actual) = ev.evaluate(expr, None)? else {
        return Err(EvalError::Model {
            object: "test".into(),
            message: "expected values".into(),
        });
    };
    assert!(equal_seq(&actual, expected), "{actual:?}");
    Ok(())
}
#[test]
fn integer_literal() -> Result<(), EvalError> {
    check(
        |b| b.int(i64::from(i32::MIN)),
        &[Value::Int(i64::from(i32::MIN))],
    )
}
#[test]
fn real_literal_bits() -> Result<(), EvalError> {
    check(|b| b.real(-0.0), &[Value::Real(-0.0)])
}
#[test]
fn boolean_literal() -> Result<(), EvalError> {
    check(|b| b.bool(true), &[Value::Bool(true)])
}
#[test]
fn string_literal() -> Result<(), EvalError> {
    check(|b| b.string("a\nβ"), &[Value::Str("a\nβ".into())])
}
#[test]
fn infinity_literal() -> Result<(), EvalError> {
    check(|b| b.infinity(), &[Value::Infinity])
}
#[test]
fn null_is_an_empty_sequence() -> Result<(), EvalError> {
    check(|b| b.null(), &[])
}
#[test]
fn unknown_function_is_unevaluated_with_reason() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    b.function(p, "user", &[]);
    let e = b.invoke("P::user", &[]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(
        matches!(ev.evaluate(e, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { at, reason: Reason::UnknownFunction { invocation } }] if *at == e && *invocation == e))
    );
    Ok(())
}
#[test]
fn step_limit_applies_to_each_nested_expression_and_resets_per_call() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let leaf = b.int(1);
    let mut e = leaf;
    for _ in 0..5 {
        e = b.body(&[], e);
    }
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        steps: 3,
        ..EvalLimits::default()
    });
    assert_eq!(ev.evaluate(e, None).err(), Some(EvalError::StepLimit(3)));
    assert_eq!(ev.evaluate(e, None).err(), Some(EvalError::StepLimit(3)));
    assert!(
        matches!(ev.evaluate(leaf, None)?, Outcome::Values(v) if equal_seq(&v, &[Value::Int(1)]))
    );
    Ok(())
}
#[test]
fn depth_limit_is_checked_without_panicking() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let mut e = b.int(1);
    for _ in 0..5 {
        e = b.body(&[], e);
    }
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        depth: 2,
        ..EvalLimits::default()
    });
    assert_eq!(ev.evaluate(e, None).err(), Some(EvalError::DepthLimit(2)));
    Ok(())
}
#[test]
fn nonexpression_and_foreign_out_of_range_handle_are_errors() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_eq!(ev.mode(), EvalMode::Extended);
    assert!(matches!(
        ev.evaluate(p, None),
        Err(EvalError::NotAnExpression(_))
    ));
    let empty = FixtureBuilder::empty().build();
    let other = Evaluator::new(&empty, EvalMode::ModelLevel)?;
    assert!(matches!(
        other.evaluate(p, None),
        Err(EvalError::Model { .. })
    ));
    Ok(())
}
