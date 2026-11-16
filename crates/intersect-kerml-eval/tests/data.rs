//! Handwritten arithmetic and Boolean compatibility cases.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, ObjId, Outcome, Reason, Value, value::equal_seq,
};

pub fn check(
    build: impl FnOnce(&mut FixtureBuilder) -> ObjId,
    expected: &[Value],
) -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let e = build(&mut b);
    let m = b.build();
    let result = Evaluator::new(&m, EvalMode::ModelLevel)?.evaluate(e, None)?;
    assert!(
        matches!(result, Outcome::Values(ref v) if equal_seq(v, expected)),
        "{result:?}"
    );
    Ok(())
}
macro_rules! binary {
    ($name:ident, $op:literal, $a:ident($x:expr), $b:ident($y:expr), $expected:expr) => {
        #[test]
        fn $name() -> Result<(), EvalError> {
            check(
                |f| {
                    let a = f.$a($x);
                    let b = f.$b($y);
                    f.op($op, &[a, b])
                },
                &$expected,
            )
        }
    };
}
binary!(division_fraction, "/", int(7), int(2), [Value::Real(3.5)]);
binary!(
    division_integral_stays_real,
    "/",
    int(4),
    int(2),
    [Value::Real(2.0)]
);
binary!(division_zero, "/", int(2), int(0), []);
binary!(division_negative_zero, "/", int(2), real(-0.0), []);
binary!(remainder_negative, "%", int(-7), int(3), [Value::Int(-1)]);
binary!(remainder_real, "%", real(7.5), int(2), [Value::Real(1.5)]);
binary!(power_real, "**", int(2), int(3), [Value::Real(8.0)]);
binary!(
    power_zero_negative,
    "**",
    int(0),
    int(-1),
    [Value::Real(f64::INFINITY)]
);
binary!(
    power_negative_fraction,
    "**",
    int(-8),
    real(1.0 / 3.0),
    [Value::Real(f64::NAN)]
);
binary!(
    string_concat,
    "+",
    string("a"),
    string("b"),
    [Value::Str("ab".into())]
);
binary!(
    string_utf16,
    "<",
    string("é"),
    string("z"),
    [Value::Bool(false)]
);
binary!(
    string_surrogates,
    "<",
    string("\u{10000}"),
    string("\u{e000}"),
    [Value::Bool(true)]
);
binary!(
    mixed_comparison,
    "<",
    int(1),
    real(2.5),
    [Value::Bool(true)]
);
binary!(
    boolean_nonboolean_is_false,
    "&",
    int(1),
    bool(true),
    [Value::Bool(false)]
);
binary!(
    boolean_or,
    "|",
    bool(false),
    bool(true),
    [Value::Bool(true)]
);
binary!(
    boolean_xor,
    "xor",
    bool(true),
    bool(true),
    [Value::Bool(false)]
);
binary!(range_descending, "..", int(5), int(3), []);
binary!(
    range_inclusive,
    "..",
    int(1),
    int(3),
    [Value::Int(1), Value::Int(2), Value::Int(3)]
);
#[test]
fn unary_string_is_empty() -> Result<(), EvalError> {
    check(
        |b| {
            let a = b.string("a");
            b.op("-", &[a])
        },
        &[],
    )
}
#[test]
fn not_integer_is_true() -> Result<(), EvalError> {
    check(
        |b| {
            let a = b.int(1);
            b.op("not", &[a])
        },
        &[Value::Bool(true)],
    )
}
#[test]
fn not_null_is_true() -> Result<(), EvalError> {
    check(
        |b| {
            let a = b.null();
            b.op("not", &[a])
        },
        &[Value::Bool(true)],
    )
}
#[test]
fn incompatible_kinds_and_undefined_operation() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let a = b.string("a");
    let n = b.int(1);
    let mixed = b.op("+", &[a, n]);
    let a = b.bool(true);
    let c = b.bool(false);
    let bools = b.op("<", &[a, c]);
    let a = b.string("a");
    let c = b.string("b");
    let minus = b.op("-", &[a, c]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, kinds) in [
        (mixed, vec!["String", "Integer"]),
        (bools, vec!["Boolean", "Boolean"]),
    ] {
        assert!(
            matches!(ev.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason:Reason::OperandKinds{invocation,kinds:k}}] if *at==e && *invocation==e && *k==kinds))
        );
    }
    assert!(
        matches!(ev.evaluate(minus,None)?,Outcome::NotEvaluable(Reason::OperationUndefined{invocation}) if invocation==minus)
    );
    Ok(())
}
pub fn max_integer(b: &mut FixtureBuilder) -> ObjId {
    let a = b.int(2147483647);
    let c = b.int(2147483647);
    let sq = b.op("*", &[a, c]);
    let two = b.int(2);
    let big = b.op("*", &[sq, two]);
    let a = b.int(2147483647);
    let four = b.int(4);
    let delta = b.op("*", &[a, four]);
    let one = b.int(1);
    let delta = b.op("+", &[delta, one]);
    b.op("+", &[big, delta])
}
#[test]
fn fix_4_min_integer_remainder_negative_one_is_zero() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let max = max_integer(&mut b);
    let neg = b.op("-", &[max]);
    let one = b.int(1);
    let min = b.op("-", &[neg, one]);
    let divisor = b.int(-1);
    let remainder = b.op("%", &[min, divisor]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(
        matches!(ev.evaluate(min,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(i64::MIN)]))
    );
    assert!(
        matches!(ev.evaluate(remainder,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(0)]))
    );
    Ok(())
}
#[test]
fn checked_i64_overflow() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let max = max_integer(&mut b);
    let one = b.int(1);
    let e = b.op("+", &[max, one]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    assert!(
        matches!(ev.evaluate(max,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(i64::MAX)]))
    );
    assert!(
        matches!(ev.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason:Reason::Overflow{invocation}}] if *at==e && *invocation==e))
    );
    Ok(())
}
#[test]
fn multivalued_operand_is_unevaluated() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let a = b.int(1);
    let c = b.int(2);
    let range = b.op("..", &[a, c]);
    let one = b.int(1);
    let e = b.op("+", &[range, one]);
    let m = b.build();
    assert!(
        matches!(Evaluator::new(&m,EvalMode::ModelLevel)?.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{reason:Reason::OperandNotSingle{invocation,operand:0,count:2},..}] if *invocation==e))
    );
    Ok(())
}
#[test]
fn range_is_bounded_before_allocation_and_boolean_is_eager() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let a = b.int(1);
    let c = b.int(2000000000);
    let range = b.op("..", &[a, c]);
    let no = b.bool(false);
    let e = b.op("&", &[no, range]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        steps: 20,
        ..EvalLimits::default()
    });
    assert_eq!(ev.evaluate(e, None).err(), Some(EvalError::StepLimit(20)));
    Ok(())
}
