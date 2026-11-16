//! Extended body iteration and left folds (REF 2.2).
use super::{bad, extreme, single};
use crate::functions::{argument_value, boolean_value};
use crate::{EvalError, Evaluator, ObjId, Outcome, Value, eval::Context};

pub(super) fn invoke(
    ev: &Evaluator<'_>,
    inv: ObjId,
    cx: &Context,
    op: &str,
) -> Result<Outcome, EvalError> {
    let first = ev.evaluate_argument(inv, 0, cx)?;
    let second = ev.evaluate_argument(inv, 1, cx)?;
    let (Outcome::Values(values), Some(body)) = (first, argument_value(&second)) else {
        return Ok(bad(inv));
    };
    if !is_body(ev, body)? {
        return Ok(bad(inv));
    }
    let mut selected = vec![];
    let mut accumulator = None;
    for value in values {
        if op == "reduce" && accumulator.is_none() {
            accumulator = Some(value);
            continue;
        }
        let result = if op == "reduce" {
            let Some(previous) = accumulator.take() else {
                return Err(ev.nav.error(inv, "reduce accumulator absent"));
            };
            apply(ev, body, cx, &[previous, value.clone()])?
        } else {
            apply(ev, body, cx, std::slice::from_ref(&value))?
        };
        match op {
            "forAll" | "exists" => {
                let Some(v) = argument_value(&result) else {
                    return Ok(bad(inv));
                };
                let expected = op == "forAll";
                if !matches!(v, Value::Bool(b) if *b == expected) {
                    return Ok(single(Value::Bool(!expected)));
                }
            }
            "reject" | "selectOne" => {
                if boolean_value(&result) == Some(op == "selectOne") {
                    selected.push(value);
                }
            }
            "reduce" => {
                let Some(v) = argument_value(&result) else {
                    return Ok(bad(inv));
                };
                accumulator = Some(v.clone());
            }
            "maximize" | "minimize" => {
                let Some(v) = argument_value(&result) else {
                    return Ok(bad(inv));
                };
                accumulator = Some(match accumulator {
                    None => v.clone(),
                    Some(previous) => {
                        // This is a function application even though no
                        // synthetic model invocation is allocated.
                        cx.step(ev.limits)?;
                        let result = extreme(inv, op == "maximize", &previous, v);
                        let Some(next) = argument_value(&result) else {
                            return Ok(bad(inv));
                        };
                        next.clone()
                    }
                });
            }
            _ => return Err(ev.nav.error(inv, "unknown iteration implementation")),
        }
    }
    Ok(match op {
        "forAll" | "exists" => single(Value::Bool(op == "forAll")),
        "reduce" | "maximize" | "minimize" => Outcome::Values(accumulator.into_iter().collect()),
        "selectOne" => Outcome::Values(selected.into_iter().take(1).collect()),
        _ => Outcome::Values(selected),
    })
}
fn is_body(ev: &Evaluator<'_>, body: &Value) -> Result<bool, EvalError> {
    match body {
        Value::Element(e) | Value::Unevaluated { at: e, .. } => ev.nav.is(*e, "Expression"),
        Value::Metaclass(_) => Ok(false),
        _ => Ok(true),
    }
}
fn apply(
    ev: &Evaluator<'_>,
    body: &Value,
    cx: &Context,
    args: &[Value],
) -> Result<Outcome, EvalError> {
    match body {
        Value::Element(e) | Value::Unevaluated { at: e, .. } => {
            ev.evaluate_expression(*e, cx, args)
        }
        _ => {
            cx.step(ev.limits)?;
            Ok(single(body.clone()))
        }
    }
}
