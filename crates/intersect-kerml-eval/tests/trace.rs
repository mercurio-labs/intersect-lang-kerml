//! Handwritten deterministic observer, stop, reason and source-position cases.
use super::{data::max_integer, fixture::FixtureBuilder};
use intersect_kerml_eval::{
    ContextView, Control, EvalError, EvalLimits, EvalMode, EvalObserver, Evaluator, FrameView,
    Lookup, ObjId, Outcome, Reason, SourceMap, Trace, TraceEvent, Value, format::outcome_json,
    trace_text, value::equal_seq,
};
#[test]
fn scalar_chain_context_retains_self_and_continues_feature_lookup() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let five = b.int(5);
    b.attribute(p, "f", Some(five));
    let one = b.int(1);
    let e = b.chain(one, "P::f");
    let two = b.int(2);
    let this = b.chain(two, "Base::Anything::self");
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, n) in [(e, 5), (this, 2)] {
        assert!(
            matches!(ev.evaluate_observed(e,None,false,&mut Trace::default())?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(n)]))
        );
    }
    Ok(())
}
#[test]
fn conditional_trace_brackets_running_expressions_and_skips_false_branch() -> Result<(), EvalError>
{
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let two = b.int(2);
    let a = b.attribute(p, "a", Some(two));
    let other = b.attribute(p, "b", None);
    let ar = b.feature_ref_to(a);
    let one = b.int(1);
    let test = b.op(">", &[ar, one]);
    let yes = b.feature_ref_to(a);
    let no = b.feature_ref_to(other);
    let e = b.op("if", &[test, yes, no]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    let mut trace = Trace::default();
    let outcome = ev.evaluate_observed(e, None, false, &mut trace)?;
    assert!(matches!(outcome,Outcome::Values(v) if equal_seq(&v,&[Value::Int(2)])));
    let mut stack = Vec::new();
    for event in &trace.events {
        match event {
            TraceEvent::Enter { expression, .. } => {
                assert_ne!(*expression, no);
                stack.push(*expression);
            }
            TraceEvent::Exit { expression, .. } => assert_eq!(stack.pop(), Some(*expression)),
            _ => {}
        }
    }
    assert!(stack.is_empty());
    assert_eq!(trace.events.iter().filter(|event|matches!(event,TraceEvent::Skipped{invocation,operand:2,..} if *invocation==e)).count(),1);
    assert!(trace.events.iter().any(|event|matches!(event,TraceEvent::Lookup{feature,found:Lookup::Value(value),..} if *feature==a && *value==two)));
    assert!(trace.events.iter().any(|event| matches!(
        event,
        TraceEvent::Apply {
            function: "ControlFunctions::if",
            step: 2,
            ..
        }
    )));
    let mut repeat = Trace::default();
    ev.evaluate_observed(e, None, false, &mut repeat)?;
    assert_eq!(
        format!("{:?}", trace.events),
        format!("{:?}", repeat.events)
    );
    Ok(())
}
#[test]
fn short_circuit_and_unbound_lookup_events() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let pkg = b.package("P");
    let t = b.part_def(pkg, "T");
    let one = b.int(1);
    let base = b.attribute(t, "x", Some(one));
    let sub = b.part_def(pkg, "S");
    b.subclassifies(sub, t);
    let redef = b.attribute(sub, "x", None);
    b.redefines(redef, base);
    let reference = b.feature_ref_to(base);
    let no = b.bool(false);
    let x = b.feature_ref_to(base);
    let e = b.op("and", &[no, x]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    let mut trace = Trace::default();
    ev.evaluate_observed(e, Some(sub), false, &mut trace)?;
    assert!(
        trace
            .events
            .iter()
            .any(|e| matches!(e, TraceEvent::Skipped { operand: 1, .. }))
    );
    assert!(
        !trace
            .events
            .iter()
            .any(|e| matches!(e,TraceEvent::Enter{expression,..} if *expression==x))
    );
    let mut trace = Trace::default();
    ev.evaluate_observed(reference, Some(sub), false, &mut trace)?;
    let lookups: Vec<_> = trace
        .events
        .iter()
        .filter(|e| matches!(e, TraceEvent::Lookup { .. }))
        .collect();
    assert!(
        matches!(lookups.as_slice(),[TraceEvent::Lookup{feature,frame:FrameView::Type{ty},found:Lookup::Unbound(f),..}] if *feature==base && *ty==sub && *f==redef)
    );
    assert!(trace_text(&m, &trace, None).contains("unbound"));
    Ok(())
}
#[test]
fn stop_at_step_three_is_repeatable_and_resets() -> Result<(), EvalError> {
    struct Stop;
    impl EvalObserver for Stop {
        fn enter(
            &mut self,
            step: u64,
            _depth: u32,
            _expression: ObjId,
            _context: &ContextView<'_>,
        ) -> Control {
            if step == 3 {
                Control::Stop
            } else {
                Control::Continue
            }
        }
    }
    let mut b = FixtureBuilder::new();
    let a = b.int(1);
    let c = b.int(2);
    let e = b.op("+", &[a, c]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for _ in 0..2 {
        assert_eq!(
            ev.evaluate_observed(e, None, false, &mut Stop).err(),
            Some(EvalError::Stopped(3))
        );
    }
    assert!(matches!(ev.evaluate(e,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(3)])));
    Ok(())
}
#[test]
fn ten_thousand_steps_match_with_and_without_trace() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let a = b.int(1);
    let c = b.int(3331);
    let source = b.op("..", &[a, c]);
    let p = b.parameter("v");
    let r = b.feature_ref_to(p);
    let body = b.body(&[p], r);
    let e = b.collect(source, body);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        steps: 10000,
        ..EvalLimits::default()
    });
    let plain = ev.evaluate(e, None)?;
    let mut trace = Trace::default();
    let observed = ev.evaluate_observed(e, None, false, &mut trace)?;
    assert_eq!(outcome_json(&m, &plain), outcome_json(&m, &observed));
    assert!(matches!(
        trace.events.last(),
        Some(TraceEvent::Exit { step: 10000, .. })
    ));
    assert_eq!(
        trace
            .events
            .iter()
            .filter(|e| matches!(
                e,
                TraceEvent::Lookup {
                    found: Lookup::Bound,
                    ..
                }
            ))
            .count(),
        3331
    );
    Ok(())
}
#[test]
fn fix_5_feature_lookup_is_free_and_range_allocation_is_charged() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("Review");
    let one = b.int(1);
    let a = b.attribute(p, "a", Some(one));
    let reference = b.feature_ref_to(a);
    let missing = b.attribute(p, "x", None);
    let unbound = b.feature_ref_to(missing);
    let first = b.int(1);
    let last = b.int(3);
    let range = b.op("..", &[first, last]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    let mut trace = Trace::default();
    ev.evaluate_observed(reference, None, false, &mut trace)?;
    assert!(matches!(
        trace.events.last(),
        Some(TraceEvent::Exit { step: 2, .. })
    ));
    assert!(
        trace
            .events
            .iter()
            .any(|e| matches!(e, TraceEvent::Lookup { step: 1, .. }))
    );
    let limited = Evaluator::new(&m, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        steps: 1,
        ..EvalLimits::default()
    });
    assert!(matches!(
        limited.evaluate(unbound, None)?,
        Outcome::Values(_)
    ));
    assert_eq!(
        limited.evaluate(reference, None).err(),
        Some(EvalError::StepLimit(1))
    );
    assert!(
        matches!(limited.evaluate_feature(a,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Int(1)]))
    );
    let mut trace = Trace::default();
    ev.evaluate_observed(range, None, false, &mut trace)?;
    assert!(matches!(
        trace.events.last(),
        Some(TraceEvent::Exit { step: 7, .. })
    ));
    let limited = Evaluator::new(&m, EvalMode::ModelLevel)?.with_limits(EvalLimits {
        steps: 6,
        ..EvalLimits::default()
    });
    assert_eq!(
        limited.evaluate(range, None).err(),
        Some(EvalError::StepLimit(6))
    );
    Ok(())
}
#[test]
fn observed_gating_reports_root_result_without_nested_execution() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    b.function(p, "f", &[]);
    let e = b.invoke("P::f", &[]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    let mut trace = Trace::default();
    let result = ev.evaluate_observed(e, None, true, &mut trace)?;
    assert_eq!(
        outcome_json(&m, &result),
        outcome_json(&m, &ev.evaluate_gated(e, None)?)
    );
    assert!(matches!(
        trace.events.as_slice(),
        [
            TraceEvent::Enter { .. },
            TraceEvent::Exit {
                outcome: Outcome::NotEvaluable(Reason::NotModelLevelEvaluable { .. }),
                ..
            }
        ]
    ));
    Ok(())
}
#[test]
fn host_text_supplies_line_column_and_wrapper_mapping() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let uri = "memory:/wrapper.sysml";
    let text = "prefix\nα\n  7\nsuffix";
    let start = text
        .find('7')
        .ok_or_else(|| EvalError::Schema("test source".into()))?;
    let e = b.object_at("LiteralInteger", uri, (start, start + 1));
    b.set(e, "value", 7.into());
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    let mut trace = Trace::default().with_source(uri, text);
    ev.evaluate_observed(e, None, false, &mut trace)?;
    assert!(trace_text(&m, &trace, None).contains("memory:/wrapper.sysml:3:3"));
    let map = SourceMap {
        uri: uri.into(),
        prefix_bytes: 7,
        expression_len: "α\n  7".len(),
    };
    assert!(trace_text(&m, &trace, Some(&map)).contains("<expression>:2:3"));
    assert_eq!(map.to_user((0, 1)), None);
    let mut no_text = Trace::default();
    ev.evaluate_observed(e, None, false, &mut no_text)?;
    assert!(trace_text(&m, &no_text, None).contains(":bytes "));
    Ok(())
}
#[test]
fn every_reason_is_produced_and_serializes_its_identity() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let pkg = b.package("P");
    b.function(pkg, "f", &[]);
    let unknown = b.invoke("P::f", &[]);
    let gated = b.invoke("P::f", &[]);
    let n = b.null();
    let one = b.int(1);
    let shape = b.op("+", &[n, one]);
    let s = b.string("a");
    let one = b.int(1);
    let kinds = b.op("+", &[s, one]);
    let a = b.string("a");
    let c = b.string("b");
    let undefined = b.op("-", &[a, c]);
    let f = b.attribute(pkg, "x", None);
    let unbound = b.feature_ref_to(f);
    let missing = b.object("MetadataAccessExpression");
    let empty = b.object("Expression");
    let max = max_integer(&mut b);
    let one = b.int(1);
    let overflow = b.op("+", &[max, one]);
    let one = b.int(1);
    let s = b.string("s");
    let index = b.index(one, s);
    let one = b.int(1);
    let ty = b.op("istype", &[one]);
    b.attribute(pkg, "qualifiedName", None);
    let metadata = b.metadata_access("P");
    let unsupported = b.chain(metadata, "P::qualifiedName");
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, gate, name) in [
        (unknown, false, "unknown-function"),
        (gated, true, "not-model-level-evaluable"),
        (shape, false, "operand-not-single"),
        (kinds, false, "operand-kinds"),
        (undefined, false, "operation-undefined"),
        (unbound, false, "no-value"),
        (missing, false, "no-referent"),
        (empty, false, "no-result-expression"),
        (overflow, false, "overflow"),
        (index, false, "bad-index"),
        (ty, false, "type-argument-missing"),
        (unsupported, false, "unsupported"),
    ] {
        let result = ev.evaluate_observed(e, None, gate, &mut Trace::default())?;
        let reason = match &result {
            Outcome::NotEvaluable(r) => Some(r),
            Outcome::Values(v) => match v.as_slice() {
                [Value::Unevaluated { reason, .. }] => Some(reason),
                _ => None,
            },
        };
        assert!(reason.is_some(), "{result:?}");
        let json = outcome_json(&m, &result);
        let r = if gate || matches!(result, Outcome::NotEvaluable(_)) {
            &json["reason"]
        } else {
            &json["values"][0]["reason"]
        };
        assert_eq!(r["reason"], name);
        assert!(r.to_string().contains("fixture:"));
    }
    Ok(())
}
#[test]
fn trace_text_names_elements_and_prints_pilot_values() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let two = b.int(2);
    let a = b.attribute(p, "a", Some(two));
    let ar = b.feature_ref_to(a);
    let one = b.int(1);
    let test = b.op(">", &[ar, one]);
    let three = b.int(3);
    let zero = b.int(0);
    let e = b.op("if", &[test, three, zero]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    let mut trace = Trace::default();
    ev.evaluate_observed(e, None, false, &mut trace)?;
    let text = trace_text(&m, &trace, None);
    // No dangling context suffix when there is no frame.
    assert!(
        !text.lines().any(|l| l.trim_end().ends_with(" in")),
        "{text}"
    );
    // Functions are applied to named expressions, never internal identities.
    assert!(
        text.contains("apply ControlFunctions::if for OperatorExpression if"),
        "{text}"
    );
    assert!(
        text.contains("apply DataFunctions::'>' for OperatorExpression >"),
        "{text}"
    );
    assert!(!text.contains("pending"), "{text}");
    // Exit lines use Pilot-style value text, not JSON.
    assert!(
        text.contains("exit OperatorExpression > = LiteralBoolean true"),
        "{text}"
    );
    assert!(
        text.contains("exit OperatorExpression if = LiteralInteger 3"),
        "{text}"
    );
    assert!(!text.contains("\"outcome\""), "{text}");
    assert!(
        text.contains("skip operand 2 of OperatorExpression if (not evaluated)"),
        "{text}"
    );
    Ok(())
}
