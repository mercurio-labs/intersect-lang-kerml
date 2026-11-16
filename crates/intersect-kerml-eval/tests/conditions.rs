//! Handwritten condition and multiplicity API tests, no loader integration.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{EvalError, EvalLimits, EvalMode, Evaluator, ObjId};

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn text_metadata_filters_and_bounds() -> Result<(), String> {
    let Some(model) = super::text::load(
        r#"package EvalMetadata {
        private import ScalarValues::*;
        metadata def Safety;
        part def P { metadata Safety; }
        part def Other;
        attribute safeFilter = @Safety;
        attribute bounded : Integer[0..*];
    }"#,
    )?
    else {
        return Ok(());
    };
    let ev = Evaluator::new(&model, EvalMode::ModelLevel).map_err(|e| e.to_string())?;
    let p = ev
        .resolve_target("EvalMetadata::P")
        .map_err(|e| e.to_string())?;
    let other = ev
        .resolve_target("EvalMetadata::Other")
        .map_err(|e| e.to_string())?;
    let filter = ev
        .resolve_target("EvalMetadata::safeFilter")
        .map_err(|e| e.to_string())?;
    let bounded = ev
        .resolve_target("EvalMetadata::bounded")
        .map_err(|e| e.to_string())?;
    let sites = intersect_kerml_eval::sites(&model, &["memory:/eval-fixture.sysml"])
        .map_err(|e| e.to_string())?;
    let condition = sites
        .iter()
        .find(|site| {
            model
                .obj(site.expression)
                .and_then(|o| o.container())
                .and_then(|(m, _)| m.container())
                .is_some_and(|(f, _)| f.id() == filter)
        })
        .ok_or("filter expression missing")?
        .expression;
    assert!(
        ev.check_conditions_on(p, &[condition])
            .map_err(|e| e.to_string())?
    );
    assert!(
        !ev.check_conditions_on(other, &[condition])
            .map_err(|e| e.to_string())?
    );
    let bounds: Vec<_> = sites
        .iter()
        .filter(|site| {
            site.kind == intersect_kerml_eval::SiteKind::MultiplicityBound
                && site.target == Some(bounded)
        })
        .map(|site| ev.multiplicity_bound(site.expression))
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    assert_eq!(bounds, [Some(0), Some(-1)]);
    Ok(())
}

fn metaclass(b: &mut FixtureBuilder, p: ObjId, name: &str) -> ObjId {
    let t = b.object("Metaclass");
    b.name(t, Some(p), name);
    b.member(p, "OwningMembership", t);
    t
}
fn metadata(b: &mut FixtureBuilder, p: ObjId, t: ObjId) -> ObjId {
    let m = b.object("MetadataUsage");
    b.member(p, "OwningMembership", m);
    b.typed(m, t);
    m
}
fn filter(b: &mut FixtureBuilder, t: ObjId) -> ObjId {
    // Grammar SelfReferenceExpression: only an empty return parameter,
    // with the library self referent supplied by evaluation navigation.
    let this = b.object("FeatureReferenceExpression");
    let result = b.object("Feature");
    b.member(this, "ReturnParameterMembership", result);
    b.type_test("@", this, t)
}

#[test]
fn filters_match_metadata_types_and_require_every_condition() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let other = b.package("Other");
    let safety = metaclass(&mut b, p, "Safety");
    let audit = metaclass(&mut b, p, "Audit");
    let safe = metadata(&mut b, p, safety);
    metadata(&mut b, p, audit);
    let is_safe = filter(&mut b, safety);
    let is_audited = filter(&mut b, audit);
    let model = b.build();
    for mode in [EvalMode::ModelLevel, EvalMode::Extended] {
        let ev = Evaluator::new(&model, mode)?;
        assert!(ev.check_condition(is_safe, Some(safe))?);
        assert!(!ev.check_condition(is_safe, None)?);
        assert!(ev.check_conditions_on(p, &[is_safe, is_audited])?);
        assert!(!ev.check_conditions_on(other, &[is_safe])?);
        assert!(ev.check_conditions_on(other, &[])?);
    }
    Ok(())
}

#[test]
fn filter_candidates_include_the_virtual_metaclass() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let sysml = b.package("SysML");
    let systems = b.object("Package");
    b.name(systems, Some(sysml), "Systems");
    b.member(sysml, "OwningMembership", systems);
    let part = metaclass(&mut b, systems, "PartDefinition");
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let condition = filter(&mut b, part);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert!(ev.check_conditions_on(t, &[condition])?);
    assert!(!ev.check_conditions_on(p, &[condition])?);
    Ok(())
}

#[test]
fn only_a_single_true_boolean_satisfies_a_condition() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let yes = b.bool(true);
    let no = b.bool(false);
    let null = b.null();
    let one = b.int(1);
    let a = b.bool(true);
    let z = b.bool(true);
    let many = b.op(",", &[a, z]);
    let p = b.package("P");
    let f = b.attribute(p, "unbound", None);
    let unbound = b.feature_ref_to(f);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert!(ev.check_condition(yes, None)?);
    for c in [no, null, one, many, unbound] {
        assert!(!ev.check_condition(c, None)?);
    }
    Ok(())
}

#[test]
fn conditions_remain_model_level_in_extended_evaluators() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.function(p, "True", &[]);
    let yes = b.bool(true);
    b.member(f, "ResultExpressionMembership", yes);
    let call = b.invoke("P::True", &[]);
    let null = b.null();
    let defined = b.object("ConstraintUsage");
    b.member(defined, "ResultExpressionMembership", null);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert!(!ev.check_condition(call, None)?);
    assert!(!ev.check_conditions_on(p, &[call])?);
    assert!(!ev.check_condition(defined, None)?);
    Ok(())
}

#[test]
fn condition_limit_is_shared_across_candidates_and_resets_for_each_api_call()
-> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = metaclass(&mut b, p, "M");
    metadata(&mut b, p, t);
    metadata(&mut b, p, t);
    let no = b.bool(false);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        steps: 2,
        ..EvalLimits::default()
    });
    for _ in 0..2 {
        assert!(matches!(
            ev.check_conditions_on(p, &[no]),
            Err(EvalError::StepLimit(2))
        ));
    }
    assert!(!ev.check_condition(no, None)?);
    Ok(())
}

#[test]
fn multiplicity_bounds_accept_naturals_and_infinity_only() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.attribute(p, "x", None);
    let m = b.object("MultiplicityRange");
    b.member(f, "OwningMembership", m);
    let zero = b.int(0);
    b.member(m, "OwningMembership", zero);
    let star = b.infinity();
    b.member(m, "OwningMembership", star);
    let real = b.real(1.5);
    let negative = b.int(-1);
    let empty = b.null();
    let yes = b.bool(true);
    let one = b.int(1);
    let two = b.int(2);
    let many = b.op(",", &[one, two]);
    let function = b.function(p, "bound", &[]);
    let one = b.int(1);
    b.member(function, "ResultExpressionMembership", one);
    let unknown = b.invoke("P::bound", &[]);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::Extended)?;
    assert_eq!(ev.multiplicity_bound(zero)?, Some(0));
    assert_eq!(ev.multiplicity_bound(star)?, Some(-1));
    for invalid in [real, negative, empty, yes, many, unknown] {
        assert_eq!(ev.multiplicity_bound(invalid)?, None);
    }
    Ok(())
}

#[test]
fn multiplicity_bounds_use_the_ranges_owning_namespace() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let t = b.part_def(p, "T");
    let one = b.int(1);
    let base = b.attribute(p, "count", Some(one));
    let five = b.int(5);
    let redefining = b.attribute(t, "count", Some(five));
    b.redefines(redefining, base);
    let range = b.object("MultiplicityRange");
    b.member(t, "OwningMembership", range);
    let bound = b.feature_ref_to(base);
    b.member(range, "OwningMembership", bound);
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_eq!(ev.multiplicity_bound(bound)?, Some(5));
    Ok(())
}
