//! Handwritten arithmetic, relational functions and inclusive ranges (REF 2.1).
use super::{LibraryFunction, argument_value, kind, real_integer, shape_reason};
use crate::{
    EvalError, Evaluator, ObjId, Outcome, Reason, Value,
    eval::{Context, unevaluated},
};

pub(crate) struct Data;
impl LibraryFunction for Data {
    fn names(&self) -> &'static [&'static str] {
        &[
            "DataFunctions::'+'",
            "DataFunctions::'-'",
            "DataFunctions::'*'",
            "DataFunctions::'/'",
            "DataFunctions::'**'",
            "DataFunctions::'^'",
            "DataFunctions::'%'",
            "DataFunctions::'<'",
            "DataFunctions::'<='",
            "DataFunctions::'>'",
            "DataFunctions::'>='",
            "DataFunctions::'..'",
        ]
    }
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        let name = ev
            .function_name(inv)?
            .ok_or_else(|| ev.nav.error(inv, "data function has no name"))?;
        let op = name
            .rsplit("::")
            .next()
            .map_or("", |s| s.trim_matches('\''));
        let arity = ev.argument_count(inv, cx)?;
        let first = ev.evaluate_argument(inv, 0, cx)?;
        if arity == 1 && op != ".." {
            return Ok(match argument_value(&first) {
                Some(Value::Int(n)) => match op {
                    "+" => value(Value::Int(*n)),
                    "-" => integer(inv, n.checked_neg()),
                    _ => undefined(inv),
                },
                Some(Value::Real(n)) => match op {
                    "+" => value(Value::Real(*n)),
                    "-" => value(Value::Real(-n)),
                    _ => undefined(inv),
                },
                _ => Outcome::Values(vec![]),
            });
        }
        let second = ev.evaluate_argument(inv, 1, cx)?;
        if let Some(reason) = shape_reason(inv, 0, &first).or_else(|| shape_reason(inv, 1, &second))
        {
            return Ok(unevaluated(inv, reason));
        }
        let (Some(a), Some(b)) = (argument_value(&first), argument_value(&second)) else {
            return Err(ev.nav.error(inv, "single-argument shape changed"));
        };
        if op == ".." {
            return match (a, b) {
                (Value::Int(a), Value::Int(b)) => range(ev, inv, cx, *a, *b),
                _ => Ok(kinds(inv, a, b)),
            };
        }
        Ok(match (a, b) {
            (Value::Int(a), Value::Int(b)) => match op {
                "+" => integer(inv, a.checked_add(*b)),
                "-" => integer(inv, a.checked_sub(*b)),
                "*" => integer(inv, a.checked_mul(*b)),
                "%" if *b == 0 => Outcome::Values(vec![]),
                // The quotient cannot fit for MIN / -1, but its remainder is
                // representable and exactly zero. checked_rem rejects both.
                "%" if *b == -1 => value(Value::Int(0)),
                "%" => integer(inv, a.checked_rem(*b)),
                "<" => value(Value::Bool(a < b)),
                "<=" => value(Value::Bool(a <= b)),
                ">" => value(Value::Bool(a > b)),
                ">=" => value(Value::Bool(a >= b)),
                _ => real(inv, op, real_integer(*a), real_integer(*b)),
            },
            (Value::Real(a), Value::Real(b)) => real(inv, op, *a, *b),
            (Value::Int(a), Value::Real(b)) => real(inv, op, real_integer(*a), *b),
            (Value::Real(a), Value::Int(b)) => real(inv, op, *a, real_integer(*b)),
            (Value::Str(a), Value::Str(b)) => match op {
                "+" => {
                    let mut text = a.to_string();
                    text.try_reserve(b.len())
                        .map_err(|_| ev.nav.error(inv, "string result allocation refused"))?;
                    text.push_str(b);
                    value(Value::Str(text.into()))
                }
                "<" => value(Value::Bool(a.encode_utf16().cmp(b.encode_utf16()).is_lt())),
                "<=" => value(Value::Bool(!a.encode_utf16().cmp(b.encode_utf16()).is_gt())),
                ">" => value(Value::Bool(a.encode_utf16().cmp(b.encode_utf16()).is_gt())),
                ">=" => value(Value::Bool(!a.encode_utf16().cmp(b.encode_utf16()).is_lt())),
                _ => undefined(inv),
            },
            _ => kinds(inv, a, b),
        })
    }
}
fn value(v: Value) -> Outcome {
    Outcome::Values(vec![v])
}
fn undefined(inv: ObjId) -> Outcome {
    Outcome::NotEvaluable(Reason::OperationUndefined { invocation: inv })
}
fn integer(inv: ObjId, n: Option<i64>) -> Outcome {
    match n {
        Some(n) => value(Value::Int(n)),
        None => unevaluated(inv, Reason::Overflow { invocation: inv }),
    }
}
fn kinds(inv: ObjId, a: &Value, b: &Value) -> Outcome {
    unevaluated(
        inv,
        Reason::OperandKinds {
            invocation: inv,
            kinds: vec![kind(a), kind(b)],
        },
    )
}
fn real(inv: ObjId, op: &str, a: f64, b: f64) -> Outcome {
    match op {
        "+" => value(Value::Real(a + b)),
        "-" => value(Value::Real(a - b)),
        "*" => value(Value::Real(a * b)),
        "/" | "%" if b == 0.0 => Outcome::Values(vec![]),
        "/" => value(Value::Real(a / b)),
        "%" => value(Value::Real(a % b)),
        "**" | "^" => value(Value::Real(a.powf(b))),
        "<" => value(Value::Bool(a < b)),
        "<=" => value(Value::Bool(a <= b)),
        ">" => value(Value::Bool(a > b)),
        ">=" => value(Value::Bool(a >= b)),
        _ => undefined(inv),
    }
}
fn range(
    ev: &Evaluator<'_>,
    inv: ObjId,
    cx: &Context,
    a: i64,
    b: i64,
) -> Result<Outcome, EvalError> {
    if a > b {
        return Ok(Outcome::Values(vec![]));
    }
    let count = u64::try_from(i128::from(b) - i128::from(a) + 1)
        .map_err(|_| EvalError::StepLimit(ev.limits.steps))?;
    // Materializing each generated literal consumes a step too: a range must
    // not allocate an unbounded result between two expression-entry checks.
    cx.require_steps(count, ev.limits)?;
    let size = usize::try_from(count)
        .map_err(|_| ev.nav.error(inv, "range result exceeds address space"))?;
    let mut values = Vec::new();
    values
        .try_reserve(size)
        .map_err(|_| ev.nav.error(inv, "range result allocation refused"))?;
    let mut n = a;
    for _ in 0..count {
        cx.step(ev.limits)?;
        values.push(Value::Int(n));
        if n == b {
            break;
        }
        n = n
            .checked_add(1)
            .ok_or_else(|| ev.nav.error(inv, "range increment overflow"))?;
    }
    Ok(Outcome::Values(values))
}
