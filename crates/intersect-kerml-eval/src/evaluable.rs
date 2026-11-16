//! Handwritten model-level evaluability and top-level EMF gating (REF 3).
use crate::{EvalError, EvalMode, Evaluator, ObjId, Outcome, Reason};
use std::collections::BTreeSet;
impl Evaluator<'_> {
    pub fn is_model_level_evaluable(&self, expression: ObjId) -> Result<bool, EvalError> {
        if !self.nav.is(expression, "Expression")? {
            return Err(EvalError::NotAnExpression(
                self.nav.object(expression)?.identity().into(),
            ));
        }
        self.evaluable(expression, &mut BTreeSet::new(), &mut BTreeSet::new(), 0)
    }
    fn evaluable(
        &self,
        e: ObjId,
        visited: &mut BTreeSet<ObjId>,
        path: &mut BTreeSet<ObjId>,
        depth: u32,
    ) -> Result<bool, EvalError> {
        if depth >= self.limits.depth {
            return Err(EvalError::DepthLimit(self.limits.depth));
        }
        if !path.insert(e) {
            return Ok(false);
        }
        let result = self.evaluable_inner(e, visited, path, depth.saturating_add(1));
        path.remove(&e);
        result
    }
    fn evaluable_inner(
        &self,
        e: ObjId,
        visited: &mut BTreeSet<ObjId>,
        path: &mut BTreeSet<ObjId>,
        depth: u32,
    ) -> Result<bool, EvalError> {
        if self.nav.is(e, "CalculationUsage")? || self.nav.is(e, "ConstraintUsage")? {
            return Ok(false);
        }
        for c in [
            "LiteralExpression",
            "NullExpression",
            "MetadataAccessExpression",
        ] {
            if self.nav.is(e, c)? {
                return Ok(true);
            }
        }
        if self.nav.is(e, "FeatureReferenceExpression")? {
            let Some(f) = self.nav.referent(e)? else {
                return Ok(true);
            };
            if let Some(base) = self.library.get("Base::Anything::self")
                && self.nav.specializes(f, base)?
            {
                return Ok(true);
            }
            if !visited.insert(f) {
                return Ok(false);
            }
            let result = (|| {
                // Pilot short-circuits only a successful Expression check;
                // an unsuccessful one still proceeds through the owner rules.
                if self.nav.is(f, "Expression")? && self.evaluable(f, visited, path, depth)? {
                    return Ok(true);
                }
                if let Some(owner) = self.nav.owning_type(f)?
                    && (self.nav.is(owner, "Metaclass")?
                        || self.nav.is(owner, "MetadataFeature")?)
                {
                    return Ok(true);
                }
                if self.nav.has_featuring_type(f)? {
                    return Ok(false);
                }
                match self.nav.feature_value(f)? {
                    Some(value) => self.evaluable(value, visited, path, depth),
                    None => Ok(true),
                }
            })();
            // This is the active reference path, not a global "ever seen"
            // set: sibling operands can legitimately reuse the same feature.
            visited.remove(&f);
            return result;
        }
        if self.nav.is(e, "InvocationExpression")? || self.nav.is(e, "ConstructorExpression")? {
            if self.nav.is(e, "InvocationExpression")?
                && !self
                    .function_name(e)?
                    .is_some_and(|n| crate::functions::is_model_level_evaluable(&n))
            {
                return Ok(false);
            }
            for arg in self.nav.arguments(e)? {
                if !self.evaluable(arg, visited, path, depth)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        for p in self.nav.all_parameters(e)? {
            if (self.nav.direction(p)? != Some("in") && !self.nav.is_return(p)?)
                || !self.nav.owned_features(p)?.is_empty()
                || self.nav.feature_value(p)?.is_some()
            {
                return Ok(false);
            }
        }
        match self.nav.result_expression(e)? {
            Some(r) => self.evaluable(r, visited, path, depth),
            None => Ok(true),
        }
    }
    pub fn evaluate_gated(
        &self,
        expression: ObjId,
        target: Option<ObjId>,
    ) -> Result<Outcome, EvalError> {
        self.eval_at(expression, &self.context(target)?, true)
    }
    pub(crate) fn dispatch_gated(
        &self,
        expression: ObjId,
        cx: &crate::context::Context,
    ) -> Result<Outcome, EvalError> {
        if self.mode() == EvalMode::Extended {
            return self.dispatch(expression, cx);
        }
        // Only the outer invocation is gated; evaluator recursion stays ungated.
        if self.nav.is(expression, "InvocationExpression")?
            && !self.is_model_level_evaluable(expression)?
        {
            return Ok(Outcome::NotEvaluable(Reason::NotModelLevelEvaluable {
                expression,
            }));
        }
        if self.nav.is(expression, "CalculationUsage")?
            || self.nav.is(expression, "ConstraintUsage")?
        {
            return Ok(Outcome::Values(vec![]));
        }
        if self.nav.is(expression, "ConstructorExpression")? {
            return Ok(Outcome::NotEvaluable(Reason::NoResultExpression {
                expression,
            }));
        }
        for class in [
            "LiteralExpression",
            "NullExpression",
            "FeatureReferenceExpression",
            "MetadataAccessExpression",
            "InvocationExpression",
        ] {
            if self.nav.is(expression, class)? {
                return self.dispatch(expression, cx);
            }
        }
        if !self.nav.is(expression, "Expression")? {
            return Err(EvalError::NotAnExpression(
                self.nav.object(expression)?.identity().into(),
            ));
        }
        match self.nav.owned_result_expression(expression)? {
            Some(result) => self.eval_at(result, cx, true),
            None => Ok(Outcome::NotEvaluable(Reason::NoResultExpression {
                expression,
            })),
        }
    }
}
