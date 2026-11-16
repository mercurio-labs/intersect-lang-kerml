//! Handwritten API-only condition and multiplicity evaluation (REF 3/4, E5).
use crate::{EvalError, Evaluator, ObjId, Outcome, Value, context::Context};
use std::collections::BTreeSet;

impl Evaluator<'_> {
    /// Evaluate a condition through the model-level EMF gate. Only [true]
    /// satisfies it; empty, unevaluated and non-Boolean values do not.
    pub fn check_condition(
        &self,
        condition: ObjId,
        target: Option<ObjId>,
    ) -> Result<bool, EvalError> {
        let ev = self.model_level_view();
        ev.condition_in(condition, &ev.context(target)?)
    }

    /// Every filter condition must hold for at least one metadata candidate.
    /// Different conditions may be satisfied by different candidates.
    pub fn check_conditions_on(
        &self,
        element: ObjId,
        conditions: &[ObjId],
    ) -> Result<bool, EvalError> {
        let ev = self.model_level_view();
        let candidates = ev.metadata_values(element)?;
        let run = Context::default();
        for condition in conditions {
            if candidates.is_empty() {
                if !ev.condition_in(*condition, &run)? {
                    return Ok(false);
                }
                continue;
            }
            let mut satisfied = false;
            for candidate in &candidates {
                let cx = match candidate {
                    Value::Element(target) => {
                        let mut cx = ev.context(Some(*target))?;
                        cx.run = run.run.clone();
                        cx
                    }
                    Value::Metaclass(target) => ev.metaclass_context(*target, &run),
                    _ => {
                        return Err(ev
                            .nav
                            .error(element, "metadata candidate is not an element"));
                    }
                };
                if ev.condition_in(*condition, &cx)? {
                    satisfied = true;
                    break;
                }
            }
            if !satisfied {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn condition_in(&self, condition: ObjId, cx: &Context) -> Result<bool, EvalError> {
        Ok(
            matches!(self.eval_at(condition,cx,true)?, Outcome::Values(v) if matches!(v.as_slice(),[Value::Bool(true)])),
        )
    }

    /// Some(-1) represents *, Some(n) a nonnegative integer; None is an
    /// invalid or non-model-level-evaluable bound. Evaluation errors propagate.
    pub fn multiplicity_bound(&self, bound: ObjId) -> Result<Option<i64>, EvalError> {
        let ev = self.model_level_view();
        if !ev.is_model_level_evaluable(bound)? {
            return Ok(None);
        }
        let mut target = None;
        let mut inside_range = false;
        let mut current = ev.nav.object(bound)?.container();
        let mut seen = BTreeSet::new();
        while let Some((owner, _)) = current {
            if !seen.insert(owner.id()) {
                return Err(ev
                    .nav
                    .error(bound, "containment cycle in multiplicity target"));
            }
            if inside_range && ev.nav.is(owner.id(), "Namespace")? {
                target = Some(owner.id());
                break;
            }
            inside_range |= ev.nav.is(owner.id(), "MultiplicityRange")?;
            current = owner.container();
        }
        Ok(match ev.evaluate_gated(bound, target)? {
            Outcome::Values(v) => match v.as_slice() {
                [Value::Infinity] => Some(-1),
                [Value::Int(n)] if *n >= 0 => Some(*n),
                _ => None,
            },
            Outcome::NotEvaluable(_) => None,
        })
    }
}
