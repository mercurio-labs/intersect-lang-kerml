//! Handwritten equality, sequences, indexing and primitive type tests (REF 2.1).
use super::LibraryFunction;
use crate::{
    EvalError, Evaluator, ObjId, Outcome, Reason, Value,
    eval::{Context, unevaluated},
    value::equal_seq,
};
pub(crate) struct Base;
impl LibraryFunction for Base {
    fn names(&self) -> &'static [&'static str] {
        &[
            "BaseFunctions::'=='",
            "BaseFunctions::'!='",
            "BaseFunctions::'==='",
            "BaseFunctions::'!=='",
            "BaseFunctions::','",
            "BaseFunctions::'#'",
            "BaseFunctions::istype",
            "BaseFunctions::hastype",
            "BaseFunctions::'@'",
            "BaseFunctions::as",
            "BaseFunctions::'@@'",
            "BaseFunctions::meta",
        ]
    }
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        let name = ev
            .function_name(inv)?
            .ok_or_else(|| ev.nav.error(inv, "missing base function"))?;
        let op = name
            .rsplit("::")
            .next()
            .map_or("", |s| s.trim_matches('\''));
        if matches!(op, "istype" | "hastype" | "@" | "as" | "@@" | "meta") {
            return type_test(ev, inv, cx, op);
        }
        let a = ev.evaluate_argument(inv, 0, cx)?;
        let b = ev.evaluate_argument(inv, 1, cx)?;
        if let Outcome::NotEvaluable(reason) = &a {
            return Ok(unevaluated(inv, reason.clone()));
        }
        if let Outcome::NotEvaluable(reason) = &b {
            return Ok(unevaluated(inv, reason.clone()));
        }
        let (Outcome::Values(mut a), Outcome::Values(mut b)) = (a, b) else {
            return Ok(bad(inv));
        };
        if op == "," {
            a.append(&mut b);
            return Ok(Outcome::Values(a));
        }
        if op == "#" {
            return index(ev, inv, cx, a, b);
        }
        if let (Some(x), Some(y)) = (
            collection(ev, &a, "Collection")?,
            collection(ev, &b, "Collection")?,
        ) {
            let (Outcome::Values(x), Outcome::Values(y)) = (
                field(ev, x, "Collections::Collection::elements", cx)?,
                field(ev, y, "Collections::Collection::elements", cx)?,
            ) else {
                return Ok(unevaluated(
                    inv,
                    Reason::OperandKinds {
                        invocation: inv,
                        kinds: vec!["Collection", "Collection"],
                    },
                ));
            };
            a = x;
            b = y;
        }
        let eq = equal_seq(&a, &b);
        Ok(Outcome::Values(vec![Value::Bool(
            if matches!(op, "!=" | "!==") { !eq } else { eq },
        )]))
    }
}
fn bad(inv: ObjId) -> Outcome {
    unevaluated(inv, Reason::BadIndex { invocation: inv })
}
fn collection(
    ev: &Evaluator<'_>,
    values: &[Value],
    name: &str,
) -> Result<Option<ObjId>, EvalError> {
    let Some(ty) = ev.library.get(&format!("Collections::{name}")) else {
        return Ok(None);
    };
    match values {
        [Value::Element(e) | Value::Unevaluated { at: e, .. }]
            if ev.nav.is(*e, "Feature")? && ev.nav.specializes(*e, ty)? =>
        {
            Ok(Some(*e))
        }
        _ => Ok(None),
    }
}
fn field(
    ev: &Evaluator<'_>,
    object: ObjId,
    name: &str,
    cx: &Context,
) -> Result<Outcome, EvalError> {
    let Some(feature) = ev.library.get(name) else {
        return Ok(Outcome::NotEvaluable(Reason::NoValue {
            feature: object,
            frame: Some(object),
        }));
    };
    ev.feature_chain(&[object, feature], cx)
}

fn index(
    ev: &Evaluator<'_>,
    inv: ObjId,
    cx: &Context,
    mut values: Vec<Value>,
    indices: Vec<Value>,
) -> Result<Outcome, EvalError> {
    let index = if let Some(array) = collection(ev, &values, "Array")? {
        let Outcome::Values(dims) = field(ev, array, "Collections::Array::dimensions", cx)? else {
            return Ok(bad(inv));
        };
        if dims.len() != indices.len() {
            return Ok(bad(inv));
        }
        let mut flat = 1i64;
        for (dim, idx) in dims.iter().zip(&indices) {
            let (Value::Int(dim), Value::Int(idx)) = (dim, idx) else {
                return Ok(bad(inv));
            };
            if idx > dim {
                return Ok(bad(inv));
            }
            let Some(n) = flat
                .checked_sub(1)
                .and_then(|n| dim.checked_mul(n))
                .and_then(|n| n.checked_add(*idx))
            else {
                return Ok(unevaluated(inv, Reason::Overflow { invocation: inv }));
            };
            flat = n;
        }
        let Outcome::Values(elements) = field(ev, array, "Collections::Collection::elements", cx)?
        else {
            return Ok(bad(inv));
        };
        values = elements;
        flat
    } else {
        let [Value::Int(index)] = indices.as_slice() else {
            return Ok(bad(inv));
        };
        if let Some(ordered) = collection(ev, &values, "OrderedCollection")? {
            let Outcome::Values(elements) =
                field(ev, ordered, "Collections::Collection::elements", cx)?
            else {
                return Ok(bad(inv));
            };
            values = elements;
        }
        *index
    };
    Ok(Outcome::Values(
        index
            .checked_sub(1)
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| values.get(n))
            .cloned()
            .into_iter()
            .collect(),
    ))
}
fn type_test(ev: &Evaluator<'_>, inv: ObjId, cx: &Context, op: &str) -> Result<Outcome, EvalError> {
    let features = ev.nav.owned_features(inv)?;
    let ty = match features.get(1) {
        Some(f) => ev.nav.types(*f)?.first().copied(),
        None => None,
    };
    let Some(ty) = ty else {
        return Ok(unevaluated(
            inv,
            Reason::TypeArgumentMissing { invocation: inv },
        ));
    };
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
    let mut matched = Vec::new();
    let mut all = true;
    let mut any = false;
    for value in values {
        let mut yes = false;
        for actual in types(ev, &value)? {
            if actual == ty || (op != "hastype" && ev.nav.specializes(actual, ty)?) {
                yes = true;
                break;
            }
        }
        all &= yes;
        any |= yes;
        if yes {
            matched.push(value);
        }
    }
    Ok(Outcome::Values(if matches!(op, "as" | "meta") {
        matched
    } else {
        vec![Value::Bool(if matches!(op, "@" | "@@") {
            any
        } else {
            all
        })]
    }))
}
fn types(ev: &Evaluator<'_>, value: &Value) -> Result<Vec<ObjId>, EvalError> {
    let primitive = match value {
        Value::Bool(_) => "Boolean",
        Value::Int(_) => "Integer",
        Value::Real(_) => "Rational",
        Value::Str(_) => "String",
        Value::Infinity => "Positive",
        // A non-Feature Type returned by self represents the immutable
        // context frame's target feature, whose type is that Type (REF 1.6).
        // Feature targets retain their actual declared/inherited types below.
        Value::Element(e) if ev.nav.is(*e, "Type")? && !ev.nav.is(*e, "Feature")? => {
            return Ok(vec![*e]);
        }
        // Inlined self is represented by the source invocation identity; its
        // immutable Call frame replaces Pilot's synthetic typed invocation.
        Value::Element(e) if ev.nav.is(*e, "InvocationExpression")? => {
            return Ok(ev.nav.instantiated_type(*e)?.into_iter().collect());
        }
        Value::Element(e) | Value::Unevaluated { at: e, .. } => {
            return if ev.nav.is(*e, "Feature")? {
                ev.nav.types(*e)
            } else {
                Ok(vec![])
            };
        }
        Value::Metaclass(e) => return Ok(ev.metaclass_type(*e)?.into_iter().collect()),
    };
    Ok(ev
        .library
        .get(&format!("ScalarValues::{primitive}"))
        .into_iter()
        .collect())
}
