//! Handwritten milestone text cases.
#[path = "support/text_fixture.rs"]
mod text_fixture;
pub use text_fixture::load;

#[test]
fn shared_host_queries_resolve_escaped_names_and_report_missing_or_ambiguous_targets() {
    use super::fixture::FixtureBuilder;
    use intersect_kerml_eval::{EvalMode, Evaluator};
    let mut b = FixtureBuilder::new();
    let pkg = b.package("P");
    let ty = b.part_def(pkg, "a b");
    let base = b.attribute(ty, "m", None);
    let sub = b.part_def(pkg, "Q");
    let redefining = b.object("AttributeUsage");
    b.member(sub, "FeatureMembership", redefining);
    b.redefines(redefining, base);
    let model = b.build();
    let evaluator = Evaluator::new(&model, EvalMode::ModelLevel).unwrap();
    assert_eq!(
        evaluator.qualified_name(ty).unwrap().as_deref(),
        Some("P::'a b'")
    );
    assert_eq!(evaluator.resolve_target("'P'::'a b'").unwrap(), ty);
    assert_eq!(
        evaluator.qualified_name(redefining).unwrap().as_deref(),
        Some("P::Q::m")
    );
    assert_eq!(evaluator.resolve_target("P::Q::m").unwrap(), redefining);
    assert!(evaluator.resolve_target("Missing").is_err());
    assert!(evaluator.resolve_target("P; import X").is_err());
    let mut b = FixtureBuilder::new();
    b.package("Duplicate");
    b.package("Duplicate");
    let model = b.build();
    let evaluator = Evaluator::new(&model, EvalMode::ModelLevel).unwrap();
    assert!(evaluator.resolve_target("Duplicate").is_err());
}

#[test]
fn wrappers_and_source_maps_preserve_the_users_text() {
    use intersect_kerml_eval::eval_source;
    for (target, prefix) in [
        (None, "calc {\n"),
        (Some("P::'a b'"), "calc {private import P::'a b'::*;\n"),
    ] {
        let expression = "'é'\n + 2";
        let (source, map) = eval_source(target, expression, 7).unwrap();
        assert_eq!(source.uri, "memory:/intersect-eval/7.sysml");
        assert_eq!(source.text, format!("{prefix}{expression}}}"));
        assert_eq!(map.uri, source.uri);
        assert_eq!(
            map.to_user((prefix.len(), prefix.len() + expression.len())),
            Some((0, expression.len()))
        );
        assert_eq!(map.to_user((0, prefix.len())), None);
    }
}

#[test]
fn wrapper_rejects_import_injection_and_malformed_qualified_names() {
    use intersect_kerml_eval::eval_source;
    for target in [
        "a; b", "", "a::", "a:::b", "1a", "'open", "a b", "a::*", "a\n", "a/*x*/", "''", "'a\\'",
        "'a'; b",
    ] {
        assert!(eval_source(Some(target), "1", 0).is_err(), "{target}");
    }
    for target in ["A", "_a::b2", "'a b'", "P::'+'", "'a::b'", "'a\\'b'"] {
        assert!(eval_source(Some(target), "1", 0).is_ok(), "{target}");
    }
}

#[test]
fn wrapper_result_discovery_is_resource_scoped_and_requires_a_result_membership() {
    use super::fixture::FixtureBuilder;
    use intersect_kerml_eval::wrapper_expression;
    let mut b = FixtureBuilder::new();
    let calc = b.object("CalculationUsage");
    let result = b.int(3);
    b.member(calc, "ResultExpressionMembership", result);
    let model = b.build();
    assert_eq!(
        wrapper_expression(&model, "memory:/eval.kerml").unwrap(),
        result
    );
    assert!(wrapper_expression(&model, "memory:/absent").is_err());
    let mut b = FixtureBuilder::new();
    let calc = b.object("CalculationUsage");
    let param = b.parameter("result");
    b.member(calc, "ReturnParameterMembership", param);
    let value = b.int(1);
    b.value(param, value);
    assert!(wrapper_expression(&b.build(), "memory:/eval.kerml").is_err());
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn expression_wrappers_evaluate_with_and_without_target() -> Result<(), String> {
    use intersect_kerml_eval::{
        EvalMode, Evaluator, Outcome, Value, eval_source, wrapper_expression,
    };
    for (target, expression, expected) in [
        (None, "1 + 2", 3),
        (None, "if true? 1 else 2", 1),
        (Some("P"), "m * 2", 10),
    ] {
        let (source, _) = eval_source(target, expression, 0).map_err(|e| e.to_string())?;
        let uri = source.uri.clone();
        let Some(model) = text_fixture::load_sources(&[
            intersect_text::Source {
                uri: "memory:/context.sysml".into(),
                text: "part def P { attribute m = 5; }".into(),
            },
            source,
        ])?
        else {
            return Ok(());
        };
        let expr = wrapper_expression(&model, &uri).map_err(|e| e.to_string())?;
        let target = if target.is_some() {
            Some(
                model
                    .objects()
                    .find(|o| {
                        o.resource() == "memory:/context.sysml"
                            && o.class_key().ends_with("PartDefinition")
                    })
                    .ok_or("missing target P")?
                    .id(),
            )
        } else {
            None
        };
        let evaluator = Evaluator::new(&model, EvalMode::ModelLevel).map_err(|e| e.to_string())?;
        assert!(
            matches!(evaluator.evaluate(expr, target).map_err(|e| e.to_string())?, Outcome::Values(v) if matches!(v.as_slice(), [Value::Int(n)] if *n == expected))
        );
    }
    Ok(())
}

#[test]
fn fix_10_pack_documentation_requires_library_and_matching_sources() {
    // A documentation contract only: no pack, parser or Java is used here.
    for documentation in [
        include_str!("support/text_fixture.rs"),
        include_str!("../README.md"),
    ] {
        assert!(documentation.contains("INTERSECT_EVAL_PACK"));
        assert!(documentation.contains("sysml_pack build . PACK --library"));
        assert!(documentation.contains("no library snapshot"));
        assert!(documentation.contains("same sources as the test binary"));
    }
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn text_feature_value_contains_operator_expression() -> Result<(), String> {
    let Some(model) = load("package P { attribute a = 1 + 2; }")? else {
        return Ok(());
    };
    let operator = model
        .metamodel()
        .class("OperatorExpression")
        .ok_or("OperatorExpression class")?;
    assert!(
        model
            .objects()
            .any(|o| o.resource() == "memory:/eval-fixture.sysml"
                && model.metamodel().is_a(o.class(), operator))
    );
    Ok(())
}
