//! Sequence operations use boxed-value equality and preserve input order.
use super::{bad, single};
use crate::functions::argument_value;
use crate::{
    EvalError, Evaluator, ObjId, Outcome, Value,
    eval::Context,
    value::{equal, equal_seq},
};

pub(super) fn invoke(
    ev: &Evaluator<'_>,
    inv: ObjId,
    cx: &Context,
    op: &str,
) -> Result<Outcome, EvalError> {
    let Outcome::Values(a) = ev.evaluate_argument(inv, 0, cx)? else {
        return Ok(bad(inv));
    };
    match op {
        "size" => {
            return Ok(single(Value::Int(
                i64::try_from(a.len())
                    .map_err(|_| ev.nav.error(inv, "sequence size exceeds i64"))?,
            )));
        }
        "isEmpty" | "notEmpty" => {
            return Ok(single(Value::Bool(a.is_empty() == (op == "isEmpty"))));
        }
        "head" => return Ok(Outcome::Values(a.first().cloned().into_iter().collect())),
        "last" => return Ok(Outcome::Values(a.last().cloned().into_iter().collect())),
        "tail" => return Ok(Outcome::Values(a.into_iter().skip(1).collect())),
        "subsequence" | "excludingAt" => return slice(ev, inv, cx, op, a),
        _ => {}
    }
    let Outcome::Values(b) = ev.evaluate_argument(inv, 1, cx)? else {
        return Ok(bad(inv));
    };
    let includes =
        |left: &[Value], right: &[Value]| right.iter().all(|v| left.iter().any(|x| equal(x, v)));
    Ok(match op {
        "includes" => single(Value::Bool(includes(&a, &b))),
        "includesOnly" => single(Value::Bool(includes(&a, &b) && includes(&b, &a))),
        "excludes" => single(Value::Bool(
            b.iter().all(|v| a.iter().all(|x| !equal(x, v))),
        )),
        "equals" | "same" => single(Value::Bool(equal_seq(&a, &b))),
        "including" | "union" => Outcome::Values(a.into_iter().chain(b).collect()),
        "excluding" | "intersection" => Outcome::Values(
            a.into_iter()
                .filter(|v| b.iter().any(|x| equal(x, v)) == (op == "intersection"))
                .collect(),
        ),
        "includingAt" => {
            let index = ev.evaluate_argument(inv, 2, cx)?;
            let position = match argument_value(&index) {
                Some(Value::Infinity) => a.len(),
                Some(Value::Int(n)) => start(*n)
                    .ok_or_else(|| ev.nav.error(inv, "sequence index exceeds address space"))?,
                _ => return Ok(bad(inv)),
            };
            let before = a.get(..position).ok_or_else(|| {
                ev.nav
                    .error(inv, "sequence insertion index is out of bounds")
            })?;
            let after = a.get(position..).ok_or_else(|| {
                ev.nav
                    .error(inv, "sequence insertion index is out of bounds")
            })?;
            Outcome::Values(
                before
                    .iter()
                    .cloned()
                    .chain(b)
                    .chain(after.iter().cloned())
                    .collect(),
            )
        }
        _ => return Err(ev.nav.error(inv, "unknown sequence implementation")),
    })
}
fn start(n: i64) -> Option<usize> {
    usize::try_from(n.saturating_sub(1).max(0)).ok()
}
fn slice(
    ev: &Evaluator<'_>,
    inv: ObjId,
    cx: &Context,
    op: &str,
    a: Vec<Value>,
) -> Result<Outcome, EvalError> {
    let first = ev.evaluate_argument(inv, 1, cx)?;
    let last = ev.evaluate_argument(inv, 2, cx)?;
    let last_value = argument_value(&last).or_else(|| {
        if op == "excludingAt" {
            argument_value(&first)
        } else {
            None
        }
    });
    if op == "excludingAt" {
        let Some(start) = argument_value(&first) else {
            return Ok(bad(inv));
        };
        // Confirmed by PilotEvalEdgeTest: preserve the Pilot precedence bug.
        // A noninteger start plus an integer end passes this guard, then the
        // Java cast throws. Surface that failure as an EvalError, never panic.
        let accepted = matches!(start, Value::Infinity)
            || (matches!(start, Value::Int(_)) && matches!(last_value, Some(Value::Infinity)))
            || matches!(last_value, Some(Value::Int(_)));
        if !accepted {
            return Ok(bad(inv));
        }
        if matches!(start, Value::Infinity) {
            return Ok(Outcome::Values(a));
        }
        if !matches!(start, Value::Int(_)) {
            return Err(ev.nav.error(
                inv,
                "excludingAt start is not an Integer (Pilot cast failure)",
            ));
        }
    }
    let end = match last_value {
        None | Some(Value::Infinity) => a.len(),
        Some(Value::Int(n)) => usize::try_from((*n).max(0))
            .map_err(|_| ev.nav.error(inv, "sequence end exceeds address space"))?,
        _ => return Ok(bad(inv)),
    };
    let begin = match argument_value(&first) {
        Some(Value::Infinity) => {
            return Ok(if op == "subsequence" {
                Outcome::Values(vec![])
            } else {
                Outcome::Values(a)
            });
        }
        Some(Value::Int(n)) => {
            start(*n).ok_or_else(|| ev.nav.error(inv, "sequence start exceeds address space"))?
        }
        _ => return Ok(bad(inv)),
    };
    if end < begin {
        return Ok(if op == "subsequence" {
            Outcome::Values(vec![])
        } else {
            Outcome::Values(a)
        });
    }
    let range = a
        .get(begin..end)
        .ok_or_else(|| ev.nav.error(inv, "sequence range is out of bounds"))?;
    if op == "subsequence" {
        return Ok(Outcome::Values(range.to_vec()));
    }
    let before = a
        .get(..begin)
        .ok_or_else(|| ev.nav.error(inv, "sequence start is out of bounds"))?;
    let after = a
        .get(end..)
        .ok_or_else(|| ev.nav.error(inv, "sequence end is out of bounds"))?;
    Ok(Outcome::Values(
        before.iter().chain(after).cloned().collect(),
    ))
}
