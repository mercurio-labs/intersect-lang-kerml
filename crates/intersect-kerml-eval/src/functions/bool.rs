//! Handwritten eager Boolean functions: non-Boolean values count as false.
use super::{LibraryFunction, boolean_value};
use crate::{EvalError, Evaluator, ObjId, Outcome, Value, eval::Context};
pub(crate) struct Boolean;
impl LibraryFunction for Boolean {
    fn names(&self) -> &'static [&'static str] {
        &[
            "DataFunctions::not",
            "DataFunctions::'&'",
            "DataFunctions::'|'",
            "DataFunctions::xor",
        ]
    }
    fn invoke(&self, ev: &Evaluator<'_>, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        let name = ev
            .function_name(inv)?
            .ok_or_else(|| ev.nav.error(inv, "Boolean function has no name"))?;
        let first = ev.evaluate_argument(inv, 0, cx)?;
        let a = boolean_value(&first) == Some(true);
        if name == "DataFunctions::not" {
            return Ok(Outcome::Values(vec![Value::Bool(!a)]));
        }
        let second = ev.evaluate_argument(inv, 1, cx)?;
        let b = boolean_value(&second) == Some(true);
        let result = match name.as_str() {
            "DataFunctions::'&'" => a & b,
            "DataFunctions::'|'" => a | b,
            "DataFunctions::xor" => a ^ b,
            _ => return Err(ev.nav.error(inv, "unregistered Boolean function")),
        };
        Ok(Outcome::Values(vec![Value::Bool(result)]))
    }
}
