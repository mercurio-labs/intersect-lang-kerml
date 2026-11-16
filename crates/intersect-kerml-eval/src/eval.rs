//! Handwritten bounded evaluator and metaclass dispatch (REF section 1.4).
pub(crate) use crate::context::Context;
use crate::context::Frame;
use crate::{
    EvalError, Model, ObjId, Outcome, Reason, Value,
    library::{LibraryIndex, operator_function},
    nav::Nav,
    schema::EvalSchema,
};
use intersect_model::core::frozen::ValueRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalMode {
    ModelLevel,
    Extended,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalLimits {
    pub steps: u64,
    pub depth: u32,
}
impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            steps: 1_000_000,
            depth: 256,
        }
    }
}

pub struct Evaluator<'m> {
    pub(crate) nav: Nav<'m>,
    pub(crate) library: std::rc::Rc<LibraryIndex>,
    mode: EvalMode,
    pub(crate) limits: EvalLimits,
}
impl<'m> Evaluator<'m> {
    /// Escaped fully qualified name, shared with host labels and target lookup.
    pub fn qualified_name(&self, element: ObjId) -> Result<Option<String>, EvalError> {
        self.nav.qualified_name(element)
    }

    /// Resolve a unique element by its escaped fully qualified name.
    pub fn resolve_target(&self, name: &str) -> Result<ObjId, EvalError> {
        let canonical = crate::text::canonical_name(name).ok_or_else(|| EvalError::Model {
            object: name.into(),
            message: "invalid target qualified name".into(),
        })?;
        let mut found = None;
        for object in self.nav.model.objects() {
            if self.nav.qualified_name(object.id())?.as_deref() == Some(canonical.as_str()) {
                if found.is_some() {
                    return Err(EvalError::Model {
                        object: name.into(),
                        message: "ambiguous target qualified name".into(),
                    });
                }
                found = Some(object.id());
            }
        }
        found.ok_or_else(|| EvalError::Model {
            object: name.into(),
            message: "target qualified name was not found".into(),
        })
    }

    pub(crate) fn context<'o>(&self, target: Option<ObjId>) -> Result<Context<'o>, EvalError> {
        let mut cx = Context::default();
        if let Some(t) = target {
            self.nav.object(t)?;
            if self.nav.is(t, "Feature")? {
                let chain = self.nav.chaining_features(t)?;
                if !chain.is_empty() {
                    cx.frames.extend(chain.into_iter().map(Frame::Type));
                    return Ok(cx);
                }
            }
            if self.nav.is(t, "Type")? {
                cx.frames.push(Frame::Type(t));
            }
        }
        Ok(cx)
    }
    pub fn evaluate_feature(
        &self,
        feature: ObjId,
        target: Option<ObjId>,
    ) -> Result<Outcome, EvalError> {
        self.feature_in(feature, &self.context(target)?)
    }
    pub(crate) fn feature_in(&self, feature: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        if !self.nav.is(feature, "Feature")? {
            return Err(self.nav.error(feature, "expected a Feature"));
        }
        let depth = cx.run.depth.get();
        if depth >= self.limits.depth {
            return Err(EvalError::DepthLimit(self.limits.depth));
        }
        cx.run.depth.set(depth.saturating_add(1));
        let result = self.feature_value_in(feature, cx);
        cx.run.depth.set(depth);
        result
    }
    fn feature_value_in(&self, feature: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        if self.library.name(feature) == Some("TrigFunctions::pi") {
            return Ok(Outcome::Values(vec![Value::Real(std::f64::consts::PI)]));
        }
        if let Some(frame) = cx.frames.last()
            && let Some(base) = self.library.get("Base::Anything::self")
            && self.nav.specializes(feature, base)?
        {
            if let Frame::Metaclass { element, .. } = frame {
                return Ok(Outcome::Values(vec![Value::Metaclass(*element)]));
            }
            if let Frame::Arguments { bound, .. } = frame
                && let Some((_, values)) = bound.iter().find(|(p, _)| *p == base)
            {
                return Ok(Outcome::Values(values.clone()));
            }
            return Ok(Outcome::Values(vec![Value::Element(frame.identity())]));
        }
        let chain = self.nav.chaining_features(feature)?;
        if !chain.is_empty() {
            return self.feature_chain(&chain, cx);
        }
        for frame in cx.frames.iter().rev() {
            match frame {
                Frame::Arguments { bound, .. } | Frame::Call { bound, .. } => {
                    let closure = self.nav.redefined_closure(feature)?;
                    // KerML: an owned parameter without explicit redefinitions
                    // implicitly redefines the general's parameter at the same
                    // position. The frozen model does not record that implicit
                    // redefinition, so a call frame also binds by position.
                    let positional = match frame {
                        Frame::Call { ty, .. } => self.positional_parameter(feature, *ty)?,
                        _ => None,
                    };
                    for (parameter, values) in bound {
                        if closure.contains(parameter)
                            || self.nav.redefined_closure(*parameter)?.contains(&feature)
                            || positional == Some(*parameter)
                        {
                            cx.observe(|o| {
                                o.lookup(
                                    feature,
                                    &crate::FrameView::from(frame),
                                    crate::Lookup::Bound,
                                )
                            })?;
                            return match self.bound_values(
                                values,
                                cx,
                                matches!(frame, Frame::Arguments { .. }),
                            )? {
                                values @ Outcome::Values(_) => Ok(values),
                                Outcome::NotEvaluable(reason) => {
                                    Ok(unevaluated(*parameter, reason))
                                }
                            };
                        }
                    }
                    if self.mode == EvalMode::Extended {
                        let ty = match frame {
                            Frame::Arguments { expression, .. } => *expression,
                            Frame::Call { ty, .. } => *ty,
                            Frame::Type(ty) => *ty,
                            Frame::Metaclass { element, .. } => *element,
                        };
                        if let Some(value) = self.lookup_in_type(feature, ty, frame, cx)? {
                            return Ok(value);
                        }
                    }
                }
                Frame::Type(t) => {
                    if self.nav.is(*t, "MetadataFeature")? && self.is_annotated_element(feature)? {
                        let values = self
                            .nav
                            .annotated_elements(*t)?
                            .into_iter()
                            .map(Value::Metaclass)
                            .collect();
                        cx.observe(|o| {
                            o.lookup(
                                feature,
                                &crate::FrameView::from(frame),
                                crate::Lookup::Bound,
                            )
                        })?;
                        return Ok(Outcome::Values(values));
                    }
                    if let Some(value) = self.lookup_in_type(feature, *t, frame, cx)? {
                        return Ok(value);
                    }
                }
                Frame::Metaclass { element, .. } => {
                    if self.is_annotated_element(feature)? {
                        cx.observe(|o| {
                            o.lookup(
                                feature,
                                &crate::FrameView::from(frame),
                                crate::Lookup::Bound,
                            )
                        })?;
                        return Ok(Outcome::Values(vec![Value::Metaclass(*element)]));
                    }
                    if let Some(outcome) = self.metaclass_feature(feature, *element)? {
                        cx.observe(|o| {
                            o.lookup(
                                feature,
                                &crate::FrameView::from(frame),
                                crate::Lookup::Bound,
                            )
                        })?;
                        return Ok(outcome);
                    }
                    if let Some(ty) = self.metaclass_type(*element)?
                        && let Some(value) = self.lookup_in_type(feature, ty, frame, cx)?
                    {
                        return Ok(value);
                    }
                }
            }
            cx.observe(|o| {
                o.lookup(
                    feature,
                    &crate::FrameView::from(frame),
                    crate::Lookup::NotHere,
                )
            })?;
        }
        let value = self.nav.feature_value(feature)?;
        cx.observe(|o| {
            o.lookup(
                feature,
                &crate::FrameView::Type { ty: feature },
                value.map_or(crate::Lookup::Unbound(feature), crate::Lookup::Value),
            )
        })?;
        if let Some(e) = value
            && let values @ Outcome::Values(_) = self.eval_in(e, cx)?
        {
            return Ok(values);
        }
        Ok(unevaluated(
            feature,
            Reason::NoValue {
                feature,
                frame: None,
            },
        ))
    }
    fn lookup_in_type(
        &self,
        feature: ObjId,
        ty: ObjId,
        frame: &Frame,
        cx: &Context,
    ) -> Result<Option<Outcome>, EvalError> {
        let Some(member) = self.nav.type_feature_for(feature, ty)? else {
            return Ok(None);
        };
        let value = self.nav.feature_value(member)?;
        cx.observe(|o| {
            o.lookup(
                feature,
                &crate::FrameView::from(frame),
                value.map_or(crate::Lookup::Unbound(member), crate::Lookup::Value),
            )
        })?;
        if let Some(e) = value
            && let values @ Outcome::Values(_) = self.eval_in(e, cx)?
        {
            return Ok(Some(values));
        }
        if self.mode == EvalMode::Extended
            && !self.nav.is(member, "Expression")?
            && matches!(self.nav.direction(member)?, Some("in" | "inout"))
        {
            return Ok(Some(Outcome::Values(vec![])));
        }
        Ok(Some(unevaluated(
            member,
            Reason::NoValue {
                feature: member,
                frame: Some(frame.identity()),
            },
        )))
    }
    pub(crate) fn feature_chain(
        &self,
        chain: &[ObjId],
        cx: &Context,
    ) -> Result<Outcome, EvalError> {
        let Some((first, rest)) = chain.split_first() else {
            return Ok(Outcome::Values(vec![]));
        };
        let result = self.feature_in(*first, cx)?;
        if rest.is_empty() {
            return Ok(result);
        }
        let Outcome::Values(values) = result else {
            return Ok(result);
        };
        let mut out = Vec::new();
        for value in values {
            if let Value::Metaclass(element) = &value {
                let next = self.metaclass_context(*element, cx);
                let depth = cx.run.depth.get();
                if depth >= self.limits.depth {
                    return Err(EvalError::DepthLimit(self.limits.depth));
                }
                cx.run.depth.set(depth.saturating_add(1));
                let result = self.feature_chain(rest, &next);
                cx.run.depth.set(depth);
                if let Outcome::Values(mut values) = result? {
                    out.append(&mut values);
                }
            } else if let Value::Element(t) | Value::Unevaluated { at: t, .. } = &value
                && self.nav.is(*t, "Type")?
            {
                let next = if self.nav.is(*t, "Feature")? {
                    cx.pushed(Frame::Type(*t))
                } else {
                    let mut next = cx.clone();
                    next.frames = vec![Frame::Type(*t)];
                    next
                };
                // Charge and bound chains as well as recursive value expressions.
                let depth = cx.run.depth.get();
                if depth >= self.limits.depth {
                    return Err(EvalError::DepthLimit(self.limits.depth));
                }
                cx.run.depth.set(depth.saturating_add(1));
                let result = self.feature_chain(rest, &next);
                cx.run.depth.set(depth);
                if let Outcome::Values(mut v) = result? {
                    out.append(&mut v);
                }
            } else if matches!(
                value,
                Value::Bool(_) | Value::Int(_) | Value::Real(_) | Value::Str(_) | Value::Infinity
            ) {
                // Pilot literals are Features and Types, even though frozen
                // evaluation represents newly computed literals as scalars.
                let next = self.scalar_context(value, *first, cx);
                if let Outcome::Values(mut values) = self.feature_chain(rest, &next)? {
                    out.append(&mut values);
                }
            } else {
                out.push(value);
            }
        }
        Ok(Outcome::Values(out))
    }
    pub(crate) fn scalar_context<'o>(
        &self,
        value: Value,
        expression: ObjId,
        cx: &Context<'o>,
    ) -> Context<'o> {
        let bound = self
            .library
            .get("Base::Anything::self")
            .map(|p| (p, vec![value]))
            .into_iter()
            .collect();
        cx.pushed(Frame::Arguments { expression, bound })
    }
    pub fn new(model: &'m Model, mode: EvalMode) -> Result<Self, EvalError> {
        let schema = EvalSchema::new(model.metamodel())?;
        let library = LibraryIndex::new(model, &schema)?;
        Ok(Self {
            nav: Nav::new(model, &schema),
            library: std::rc::Rc::new(library),
            mode,
            limits: EvalLimits::default(),
        })
    }
    pub fn with_limits(self, limits: EvalLimits) -> Self {
        Self { limits, ..self }
    }
    pub fn mode(&self) -> EvalMode {
        self.mode
    }
    /// A fresh navigation cache and run state over the same immutable library.
    /// EMF condition/bound operations always use model-level semantics.
    pub(crate) fn model_level_view(&self) -> Self {
        Self {
            nav: Nav::new(self.nav.model, &self.nav.schema),
            library: self.library.clone(),
            mode: EvalMode::ModelLevel,
            limits: self.limits,
        }
    }
    pub fn evaluate(&self, expression: ObjId, target: Option<ObjId>) -> Result<Outcome, EvalError> {
        self.eval_in(expression, &self.context(target)?)
    }
    pub fn evaluate_observed(
        &self,
        expression: ObjId,
        target: Option<ObjId>,
        gated: bool,
        observer: &mut dyn crate::EvalObserver,
    ) -> Result<Outcome, EvalError> {
        let mut cx = self.context(target)?;
        cx.run = std::rc::Rc::new(crate::context::Run {
            observer: std::cell::RefCell::new(Some(observer)),
            ..Default::default()
        });
        self.eval_at(expression, &cx, gated)
    }
    pub(crate) fn eval_in(&self, expression: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        self.eval_at(expression, cx, false)
    }
    pub(crate) fn eval_at(
        &self,
        expression: ObjId,
        cx: &Context,
        gated: bool,
    ) -> Result<Outcome, EvalError> {
        let object = self.nav.object(expression)?;
        if !self.nav.is(expression, "Expression")? {
            return Err(EvalError::NotAnExpression(object.identity().into()));
        }
        let depth = cx.run.depth.get();
        if depth >= self.limits.depth {
            return Err(EvalError::DepthLimit(self.limits.depth));
        }
        let step = cx.step(self.limits)?;
        if cx.observe(|o| o.enter(step, depth, expression, &crate::ContextView::new(cx)))?
            == Some(crate::Control::Stop)
        {
            return Err(EvalError::Stopped(step));
        }
        cx.run.depth.set(
            depth
                .checked_add(1)
                .ok_or(EvalError::DepthLimit(self.limits.depth))?,
        );
        let result = if gated {
            self.dispatch_gated(expression, cx)
        } else {
            self.dispatch(expression, cx)
        };
        cx.run.depth.set(depth);
        if let Ok(outcome) = &result {
            cx.observe(|o| o.exit(cx.run.steps.get(), expression, outcome))?;
        }
        result
    }
    pub(crate) fn dispatch(&self, e: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        if self.nav.is(e, "NullExpression")? {
            return Ok(Outcome::Values(vec![]));
        }
        if self.nav.is(e, "LiteralExpression")? {
            return Ok(Outcome::Values(vec![self.literal(e)?]));
        }
        if self.nav.is(e, "FeatureReferenceExpression")? {
            // Pilot's referent setting delegate substitutes the library self
            // feature when getReferentFor finds no member (the grammar's
            // SelfReferenceExpression has only an empty return parameter).
            let referent = self
                .nav
                .referent(e)?
                .or_else(|| self.library.get("Base::Anything::self"));
            return match referent {
                Some(feature) => self.feature_in(feature, cx),
                None => Ok(Outcome::NotEvaluable(Reason::NoReferent { expression: e })),
            };
        }
        if self.nav.is(e, "MetadataAccessExpression")? {
            return match self.nav.referent(e)? {
                Some(element) => Ok(Outcome::Values(self.metadata_values(element)?)),
                None => Ok(Outcome::NotEvaluable(Reason::NoReferent { expression: e })),
            };
        }
        if self.nav.is(e, "InvocationExpression")? {
            return self.invoke(e, cx);
        }
        if self.nav.is(e, "ConstructorExpression")? {
            for parameter in self.nav.parameters(e)? {
                if self.nav.is_return(parameter)? {
                    return Ok(Outcome::Values(vec![Value::Element(parameter)]));
                }
            }
            return Ok(Outcome::NotEvaluable(Reason::NoResultExpression {
                expression: e,
            }));
        }
        let bound_cx = if matches!(cx.frames.last(),Some(Frame::Arguments{expression,..}) if *expression==e)
        {
            cx.clone()
        } else {
            cx.pushed(Frame::Arguments {
                expression: e,
                bound: vec![],
            })
        };
        if let Some(name) = self.function_name(e)?
            && crate::functions::get(&name, self.mode).is_some()
        {
            return self.invoke(e, &bound_cx);
        }
        match self.nav.result_expression(e)? {
            Some(result) => match self.eval_in(result, &bound_cx)? {
                Outcome::NotEvaluable(reason) => Ok(unevaluated(result, reason)),
                values => Ok(values),
            },
            None => Ok(unevaluated(e, Reason::NoResultExpression { expression: e })),
        }
    }
    pub(crate) fn invoke(&self, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        if let Some(name) = self.function_name(inv)?
            && let Some(function) = crate::functions::get(&name, self.mode)
        {
            cx.step(self.limits)?;
            if let Some(key) = function.names().iter().copied().find(|n| *n == name) {
                cx.observe(|o| -> Result<(), EvalError> {
                    let mut arguments = vec![None; self.argument_count(inv, cx)?];
                    for frame in cx.frames.iter().rev() {
                        if let Frame::Arguments { expression, bound } = frame
                            && *expression == inv
                        {
                            arguments = self
                                .input_parameters(inv)?
                                .iter()
                                .map(|p| {
                                    bound
                                        .iter()
                                        .find(|(q, _)| p == q)
                                        .map(|(_, v)| Outcome::Values(v.clone()))
                                })
                                .collect();
                            break;
                        }
                    }
                    let refs: Vec<_> = arguments.iter().map(Option::as_ref).collect();
                    o.apply(inv, key, &refs);
                    Ok(())
                })?
                .transpose()?;
            }
            return function.invoke(self, inv, cx);
        }
        if self.mode == EvalMode::Extended {
            return self.inline_invocation(inv, cx);
        }
        Ok(unevaluated(
            inv,
            Reason::UnknownFunction { invocation: inv },
        ))
    }
    fn inline_invocation(&self, inv: ObjId, cx: &Context) -> Result<Outcome, EvalError> {
        let Some(ty) = self.nav.instantiated_type(inv)? else {
            return Ok(unevaluated(
                inv,
                Reason::UnknownFunction { invocation: inv },
            ));
        };
        let Some(result) = self.nav.result_expression(ty)? else {
            return Ok(unevaluated(
                inv,
                Reason::NoResultExpression { expression: ty },
            ));
        };
        cx.step(self.limits)?;
        let parameters = self.input_parameters(ty)?;
        let mut bound = vec![];
        // Evaluate once, in the caller context and owned-argument order. A
        // Call frame distinguishes this from evaluate_expression's already
        // bound arguments, including when a recursive call reuses the same ID.
        for argument in self.nav.argument_bindings(inv)? {
            let Some(expression) = argument.expression else {
                continue;
            };
            let values = match self.eval_in(expression, cx)? {
                Outcome::Values(values) if self.encodable_arguments(&values)? => values,
                // Pilot creates a redefining parameter without a value when
                // expressionFor cannot encode the result. That parameter hides
                // an inherited default and evaluates as null in Extended mode.
                _ => vec![],
            };
            let parameter = parameters
                .get(argument.position)
                .copied()
                .unwrap_or(argument.parameter);
            bound.push((parameter, values));
        }
        let next = if self.nav.is(ty, "Feature")? {
            cx.pushed(Frame::Type(ty))
        } else {
            cx.clone()
        }
        .pushed(Frame::Call {
            invocation: inv,
            ty,
            bound,
        });
        match self.eval_in(result, &next)? {
            Outcome::NotEvaluable(reason) => Ok(unevaluated(result, reason)),
            values => Ok(values),
        }
    }
    fn encodable_arguments(&self, values: &[Value]) -> Result<bool, EvalError> {
        for value in values {
            if let Value::Element(e) | Value::Unevaluated { at: e, .. } = value
                && (!self.nav.is(*e, "Feature")? || self.nav.is(*e, "InvocationExpression")?)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
    // Pilot re-encodes eager arguments as expressions and body arguments as
    // FeatureValues. Scalars are virtual literals; a model Feature is a
    // reference evaluated in the callee/body context, not a frozen result.
    fn bound_values(
        &self,
        values: &[Value],
        cx: &Context,
        evaluate_expressions: bool,
    ) -> Result<Outcome, EvalError> {
        let mut result = Vec::new();
        for value in values {
            let outcome = match value {
                Value::Element(e) | Value::Unevaluated { at: e, .. }
                    if evaluate_expressions && self.nav.is(*e, "Expression")? =>
                {
                    self.eval_in(*e, cx)?
                }
                Value::Element(e) | Value::Unevaluated { at: e, .. }
                    if self.nav.is(*e, "Feature")? =>
                {
                    self.feature_in(*e, cx)?
                }
                _ => Outcome::Values(vec![value.clone()]),
            };
            match outcome {
                Outcome::Values(mut values) => result.append(&mut values),
                not_evaluable @ Outcome::NotEvaluable(_) => return Ok(not_evaluable),
            }
        }
        Ok(Outcome::Values(result))
    }
    pub(crate) fn evaluate_argument(
        &self,
        inv: ObjId,
        index: usize,
        cx: &Context,
    ) -> Result<Outcome, EvalError> {
        for frame in cx.frames.iter().rev() {
            if let Frame::Arguments { expression, bound } = frame
                && *expression == inv
            {
                let parameters = self.input_parameters(inv)?;
                return match parameters
                    .get(index)
                    .and_then(|p| bound.iter().find(|(q, _)| q == p))
                {
                    Some((_, values)) => self.bound_values(values, cx, true),
                    None => Ok(Outcome::Values(vec![])),
                };
            }
        }
        match self.nav.argument_slots(inv)?.get(index).copied().flatten() {
            Some(expression) => self.eval_in(expression, cx),
            None => Ok(Outcome::Values(vec![])),
        }
    }
    /// The owned input parameter of `ty` that implicitly redefines the
    /// inherited input parameter `feature` of a general type by position
    /// (KerML implicit parameter redefinition). None when `feature` is not an
    /// inherited parameter, or when that owned parameter declares its own
    /// redefinitions (which then decide the binding).
    fn positional_parameter(&self, feature: ObjId, ty: ObjId) -> Result<Option<ObjId>, EvalError> {
        let Some(general) = self.nav.owning_type(feature)? else {
            return Ok(None);
        };
        if general == ty || !self.nav.specializes(ty, general)? {
            return Ok(None);
        }
        let Some(position) = self
            .owned_input_parameters(general)?
            .iter()
            .position(|p| *p == feature)
        else {
            return Ok(None);
        };
        let Some(own) = self.owned_input_parameters(ty)?.get(position).copied() else {
            return Ok(None);
        };
        let explicit = self.nav.redefined_closure(own)?.iter().any(|r| *r != own);
        Ok((!explicit).then_some(own))
    }
    fn owned_input_parameters(&self, ty: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut out = Vec::new();
        for p in self.input_parameters(ty)? {
            if self.nav.owning_type(p)? == Some(ty) {
                out.push(p);
            }
        }
        Ok(out)
    }
    pub(crate) fn input_parameters(&self, e: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut out = Vec::new();
        for p in self.nav.parameters(e)? {
            if !self.nav.is_return(p)? {
                out.push(p);
            }
        }
        Ok(out)
    }
    pub(crate) fn argument_count(&self, inv: ObjId, cx: &Context) -> Result<usize, EvalError> {
        for frame in cx.frames.iter().rev() {
            if let Frame::Arguments { expression, bound } = frame
                && *expression == inv
            {
                return Ok(bound.len());
            }
        }
        Ok(self.nav.argument_slots(inv)?.len())
    }
    pub(crate) fn evaluate_expression(
        &self,
        e: ObjId,
        cx: &Context,
        args: &[Value],
    ) -> Result<Outcome, EvalError> {
        let parameters = self.input_parameters(e)?;
        let bound = parameters
            .into_iter()
            .zip(args)
            .map(|(p, v)| (p, vec![v.clone()]))
            .collect();
        self.eval_in(
            e,
            &cx.pushed(Frame::Arguments {
                expression: e,
                bound,
            }),
        )
    }
    #[allow(dead_code)] // Lazy control functions consume this in M1.6.
    pub(crate) fn expression_value(
        &self,
        inv: ObjId,
        index: usize,
        cx: &Context,
    ) -> Result<Option<Outcome>, EvalError> {
        let argument = self.evaluate_argument(inv, index, cx)?;
        match crate::functions::argument_value(&argument) {
            Some(Value::Element(e) | Value::Unevaluated { at: e, .. })
                if self.nav.is(*e, "Expression")? =>
            {
                self.eval_in(*e, cx).map(Some)
            }
            Some(
                v @ (Value::Bool(_)
                | Value::Int(_)
                | Value::Real(_)
                | Value::Str(_)
                | Value::Infinity),
            ) => Ok(Some(Outcome::Values(vec![v.clone()]))),
            _ => Ok(None),
        }
    }
    #[allow(dead_code)] // Lazy Boolean functions consume this in M1.6.
    pub(crate) fn boolean_expression_value(
        &self,
        inv: ObjId,
        index: usize,
        cx: &Context,
    ) -> Result<Option<bool>, EvalError> {
        Ok(self
            .expression_value(inv, index, cx)?
            .as_ref()
            .and_then(crate::functions::boolean_value))
    }
    pub(crate) fn function_name(&self, e: ObjId) -> Result<Option<String>, EvalError> {
        if self.nav.is(e, "OperatorExpression")? {
            let op = self
                .nav
                .string(e, "OperatorExpression", "operator")?
                .ok_or_else(|| self.nav.error(e, "operator has no spelling"))?;
            return Ok(operator_function(op).map(str::to_owned));
        }
        let mut candidates = vec![];
        if self.nav.is(e, "InstantiationExpression")? {
            if let Some(t) = self.nav.instantiated_type(e)? {
                if self.nav.is(t, "Function")? {
                    candidates.push(t);
                } else {
                    candidates.extend(self.nav.types(t)?);
                }
            }
        } else {
            candidates.extend(self.nav.types(e)?);
        }
        for t in candidates {
            if self.nav.is(t, "Function")? {
                return match self.library.name(t) {
                    Some(name) => Ok(Some(name.into())),
                    None => self.nav.qualified_name(t),
                };
            }
        }
        Ok(None)
    }
    fn literal(&self, e: ObjId) -> Result<Value, EvalError> {
        let object = self.nav.object(e)?;
        if self.nav.is(e, "LiteralInfinity")? {
            return Ok(Value::Infinity);
        }
        if self.nav.is(e, "LiteralString")? {
            return object
                .str(self.nav.schema.field("LiteralString", "value")?)
                .map(|v| Value::Str(v.into()))
                .ok_or_else(|| self.nav.error(e, "literal string value is absent"));
        }
        if self.nav.is(e, "LiteralInteger")? {
            let n = match object.get(self.nav.schema.field("LiteralInteger", "value")?) {
                Some(ValueRef::Int(n)) => Some(n),
                Some(ValueRef::Default(v) | ValueRef::Json(v)) => v.as_i64(),
                _ => None,
            };
            return n
                .map(Value::Int)
                .ok_or_else(|| self.nav.error(e, "literal integer value is not i64"));
        }
        if self.nav.is(e, "LiteralBoolean")? {
            let b = match object.get(self.nav.schema.field("LiteralBoolean", "value")?) {
                Some(ValueRef::Bool(b)) => Some(b),
                Some(ValueRef::Default(v) | ValueRef::Json(v)) => v.as_bool(),
                _ => None,
            };
            return b
                .map(Value::Bool)
                .ok_or_else(|| self.nav.error(e, "literal Boolean value is not Boolean"));
        }
        if self.nav.is(e, "LiteralRational")? {
            if let Some(ValueRef::Json(v) | ValueRef::Default(v)) =
                object.get(self.nav.schema.field("LiteralRational", "value")?)
                && v.get("kind").and_then(|v| v.as_str()) == Some("float64")
                && let Some(bits) = v
                    .get("bits")
                    .and_then(|v| v.as_str())
                    .filter(|s| s.len() == 16)
            {
                let bits = u64::from_str_radix(bits, 16)
                    .map_err(|_| self.nav.error(e, "invalid float64 bits"))?;
                return Ok(Value::Real(f64::from_bits(bits)));
            }
            return Err(self
                .nav
                .error(e, "literal rational has no float64 encoding"));
        }
        Err(self.nav.error(e, "unrecognized literal metaclass"))
    }
}
pub(crate) fn unevaluated(at: ObjId, reason: Reason) -> Outcome {
    Outcome::Values(vec![Value::Unevaluated { at, reason }])
}
