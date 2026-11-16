//! Handwritten deterministic evaluation queries over stored relationships.
//! These are the E8-default evaluation queries, not a second name resolver.
#![allow(dead_code)] // Consumers arrive in the following ordered M1 tasks.
use crate::{EvalError, Model, ObjId, schema::EvalSchema};
use intersect_model::core::frozen::{Obj, ValueRef};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub(crate) struct Nav<'m> {
    pub model: &'m Model,
    pub schema: Rc<EvalSchema>,
    features: RefCell<BTreeMap<ObjId, Vec<ObjId>>>,
    redefinitions: RefCell<BTreeMap<ObjId, Vec<ObjId>>>,
}
pub(crate) struct ArgumentBinding {
    pub parameter: ObjId,
    pub position: usize,
    pub expression: Option<ObjId>,
}
impl<'m> Nav<'m> {
    /// Feature names and short names are inherited through redefinition when
    /// not declared. The closure includes self first and terminates cycles.
    pub fn effective_name(
        &self,
        e: ObjId,
        field: &'static str,
    ) -> Result<Option<String>, EvalError> {
        let candidates = if self.is(e, "Feature")? {
            self.redefined_closure(e)?
        } else {
            vec![e]
        };
        for candidate in candidates {
            if let Some(name) = self.string(candidate, "Element", field)? {
                return Ok(Some(name.into()));
            }
        }
        Ok(None)
    }
    pub fn all_parameters(&self, ty: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut out = Vec::new();
        for f in self.all_features(ty)? {
            // TypeUtil.getAllParametersOf uses directed features. SysML
            // directed usages need not have ParameterMembership containers.
            if self.direction(f)?.is_some() {
                out.push(f);
            }
        }
        Ok(out)
    }
    pub fn owning_type(&self, feature: ObjId) -> Result<Option<ObjId>, EvalError> {
        if let Some((membership, _)) = self.object(feature)?.container()
            && self.is(membership.id(), "FeatureMembership")?
            && let Some((owner, _)) = membership.container()
            && self.is(owner.id(), "Type")?
        {
            return Ok(Some(owner.id()));
        }
        Ok(None)
    }
    pub fn has_featuring_type(&self, feature: ObjId) -> Result<bool, EvalError> {
        let mut current = feature;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(current) {
                return Err(self.error(
                    feature,
                    "cyclic first chaining feature has no determinable featuring type",
                ));
            }
            // A chain's featuring type comes from its first segment, not its
            // own container or any later segment. Resolve nested chains too.
            if let Some(first) = self.chaining_features(current)?.first().copied() {
                current = first;
                continue;
            }
            if self.owning_type(current)?.is_some() {
                return Ok(true);
            }
            for rel in self.relationships(current)? {
                if self.is(rel, "TypeFeaturing")?
                    && !self.refs(rel, "TypeFeaturing", "featuringType")?.is_empty()
                {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
    }
    pub fn type_feature_for(&self, feature: ObjId, ty: ObjId) -> Result<Option<ObjId>, EvalError> {
        for candidate in self.all_features(ty)? {
            if self.redefined_closure(candidate)?.contains(&feature) {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
    pub fn new(model: &'m Model, schema: &EvalSchema) -> Self {
        Self {
            model,
            schema: Rc::new(schema.clone()),
            features: RefCell::new(BTreeMap::new()),
            redefinitions: RefCell::new(BTreeMap::new()),
        }
    }
    pub fn object(&self, id: ObjId) -> Result<Obj<'m>, EvalError> {
        self.model.obj(id).ok_or_else(|| EvalError::Model {
            object: format!("object-index:{}", id.index()),
            message: "object is absent from this model".into(),
        })
    }
    pub fn error(&self, id: ObjId, message: impl Into<String>) -> EvalError {
        EvalError::Model {
            object: self.model.obj(id).map_or_else(
                || format!("object-index:{}", id.index()),
                |o| o.identity().to_owned(),
            ),
            message: message.into(),
        }
    }
    pub fn is(&self, id: ObjId, class: &str) -> Result<bool, EvalError> {
        Ok(self
            .model
            .metamodel()
            .is_a(self.object(id)?.class(), self.schema.class(class)?))
    }
    pub fn refs(
        &self,
        id: ObjId,
        class: &'static str,
        name: &'static str,
    ) -> Result<Vec<ObjId>, EvalError> {
        Ok(self
            .object(id)?
            .refs(self.schema.field(class, name)?)
            .map(|o| o.id())
            .collect())
    }
    pub fn string(
        &self,
        id: ObjId,
        class: &'static str,
        name: &'static str,
    ) -> Result<Option<&'m str>, EvalError> {
        Ok(self.object(id)?.str(self.schema.field(class, name)?))
    }
    pub fn relationships(&self, id: ObjId) -> Result<Vec<ObjId>, EvalError> {
        self.refs(id, "Element", "ownedRelationship")
    }
    pub fn owned(&self, id: ObjId) -> Result<Vec<ObjId>, EvalError> {
        self.refs(id, "Relationship", "ownedRelatedElement")
    }
    fn child(&self, id: ObjId) -> Result<ObjId, EvalError> {
        self.owned(id)?
            .first()
            .copied()
            .ok_or_else(|| self.error(id, "owning relationship has no owned element"))
    }
    pub fn direction(&self, id: ObjId) -> Result<Option<&'static str>, EvalError> {
        match self
            .object(id)?
            .get(self.schema.field("Feature", "direction")?)
        {
            None | Some(ValueRef::Null) => Ok(None),
            Some(ValueRef::Default(v) | ValueRef::Json(v)) if v.is_null() => Ok(None),
            Some(ValueRef::Default(v) | ValueRef::Json(v)) => self
                .schema
                .directions
                .iter()
                .find(|(_, expected)| expected == v)
                .map(|(name, _)| Some(*name))
                .ok_or_else(|| self.error(id, "unknown feature direction")),
            _ => Err(self.error(id, "feature direction is not an enumeration")),
        }
    }
    pub fn owned_features(&self, t: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut result = vec![];
        for r in self.relationships(t)? {
            if self.is(r, "FeatureMembership")? {
                let f = self.child(r)?;
                if !self.is(f, "Feature")? {
                    return Err(self.error(r, "feature membership does not own a Feature"));
                }
                result.push(f);
            }
        }
        Ok(result)
    }
    pub fn operands(&self, e: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut result = vec![];
        for f in self.owned_features(e)? {
            if matches!(self.direction(f)?, Some("in" | "inout"))
                && let Some(value) = self.feature_value(f)?
            {
                result.push(value);
            }
        }
        Ok(result)
    }
    pub fn feature_value(&self, f: ObjId) -> Result<Option<ObjId>, EvalError> {
        for r in self.relationships(f)? {
            if self.is(r, "FeatureValue")? {
                let value = self.child(r)?;
                if !self.is(value, "Expression")? {
                    return Err(self.error(r, "feature value is not an Expression"));
                }
                return Ok(Some(value));
            }
        }
        Ok(None)
    }
    pub fn referent(&self, e: ObjId) -> Result<Option<ObjId>, EvalError> {
        for r in self.relationships(e)? {
            if self.is(r, "Membership")? && !self.is(r, "OwningMembership")? {
                return self
                    .refs(r, "Membership", "memberElement")?
                    .first()
                    .copied()
                    .map(Some)
                    .ok_or_else(|| self.error(r, "membership has no memberElement"));
            }
        }
        for r in self.relationships(e)? {
            // OwnedExpressionReference (including lazy branches) owns its
            // expression through FeatureMembership. Only parameter memberships
            // must be skipped, especially the implicit return parameter.
            if self.is(r, "OwningMembership")? && !self.is(r, "ParameterMembership")? {
                let child = self.child(r)?;
                if self.is(child, "Feature")? {
                    return Ok(Some(child));
                }
            }
        }
        Ok(None)
    }
    pub fn instantiated_type(&self, e: ObjId) -> Result<Option<ObjId>, EvalError> {
        if let Some(t) = self.referent(e)? {
            if !self.is(t, "Type")? {
                return Err(self.error(e, "instantiated type is not a Type"));
            }
            return Ok(Some(t));
        }
        Ok(None)
    }
    pub fn is_return(&self, f: ObjId) -> Result<bool, EvalError> {
        match self.object(f)?.container() {
            Some((owner, _)) => self.is(owner.id(), "ReturnParameterMembership"),
            None => Ok(false),
        }
    }
    pub fn parameters(&self, t: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut result = vec![];
        for f in self.all_features(t)? {
            // SysML directed ReferenceUsages use FeatureMembership, while
            // KerML parameters commonly use ParameterMembership. Direction,
            // not that narrower membership class, determines the inputs.
            if matches!(self.direction(f)?, Some("in" | "inout")) || self.is_return(f)? {
                result.push(f);
            }
        }
        Ok(result)
    }
    pub fn result_expression(&self, t: ObjId) -> Result<Option<ObjId>, EvalError> {
        // EvaluationUtil.getResultExpressionFor uses feature memberships,
        // including inherited ones. The gated default still uses only the
        // dedicated owned-result query below.
        for result in self.all_features(t)? {
            if let Some((membership, _)) = self.object(result)?.container()
                && self.is(membership.id(), "ResultExpressionMembership")?
            {
                if !self.is(result, "Expression")? {
                    return Err(self.error(result, "result is not an Expression"));
                }
                return Ok(Some(result));
            }
        }
        for f in self.parameters(t)? {
            if self.is_return(f)? {
                return self.feature_value(f);
            }
        }
        Ok(None)
    }
    /// The default EMF Expression::evaluate delegate reads only an owned
    /// ResultExpressionMembership. Ungated bodies additionally use a return
    /// parameter's value through result_expression above (REF 1.6 and 3).
    pub fn owned_result_expression(&self, t: ObjId) -> Result<Option<ObjId>, EvalError> {
        for r in self.relationships(t)? {
            if self.is(r, "ResultExpressionMembership")? {
                let e = self.child(r)?;
                if !self.is(e, "Expression")? {
                    return Err(self.error(r, "result is not an Expression"));
                }
                return Ok(Some(e));
            }
        }
        Ok(None)
    }
    pub fn generals(&self, t: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut result = vec![];
        for r in self.relationships(t)? {
            let field = if self.is(r, "FeatureTyping")? {
                Some(("FeatureTyping", "type"))
            } else if self.is(r, "Redefinition")? {
                Some(("Redefinition", "redefinedFeature"))
            } else if self.is(r, "Subsetting")? {
                Some(("Subsetting", "subsettedFeature"))
            } else if self.is(r, "Subclassification")? {
                Some(("Subclassification", "superclassifier"))
            } else if self.is(r, "Specialization")? {
                Some(("Specialization", "general"))
            } else if self.is(r, "Conjugation")? {
                Some(("Conjugation", "originalType"))
            } else {
                None
            };
            if let Some((class, field)) = field {
                let g = self
                    .refs(r, class, field)?
                    .first()
                    .copied()
                    .ok_or_else(|| self.error(r, "specialization has no target"))?;
                if !self.is(g, "Type")? {
                    return Err(self.error(r, "specialization target is not a Type"));
                }
                result.push(g);
            }
        }
        Ok(result)
    }
    pub fn redefined_closure(&self, f: ObjId) -> Result<Vec<ObjId>, EvalError> {
        if let Some(v) = self.redefinitions.borrow().get(&f) {
            return Ok(v.clone());
        }
        let mut result = vec![];
        let mut seen = BTreeSet::new();
        let mut stack = vec![f];
        while let Some(f) = stack.pop() {
            self.object(f)?;
            if !seen.insert(f) {
                continue;
            }
            result.push(f);
            let mut next = vec![];
            for r in self.relationships(f)? {
                if self.is(r, "Redefinition")? {
                    let g = self
                        .refs(r, "Redefinition", "redefinedFeature")?
                        .first()
                        .copied()
                        .ok_or_else(|| self.error(r, "redefinition has no target"))?;
                    next.push(g);
                }
            }
            stack.extend(next.into_iter().rev());
        }
        self.redefinitions.borrow_mut().insert(f, result.clone());
        Ok(result)
    }
    pub fn all_features(&self, t: ObjId) -> Result<Vec<ObjId>, EvalError> {
        if let Some(v) = self.features.borrow().get(&t) {
            return Ok(v.clone());
        }
        let owned = self.owned_features(t)?;
        let mut inherited = vec![];
        let mut hidden = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut emitted: BTreeSet<_> = owned.iter().copied().collect();
        seen.insert(t);
        let mut stack: Vec<_> = self.generals(t)?.into_iter().rev().collect();
        while let Some(t) = stack.pop() {
            if !seen.insert(t) {
                continue;
            }
            for f in self.owned_features(t)? {
                if emitted.insert(f) {
                    inherited.push(f);
                }
            }
            stack.extend(self.generals(t)?.into_iter().rev());
        }
        // Collect first, then remove inherited redefinitions globally. A
        // more-specific feature may be reached after its base through another
        // general; DFS order must not decide whether that base remains visible.
        for f in owned.iter().chain(&inherited) {
            hidden.extend(self.redefined_closure(*f)?.into_iter().filter(|r| r != f));
        }
        // Preserve the existing first-owned-feature rule, including cycles
        // among directly owned features. Only inherited filtering is global.
        let mut result = vec![];
        let mut owned_hidden = BTreeSet::new();
        for f in owned {
            if owned_hidden.insert(f) {
                result.push(f);
                owned_hidden.extend(self.redefined_closure(f)?);
            }
        }
        result.extend(inherited.into_iter().filter(|f| !hidden.contains(f)));
        self.features.borrow_mut().insert(t, result.clone());
        Ok(result)
    }
    pub fn specializes(&self, t: ObjId, general: ObjId) -> Result<bool, EvalError> {
        self.object(general)?;
        let mut seen = BTreeSet::new();
        let mut stack = vec![t];
        while let Some(t) = stack.pop() {
            self.object(t)?;
            if t == general {
                return Ok(true);
            }
            if seen.insert(t) {
                stack.extend(self.generals(t)?.into_iter().rev());
            }
        }
        Ok(false)
    }
    pub fn types(&self, f: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut result = vec![];
        let mut seen = BTreeSet::new();
        let mut emitted = BTreeSet::new();
        let mut stack = vec![f];
        while let Some(f) = stack.pop() {
            if !seen.insert(f) {
                continue;
            }
            let mut inherited = vec![];
            for r in self.relationships(f)? {
                if self.is(r, "FeatureTyping")? {
                    for t in self.refs(r, "FeatureTyping", "type")? {
                        if emitted.insert(t) {
                            result.push(t);
                        }
                    }
                } else if self.is(r, "Subsetting")? {
                    let (class, field) = if self.is(r, "Redefinition")? {
                        ("Redefinition", "redefinedFeature")
                    } else {
                        ("Subsetting", "subsettedFeature")
                    };
                    inherited.extend(self.refs(r, class, field)?);
                }
            }
            stack.extend(inherited.into_iter().rev());
        }
        Ok(result)
    }
    pub fn chaining_features(&self, f: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut result = vec![];
        for r in self.relationships(f)? {
            if self.is(r, "FeatureChaining")? {
                result.push(
                    self.refs(r, "FeatureChaining", "chainingFeature")?
                        .first()
                        .copied()
                        .ok_or_else(|| self.error(r, "feature chaining has no target"))?,
                );
            }
        }
        Ok(result)
    }
    /// Positional slots retain holes for omitted named arguments.
    pub fn argument_slots(&self, inv: ObjId) -> Result<Vec<Option<ObjId>>, EvalError> {
        let mut slots = vec![];
        for argument in self.argument_bindings(inv)? {
            let length = argument
                .position
                .checked_add(1)
                .ok_or_else(|| self.error(inv, "argument index overflow"))?;
            if slots.len() < length {
                slots.resize(length, None);
            }
            let slot = slots
                .get_mut(argument.position)
                .ok_or_else(|| self.error(inv, "argument slot absent"))?;
            *slot = argument.expression;
        }
        Ok(slots)
    }
    /// Owned argument order, retaining formal positions for named arguments.
    pub fn argument_bindings(&self, inv: ObjId) -> Result<Vec<ArgumentBinding>, EvalError> {
        let mut formals = vec![];
        if let Some(t) = self.instantiated_type(inv)? {
            for p in self.parameters(t)? {
                if !self.is_return(p)? {
                    formals.push(p);
                }
            }
        }
        let mut bindings = vec![];
        let mut seen = BTreeSet::new();
        let mut ordinal = 0usize;
        for p in self.owned_features(inv)? {
            if !matches!(self.direction(p)?, Some("in" | "inout")) {
                continue;
            }
            let closure = self.redefined_closure(p)?;
            let position = formals
                .iter()
                .position(|f| closure.contains(f))
                .unwrap_or(ordinal);
            ordinal = ordinal
                .checked_add(1)
                .ok_or_else(|| self.error(inv, "argument count overflow"))?;
            if !seen.insert(position) {
                return Err(self.error(inv, "duplicate argument for parameter"));
            }
            bindings.push(ArgumentBinding {
                parameter: p,
                position,
                expression: self.feature_value(p)?,
            });
        }
        Ok(bindings)
    }
    pub fn arguments(&self, inv: ObjId) -> Result<Vec<ObjId>, EvalError> {
        Ok(self.argument_slots(inv)?.into_iter().flatten().collect())
    }
    pub fn qualified_name(&self, e: ObjId) -> Result<Option<String>, EvalError> {
        let Some(name) = self
            .effective_name(e, "declaredName")?
            .or(self.effective_name(e, "declaredShortName")?)
        else {
            return Ok(None);
        };
        let mut parts = vec![escape(&name)];
        let mut seen = BTreeSet::from([e]);
        let mut current = self.object(e)?;
        while let Some((parent, _)) = current.container() {
            if !seen.insert(parent.id()) {
                return Err(self.error(e, "containment cycle in qualified name"));
            }
            if self.is(parent.id(), "Namespace")?
                && let Some(name) = self
                    .effective_name(parent.id(), "declaredName")?
                    .or(self.effective_name(parent.id(), "declaredShortName")?)
            {
                parts.push(escape(&name));
            }
            current = parent;
        }
        parts.reverse();
        Ok(Some(parts.join("::")))
    }
}
pub(crate) fn escape(name: &str) -> String {
    let mut chars = name.chars();
    if chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        name.into()
    } else {
        format!("'{}'", name.replace('\\', "\\\\").replace('\'', "\\'"))
    }
}

#[cfg(test)]
#[path = "nav_tests.rs"]
mod tests;
