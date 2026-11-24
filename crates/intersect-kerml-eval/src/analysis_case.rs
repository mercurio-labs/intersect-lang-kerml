//! Handwritten extraction of the scalar solve subset of SysML analysis cases.
//! Solver selection and execution live in the linked analysis adapter.
use crate::{EvalError, Evaluator, ObjId, Outcome, Value};
use intersect_model::core::frozen::ValueRef;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarUnit {
    Scalar,
    Litre,
    Second,
    LitresPerSecond,
}

pub struct Parameter {
    pub source: ObjId,
    pub unit: ScalarUnit,
    pub value: Option<(f64, ScalarUnit)>,
}

pub enum Term {
    Parameter(String),
    Literal(f64, ScalarUnit),
    Binary {
        operator: Operator,
        left: Box<Term>,
        right: Box<Term>,
    },
}

pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

pub enum SolveBody {
    Equation {
        object: ObjId,
        left: Term,
        right: Term,
    },
    Representation {
        object: ObjId,
        language: String,
        body: String,
    },
}

pub struct SolveCase {
    pub object: ObjId,
    pub parameters: BTreeMap<String, Parameter>,
    pub result: String,
    pub body: SolveBody,
}

struct Parameters {
    values: BTreeMap<String, Parameter>,
    result: String,
    references: BTreeMap<ObjId, String>,
    declarations: BTreeMap<String, ObjId>,
    subject: Option<(ObjId, Option<ObjId>)>,
}

pub(crate) struct SolveDescription {
    pub case: SolveCase,
    pub declarations: BTreeMap<String, ObjId>,
    pub subject: Option<(ObjId, Option<ObjId>)>,
}

impl Evaluator<'_> {
    /// Resolve an analysis usage with one scalar return parameter and one
    /// asserted equality or foreign representation. The linked model supplies
    /// feature identities, inheritance and redefinition.
    pub fn describe_solve(&self, name: &str) -> Result<SolveCase, EvalError> {
        let case = self.resolve_target(name)?;
        Ok(self.describe_analysis(case, false)?.case)
    }

    pub(crate) fn describe_analysis(
        &self,
        case: ObjId,
        execute: bool,
    ) -> Result<SolveDescription, EvalError> {
        if self.class_name(case)? != "AnalysisCaseUsage" {
            return Err(self
                .nav
                .error(case, "expected the name of a SysML analysis usage"));
        }
        let mut definitions = Vec::new();
        for id in self.nav.types(case)? {
            if self.class_name(id)? == "AnalysisCaseDefinition"
                && (!execute
                    || self.nav.qualified_name(id)?.as_deref()
                        != Some("AnalysisCases::AnalysisCase"))
            {
                definitions.push(id);
            }
        }
        let Some(&definition) = definitions.first() else {
            return Err(self.nav.error(case, "analysis has no definition"));
        };
        if execute {
            if definitions.len() != 1 {
                return Err(self.nav.error(case, "analysis must have one definition"));
            }
            self.check_case_specialization(case, definition)?;
        }
        let mut bodies = Vec::new();
        let mm = self.nav.model.metamodel();
        let action_class = if execute {
            Some(
                mm.schema()
                    .classes
                    .iter()
                    .find(|(_, class)| class.name == "ActionUsage")
                    .and_then(|(key, _)| mm.class(key))
                    .ok_or_else(|| EvalError::Schema("ActionUsage".into()))?,
            )
        } else {
            None
        };
        let mut pending = vec![(definition, false), (case, false)];
        let mut seen = BTreeSet::new();
        while let Some((owner, nested)) = pending.pop() {
            if !seen.insert(owner) {
                continue;
            }
            for relation in self.nav.relationships(owner)? {
                if self.class_name(relation)? == "ObjectiveMembership" {
                    return Err(self.nav.error(
                        relation,
                        "explicit SysML objectives are outside the scalar solve profile",
                    ));
                }
                for element in self.nav.owned(relation)? {
                    if let Some(action_class) = action_class {
                        if nested
                            && (mm.is_a(self.nav.object(element)?.class(), action_class)
                                || self.nav.is(element, "ConstraintUsage")?)
                        {
                            return Err(self.nav.error(
                                element,
                                "nested actions and constraints are outside the scalar-affine profile",
                            ));
                        }
                        pending.push((element, true));
                    }
                    if self.class_name(element)? == "TextualRepresentation" {
                        if execute {
                            return Err(self.nav.error(
                                element,
                                "foreign analysis bodies are outside the scalar-affine profile",
                            ));
                        }
                        let object = self.nav.object(element)?;
                        let content = |feature| {
                            mm.feature(object.class(), feature)
                                .and_then(|field| object.str(field))
                                .map(str::to_owned)
                                .ok_or_else(|| {
                                    self.nav.error(
                                        element,
                                        format!("textual representation lacks {feature}"),
                                    )
                                })
                        };
                        bodies.push(SolveBody::Representation {
                            object: element,
                            language: content("language")?,
                            body: content("body")?,
                        });
                    }
                }
            }
        }
        let Parameters {
            values,
            result,
            references,
            declarations,
            subject,
        } = self.solve_parameters(case, definition, execute)?;
        for feature in self.nav.all_features(case)? {
            if self.class_name(feature)? != "AssertConstraintUsage" {
                continue;
            }
            let object = self.nav.object(feature)?;
            let mm = self.nav.model.metamodel();
            let negation = mm
                .feature(object.class(), "isNegated")
                .and_then(|field| object.get(field));
            if matches!(negation, Some(ValueRef::Bool(true)))
                || matches!(negation, Some(ValueRef::Default(value) | ValueRef::Json(value)) if value.as_bool() == Some(true))
            {
                return Err(self.nav.error(
                    feature,
                    "a negated assertion is outside the scalar solve profile",
                ));
            }
            let expr = self.nav.owned_result_expression(feature)?.ok_or_else(|| {
                self.nav
                    .error(feature, "expected an inline asserted equality")
            })?;
            let (operator, operands) = self.operator(expr)?;
            let [left, right] = operands.as_slice() else {
                return Err(self
                    .nav
                    .error(expr, "expected two operands in the asserted equality"));
            };
            if operator != "==" {
                return Err(self.nav.error(expr, "expected an asserted equality"));
            }
            let term = |e| self.analysis_term(e, case, &references, 0);
            bodies.push(SolveBody::Equation {
                object: expr,
                left: term(*left)?,
                right: term(*right)?,
            });
        }
        if bodies.len() != 1 {
            let expected = if execute {
                "expected exactly one asserted equality"
            } else {
                "expected exactly one asserted equality or foreign representation"
            };
            return Err(self.nav.error(case, expected));
        }
        let body = bodies
            .pop()
            .ok_or_else(|| self.nav.error(case, "analysis body is missing"))?;
        Ok(SolveDescription {
            case: SolveCase {
                object: case,
                parameters: values,
                result,
                body,
            },
            declarations,
            subject,
        })
    }

    fn check_case_specialization(&self, case: ObjId, definition: ObjId) -> Result<(), EvalError> {
        for owner in [case, definition] {
            for general in self.nav.generals(owner)? {
                if general != definition
                    && !matches!(
                        self.nav.qualified_name(general)?.as_deref(),
                        Some("AnalysisCases::AnalysisCase" | "AnalysisCases::analysisCases")
                    )
                {
                    return Err(self.nav.error(
                        owner,
                        "additional analysis specializations are outside the scalar-affine profile",
                    ));
                }
            }
        }
        Ok(())
    }

    fn solve_parameters(
        &self,
        case: ObjId,
        definition: ObjId,
        execute: bool,
    ) -> Result<Parameters, EvalError> {
        let mut parameters = BTreeMap::new();
        let mut references = BTreeMap::new();
        let mut declarations = BTreeMap::new();
        let mut result = None;
        let mut subject = None;
        for feature in self.nav.all_features(case)? {
            let closure = self.nav.redefined_closure(feature)?;
            let owned_here = self
                .nav
                .owning_type(feature)?
                .is_some_and(|owner| owner == case || owner == definition);
            if !owned_here {
                continue;
            }
            let object = self.nav.object(feature)?;
            if let Some((membership, _)) = object.container()
                && self.class_name(membership.id())? == "SubjectMembership"
            {
                if execute {
                    let target = self.analysis_subject(&closure)?;
                    if subject.replace((feature, target)).is_some() {
                        return Err(self
                            .nav
                            .error(feature, "analysis has multiple effective subjects"));
                    }
                }
                continue;
            }
            let returns = self.nav.is_return(feature)?;
            let mut binding = None;
            let mut input = false;
            for id in &closure {
                input |= self.nav.direction(*id)? == Some("in");
                if binding.is_none() {
                    binding = self.nav.feature_value(*id)?;
                }
            }
            if execute {
                let class = self.class_name(feature)?;
                if class == "AssertConstraintUsage" && !returns && !input {
                    continue;
                }
                if !(returns || input) || !matches!(class, "ReferenceUsage" | "AttributeUsage") {
                    return Err(self.nav.error(
                        feature,
                        format!("analysis member {class} is outside the scalar-affine profile"),
                    ));
                }
                self.check_scalar_multiplicity(&closure)?;
            } else if !returns && !input {
                continue;
            }
            let name = self
                .nav
                .effective_name(feature, "declaredName")?
                .ok_or_else(|| {
                    self.nav
                        .error(feature, "analysis parameter must have a name")
                })?;
            let unit = self.analysis_unit(feature)?;
            let value = if returns {
                if binding.is_some() || result.replace(name.clone()).is_some() {
                    return Err(self
                        .nav
                        .error(feature, "analysis needs one unbound return parameter"));
                }
                None
            } else {
                binding.map(|expr| self.quantity(expr, case)).transpose()?
            };
            if parameters
                .insert(
                    name.clone(),
                    Parameter {
                        source: binding.unwrap_or(feature),
                        unit,
                        value,
                    },
                )
                .is_some()
            {
                return Err(self.nav.error(feature, "ambiguous analysis parameter name"));
            }
            declarations.insert(name.clone(), feature);
            for id in closure {
                if references.insert(id, name.clone()).is_some() {
                    return Err(self
                        .nav
                        .error(feature, "analysis parameters redefine the same feature"));
                }
            }
        }
        let result =
            result.ok_or_else(|| self.nav.error(case, "analysis has no return parameter"))?;
        Ok(Parameters {
            values: parameters,
            result,
            references,
            declarations,
            subject,
        })
    }

    pub(crate) fn check_scalar_multiplicity(&self, features: &[ObjId]) -> Result<(), EvalError> {
        for &feature in features {
            for membership in self.nav.relationships(feature)? {
                for range in self.nav.owned(membership)? {
                    if !self.nav.is(range, "MultiplicityRange")? {
                        continue;
                    }
                    let mut bounds = Vec::new();
                    for member in self.nav.relationships(range)? {
                        for bound in self.nav.owned(member)? {
                            if self.nav.is(bound, "Expression")? {
                                bounds.push(self.multiplicity_bound(bound)?);
                            }
                        }
                    }
                    if !matches!(bounds.as_slice(), [Some(1)] | [Some(1), Some(1)]) {
                        return Err(self
                            .nav
                            .error(range, "the scalar-affine profile requires multiplicity one"));
                    }
                }
            }
        }
        Ok(())
    }

    fn analysis_subject(&self, features: &[ObjId]) -> Result<Option<ObjId>, EvalError> {
        for &feature in features {
            if let Some(expression) = self.nav.feature_value(feature)? {
                if self.nav.is(expression, "FeatureReferenceExpression")?
                    && let Some(target) = self.nav.referent(expression)?
                    && self.nav.is(target, "Feature")?
                {
                    return Ok(Some(target));
                }
                return Err(self.nav.error(
                    expression,
                    "the scalar-affine profile requires a directly referenced subject",
                ));
            }
        }
        Ok(None)
    }

    pub(crate) fn analysis_unit(&self, parameter: ObjId) -> Result<ScalarUnit, EvalError> {
        // Resolve by linked library identity, including imports and aliases.
        let mut unit = None;
        for id in self.nav.types(parameter)? {
            let ty = self.nav.qualified_name(id)?;
            let candidate = match ty.as_deref() {
                Some("ISQSpaceTime::VolumeValue") => Some(ScalarUnit::Litre),
                Some("ISQBase::DurationValue") => Some(ScalarUnit::Second),
                Some("ISQMechanics::VolumeFlowRateValue") => Some(ScalarUnit::LitresPerSecond),
                Some("ScalarValues::Real") => Some(ScalarUnit::Scalar),
                _ => None,
            };
            if let Some(candidate) = candidate {
                if unit.is_some_and(|unit| unit != candidate) {
                    return Err(self.nav.error(
                        parameter,
                        "analysis parameter has conflicting quantity types",
                    ));
                }
                unit = Some(candidate);
            }
        }
        unit.ok_or_else(|| {
            self.nav.error(
                parameter,
                "analysis parameter has an unsupported quantity type",
            )
        })
    }

    fn analysis_term(
        &self,
        expr: ObjId,
        case: ObjId,
        references: &BTreeMap<ObjId, String>,
        depth: usize,
    ) -> Result<Term, EvalError> {
        if depth >= 64 {
            return Err(self
                .nav
                .error(expr, "analysis expression depth limit exceeded"));
        }
        if self.nav.is(expr, "FeatureReferenceExpression")? {
            let name = self
                .nav
                .referent(expr)?
                .and_then(|id| references.get(&id))
                .ok_or_else(|| {
                    self.nav
                        .error(expr, "expression must reference an analysis parameter")
                })?;
            return Ok(Term::Parameter(name.clone()));
        }
        if self.nav.is(expr, "OperatorExpression")? {
            let (operator, operands) = self.operator(expr)?;
            if operator != "[" {
                let operator = match operator {
                    "+" => Operator::Add,
                    "-" => Operator::Subtract,
                    "*" => Operator::Multiply,
                    "/" => Operator::Divide,
                    _ => {
                        return Err(self
                            .nav
                            .error(expr, "unsupported analysis arithmetic operator"));
                    }
                };
                let [left, right] = operands.as_slice() else {
                    return Err(self
                        .nav
                        .error(expr, "analysis arithmetic requires two operands"));
                };
                let lower = |child| {
                    self.analysis_term(child, case, references, depth + 1)
                        .map(Box::new)
                };
                return Ok(Term::Binary {
                    operator,
                    left: lower(*left)?,
                    right: lower(*right)?,
                });
            }
        } else if !self.nav.is(expr, "LiteralExpression")? {
            return Err(self.nav.error(expr, "unsupported analysis expression"));
        }
        let (value, unit) = self.quantity(expr, case)?;
        Ok(Term::Literal(value, unit))
    }

    fn quantity(&self, expr: ObjId, case: ObjId) -> Result<(f64, ScalarUnit), EvalError> {
        if self.nav.is(expr, "OperatorExpression")? {
            let (operator, operands) = self.operator(expr)?;
            if operator == "[" {
                let [value, unit] = operands.as_slice() else {
                    return Err(self
                        .nav
                        .error(expr, "expected exactly one quantity and unit"));
                };
                let unit = self.unit_reference(*unit)?;
                return Ok((self.number(*value, case)?, unit));
            }
        }
        Ok((self.number(expr, case)?, ScalarUnit::Scalar))
    }

    fn unit_reference(&self, id: ObjId) -> Result<ScalarUnit, EvalError> {
        if self.nav.is(id, "FeatureReferenceExpression")?
            && let Some(referent) = self.nav.referent(id)?
        {
            return match self.nav.qualified_name(referent)?.as_deref() {
                Some("SI::litre") => Ok(ScalarUnit::Litre),
                Some("SI::second") => Ok(ScalarUnit::Second),
                _ => Err(self
                    .nav
                    .error(id, "unit is outside the scalar solve profile")),
            };
        }
        if self.nav.is(id, "OperatorExpression")? {
            let (operator, operands) = self.operator(id)?;
            if operator == "/"
                && let [left, right] = operands.as_slice()
                && self.nav.is(*left, "FeatureReferenceExpression")?
                && self.nav.is(*right, "FeatureReferenceExpression")?
                && self.unit_reference(*left)? == ScalarUnit::Litre
                && self.unit_reference(*right)? == ScalarUnit::Second
            {
                return Ok(ScalarUnit::LitresPerSecond);
            }
        }
        Err(self.nav.error(id, "unsupported analysis unit expression"))
    }

    fn operator(&self, expr: ObjId) -> Result<(&str, Vec<ObjId>), EvalError> {
        let operator = self
            .nav
            .string(expr, "OperatorExpression", "operator")?
            .ok_or_else(|| self.nav.error(expr, "expected an operator expression"))?;
        Ok((operator, self.nav.operands(expr)?))
    }

    fn number(&self, expr: ObjId, case: ObjId) -> Result<f64, EvalError> {
        if let Outcome::Values(values) = self.evaluate(expr, Some(case))? {
            match values.as_slice() {
                [Value::Int(value)] if value.unsigned_abs() <= (1_u64 << 53) => {
                    return Ok(*value as f64);
                }
                [Value::Real(value)] if value.is_finite() => return Ok(*value),
                _ => (),
            }
        }
        Err(self.nav.error(expr, "expected one finite numeric value"))
    }

    pub(crate) fn class_name(&self, id: ObjId) -> Result<&str, EvalError> {
        let object = self.nav.object(id)?;
        self.nav
            .model
            .metamodel()
            .schema()
            .classes
            .get(object.class_key())
            .map(|class| class.name.as_str())
            .ok_or_else(|| self.nav.error(id, "object class is absent from the schema"))
    }
}
