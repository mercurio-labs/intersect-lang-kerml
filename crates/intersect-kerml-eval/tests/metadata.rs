//! Handwritten metadata evaluation cases, built without parser or Java.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    EvalError, EvalMode, Evaluator, FrameView, ObjId, Outcome, Trace, TraceEvent, Value,
    value::equal_seq,
};

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn text_metadata_reflection() -> Result<(), String> {
    let Some(model) = super::text::load(
        r#"package EvalMetadata {
        private import ScalarValues::*;
        metadata def Safety { attribute isMandatory : Boolean default false; }
        abstract part def P { metadata Safety { isMandatory = true; } }
        part def Other;
        part p : P;
        attribute marked = P @@ Safety;
        attribute flag = (P meta Safety).isMandatory;
        attribute abstractFlag = (P meta SysML::PartDefinition).isAbstract;
        attribute partKind = p @@ SysML::PartUsage;
    }"#,
    )?
    else {
        return Ok(());
    };
    let ev = Evaluator::new(&model, EvalMode::ModelLevel).map_err(|e| e.to_string())?;
    for name in ["marked", "flag", "abstractFlag", "partKind"] {
        let feature = ev
            .resolve_target(&format!("EvalMetadata::{name}"))
            .map_err(|e| e.to_string())?;
        let result = ev
            .evaluate_feature(feature, None)
            .map_err(|e| e.to_string())?;
        assert!(
            matches!(&result,Outcome::Values(v) if equal_seq(v,&[Value::Bool(true)])),
            "{name}: {result:?}"
        );
    }
    Ok(())
}

fn metaclass(b: &mut FixtureBuilder, owner: ObjId, name: &str) -> ObjId {
    let t = b.object("Metaclass");
    b.name(t, Some(owner), name);
    b.member(owner, "OwningMembership", t);
    t
}
fn systems(b: &mut FixtureBuilder) -> ObjId {
    let root = b.package("SysML");
    let systems = b.object("Package");
    b.name(systems, Some(root), "Systems");
    b.member(root, "OwningMembership", systems);
    systems
}
fn metadata(b: &mut FixtureBuilder, owner: ObjId, name: &str, ty: ObjId) -> ObjId {
    let m = b.object("MetadataUsage");
    b.name(m, Some(owner), name);
    b.member(owner, "OwningMembership", m);
    b.typed(m, ty);
    m
}
fn assert_values(result: Outcome, expected: &[Value]) {
    assert!(
        matches!(&result,Outcome::Values(v) if equal_seq(v,expected)),
        "{result:?}"
    );
}

#[test]
fn metadata_access_and_meta_filter_preserve_order_in_both_modes() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let safety = metaclass(&mut b, p, "Safety");
    let one = metadata(&mut b, p, "one", safety);
    let two = metadata(&mut b, p, "two", safety);
    let access = b.metadata_access("P");
    let list = b.metadata_access("P");
    let filtered = b.type_test("meta", list, safety);
    let list = b.metadata_access("P");
    let present = b.type_test("@@", list, safety);
    let model = b.build();
    for mode in [EvalMode::ModelLevel, EvalMode::Extended] {
        let ev = Evaluator::new(&model, mode)?;
        assert_values(
            ev.evaluate(access, None)?,
            &[
                Value::Element(one),
                Value::Element(two),
                Value::Metaclass(p),
            ],
        );
        assert_values(
            ev.evaluate(filtered, None)?,
            &[Value::Element(one), Value::Element(two)],
        );
        assert_values(ev.evaluate_gated(present, None)?, &[Value::Bool(true)]);
    }
    Ok(())
}

#[test]
fn part_metaclass_supports_type_operators_and_self_without_model_mutation() -> Result<(), EvalError>
{
    let mut b = FixtureBuilder::new();
    let sysml = systems(&mut b);
    let ty = metaclass(&mut b, sysml, "PartUsage");
    let p = b.package("P");
    let part = b.object("PartUsage");
    b.name(part, Some(p), "p");
    b.member(p, "OwningMembership", part);
    let mut calls = vec![];
    for op in ["istype", "hastype", "@", "@@", "as", "meta"] {
        let access = b.metadata_access("P::p");
        calls.push((op, b.type_test(op, access, ty)));
    }
    let access = b.metadata_access("P::p");
    let this = b.chain(access, "Base::Anything::self");
    let model = b.build();
    let count = model.objects().count();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    for (op, call) in calls {
        let expected = if matches!(op, "as" | "meta") {
            Value::Metaclass(part)
        } else {
            Value::Bool(true)
        };
        assert_values(ev.evaluate(call, None)?, &[expected]);
    }
    let mut trace = Trace::default();
    assert_values(
        ev.evaluate_observed(this, None, false, &mut trace)?,
        &[Value::Metaclass(part)],
    );
    // The existing observer representation carries the virtual self binding.
    assert!(trace.events.iter().any(|event| matches!(event,TraceEvent::Enter{context,..} if context.iter().any(|frame| matches!(frame,FrameView::Arguments{bound,..} if bound.iter().any(|(_,v)| matches!(v.as_slice(),[Value::Metaclass(e)] if *e==part))))))
        || trace.events.iter().any(|event| matches!(event,TraceEvent::Exit{outcome:Outcome::Values(v),..} if matches!(v.as_slice(),[Value::Metaclass(e)] if *e==part))));
    assert_eq!(model.objects().count(), count);
    Ok(())
}

#[test]
fn metadata_usage_redefinitions_answer_base_feature_values() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let safety = metaclass(&mut b, p, "Safety");
    let no = b.bool(false);
    let base = b.attribute(safety, "isMandatory", Some(no));
    let usage = metadata(&mut b, p, "safe", safety);
    let yes = b.bool(true);
    let redefining = b.attribute(usage, "isMandatory", Some(yes));
    b.redefines(redefining, base);
    let access = b.metadata_access("P");
    let select = b.type_test("meta", access, safety);
    let call = b.chain(select, "P::Safety::isMandatory");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_values(ev.evaluate(call, None)?, &[Value::Bool(true)]);
    assert_values(
        ev.evaluate_feature(base, Some(usage))?,
        &[Value::Bool(true)],
    );
    Ok(())
}

#[test]
fn meta_filtered_part_definition_reflects_stored_is_abstract() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let sysml = systems(&mut b);
    let ty = metaclass(&mut b, sysml, "PartDefinition");
    b.attribute(ty, "isAbstract", None);
    let p = b.package("P");
    let part = b.part_def(p, "T");
    b.set(part, "isAbstract", true.into());
    let access = b.metadata_access("P::T");
    let select = b.type_test("meta", access, ty);
    let field = b.chain(select, "SysML::Systems::PartDefinition::isAbstract");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_values(ev.evaluate_gated(field, None)?, &[Value::Bool(true)]);
    Ok(())
}

#[test]
fn reflective_attributes_preserve_scalar_kinds_and_real_bits() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let queries = b.part_def(p, "Queries");
    b.attribute(queries, "value", None);
    let mut cases = vec![];
    for (name, value) in [
        ("boolean", Value::Bool(true)),
        ("integer", Value::Int(42)),
        ("string", Value::Str("a😀\nb".into())),
        ("negativeZero", Value::Real(-0.0)),
        ("nan", Value::Real(f64::NAN)),
        ("infinite", Value::Real(f64::INFINITY)),
    ] {
        let object = match &value {
            Value::Bool(v) => b.bool(*v),
            Value::Int(v) => b.int(*v),
            Value::Str(v) => b.string(v),
            Value::Real(v) => b.real(*v),
            _ => panic!("scalar fixture"),
        };
        b.name(object, Some(p), name);
        b.member(p, "OwningMembership", object);
        let access = b.metadata_access(&format!("P::{name}"));
        let field = b.chain(access, "P::Queries::value");
        cases.push((field, value));
    }
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    for (field, value) in cases {
        assert_values(ev.evaluate(field, None)?, &[value]);
    }
    Ok(())
}

#[test]
fn reflective_references_are_metaclasses_and_can_be_chained() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let queries = b.part_def(p, "Queries");
    b.attribute(queries, "ownedRelationship", None);
    b.attribute(queries, "ownedRelatedElement", None);
    b.attribute(queries, "declaredName", None);
    let t = b.part_def(p, "T");
    let a = b.attribute(t, "a", None);
    let z = b.attribute(t, "z", None);
    let access = b.metadata_access("P::T");
    let relationships = b.chain(access, "P::Queries::ownedRelationship");
    let children = b.chain(relationships, "P::Queries::ownedRelatedElement");
    let access = b.metadata_access("P::T");
    let relationships = b.chain(access, "P::Queries::ownedRelationship");
    let child_names = b.chain(relationships, "P::Queries::ownedRelatedElement");
    let child_names = b.chain(child_names, "P::Queries::declaredName");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_values(
        ev.evaluate(children, None)?,
        &[Value::Metaclass(a), Value::Metaclass(z)],
    );
    assert_values(
        ev.evaluate(child_names, None)?,
        &[Value::Str("a".into()), Value::Str("z".into())],
    );
    Ok(())
}

#[test]
fn reflective_unset_and_default_fields_are_empty_or_the_stored_default() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let queries = b.part_def(p, "Queries");
    b.attribute(queries, "declaredShortName", None);
    b.attribute(queries, "isAbstract", None);
    let t = b.part_def(p, "T");
    b.set(t, "declaredShortName", serde_json::Value::Null);
    let access = b.metadata_access("P::T");
    let short = b.chain(access, "P::Queries::declaredShortName");
    let access = b.metadata_access("P::T");
    let default = b.chain(access, "P::Queries::isAbstract");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_values(ev.evaluate(short, None)?, &[]);
    assert_values(ev.evaluate(default, None)?, &[Value::Bool(false)]);
    Ok(())
}

#[test]
fn reflected_enum_null_items_are_explicitly_unsupported() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let queries = b.part_def(p, "Queries");
    b.attribute(queries, "direction", None);
    let feature = b.attribute(p, "input", None);
    b.enumeration(feature, "direction", "in");
    let access = b.metadata_access("P::input");
    let field = b.chain(access, "P::Queries::direction");
    let unset = b.attribute(p, "unset", None);
    b.set(unset, "direction", serde_json::Value::Null);
    let access = b.metadata_access("P::unset");
    let unset_field = b.chain(access, "P::Queries::direction");
    let model = b.build();
    for mode in [EvalMode::ModelLevel, EvalMode::Extended] {
        let ev = Evaluator::new(&model, mode)?;
        assert_values(ev.evaluate(unset_field, None)?, &[]);
        assert!(matches!(ev.evaluate(field, None)?, Outcome::Values(values)
            if matches!(values.as_slice(), [Value::Unevaluated {
                reason: intersect_kerml_eval::Reason::Unsupported {
                    element, what: "enum reflection produces a null item in the Pilot"
                }, ..
            }] if *element == feature)));
    }
    Ok(())
}

#[test]
fn reflective_lookup_uses_declared_names_and_reports_derived_fields_explicitly()
-> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let queries = b.part_def(p, "Queries");
    let base = b.attribute(queries, "isAbstract", None);
    let unnamed = b.object("Feature");
    b.redefines(unnamed, base);
    let seven = b.int(7);
    b.value(unnamed, seven);
    let t = b.part_def(p, "T");
    let access = b.metadata_access("P::T");
    let field = b.object("FeatureChainExpression");
    b.set(field, "operator", ".".into());
    b.operand(field, access);
    let membership = b.object("Membership");
    b.reference(field, "ownedRelationship", membership);
    b.reference(membership, "memberElement", unnamed);
    b.attribute(queries, "qualifiedName", None);
    let access = b.metadata_access("P::T");
    let derived = b.chain(access, "P::Queries::qualifiedName");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_values(ev.evaluate(field, None)?, &[Value::Int(7)]);
    assert!(
        matches!(ev.evaluate(derived,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{reason:intersect_kerml_eval::Reason::Unsupported{element,what:"derived metaclass feature"},..}] if *element==t))
    );
    Ok(())
}

#[test]
fn annotated_element_returns_metaclasses_for_metadata_and_virtual_contexts() -> Result<(), EvalError>
{
    let mut b = FixtureBuilder::new();
    let pkg = b.package("Metaobjects");
    let base = metaclass(&mut b, pkg, "Metaobject");
    let field = b.attribute(base, "annotatedElement", None);
    let p = b.package("P");
    let usage = metadata(&mut b, p, "m", base);
    let access = b.metadata_access("P");
    let call = b.chain(access, "Metaobjects::Metaobject::annotatedElement");
    let model = b.build();
    let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
    assert_values(
        ev.evaluate_feature(field, Some(usage))?,
        &[Value::Metaclass(p)],
    );
    assert_values(
        ev.evaluate(call, None)?,
        &[Value::Metaclass(p), Value::Metaclass(p)],
    );
    Ok(())
}
