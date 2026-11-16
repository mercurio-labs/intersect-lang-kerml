//! Handwritten library function dispatch and evaluated-argument helpers.
mod base;
mod bool;
mod control;
mod data;
mod extended;
use crate::{EvalError, EvalMode, Evaluator, ObjId, Outcome, Reason, Value, eval::Context};

pub(crate) trait LibraryFunction {
    fn names(&self) -> &'static [&'static str];
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError>;
}
pub(crate) fn get(name: &str, mode: EvalMode) -> Option<&'static dyn LibraryFunction> {
    static DATA: data::Data = data::Data;
    static BOOL: bool::Boolean = bool::Boolean;
    static BASE: base::Base = base::Base;
    static CONTROL: control::Control = control::Control;
    static EXTENDED: extended::Extended = extended::Extended;
    if mode == EvalMode::Extended && EXTENDED.names().contains(&name) {
        return Some(&EXTENDED);
    }
    [&DATA as &dyn LibraryFunction, &BOOL, &BASE, &CONTROL]
        .into_iter()
        .find(|f| f.names().contains(&name))
}
/// REF's model-level table includes metadata predicates whose implementations
/// arrive in M5.1. Admission is separate from currently executable functions.
pub(crate) fn is_model_level_evaluable(name: &str) -> bool {
    matches!(name, "BaseFunctions::'@@'" | "BaseFunctions::meta")
        || get(name, EvalMode::ModelLevel).is_some()
}
pub(crate) fn argument_value(outcome: &Outcome) -> Option<&Value> {
    match outcome {
        Outcome::Values(values) => match values.as_slice() {
            [value] => Some(value),
            _ => None,
        },
        Outcome::NotEvaluable(_) => None,
    }
}
pub(crate) fn boolean_value(outcome: &Outcome) -> Option<bool> {
    match argument_value(outcome) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}
pub(crate) fn shape_reason(invocation: ObjId, operand: usize, outcome: &Outcome) -> Option<Reason> {
    if argument_value(outcome).is_some() {
        return None;
    }
    let count = match outcome {
        Outcome::Values(v) => v.len(),
        Outcome::NotEvaluable(_) => 0,
    };
    Some(Reason::OperandNotSingle {
        invocation,
        operand,
        count,
    })
}
pub(crate) fn kind(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "Boolean",
        Value::Int(_) => "Integer",
        Value::Real(_) => "Real",
        Value::Str(_) => "String",
        Value::Infinity => "Infinity",
        Value::Element(_) => "Element",
        Value::Unevaluated { .. } => "Unevaluated",
        Value::Metaclass(_) => "Metaclass",
    }
}
/// Exact decomposition before IEEE rounding; no truncating numeric casts.
pub(crate) fn real_integer(n: i64) -> f64 {
    let [a, b, c, d, e, f, g, h] = n.unsigned_abs().to_le_bytes();
    let magnitude = f64::from(u32::from_le_bytes([e, f, g, h])) * 4_294_967_296.0
        + f64::from(u32::from_le_bytes([a, b, c, d]));
    if n < 0 { -magnitude } else { magnitude }
}
