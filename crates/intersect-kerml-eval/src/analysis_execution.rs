//! Handwritten adapter from a linked SysML analysis to the shared affine solver.

use crate::{
    EvalError, Evaluator, ObjId,
    analysis_case::{Operator, ScalarUnit, SolveBody, SolveDescription, Term},
};
use intersect_run::analysis::{Comparison, Constraint, Expression, Variable, Variables, solve};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub use intersect_run::analysis::{AnalysisError, ErrorCode, Quantity, Source, Unit};

pub const ENGINE_ID: &str = intersect_run_catalog::Engine::ScalarAffine.id();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisInput {
    pub name: String,
    pub quantity: Quantity,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisElement {
    pub identity: String,
    pub source: Source,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisSubject {
    pub declaration: AnalysisElement,
    pub target: Option<AnalysisElement>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisBinding {
    pub identity: String,
    pub source: Source,
    pub quantity: Quantity,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisSolution {
    pub case: String,
    pub case_identity: String,
    pub case_source: Source,
    pub equation_source: Source,
    pub subject: Option<AnalysisSubject>,
    pub result_name: String,
    pub result: AnalysisBinding,
    pub inputs: BTreeMap<String, AnalysisBinding>,
}

impl Evaluator<'_> {
    /// Execute one linked analysis usage with an asserted scalar equality.
    /// The SysML model supplies fixed inputs, units, and the return parameter.
    /// Supplied bindings fill only declared input parameters that lack values.
    pub fn execute_scalar_analysis(
        &self,
        name: &str,
        inputs: &[AnalysisInput],
    ) -> Result<AnalysisSolution, AnalysisError> {
        let target = self
            .resolve_target(name)
            .map_err(|issue| self.analysis_error(issue, ErrorCode::Invalid, None))?;
        let description = self.linked_scalar_analysis(target)?;
        self.solve_linked_scalar_analysis(name, &description, inputs)
    }

    pub(crate) fn linked_scalar_analysis(
        &self,
        target: ObjId,
    ) -> Result<SolveDescription, AnalysisError> {
        let incoming = self.analysis_element(target)?;
        self.describe_analysis(target, true).map_err(|issue| {
            self.analysis_error(issue, ErrorCode::Unsupported, Some(&incoming.source))
        })
    }

    pub(crate) fn solve_linked_scalar_analysis(
        &self,
        name: &str,
        description: &SolveDescription,
        inputs: &[AnalysisInput],
    ) -> Result<AnalysisSolution, AnalysisError> {
        let analysis = &description.case;
        let target = analysis.object;
        let incoming = self.analysis_element(target)?;
        let SolveBody::Equation {
            object,
            left,
            right,
        } = &analysis.body
        else {
            return Err(AnalysisError::new(
                ErrorCode::Unsupported,
                "scalar-affine execution requires a linked SysML asserted equality",
                Some(&incoming.source),
            ));
        };
        let equation_source = self.analysis_element(*object)?.source;

        let mut variables = Variables::new();
        for (name, parameter) in &analysis.parameters {
            let unit = unit_for(parameter.unit);
            let source = self.analysis_element(parameter.source)?.source;
            let value = parameter
                .value
                .map(|(value, declared)| {
                    Quantity {
                        value,
                        unit: unit_for(declared),
                    }
                    .in_unit(unit, Some(&source))
                })
                .transpose()?;
            variables.insert(
                name.clone(),
                Variable {
                    unit,
                    value,
                    source: Some(source),
                },
            );
        }
        bind_inputs(&mut variables, &analysis.result, inputs, &incoming.source)?;
        let result = solve(
            &Constraint {
                left: expression(left),
                comparison: Comparison::Equal,
                right: expression(right),
                source: Some(equation_source.clone()),
            },
            &variables,
            &analysis.result,
        )?;

        let mut bindings = BTreeMap::new();
        for (name, variable) in variables {
            if name != analysis.result {
                let value = variable
                    .value
                    .ok_or_else(|| missing_input(&name, &variable))?;
                let binding = self.analysis_binding(
                    &name,
                    &description.declarations,
                    Quantity {
                        value,
                        unit: variable.unit,
                    },
                )?;
                bindings.insert(name, binding);
            }
        }
        let result = self.analysis_binding(&analysis.result, &description.declarations, result)?;
        let subject = description
            .subject
            .map(|(declaration, target)| {
                Ok::<_, AnalysisError>(AnalysisSubject {
                    declaration: self.analysis_element(declaration)?,
                    target: target.map(|id| self.analysis_element(id)).transpose()?,
                })
            })
            .transpose()?;
        let case = self
            .qualified_name(target)
            .map_err(|issue| {
                self.analysis_error(issue, ErrorCode::Invalid, Some(&incoming.source))
            })?
            .unwrap_or_else(|| name.into());

        Ok(AnalysisSolution {
            case,
            case_identity: incoming.identity,
            case_source: incoming.source,
            equation_source,
            subject,
            result_name: analysis.result.clone(),
            result,
            inputs: bindings,
        })
    }

    fn analysis_binding(
        &self,
        name: &str,
        declarations: &BTreeMap<String, ObjId>,
        quantity: Quantity,
    ) -> Result<AnalysisBinding, AnalysisError> {
        let id = declarations.get(name).ok_or_else(|| {
            AnalysisError::new(
                ErrorCode::Invalid,
                format!("missing declaration for {name}"),
                None,
            )
        })?;
        let element = self.analysis_element(*id)?;
        Ok(AnalysisBinding {
            identity: element.identity,
            source: element.source,
            quantity,
        })
    }

    pub(crate) fn analysis_element(&self, id: ObjId) -> Result<AnalysisElement, AnalysisError> {
        let object = self
            .nav
            .object(id)
            .map_err(|issue| self.analysis_error(issue, ErrorCode::Invalid, None))?;
        Ok(AnalysisElement {
            identity: object.identity().into(),
            source: source(&object),
        })
    }

    pub(crate) fn analysis_error(
        &self,
        issue: EvalError,
        mut code: ErrorCode,
        fallback: Option<&Source>,
    ) -> AnalysisError {
        let location = match &issue {
            EvalError::Model { object, .. } | EvalError::NotAnExpression(object) => {
                self.nav.model.lookup(object).map(|object| source(&object))
            }
            EvalError::DepthLimit(_) | EvalError::StepLimit(_) | EvalError::Stopped(_) => {
                code = ErrorCode::Budget;
                None
            }
            EvalError::Schema(_) => None,
        };
        AnalysisError::new(code, issue.to_string(), location.as_ref().or(fallback))
    }
}

fn source(object: &intersect_model::core::frozen::Obj<'_>) -> Source {
    let (start, end) = object.span();
    Source {
        uri: object.resource().into(),
        start,
        end,
    }
}

fn bind_inputs(
    variables: &mut Variables,
    result: &str,
    inputs: &[AnalysisInput],
    source: &Source,
) -> Result<(), AnalysisError> {
    let mut seen = BTreeSet::new();
    for input in inputs {
        let name = input.name.as_str();
        if !seen.insert(name) {
            return Err(AnalysisError::new(
                ErrorCode::Invalid,
                format!("duplicate analysis input {name}"),
                Some(source),
            ));
        }
        let variable = variables.get_mut(name).ok_or_else(|| {
            AnalysisError::new(
                ErrorCode::Unbound,
                format!("no analysis input {name}"),
                Some(source),
            )
        })?;
        if name == result || variable.value.is_some() {
            return Err(AnalysisError::new(
                ErrorCode::Invalid,
                format!("cannot bind analysis result or fixed input {name}"),
                variable.source.as_ref(),
            ));
        }
        variable.value = Some(
            input
                .quantity
                .in_unit(variable.unit, variable.source.as_ref())?,
        );
    }
    for (name, variable) in variables {
        if name != result && variable.value.is_none() {
            return Err(missing_input(name, variable));
        }
    }
    Ok(())
}

fn missing_input(name: &str, variable: &Variable) -> AnalysisError {
    AnalysisError::new(
        ErrorCode::Unbound,
        format!("no value for SysML input {name}"),
        variable.source.as_ref(),
    )
}

fn unit_for(unit: ScalarUnit) -> Unit {
    match unit {
        ScalarUnit::Scalar => Unit::Scalar,
        ScalarUnit::Litre => Unit::Litre,
        ScalarUnit::Second => Unit::Second,
        ScalarUnit::LitresPerSecond => Unit::LitresPerSecond,
    }
}

fn expression(term: &Term) -> Expression {
    match term {
        Term::Parameter(name) => Expression::Variable { name: name.clone() },
        Term::Literal(value, unit) => Expression::Literal {
            quantity: Quantity {
                value: *value,
                unit: unit_for(*unit),
            },
        },
        Term::Binary {
            operator,
            left,
            right,
        } => {
            let left = Box::new(expression(left));
            let right = Box::new(expression(right));
            match operator {
                Operator::Add => Expression::Add { left, right },
                Operator::Subtract => Expression::Subtract { left, right },
                Operator::Multiply => Expression::Multiply { left, right },
                Operator::Divide => Expression::Divide { left, right },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variables() -> Variables {
        [
            ("capacity", Unit::Litre, Some(6.0)),
            ("duration", Unit::Second, None),
            ("flow", Unit::LitresPerSecond, None),
        ]
        .into_iter()
        .map(|(name, unit, value)| {
            (
                name.to_owned(),
                Variable {
                    unit,
                    value,
                    source: Some(test_source()),
                },
            )
        })
        .collect()
    }

    fn input(name: &str, value: f64, unit: Unit) -> AnalysisInput {
        AnalysisInput {
            name: name.into(),
            quantity: Quantity { value, unit },
        }
    }

    fn test_source() -> Source {
        Source {
            uri: "fixture.sysml".into(),
            start: 5,
            end: 17,
        }
    }

    #[test]
    fn normalizes_a_supplied_quantity_to_the_declared_unit() -> Result<(), AnalysisError> {
        let mut variables = variables();
        bind_inputs(
            &mut variables,
            "flow",
            &[input("duration", 4_000.0, Unit::Millisecond)],
            &test_source(),
        )?;
        assert_eq!(variables["duration"].value, Some(4.0));
        assert_eq!(variables["capacity"].value, Some(6.0));
        assert_eq!(variables["flow"].value, None);
        Ok(())
    }

    #[test]
    fn refuses_incomplete_incompatible_and_conflicting_bindings() {
        let duration = input("duration", 4.0, Unit::Second);
        let cases = [
            (
                vec![],
                ErrorCode::Unbound,
                "no value for SysML input duration",
            ),
            (
                vec![input("missing", 4.0, Unit::Second)],
                ErrorCode::Unbound,
                "no analysis input missing",
            ),
            (
                vec![input("duration", 4.0, Unit::Litre)],
                ErrorCode::Dimension,
                "expected s, found L",
            ),
            (
                vec![input("duration", f64::NAN, Unit::Second)],
                ErrorCode::Invalid,
                "finite",
            ),
            (
                vec![duration.clone(), duration],
                ErrorCode::Invalid,
                "duplicate analysis input duration",
            ),
            (
                vec![input("capacity", 6.0, Unit::Litre)],
                ErrorCode::Invalid,
                "cannot bind analysis result or fixed input capacity",
            ),
            (
                vec![input("flow", 1.0, Unit::LitresPerSecond)],
                ErrorCode::Invalid,
                "cannot bind analysis result or fixed input flow",
            ),
        ];
        for (provided, code, message) in cases {
            let error = bind_inputs(&mut variables(), "flow", &provided, &test_source())
                .expect_err("invalid bindings must fail");
            assert_eq!(error.code, code, "{error}");
            assert!(error.message.contains(message), "{error}");
            assert_eq!(error.source, Some(test_source()));
        }
    }
}
