//! Handwritten extended-only functions (REF 2.2).
mod iteration;
mod sequence;
use super::{LibraryFunction, argument_value, real_integer};
use crate::{
    EvalError, Evaluator, ObjId, Outcome, Reason, Value,
    eval::{Context, unevaluated},
};

pub(crate) struct Extended;
impl LibraryFunction for Extended {
    fn names(&self) -> &'static [&'static str] {
        &[
            "ControlFunctions::forAll",
            "ControlFunctions::exists",
            "ControlFunctions::reject",
            "ControlFunctions::selectOne",
            "ControlFunctions::reduce",
            "ControlFunctions::maximize",
            "ControlFunctions::minimize",
            "DataFunctions::max",
            "DataFunctions::min",
            "NumericalFunctions::sum",
            "NumericalFunctions::product",
            "StringFunctions::Length",
            "StringFunctions::Substring",
            "SequenceFunctions::size",
            "SequenceFunctions::isEmpty",
            "SequenceFunctions::notEmpty",
            "SequenceFunctions::includes",
            "SequenceFunctions::includesOnly",
            "SequenceFunctions::excludes",
            "SequenceFunctions::equals",
            "SequenceFunctions::same",
            "SequenceFunctions::including",
            "SequenceFunctions::union",
            "SequenceFunctions::includingAt",
            "SequenceFunctions::excluding",
            "SequenceFunctions::intersection",
            "SequenceFunctions::excludingAt",
            "SequenceFunctions::subsequence",
            "SequenceFunctions::head",
            "SequenceFunctions::last",
            "SequenceFunctions::tail",
            "TrigFunctions::sin",
            "TrigFunctions::cos",
            "TrigFunctions::tan",
            "TrigFunctions::cot",
            "TrigFunctions::arcsin",
            "TrigFunctions::arccos",
            "TrigFunctions::arctan",
            "TrigFunctions::deg",
            "TrigFunctions::rad",
        ]
    }
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        let name = ev
            .function_name(inv)?
            .ok_or_else(|| ev.nav.error(inv, "extended function has no name"))?;
        let (package, op) = name
            .rsplit_once("::")
            .ok_or_else(|| ev.nav.error(inv, "extended function name is not qualified"))?;
        match package {
            "SequenceFunctions" => sequence::invoke(ev, inv, cx, op),
            "ControlFunctions" => iteration::invoke(ev, inv, cx, op),
            "NumericalFunctions" => numerical(ev, inv, cx, op),
            "StringFunctions" => string(ev, inv, cx, op),
            "DataFunctions" => {
                let a = ev.evaluate_argument(inv, 0, cx)?;
                if ev.argument_count(inv, cx)? == 1 {
                    return Ok(match argument_value(&a) {
                        Some(Value::Int(_) | Value::Real(_)) => {
                            Outcome::NotEvaluable(Reason::OperationUndefined { invocation: inv })
                        }
                        _ => Outcome::Values(vec![]),
                    });
                }
                let b = ev.evaluate_argument(inv, 1, cx)?;
                Ok(match (argument_value(&a), argument_value(&b)) {
                    (Some(a), Some(b)) => extreme(inv, op == "max", a, b),
                    _ => bad(inv),
                })
            }
            "TrigFunctions" => {
                let a = ev.evaluate_argument(inv, 0, cx)?;
                let Some(x) = argument_value(&a).and_then(number) else {
                    return Ok(bad(inv));
                };
                let n = match op {
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "cot" => 1.0 / x.tan(),
                    "arcsin" => x.asin(),
                    "arccos" => x.acos(),
                    "arctan" => x.atan(),
                    "deg" => x.to_degrees(),
                    "rad" => x.to_radians(),
                    _ => return Err(ev.nav.error(inv, "unknown trig implementation")),
                };
                Ok(single(Value::Real(n)))
            }
            _ => Err(ev.nav.error(inv, "unknown extended implementation")),
        }
    }
}
fn string(ev: &Evaluator<'_>, inv: ObjId, cx: &Context, op: &str) -> Result<Outcome, EvalError> {
    let argument = ev.evaluate_argument(inv, 0, cx)?;
    // Pilot evaluates both bounds even if the string operand has a wrong kind.
    let bounds = if op == "Substring" {
        Some((
            ev.evaluate_argument(inv, 1, cx)?,
            ev.evaluate_argument(inv, 2, cx)?,
        ))
    } else {
        None
    };
    let Some(Value::Str(text)) = argument_value(&argument) else {
        return Ok(bad(inv));
    };
    let length = i64::try_from(text.encode_utf16().count())
        .map_err(|_| ev.nav.error(inv, "UTF-16 string length exceeds i64"))?;
    let Some((lower, upper)) = bounds else {
        return Ok(single(Value::Int(length)));
    };
    let (Some(Value::Int(lower)), Some(Value::Int(upper))) =
        (argument_value(&lower), argument_value(&upper))
    else {
        return Ok(bad(inv));
    };
    if *lower < 1 || *upper > length || lower.saturating_sub(1) > *upper {
        return Ok(Outcome::Values(vec![]));
    }
    // Valid one-based inclusive bounds include lower == upper + 1, yielding
    // one empty string rather than an empty result list.
    let start = usize::try_from(lower.saturating_sub(1))
        .map_err(|_| ev.nav.error(inv, "substring start exceeds address space"))?;
    let end = usize::try_from(*upper)
        .map_err(|_| ev.nav.error(inv, "substring end exceeds address space"))?;
    let count = end
        .checked_sub(start)
        .ok_or_else(|| ev.nav.error(inv, "substring bounds are reversed"))?;
    let units: Vec<_> = text.encode_utf16().skip(start).take(count).collect();
    // Approved representation limitation: Value::Str is Unicode scalar text,
    // so preserve valid UTF-16 exactly and report split pairs without replacement.
    let result = String::from_utf16(&units).map_err(|_| {
        ev.nav.error(
            inv,
            "Substring result contains an unpaired UTF-16 surrogate",
        )
    })?;
    Ok(single(Value::Str(result.into())))
}
fn bad(inv: ObjId) -> Outcome {
    unevaluated(
        inv,
        Reason::OperandKinds {
            invocation: inv,
            kinds: vec!["extended operand"],
        },
    )
}
fn single(v: Value) -> Outcome {
    Outcome::Values(vec![v])
}
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(real_integer(*n)),
        Value::Real(n) => Some(*n),
        _ => None,
    }
}
fn extreme(inv: ObjId, max: bool, a: &Value, b: &Value) -> Outcome {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => single(Value::Int(if if max { x > y } else { x < y } {
            *x
        } else {
            *y
        })),
        (Value::Str(x), Value::Str(y)) => {
            let order = x.encode_utf16().cmp(y.encode_utf16());
            single(Value::Str(
                if if max { order.is_gt() } else { order.is_lt() } {
                    x.clone()
                } else {
                    y.clone()
                },
            ))
        }
        _ => match (number(a), number(b)) {
            // Pilot uses a comparison, not Math.max/min: ties select the right
            // operand, and NaN is not made contagious when it is on the left.
            (Some(x), Some(y)) => single(Value::Real(if if max { x > y } else { x < y } {
                x
            } else {
                y
            })),
            _ => bad(inv),
        },
    }
}
fn numerical(ev: &Evaluator<'_>, inv: ObjId, cx: &Context, op: &str) -> Result<Outcome, EvalError> {
    let Outcome::Values(values) = ev.evaluate_argument(inv, 0, cx)? else {
        return Ok(bad(inv));
    };
    let mut integer = if op == "sum" { 0i64 } else { 1i64 };
    let mut real = None;
    for value in values {
        match (real, value) {
            (None, Value::Int(n)) => {
                let next = if op == "sum" {
                    integer.checked_add(n)
                } else {
                    integer.checked_mul(n)
                };
                let Some(n) = next else {
                    return Ok(unevaluated(inv, Reason::Overflow { invocation: inv }));
                };
                integer = n;
            }
            (current, value) => {
                let Some(n) = number(&value) else {
                    return Ok(bad(inv));
                };
                let current = current.unwrap_or_else(|| real_integer(integer));
                real = Some(if op == "sum" {
                    current + n
                } else {
                    current * n
                });
            }
        }
    }
    Ok(single(real.map_or(Value::Int(integer), Value::Real)))
}
