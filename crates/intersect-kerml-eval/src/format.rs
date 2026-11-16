//! Handwritten Pilot-style text and lossless result JSON (REF 6.1).
use crate::{EvalError, Model, ObjId, Outcome, Reason, Value, nav::Nav, schema::EvalSchema};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
pub fn java_double(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == f64::INFINITY {
        return "Infinity".into();
    }
    if x == f64::NEG_INFINITY {
        return "-Infinity".into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    if (1e-3..1e7).contains(&x.abs()) {
        let s = x.to_string();
        return if s.contains('.') { s } else { format!("{s}.0") };
    }
    let shortest = format!("{x:e}");
    // JDK 19+ Double.toString selects the closest decimal among lengths 1
    // AND 2 when the shortest round-tripping decimal has length 1. Rounding
    // to two significant digits implements that selection (ties to even).
    // For example 2^-1074 is nearer 4.9e-324 than 5e-324; this is not a
    // minimum-value special case, and also fixes 2 * MIN_VALUE and others.
    // https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/Double.html#toString(double)
    let scientific = if shortest
        .split_once('e')
        .is_some_and(|(digits, _)| !digits.contains('.'))
    {
        format!("{x:.1e}")
    } else {
        shortest
    };
    match scientific.split_once('e') {
        Some((digits, exponent)) => format!(
            "{}E{}",
            if digits.contains('.') {
                digits.to_owned()
            } else {
                format!("{digits}.0")
            },
            exponent
        ),
        None => scientific,
    }
}
pub(crate) fn identity(model: &Model, id: ObjId) -> String {
    model.obj(id).map_or_else(
        || format!("object-index:{}", id.index()),
        |o| o.identity().into(),
    )
}
pub(crate) fn class(model: &Model, id: ObjId) -> String {
    model
        .obj(id)
        .and_then(|o| model.metamodel().schema().classes.get(o.class_key()))
        .map_or_else(|| "UnknownElement".into(), |c| c.name.clone())
}
pub fn pilot_text(model: &Model, value: &Value) -> String {
    match value {
        Value::Bool(v) => format!("LiteralBoolean {v}"),
        Value::Int(v) => format!("LiteralInteger {v}"),
        Value::Real(v) => format!("LiteralRational {}", java_double(*v)),
        Value::Str(v) => format!("LiteralString {v}"),
        Value::Infinity => "LiteralInfinity *".into(),
        Value::Element(e) | Value::Unevaluated { at: e, .. } | Value::Metaclass(e) => {
            let prefix = if matches!(value, Value::Metaclass(_)) {
                "MetadataFeature "
            } else {
                ""
            };
            let suffix = element_suffix(model, *e).unwrap_or_else(|error| format!(" <{error}>"));
            format!("{prefix}{}{suffix}", class(model, *e))
        }
    }
}
fn element_suffix(model: &Model, e: ObjId) -> Result<String, EvalError> {
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(model, &schema);
    let mut out = String::new();
    if let Some(short) = nav.effective_name(e, "declaredShortName")? {
        out.push_str(&format!(" <<{short}>>"));
    }
    if let Some(name) = name(&nav, e, &mut BTreeSet::new())? {
        out.push(' ');
        out.push_str(&name);
    }
    Ok(out)
}
fn name(nav: &Nav<'_>, e: ObjId, seen: &mut BTreeSet<ObjId>) -> Result<Option<String>, EvalError> {
    if !seen.insert(e) {
        return Ok(None);
    }
    if nav.is(e, "OperatorExpression")? {
        return Ok(nav
            .string(e, "OperatorExpression", "operator")?
            .map(str::to_owned));
    }
    if nav.is(e, "FeatureReferenceExpression")? {
        return match nav.referent(e)? {
            Some(r) => name(nav, r, seen),
            None => Ok(None),
        };
    }
    if nav.is(e, "Feature")? {
        let chain = nav.chaining_features(e)?;
        if !chain.is_empty() {
            let mut names = Vec::new();
            for f in chain {
                if let Some(n) = name(nav, f, seen)? {
                    names.push(n);
                }
            }
            return Ok(Some(names.join(".")));
        }
    }
    if nav.is(e, "InvocationExpression")?
        && let Some(t) = nav.instantiated_type(e)?
    {
        if nav.is(t, "Function")? {
            return name(nav, t, seen);
        }
        for ty in nav.types(t)? {
            if nav.is(ty, "Function")? {
                return name(nav, ty, seen);
            }
        }
    }
    nav.effective_name(e, "declaredName")
}
pub fn outcome_json(model: &Model, outcome: &Outcome) -> Json {
    match outcome {
        Outcome::NotEvaluable(reason) => {
            json!({"outcome":"not-evaluable","reason":reason_json(model,reason)})
        }
        Outcome::Values(values) => {
            json!({"outcome":"values","values":values.iter().map(|v|value_json(model,v)).collect::<Vec<_>>()})
        }
    }
}
pub(crate) fn reason_json(model: &Model, r: &Reason) -> Json {
    match r {
        Reason::UnknownFunction { invocation } => {
            json!({"reason":"unknown-function","invocation":identity(model,*invocation)})
        }
        Reason::NotModelLevelEvaluable { expression } => {
            json!({"reason":"not-model-level-evaluable","expression":identity(model,*expression)})
        }
        Reason::OperandNotSingle {
            invocation,
            operand,
            count,
        } => {
            json!({"reason":"operand-not-single","invocation":identity(model,*invocation),"operand":operand,"count":count})
        }
        Reason::OperandKinds { invocation, kinds } => {
            json!({"reason":"operand-kinds","invocation":identity(model,*invocation),"kinds":kinds})
        }
        Reason::OperationUndefined { invocation } => {
            json!({"reason":"operation-undefined","invocation":identity(model,*invocation)})
        }
        Reason::NoValue { feature, frame } => {
            json!({"reason":"no-value","feature":identity(model,*feature),"frame":frame.map(|e|identity(model,e))})
        }
        Reason::NoReferent { expression } => {
            json!({"reason":"no-referent","expression":identity(model,*expression)})
        }
        Reason::NoResultExpression { expression } => {
            json!({"reason":"no-result-expression","expression":identity(model,*expression)})
        }
        Reason::Overflow { invocation } => {
            json!({"reason":"overflow","invocation":identity(model,*invocation)})
        }
        Reason::BadIndex { invocation } => {
            json!({"reason":"bad-index","invocation":identity(model,*invocation)})
        }
        Reason::TypeArgumentMissing { invocation } => {
            json!({"reason":"type-argument-missing","invocation":identity(model,*invocation)})
        }
        Reason::Unsupported { element, what } => {
            json!({"reason":"unsupported","element":identity(model,*element),"what":what})
        }
    }
}
fn value_json(model: &Model, value: &Value) -> Json {
    match value {
        Value::Bool(v) => json!({"kind":"bool","value":v}),
        Value::Int(v) => json!({"kind":"int","value":v}),
        Value::Str(v) => json!({"kind":"string","value":v.as_ref()}),
        Value::Infinity => json!({"kind":"infinity"}),
        Value::Real(v) => {
            json!({"kind":"real","bits":format!("{:016x}",v.to_bits()),"text":java_double(*v)})
        }
        Value::Element(e) => {
            json!({"kind":"element","id":identity(model,*e),"class":class(model,*e)})
        }
        Value::Unevaluated { at, reason } => {
            json!({"kind":"unevaluated","id":identity(model,*at),"class":class(model,*at),"reason":reason_json(model,reason)})
        }
        Value::Metaclass(e) => {
            json!({"kind":"metaclass","of":identity(model,*e),"class":class(model,*e)})
        }
    }
}
/// Human-readable explanation of a [`Reason`], naming elements by their
/// Pilot-style text rather than internal identities.
pub fn reason_text(model: &Model, reason: &Reason) -> String {
    let name = |id| pilot_text(model, &Value::Element(id));
    match reason {
        Reason::UnknownFunction { invocation } => format!(
            "{} has no supported function in this mode",
            name(*invocation)
        ),
        Reason::NotModelLevelEvaluable { expression } => {
            format!("{} is not model-level evaluable", name(*expression))
        }
        Reason::OperandNotSingle { operand, count, .. } => {
            format!("operand {operand} has {count} values; exactly one is required")
        }
        Reason::OperandKinds { kinds, .. } => {
            format!("incompatible operand kinds: {}", kinds.join(", "))
        }
        Reason::OperationUndefined { invocation } => {
            format!("{} is undefined for these operands", name(*invocation))
        }
        Reason::NoValue { feature, frame } => format!(
            "{} has no value{}",
            name(*feature),
            frame.map_or_else(String::new, |f| format!(" in {}", name(f)))
        ),
        Reason::NoReferent { expression } => format!("{} has no referent", name(*expression)),
        Reason::NoResultExpression { expression } => {
            format!("{} has no result expression", name(*expression))
        }
        Reason::Overflow { .. } => "checked integer arithmetic overflowed".into(),
        Reason::BadIndex { .. } => "index has an invalid type or shape".into(),
        Reason::TypeArgumentMissing { .. } => "the required type argument is missing".into(),
        Reason::Unsupported { what, .. } => format!("unsupported: {what}"),
    }
}
/// Pilot-style text of a result: values joined by `, `, `()` for an empty
/// sequence and `(not evaluable)` for an absent result.
pub fn values_text(model: &Model, outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotEvaluable(_) => "(not evaluable)".into(),
        Outcome::Values(values) if values.is_empty() => "()".into(),
        Outcome::Values(values) => values
            .iter()
            .map(|v| pilot_text(model, v))
            .collect::<Vec<_>>()
            .join(", "),
    }
}
