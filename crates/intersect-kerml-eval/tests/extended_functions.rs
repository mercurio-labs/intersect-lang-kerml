//! Handwritten quick tests of REF 2.2 functions, without parser or Java.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, ObjId, Outcome, Reason, Value, value::equal_seq,
};

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn text_extended_table_and_standard_library_bodies() -> Result<(), String> {
    let Some(model) = super::text::load(
        r#"package ExtendedLibrary {
            private import ScalarValues::*;
            private import ControlFunctions::selectOne;
            attribute count = SequenceFunctions::size((1, 2, 3));
            attribute total = NumericalFunctions::sum((1, 2, 3));
            attribute product = NumericalFunctions::product((2, 3, 4));
            attribute length = StringFunctions::Length("a😀");
            attribute substring = StringFunctions::Substring("a😀b", 2, 3);
            attribute largest = DataFunctions::max(2, 5);
            attribute sine = TrigFunctions::sin(0);
            attribute every = ControlFunctions::allTrue((true, true));
            attribute notEvery = ControlFunctions::allTrue((true, false));
            attribute any = ControlFunctions::anyTrue((false, true));
            attribute selected = (1, 2, 3)->selectOne { in x; x > 1 };
        }"#,
    )?
    else {
        return Ok(());
    };
    let ev = Evaluator::new(&model, EvalMode::Extended).map_err(|e| e.to_string())?;
    for (name, expected) in [
        ("count", Value::Int(3)),
        ("total", Value::Int(6)),
        ("product", Value::Int(24)),
        ("length", Value::Int(3)),
        ("substring", Value::Str("😀".into())),
        ("largest", Value::Int(5)),
        ("sine", Value::Real(0.0)),
        ("every", Value::Bool(true)),
        ("notEvery", Value::Bool(false)),
        ("any", Value::Bool(true)),
        ("selected", Value::Int(2)),
    ] {
        let feature = ev
            .resolve_target(&format!("ExtendedLibrary::{name}"))
            .map_err(|e| e.to_string())?;
        let outcome = ev
            .evaluate_feature(feature, None)
            .map_err(|e| e.to_string())?;
        assert!(
            matches!(&outcome, Outcome::Values(v) if equal_seq(v, &[expected])),
            "{name}: {outcome:?}"
        );
    }
    Ok(())
}

fn register(b: &mut FixtureBuilder, name: &str) {
    let (package, function) = name.split_once("::").unwrap();
    // Only the function identity is needed by direct table dispatch.
    let p = b.package(package);
    b.function(p, function, &["a", "b", "c"]);
}
fn sequence(b: &mut FixtureBuilder, values: &[Value]) -> ObjId {
    let mut result = None;
    for value in values {
        let e = match value {
            Value::Int(n) => b.int(*n),
            Value::Real(n) => b.real(*n),
            Value::Str(s) => b.string(s),
            Value::Bool(v) => b.bool(*v),
            Value::Infinity => b.infinity(),
            _ => panic!("unsupported test scalar"),
        };
        result = Some(result.map_or(e, |previous| b.op(",", &[previous, e])));
    }
    result.unwrap_or_else(|| b.null())
}
fn check(name: &str, args: &[&[Value]], expected: &[Value]) -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    register(&mut b, name);
    let args: Vec<_> = args.iter().map(|v| sequence(&mut b, v)).collect();
    let call = b.invoke(name, &args);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    let result = ev.evaluate(call, None)?;
    assert!(
        matches!(&result, Outcome::Values(v) if equal_seq(v, expected)),
        "{name}: {result:?}"
    );
    assert!(!ev.is_model_level_evaluable(call)?);
    let ml = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(matches!(
        ml.evaluate_gated(call, None)?,
        Outcome::NotEvaluable(_)
    ));
    assert!(
        matches!(ml.evaluate(call, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { at, .. }] if *at == call))
    );
    Ok(())
}
fn undefined_argument(name: &str) -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    register(&mut b, name);
    let a = b.string("a");
    let z = b.string("z");
    let undefined = b.op("-", &[a, z]);
    let one = b.int(1);
    let call = b.invoke(name, &[undefined, one]);
    let model = b.build();
    assert!(
        matches!(Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { at, .. }] if *at == call)),
        "{name}"
    );
    Ok(())
}
use Value::{Bool as B, Infinity as Star, Int as I, Real as R};
macro_rules! sequence_case {
    ($test:ident, $op:literal, $args:expr, $expected:expr, $empty_args:expr, $empty:expr) => {
        #[test]
        fn $test() -> Result<(), EvalError> {
            let name = concat!("SequenceFunctions::", $op);
            check(name, $args, $expected)?;
            check(name, $empty_args, $empty)?;
            undefined_argument(name)
        }
    };
}
sequence_case!(
    sequence_size,
    "size",
    &[&[I(4), I(4), I(1)]],
    &[I(3)],
    &[&[]],
    &[I(0)]
);
sequence_case!(
    sequence_is_empty,
    "isEmpty",
    &[&[I(1)]],
    &[B(false)],
    &[&[]],
    &[B(true)]
);
sequence_case!(
    sequence_not_empty,
    "notEmpty",
    &[&[I(1)]],
    &[B(true)],
    &[&[]],
    &[B(false)]
);
sequence_case!(
    sequence_includes,
    "includes",
    &[&[I(1), I(2)], &[I(2), I(2)]],
    &[B(true)],
    &[&[], &[]],
    &[B(true)]
);
sequence_case!(
    sequence_includes_only,
    "includesOnly",
    &[&[I(1), I(2)], &[I(2), I(1), I(1)]],
    &[B(true)],
    &[&[], &[]],
    &[B(true)]
);
sequence_case!(
    sequence_excludes,
    "excludes",
    &[&[I(1), I(2)], &[R(1.0)]],
    &[B(true)],
    &[&[], &[]],
    &[B(true)]
);
sequence_case!(
    sequence_equals,
    "equals",
    &[&[I(1), I(2)], &[I(2), I(1)]],
    &[B(false)],
    &[&[], &[]],
    &[B(true)]
);
sequence_case!(
    sequence_same,
    "same",
    &[&[I(1), I(2)], &[I(1), I(2)]],
    &[B(true)],
    &[&[], &[]],
    &[B(true)]
);
sequence_case!(
    sequence_including,
    "including",
    &[&[I(1)], &[I(2), I(1)]],
    &[I(1), I(2), I(1)],
    &[&[], &[]],
    &[]
);
sequence_case!(
    sequence_union,
    "union",
    &[&[I(1)], &[I(2), I(1)]],
    &[I(1), I(2), I(1)],
    &[&[], &[]],
    &[]
);
sequence_case!(
    sequence_including_at,
    "includingAt",
    &[&[I(1), I(3)], &[I(2)], &[I(2)]],
    &[I(1), I(2), I(3)],
    &[&[], &[I(8)], &[Star]],
    &[I(8)]
);
sequence_case!(
    sequence_excluding,
    "excluding",
    &[&[I(1), I(2), I(1)], &[I(1)]],
    &[I(2)],
    &[&[], &[]],
    &[]
);
sequence_case!(
    sequence_intersection,
    "intersection",
    &[&[I(1), I(2), I(1)], &[I(1)]],
    &[I(1), I(1)],
    &[&[], &[]],
    &[]
);
sequence_case!(
    sequence_excluding_at,
    "excludingAt",
    &[&[I(1), I(2), I(3)], &[I(2)]],
    &[I(1), I(3)],
    &[&[], &[I(1)], &[Star]],
    &[]
);
sequence_case!(
    sequence_subsequence,
    "subsequence",
    &[&[I(1), I(2), I(3)], &[I(2)]],
    &[I(2), I(3)],
    &[&[], &[I(1)], &[Star]],
    &[]
);
sequence_case!(
    sequence_head,
    "head",
    &[&[I(1), I(2)]],
    &[I(1)],
    &[&[]],
    &[]
);
sequence_case!(
    sequence_last,
    "last",
    &[&[I(1), I(2)]],
    &[I(2)],
    &[&[]],
    &[]
);
sequence_case!(
    sequence_tail,
    "tail",
    &[&[I(1), I(2)]],
    &[I(2)],
    &[&[]],
    &[]
);

#[test]
fn sequence_ranges_and_indices_are_checked() -> Result<(), EvalError> {
    check(
        "SequenceFunctions::includingAt",
        &[&[I(1)], &[I(2)], &[I(-3)]],
        &[I(2), I(1)],
    )?;
    check(
        "SequenceFunctions::subsequence",
        &[&[I(1), I(2)], &[I(-3)], &[I(1)]],
        &[I(1)],
    )?;
    check("SequenceFunctions::subsequence", &[&[I(1)], &[Star]], &[])?;
    check(
        "SequenceFunctions::excludingAt",
        &[&[I(1)], &[Star]],
        &[I(1)],
    )?;
    check(
        "SequenceFunctions::excludingAt",
        &[&[I(1), I(2), I(3)], &[I(2)], &[Star]],
        &[I(1)],
    )?;
    check(
        "SequenceFunctions::subsequence",
        &[&[I(1)], &[I(3)], &[I(1)]],
        &[],
    )?;
    for (name, args) in [
        (
            "SequenceFunctions::includingAt",
            vec![vec![I(1)], vec![I(2)], vec![I(4)]],
        ),
        (
            "SequenceFunctions::subsequence",
            vec![vec![I(1)], vec![I(1)], vec![I(4)]],
        ),
        (
            "SequenceFunctions::excludingAt",
            vec![vec![I(1)], vec![I(1)], vec![I(4)]],
        ),
    ] {
        let mut b = FixtureBuilder::new();
        register(&mut b, name);
        let args: Vec<_> = args.iter().map(|v| sequence(&mut b, v)).collect();
        let call = b.invoke(name, &args);
        let model = b.build();
        assert!(matches!(
            Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None),
            Err(EvalError::Model { .. })
        ));
    }
    Ok(())
}

#[test]
fn excluding_at_preserves_the_oracle_precedence_quirk_without_panicking() -> Result<(), EvalError> {
    check(
        "SequenceFunctions::excludingAt",
        &[&[I(9)], &[Star], &[Value::Str("bad".into())]],
        &[I(9)],
    )?;
    let mut b = FixtureBuilder::new();
    register(&mut b, "SequenceFunctions::excludingAt");
    let input = b.int(9);
    let start = b.string("bad");
    let end = b.int(1);
    let call = b.invoke("SequenceFunctions::excludingAt", &[input, start, end]);
    let model = b.build();
    assert!(
        matches!(Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None), Err(EvalError::Model { message, .. }) if message.contains("Pilot cast failure"))
    );
    Ok(())
}

#[test]
fn string_length_counts_utf16_units() -> Result<(), EvalError> {
    let name = "StringFunctions::Length";
    check(name, &[&[Value::Str("a😀é".into())]], &[I(4)])?;
    check(name, &[&[Value::Str("".into())]], &[I(0)])?;
    undefined_argument(name)
}

#[test]
fn substring_uses_inclusive_utf16_bounds_and_distinguishes_empty_results() -> Result<(), EvalError>
{
    let name = "StringFunctions::Substring";
    for (text, lower, upper, expected) in [
        ("a😀b", 2, 3, Some("😀")),
        ("a😀b", 4, 4, Some("b")),
        ("a😀b", 1, 4, Some("a😀b")),
        ("abc", 2, 1, Some("")),
        ("", 1, 0, Some("")),
        ("abc", 0, 1, None),
        ("abc", 1, 4, None),
        ("abc", 3, 1, None),
        ("abc", 1, -1, None),
    ] {
        let expected: Vec<_> = expected.map(|s| Value::Str(s.into())).into_iter().collect();
        check(
            name,
            &[&[Value::Str(text.into())], &[I(lower)], &[I(upper)]],
            &expected,
        )?;
    }
    undefined_argument(name)
}

#[test]
fn substring_rejects_split_surrogates_without_replacement() -> Result<(), EvalError> {
    for position in [1, 2] {
        let mut b = FixtureBuilder::new();
        register(&mut b, "StringFunctions::Substring");
        let text = b.string("😀");
        let lower = b.int(position);
        let upper = b.int(position);
        let call = b.invoke("StringFunctions::Substring", &[text, lower, upper]);
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::Extended)?;
        assert!(
            matches!(ev.evaluate(call, None), Err(EvalError::Model { message, .. })
            if message.contains("unpaired UTF-16 surrogate"))
        );
    }
    Ok(())
}

#[test]
fn substring_requires_single_string_and_integer_bounds() -> Result<(), EvalError> {
    for (text, lower, upper) in [
        (vec![I(4)], vec![I(1)], vec![I(1)]),
        (vec![Value::Str("abc".into())], vec![R(1.0)], vec![I(1)]),
        (vec![Value::Str("abc".into())], vec![I(1)], vec![Star]),
        (vec![Value::Str("abc".into())], vec![I(1), I(2)], vec![I(3)]),
        (vec![Value::Str("abc".into())], vec![I(1)], vec![]),
    ] {
        let mut b = FixtureBuilder::new();
        register(&mut b, "StringFunctions::Substring");
        let text = sequence(&mut b, &text);
        let lower = sequence(&mut b, &lower);
        let upper = sequence(&mut b, &upper);
        let call = b.invoke("StringFunctions::Substring", &[text, lower, upper]);
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::Extended)?;
        assert!(matches!(ev.evaluate(call, None)?, Outcome::Values(values)
            if matches!(values.as_slice(), [Value::Unevaluated { at, .. }] if *at == call)));
    }
    Ok(())
}

#[test]
fn sequence_equality_and_filtering_preserve_unevaluated_identity() -> Result<(), EvalError> {
    for (op, same, expected) in [
        ("includes", true, true),
        ("includes", false, false),
        ("excludes", false, true),
        ("equals", true, true),
    ] {
        let name = format!("SequenceFunctions::{op}");
        let mut b = FixtureBuilder::new();
        register(&mut b, &name);
        let p = b.package("P");
        let x = b.attribute(p, "x", None);
        let y = b.attribute(p, "y", None);
        let a = b.feature_ref_to(x);
        let z = b.feature_ref_to(if same { x } else { y });
        let call = b.invoke(&name, &[a, z]);
        let model = b.build();
        assert!(
            matches!(Evaluator::new(&model,EvalMode::Extended)?.evaluate(call,None)?,Outcome::Values(v) if equal_seq(&v,&[B(expected)]))
        );
    }
    Ok(())
}

#[test]
fn trig_domain_nan_and_signed_infinity_are_preserved() -> Result<(), EvalError> {
    check("TrigFunctions::arcsin", &[&[I(2)]], &[R(f64::NAN)])?;
    check("TrigFunctions::cot", &[&[R(-0.0)]], &[R(f64::NEG_INFINITY)])?;
    check(
        "TrigFunctions::arctan",
        &[&[R(f64::INFINITY)]],
        &[R(std::f64::consts::FRAC_PI_2)],
    )
}

macro_rules! trig_case {
    ($test:ident, $op:literal, $input:expr, $expected:expr) => {
        #[test] fn $test() -> Result<(), EvalError> {
            let name = concat!("TrigFunctions::", $op);
            check(name, &[&[$input]], &[R($expected)])?;
            undefined_argument(name)?;
            for values in [vec![], vec![B(true)], vec![I(1), I(2)]] {
                let mut b = FixtureBuilder::new(); register(&mut b, name);
                let arg = sequence(&mut b, &values); let call = b.invoke(name, &[arg]); let model = b.build();
                assert!(matches!(Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { at, .. }] if *at == call)));
            }
            Ok(())
        }
    };
}
trig_case!(trig_sin, "sin", R(0.0), 0.0);
trig_case!(trig_cos, "cos", I(0), 1.0);
trig_case!(trig_tan, "tan", R(-0.0), -0.0);
trig_case!(trig_cot, "cot", I(0), f64::INFINITY);
trig_case!(trig_arcsin, "arcsin", I(1), std::f64::consts::FRAC_PI_2);
trig_case!(trig_arccos, "arccos", I(1), 0.0);
trig_case!(trig_arctan, "arctan", I(1), std::f64::consts::FRAC_PI_4);
trig_case!(trig_deg, "deg", R(std::f64::consts::PI), 180.0);
trig_case!(trig_rad, "rad", I(180), std::f64::consts::PI);

fn aggregate(name: &str, product: bool) -> Result<(), EvalError> {
    check(
        name,
        &[&[I(2), I(3), I(4)]],
        &[I(if product { 24 } else { 9 })],
    )?;
    check(
        name,
        &[&[I(2), R(0.5), I(4)]],
        &[R(if product { 4.0 } else { 6.5 })],
    )?;
    check(name, &[&[]], &[I(i64::from(product))])?;
    undefined_argument(name)?;
    let mut b = FixtureBuilder::new();
    register(&mut b, name);
    let yes = b.bool(true);
    let call = b.invoke(name, &[yes]);
    let powers: Vec<_> = (0..3)
        .map(|_| {
            let a = b.int(2_000_000_000);
            let z = b.int(2_000_000_000);
            b.op("*", &[a, z])
        })
        .collect();
    let list = b.op(",", &[powers[0], powers[1]]);
    let list = b.op(",", &[list, powers[2]]);
    let overflow = b.invoke(name, &[list]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert!(
        matches!(ev.evaluate(call, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { .. }]))
    );
    assert!(
        matches!(ev.evaluate(overflow, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { reason: Reason::Overflow { .. }, .. }]))
    );
    Ok(())
}
#[test]
fn numerical_sum() -> Result<(), EvalError> {
    aggregate("NumericalFunctions::sum", false)
}
#[test]
fn numerical_product() -> Result<(), EvalError> {
    aggregate("NumericalFunctions::product", true)
}

fn extreme(name: &str, max: bool) -> Result<(), EvalError> {
    check(name, &[&[I(2)], &[I(3)]], &[I(if max { 3 } else { 2 })])?;
    check(
        name,
        &[&[I(2)], &[R(3.0)]],
        &[R(if max { 3.0 } else { 2.0 })],
    )?;
    check(
        name,
        &[&[Value::Str("a".into())], &[Value::Str("b".into())]],
        &[Value::Str(if max { "b" } else { "a" }.into())],
    )?;
    check(name, &[&[R(0.0)], &[R(-0.0)]], &[R(-0.0)])?;
    check(name, &[&[R(f64::NAN)], &[R(4.0)]], &[R(4.0)])?;
    undefined_argument(name)?;
    let mut b = FixtureBuilder::new();
    register(&mut b, name);
    let empty = b.null();
    let one = b.int(1);
    let call = b.invoke(name, &[empty, one]);
    let model = b.build();
    assert!(
        matches!(Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { .. }]))
    );
    Ok(())
}
#[test]
fn data_max() -> Result<(), EvalError> {
    extreme("DataFunctions::max", true)
}
#[test]
fn data_min() -> Result<(), EvalError> {
    extreme("DataFunctions::min", false)
}

fn iteration(op: &str) -> Result<(), EvalError> {
    let name = format!("ControlFunctions::{op}");
    let mut b = FixtureBuilder::new();
    register(&mut b, &name);
    let x = b.parameter("x");
    let xr = b.feature_ref_to(x);
    let body = if op == "reduce" {
        let y = b.parameter("y");
        let yr = b.feature_ref_to(y);
        let add = b.op("+", &[xr, yr]);
        b.body(&[x, y], add)
    } else if matches!(op, "maximize" | "minimize") {
        let two = b.int(2);
        let times = b.op("*", &[xr, two]);
        b.body(&[x], times)
    } else {
        let two = b.int(2);
        let cmp = b.op(">", &[xr, two]);
        b.body(&[x], cmp)
    };
    let body_ref = b.feature_ref_to(body);
    let input = sequence(&mut b, &[I(1), I(3), I(2)]);
    let empty = b.null();
    let call = b.invoke(&name, &[input, body_ref]);
    let body_ref = b.feature_ref_to(body);
    let empty_call = b.invoke(&name, &[empty, body_ref]);
    let input = sequence(&mut b, &[I(1), I(3), I(2)]);
    let empty = b.null();
    let bad_call = b.invoke(&name, &[input, empty]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    let expected = match op {
        "forAll" => vec![B(false)],
        "exists" => vec![B(true)],
        "reject" => vec![I(1), I(2)],
        "selectOne" => vec![I(3)],
        "reduce" => vec![I(6)],
        "maximize" => vec![I(6)],
        "minimize" => vec![I(2)],
        _ => panic!("test op"),
    };
    let result = ev.evaluate(call, None)?;
    assert!(
        matches!(&result, Outcome::Values(v) if equal_seq(v,&expected)),
        "{name}: {result:?}"
    );
    let expected = match op {
        "forAll" => vec![B(true)],
        "exists" => vec![B(false)],
        _ => vec![],
    };
    assert!(matches!(ev.evaluate(empty_call,None)?, Outcome::Values(v) if equal_seq(&v,&expected)));
    assert!(
        matches!(ev.evaluate(bad_call,None)?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Unevaluated { at, .. }] if *at == bad_call))
    );
    undefined_argument(&name)
}
macro_rules! control_case {
    ($test:ident, $op:literal) => {
        #[test]
        fn $test() -> Result<(), EvalError> {
            iteration($op)
        }
    };
}
control_case!(control_for_all, "forAll");
control_case!(control_exists, "exists");
control_case!(control_reject, "reject");
control_case!(control_select_one, "selectOne");
control_case!(control_reduce, "reduce");
control_case!(control_maximize, "maximize");
control_case!(control_minimize, "minimize");

#[test]
fn select_one_evaluates_later_predicates_and_quantifiers_short_circuit() -> Result<(), EvalError> {
    for op in ["selectOne", "forAll", "exists"] {
        let name = format!("ControlFunctions::{op}");
        let mut b = FixtureBuilder::new();
        register(&mut b, &name);
        let x = b.parameter("x");
        let xr = b.feature_ref_to(x);
        let one = b.int(1);
        let first = b.op("==", &[xr, one]);
        let yes = b.bool(op != "forAll");
        let thousand = b.int(1000);
        let one = b.int(1);
        let expensive = b.op("..", &[one, thousand]);
        let conditional = b.op("if", &[first, yes, expensive]);
        let body = b.body(&[x], conditional);
        let body = b.feature_ref_to(body);
        let input = sequence(&mut b, &[I(1), I(2)]);
        let call = b.invoke(&name, &[input, body]);
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::Extended)?.with_limits(EvalLimits {
            steps: 60,
            ..EvalLimits::default()
        });
        if op == "selectOne" {
            assert!(matches!(
                ev.evaluate(call, None),
                Err(EvalError::StepLimit(60))
            ));
        } else {
            assert!(
                matches!(ev.evaluate(call,None)?,Outcome::Values(v) if equal_seq(&v,&[B(op=="exists")]))
            );
        }
    }
    Ok(())
}
