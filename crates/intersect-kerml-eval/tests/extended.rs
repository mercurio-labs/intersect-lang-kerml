//! Handwritten extended invocation cases (M4.1), using validated model fixtures.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, ObjId, Outcome, Reason, Trace, TraceEvent, Value,
    value::equal_seq,
};

fn calc(b: &mut FixtureBuilder, package: ObjId, name: &str, parameters: &[&str]) -> ObjId {
    let definition = b.object("CalculationDefinition");
    b.name(definition, Some(package), name);
    b.member(package, "OwningMembership", definition);
    for name in parameters {
        let parameter = b.parameter(name);
        b.name(parameter, Some(definition), name);
        b.member(definition, "FeatureMembership", parameter);
    }
    let result = b.object("Feature");
    b.name(result, Some(definition), "result");
    b.enumeration(result, "direction", "out");
    b.member(definition, "ReturnParameterMembership", result);
    definition
}

fn assert_values(outcome: Outcome, expected: &[Value]) {
    assert!(
        matches!(outcome, Outcome::Values(ref actual) if equal_seq(actual, expected)),
        "{outcome:?}"
    );
}

#[test]
fn user_calculation_adds_arguments_only_in_extended_mode() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let add = calc(&mut b, package, "Add", &["a", "b"]);
    let a = b.feature_ref("P::Add::a");
    let second = b.feature_ref("P::Add::b");
    let sum = b.op("+", &[a, second]);
    b.member(add, "ResultExpressionMembership", sum);
    let one = b.int(1);
    let two = b.int(2);
    let call = b.invoke("P::Add", &[one, two]);
    let model = b.build();
    let ml = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(!ml.is_model_level_evaluable(call)?);
    assert!(matches!(ml.evaluate(call, None)?, Outcome::Values(v)
        if matches!(v.as_slice(), [Value::Unevaluated { at, reason: Reason::UnknownFunction { .. } }] if *at == call)));
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_values(ev.evaluate(call, None)?, &[Value::Int(3)]);
    assert_values(ev.evaluate_gated(call, None)?, &[Value::Int(3)]);
    assert!(!ev.is_model_level_evaluable(call)?);
    Ok(())
}

#[test]
fn named_arguments_bind_formals_but_evaluate_in_owned_order() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let subtract = calc(&mut b, package, "Subtract", &["a", "b"]);
    let a = b.feature_ref("P::Subtract::a");
    let second = b.feature_ref("P::Subtract::b");
    let difference = b.op("-", &[a, second]);
    b.member(subtract, "ResultExpressionMembership", difference);
    let call = b.invoke("P::Subtract", &[]);
    let two = b.int(2);
    let second_arg = b.operand(call, two);
    b.redefines(second_arg, b.named("P::Subtract::b"));
    let five = b.int(5);
    let first_arg = b.operand(call, five);
    b.redefines(first_arg, b.named("P::Subtract::a"));
    let model = b.build();
    let mut trace = Trace::default();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_values(
        ev.evaluate_observed(call, None, false, &mut trace)?,
        &[Value::Int(3)],
    );
    let arguments: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Enter { expression, .. } if *expression == two || *expression == five => {
                Some(*expression)
            }
            _ => None,
        })
        .collect();
    assert_eq!(arguments, vec![two, five]);
    Ok(())
}

#[test]
fn factorial_recursion_to_ten_is_bounded_and_repeatable() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let fact = calc(&mut b, package, "Fact", &["n"]);
    let n = b.feature_ref("P::Fact::n");
    let one = b.int(1);
    let test = b.op("<=", &[n, one]);
    let n = b.feature_ref("P::Fact::n");
    let one = b.int(1);
    let decrement = b.op("-", &[n, one]);
    let recurse = b.invoke("P::Fact", &[decrement]);
    let n = b.feature_ref("P::Fact::n");
    let product = b.op("*", &[n, recurse]);
    let one = b.int(1);
    let body = b.op("if", &[test, one, product]);
    b.member(fact, "ResultExpressionMembership", body);
    let ten = b.int(10);
    let call = b.invoke("P::Fact", &[ten]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    for _ in 0..2 {
        assert_values(ev.evaluate(call, None)?, &[Value::Int(3_628_800)]);
    }
    Ok(())
}

#[test]
fn recursion_without_base_case_returns_depth_limit() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let forever = calc(&mut b, package, "Forever", &[]);
    let recursive = b.invoke("P::Forever", &[]);
    b.member(forever, "ResultExpressionMembership", recursive);
    let call = b.invoke("P::Forever", &[]);
    let one = b.int(1);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?.with_limits(EvalLimits {
        depth: 32,
        ..EvalLimits::default()
    });
    assert!(matches!(
        ev.evaluate(call, None),
        Err(EvalError::DepthLimit(32))
    ));
    assert_values(ev.evaluate(one, None)?, &[Value::Int(1)]);
    Ok(())
}

#[test]
fn omitted_input_and_inout_are_null_only_with_a_context() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let pair = calc(&mut b, package, "Pair", &["a", "b"]);
    let second = b.named("P::Pair::b");
    b.enumeration(second, "direction", "inout");
    let a = b.feature_ref("P::Pair::a");
    let second_ref = b.feature_ref_to(second);
    let body = b.op(",", &[a, second_ref]);
    b.member(pair, "ResultExpressionMembership", body);
    let one = b.int(1);
    let call = b.invoke("P::Pair", &[one]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_values(ev.evaluate(call, None)?, &[Value::Int(1)]);
    assert_values(ev.evaluate_feature(second, Some(pair))?, &[]);
    assert!(
        matches!(ev.evaluate_feature(second, None)?, Outcome::Values(v)
        if matches!(v.as_slice(), [Value::Unevaluated { at, .. }] if *at == second))
    );
    let ml = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(
        matches!(ml.evaluate_feature(second, Some(pair))?, Outcome::Values(v)
        if matches!(v.as_slice(), [Value::Unevaluated { at, .. }] if *at == second))
    );
    Ok(())
}

#[test]
fn omitted_recursive_parameter_does_not_capture_outer_argument() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let f = calc(&mut b, package, "F", &["n", "tail"]);
    let n = b.feature_ref("P::F::n");
    let zero = b.int(0);
    let test = b.op(">", &[n, zero]);
    let zero = b.int(0);
    let recursive = b.invoke("P::F", &[zero]);
    let tail = b.feature_ref("P::F::tail");
    let body = b.op("if", &[test, recursive, tail]);
    b.member(f, "ResultExpressionMembership", body);
    let one = b.int(1);
    let outer_tail = b.int(42);
    let call = b.invoke("P::F", &[one, outer_tail]);
    let model = b.build();
    assert_values(
        Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?,
        &[],
    );
    Ok(())
}

#[test]
fn omitted_arguments_use_defaults_but_unencodable_actuals_hide_them() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let identity = calc(&mut b, package, "Identity", &["x"]);
    let nine = b.int(9);
    b.value(b.named("P::Identity::x"), nine);
    let body = b.feature_ref("P::Identity::x");
    b.member(identity, "ResultExpressionMembership", body);
    calc(&mut b, package, "Abstract", &[]);
    let unknown = b.invoke("P::Abstract", &[]);
    let call = b.invoke("P::Identity", &[unknown]);
    let omitted = b.invoke("P::Identity", &[]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_values(ev.evaluate(omitted, None)?, &[Value::Int(9)]);
    assert_values(ev.evaluate(call, None)?, &[]);
    Ok(())
}

#[test]
fn even_unused_arguments_are_evaluated_eagerly() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let ignore = calc(&mut b, package, "Ignore", &["x"]);
    let seven = b.int(7);
    b.member(ignore, "ResultExpressionMembership", seven);
    let one = b.int(1);
    let thousand = b.int(1000);
    let range = b.op("..", &[one, thousand]);
    let call = b.invoke("P::Ignore", &[range]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?.with_limits(EvalLimits {
        steps: 20,
        ..EvalLimits::default()
    });
    assert!(matches!(
        ev.evaluate(call, None),
        Err(EvalError::StepLimit(20))
    ));
    Ok(())
}

#[test]
fn arguments_use_caller_values_before_the_callee_feature_frame() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let caller = b.part_def(package, "Caller");
    let three = b.int(3);
    let m = b.attribute(caller, "m", Some(three));
    b.set(m, "direction", serde_json::Value::Null);
    let identity = calc(&mut b, package, "Identity", &["x"]);
    let body = b.feature_ref("P::Identity::x");
    b.member(identity, "ResultExpressionMembership", body);
    let callee = b.object("CalculationUsage");
    b.name(callee, Some(package), "callee");
    b.member(package, "OwningMembership", callee);
    b.typed(callee, identity);
    let seven = b.int(7);
    let redefined = b.attribute(callee, "m", Some(seven));
    b.set(redefined, "direction", serde_json::Value::Null);
    b.redefines(redefined, m);
    let actual = b.feature_ref_to(m);
    let call = b.invoke("P::callee", &[actual]);
    let model = b.build();
    assert_values(
        Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, Some(caller))?,
        &[Value::Int(3)],
    );
    Ok(())
}

#[test]
fn inherited_constraint_body_uses_the_invoked_feature_context() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let constraint = b.object("ConstraintDefinition");
    b.name(constraint, Some(package), "Positive");
    b.member(package, "OwningMembership", constraint);
    let base_value = b.attribute(constraint, "value", None);
    let value = b.feature_ref_to(base_value);
    let zero = b.int(0);
    let body = b.op(">", &[value, zero]);
    b.member(constraint, "ResultExpressionMembership", body);
    let usage = b.object("ConstraintUsage");
    b.name(usage, Some(package), "check");
    b.member(package, "OwningMembership", usage);
    b.typed(usage, constraint);
    let three = b.int(3);
    let redefined = b.attribute(usage, "value", Some(three));
    b.redefines(redefined, base_value);
    let call = b.invoke("P::check", &[]);
    let model = b.build();
    assert_values(
        Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?,
        &[Value::Bool(true)],
    );
    Ok(())
}

#[test]
fn inlined_self_has_the_instantiated_type() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let function = calc(&mut b, package, "SelfType", &[]);
    let this = b.feature_ref("Base::Anything::self");
    let test = b.type_test("hastype", this, function);
    b.member(function, "ResultExpressionMembership", test);
    let call = b.invoke("P::SelfType", &[]);
    let model = b.build();
    assert_values(
        Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?,
        &[Value::Bool(true)],
    );
    Ok(())
}

#[test]
fn library_functions_outside_the_dispatch_table_inline_their_bodies() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("CollectionFunctions");
    let function = b.function(package, "size", &[]);
    let body = b.int(17);
    b.member(function, "ResultExpressionMembership", body);
    let call = b.invoke("CollectionFunctions::size", &[]);
    let model = b.build();
    assert_values(
        Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, None)?,
        &[Value::Int(17)],
    );
    Ok(())
}

#[test]
fn inlined_application_costs_a_step_and_missing_body_stays_unevaluated() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let function = calc(&mut b, package, "Constant", &[]);
    let body = b.int(5);
    b.member(function, "ResultExpressionMembership", body);
    let call = b.invoke("P::Constant", &[]);
    let abstract_function = calc(&mut b, package, "Abstract", &[]);
    let abstract_call = b.invoke("P::Abstract", &[]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?.with_limits(EvalLimits {
        steps: 2,
        ..EvalLimits::default()
    });
    assert!(matches!(
        ev.evaluate(call, None),
        Err(EvalError::StepLimit(2))
    ));
    assert!(
        matches!(ev.evaluate(abstract_call, None)?, Outcome::Values(v)
        if matches!(v.as_slice(), [Value::Unevaluated { at, reason: Reason::NoResultExpression { expression } }] if *at == abstract_call && *expression == abstract_function))
    );
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn text_extended_calculations_named_arguments_recursion_and_constraint() -> Result<(), String> {
    let Some(model) = super::text::load(
        r#"package ExtendedText {
            private import ScalarValues::*;
            calc def Add {
                in a : Integer; in b : Integer;
                return : Integer = a + b;
            }
            calc def Fact {
                in n : Integer;
                return : Integer = if n <= 1? 1 else n * Fact(n - 1);
            }
            calc def Pair {
                in x : Integer; in y : Integer;
                return : Integer[*] = (x, y);
            }
            constraint def Positive { in x : Integer; x > 0 }
            constraint check : Positive { in :>> x = 3; }
            attribute sum = Add(1, 2);
            attribute named = Add(b=2, a=1);
            attribute factorial = Fact(10);
            attribute missing : Integer[*] = Pair(1);
            attribute checked = check();
        }"#,
    )?
    else {
        return Ok(());
    };
    let ev = Evaluator::new(&model, EvalMode::Extended).map_err(|e| e.to_string())?;
    for (name, expected) in [
        ("sum", Value::Int(3)),
        ("named", Value::Int(3)),
        ("factorial", Value::Int(3_628_800)),
        ("missing", Value::Int(1)),
        ("checked", Value::Bool(true)),
    ] {
        let feature = ev
            .resolve_target(&format!("ExtendedText::{name}"))
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

#[test]
fn m3_eager_feature_arguments_are_referenced_in_the_callee_context() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let inner = b.part_def(package, "Inner");
    let base = b.attribute(inner, "x", None);
    b.set(base, "direction", serde_json::Value::Null);
    let outer = b.attribute(package, "outer", None);
    let three = b.int(3);
    let redefining = b.attribute(outer, "x", Some(three));
    b.redefines(redefining, base);
    let child = b.attribute(outer, "child", None);
    b.typed(child, inner);
    b.set(child, "direction", serde_json::Value::Null);
    let child = b.feature_ref_to(child);
    let argument = b.chain(child, "P::Inner::x");
    let identity = calc(&mut b, package, "Identity", &["value"]);
    let result = b.feature_ref("P::Identity::value");
    b.member(identity, "ResultExpressionMembership", result);
    let call = b.invoke("P::Identity", &[argument]);
    let model = b.build();
    assert_values(
        Evaluator::new(&model, EvalMode::Extended)?.evaluate(call, Some(outer))?,
        &[Value::Int(3)],
    );
    Ok(())
}

#[test]
fn specialized_calculation_binds_inherited_parameters_by_position() -> Result<(), EvalError> {
    // calc def B { in a; in b; a - b }  calc def D :> B { in x; in y; }
    // D's owned parameters implicitly redefine B's by position (KerML), so
    // B's inherited result `a - b` sees D's arguments: D(5, 2) = 3.
    let mut b = FixtureBuilder::new();
    let package = b.package("P");
    let base = calc(&mut b, package, "B", &["a", "b"]);
    let a = b.feature_ref("P::B::a");
    let second = b.feature_ref("P::B::b");
    let difference = b.op("-", &[a, second]);
    b.member(base, "ResultExpressionMembership", difference);
    let derived = calc(&mut b, package, "D", &["x", "y"]);
    b.subclassifies(derived, base);
    let five = b.int(5);
    let two = b.int(2);
    let call = b.invoke("P::D", &[five, two]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_values(ev.evaluate(call, None)?, &[Value::Int(3)]);
    Ok(())
}
