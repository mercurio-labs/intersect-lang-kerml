//! Handwritten feature chains, lazy operators and body iteration (REF 2.1).
use super::LibraryFunction;
use crate::{
    EvalError, Evaluator, ObjId, Outcome, Reason, Value,
    eval::{Context, unevaluated},
};
pub(crate) struct Control;
impl LibraryFunction for Control {
    fn names(&self) -> &'static [&'static str] {
        &[
            "ControlFunctions::'.'",
            "ControlFunctions::if",
            "ControlFunctions::and",
            "ControlFunctions::or",
            "ControlFunctions::implies",
            "ControlFunctions::'??'",
            "ControlFunctions::collect",
            "ControlFunctions::select",
        ]
    }
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        let name = ev
            .function_name(inv)?
            .ok_or_else(|| ev.nav.error(inv, "missing control function"))?;
        let op = name
            .rsplit("::")
            .next()
            .map_or("", |s| s.trim_matches('\''));
        if op != "." {
            return control(ev, inv, cx, op);
        }
        let Outcome::Values(values) = ev.evaluate_argument(inv, 0, cx)? else {
            return Ok(unevaluated(
                inv,
                Reason::OperandNotSingle {
                    invocation: inv,
                    operand: 0,
                    count: 0,
                },
            ));
        };
        let Some(target) = ev.nav.referent(inv)? else {
            return Ok(unevaluated(inv, Reason::NoReferent { expression: inv }));
        };
        if !ev.nav.is(target, "Feature")? {
            return Ok(unevaluated(
                inv,
                Reason::OperandKinds {
                    invocation: inv,
                    kinds: vec!["non-Feature target"],
                },
            ));
        }
        let mut tail = ev.nav.chaining_features(target)?;
        if tail.is_empty() {
            tail.push(target);
        }
        let mut out = Vec::new();
        for value in values {
            match value {
                Value::Metaclass(element) => {
                    let next = ev.metaclass_context(element, cx);
                    if let Outcome::Values(mut values) = ev.feature_chain(&tail, &next)? {
                        out.append(&mut values);
                    }
                }
                Value::Element(e) | Value::Unevaluated { at: e, .. }
                    if ev.nav.is(e, "Feature")? =>
                {
                    let mut chain = vec![e];
                    chain.extend(&tail);
                    if let Outcome::Values(mut values) = ev.feature_chain(&chain, cx)? {
                        out.append(&mut values);
                    }
                }
                // Literal values are Features and Types in the Pilot too.
                Value::Bool(_)
                | Value::Int(_)
                | Value::Real(_)
                | Value::Str(_)
                | Value::Infinity => {
                    let next = ev.scalar_context(value, inv, cx);
                    if let Outcome::Values(mut values) = ev.feature_chain(&tail, &next)? {
                        out.append(&mut values);
                    }
                }
                _ => {}
            }
        }
        Ok(Outcome::Values(out))
    }
}
fn bad(inv: ObjId) -> Outcome {
    unevaluated(
        inv,
        Reason::OperandKinds {
            invocation: inv,
            kinds: vec!["control operand"],
        },
    )
}
fn control(ev: &Evaluator<'_>, inv: ObjId, cx: &Context, op: &str) -> Result<Outcome, EvalError> {
    let first = ev.evaluate_argument(inv, 0, cx)?;
    let boolean = super::boolean_value(&first);
    match op {
        "if" => {
            let Some(test) = boolean else {
                cx.skipped(inv, 1)?;
                cx.skipped(inv, 2)?;
                return Ok(bad(inv));
            };
            cx.skipped(inv, if test { 2 } else { 1 })?;
            Ok(ev
                .expression_value(inv, if test { 1 } else { 2 }, cx)?
                .unwrap_or_else(|| {
                    Outcome::NotEvaluable(Reason::OperandKinds {
                        invocation: inv,
                        kinds: vec!["non-Expression branch"],
                    })
                }))
        }
        "and" | "or" | "implies" => {
            if (op == "and" || op == "implies") && boolean != Some(true) {
                cx.skipped(inv, 1)?;
                return Ok(Outcome::Values(vec![Value::Bool(op == "implies")]));
            }
            if op == "or" && boolean == Some(true) {
                cx.skipped(inv, 1)?;
                return Ok(Outcome::Values(vec![Value::Bool(true)]));
            }
            Ok(match ev.boolean_expression_value(inv, 1, cx)? {
                Some(b) => Outcome::Values(vec![Value::Bool(b)]),
                None => bad(inv),
            })
        }
        "??" => match first {
            Outcome::NotEvaluable(_) => {
                cx.skipped(inv, 1)?;
                Ok(bad(inv))
            }
            Outcome::Values(v) if v.is_empty() => {
                Ok(ev.expression_value(inv, 1, cx)?.unwrap_or_else(|| {
                    Outcome::NotEvaluable(Reason::OperandKinds {
                        invocation: inv,
                        kinds: vec!["non-Expression fallback"],
                    })
                }))
            }
            values => {
                cx.skipped(inv, 1)?;
                Ok(values)
            }
        },
        "collect" | "select" => {
            let Outcome::Values(values) = first else {
                cx.skipped(inv, 1)?;
                return Ok(bad(inv));
            };
            let second = ev.evaluate_argument(inv, 1, cx)?;
            if values.is_empty() {
                cx.skipped(inv, 1)?;
            }
            let mut out = Vec::new();
            for value in values {
                let result = match super::argument_value(&second) {
                    Some(Value::Element(body) | Value::Unevaluated { at: body, .. })
                        if ev.nav.is(*body, "Expression")? =>
                    {
                        ev.evaluate_expression(*body, cx, std::slice::from_ref(&value))?
                    }
                    // Pilot literals are themselves Expressions; evaluating a
                    // literal body returns that literal, ignoring its argument.
                    Some(
                        v @ (Value::Bool(_)
                        | Value::Int(_)
                        | Value::Real(_)
                        | Value::Str(_)
                        | Value::Infinity),
                    ) => Outcome::Values(vec![v.clone()]),
                    _ => return Ok(bad(inv)),
                };
                if op == "select" {
                    if super::boolean_value(&result) == Some(true) {
                        out.push(value);
                    }
                } else if let Outcome::Values(mut v) = result {
                    out.append(&mut v);
                }
            }
            // Even empty sequences require a valid body expression.
            if !matches!(
                super::argument_value(&second),
                Some(
                    Value::Bool(_)
                        | Value::Int(_)
                        | Value::Real(_)
                        | Value::Str(_)
                        | Value::Infinity
                )
            ) {
                match super::argument_value(&second) {
                    Some(Value::Element(b) | Value::Unevaluated { at: b, .. })
                        if ev.nav.is(*b, "Expression")? => {}
                    _ => return Ok(bad(inv)),
                }
            }
            Ok(Outcome::Values(out))
        }
        _ => Err(ev.nav.error(inv, "unrecognized control implementation")),
    }
}
