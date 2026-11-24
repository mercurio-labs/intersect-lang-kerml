//! Handwritten adapter from a linked SysML derivative case and controller to Run.

use crate::{
    Evaluator,
    analysis_case::{ScalarUnit, SolveDescription},
    analysis_execution::{
        AnalysisBinding, AnalysisElement, AnalysisError, AnalysisInput, ErrorCode, Quantity,
        Source, Unit,
    },
    stateful_analysis::{LinkedChart, StatefulBinding},
    text::canonical_name,
};
use intersect_run::{
    RunError,
    coupled_ode::{Crossing, ObservationKind, Settings, simulate},
    state_machine::Chart,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One continuous Real coordinate and one state-selected Real input to a linked
/// derivative analysis case. The derivative returns coordinate units per second.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousBinding {
    pub state_machine: StatefulBinding,
    pub state_input: String,
    /// Qualified name of a bound Real attribute.
    pub initial: String,
    pub end_ms: u64,
    pub sample_ms: u64,
    pub max_step_ms: u64,
    pub crossings: Vec<ContinuousCrossing>,
}

/// One threshold value authored in the linked model and the item it delivers.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinuousCrossing {
    pub level: String,
    pub event: String,
    pub rising: bool,
}

#[derive(Debug, Serialize)]
pub struct ContinuousAnalysis {
    pub case: String,
    pub case_identity: String,
    pub case_source: Source,
    pub equation_source: Source,
    pub machine: AnalysisElement,
    pub definition: AnalysisElement,
    pub subject: AnalysisElement,
    pub initial: AnalysisBinding,
    pub crossings: Vec<ContinuousMonitor>,
    pub steps: Vec<ContinuousStep>,
}

#[derive(Debug, Serialize)]
pub struct ContinuousMonitor {
    pub level: AnalysisBinding,
    pub event: AnalysisElement,
    pub event_name: String,
    pub rising: bool,
}

#[derive(Debug, Serialize)]
pub struct ContinuousStep {
    pub at: u64,
    pub value: f64,
    pub state: AnalysisElement,
    pub state_name: String,
    pub state_output: AnalysisBinding,
    pub outcome: ContinuousOutcome,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContinuousOutcome {
    Sample,
    Crossing {
        event: String,
        transition: AnalysisElement,
    },
}

impl Evaluator<'_> {
    /// Integrate one scalar derivative and deliver crossings to the flat
    /// controller exhibited by the derivative case's bound subject.
    pub fn execute_continuous_analysis(
        &self,
        case: &str,
        inputs: &[AnalysisInput],
        binding: &ContinuousBinding,
    ) -> Result<ContinuousAnalysis, AnalysisError> {
        let resolve = |name| {
            self.resolve_target(name)
                .map_err(|issue| self.analysis_error(issue, ErrorCode::Invalid, None))
        };
        let case_id = resolve(case)?;
        let case_element = self.analysis_element(case_id)?;
        let analysis = self.linked_scalar_analysis(case_id)?;
        let Some((_, Some(subject_id))) = analysis.subject else {
            return Err(AnalysisError::new(
                ErrorCode::Unsupported,
                "continuous execution requires a bound analysis subject",
                Some(&case_element.source),
            ));
        };
        let subject = self.analysis_element(subject_id)?;
        validate_continuous_inputs(&analysis, inputs, binding, &case_element.source)?;
        if binding.crossings.is_empty() || binding.crossings.len() > 16 {
            return Err(AnalysisError::new(
                ErrorCode::Invalid,
                "continuous execution requires one to 16 crossings",
                Some(&case_element.source),
            ));
        }

        let machine_id = resolve(&binding.state_machine.machine)?;
        let machine = self.analysis_element(machine_id)?;
        let LinkedChart {
            definition,
            spec,
            states,
            signals,
        } = self
            .project_linked_chart(machine_id, subject_id, &binding.state_machine)
            .map_err(|issue| {
                self.analysis_error(issue, ErrorCode::Unsupported, Some(&machine.source))
            })?;
        let definition = self.analysis_element(definition)?;
        let engine_error = |issue: RunError| {
            AnalysisError::new(
                ErrorCode::Invalid,
                issue.to_string(),
                Some(&definition.source),
            )
        };
        let chart = Chart::new(spec).map_err(engine_error)?;
        let initial =
            self.continuous_value(&binding.initial, "initial coordinate", &definition.source)?;

        let mut crossings = Vec::with_capacity(binding.crossings.len());
        let mut monitors = Vec::with_capacity(binding.crossings.len());
        for crossing in &binding.crossings {
            let event_name = canonical_name(&crossing.event).ok_or_else(|| {
                AnalysisError::new(
                    ErrorCode::Invalid,
                    format!("invalid crossing event qualified name {:?}", crossing.event),
                    Some(&machine.source),
                )
            })?;
            let identity = signals.get(&event_name).ok_or_else(|| {
                AnalysisError::new(
                    ErrorCode::Unbound,
                    format!("crossing event {event_name} is outside the linked controller"),
                    Some(&machine.source),
                )
            })?;
            let level =
                self.continuous_value(&crossing.level, "crossing threshold", &definition.source)?;
            crossings.push(Crossing {
                event: identity.clone(),
                level: level.quantity.value,
                rising: crossing.rising,
            });
            monitors.push(ContinuousMonitor {
                level,
                event: self.continuous_element(identity, &machine.source)?,
                event_name,
                rising: crossing.rising,
            });
        }

        let lookup_state = |id: &str| {
            states.get(id).ok_or_else(|| {
                AnalysisError::new(
                    ErrorCode::Invalid,
                    "controller has no linked active state",
                    Some(&definition.source),
                )
            })
        };
        let initial_state = chart.initial();
        let initial_output = lookup_state(chart.active_leaf(&initial_state))?
            .output
            .quantity;
        let mut bound = inputs.to_vec();
        let output_slot = bound.len();
        bound.push(AnalysisInput {
            name: binding.state_machine.analysis_input.clone(),
            quantity: initial_output,
        });
        let state_slot = bound.len();
        bound.push(AnalysisInput {
            name: binding.state_input.clone(),
            quantity: initial.quantity,
        });
        let initial_rate = self.solve_linked_scalar_analysis(case, &analysis, &bound)?;
        let mut derivative_error = None;
        let observations = simulate(
            &chart,
            Settings {
                end_ms: binding.end_ms,
                sample_ms: binding.sample_ms,
                max_step_ms: binding.max_step_ms,
            },
            initial.quantity.value,
            &crossings,
            |_, value, state| {
                let result = (|| {
                    bound[output_slot].quantity = lookup_state(state)?.output.quantity;
                    bound[state_slot].quantity.value = value;
                    let rate = self.solve_linked_scalar_analysis(case, &analysis, &bound)?;
                    Ok::<_, AnalysisError>(rate.result.quantity.value)
                })();
                result.map_err(|issue| {
                    let error = RunError::Semantics(issue.message.clone());
                    derivative_error = Some(issue);
                    error
                })
            },
        );
        if let Some(issue) = derivative_error {
            return Err(issue);
        }
        let event_names: BTreeMap<_, _> = monitors
            .iter()
            .map(|monitor| (monitor.event.identity.as_str(), monitor.event_name.as_str()))
            .collect();
        let mut steps = Vec::new();
        for observation in observations.map_err(engine_error)? {
            let state = lookup_state(&observation.state)?;
            let outcome = match observation.kind {
                ObservationKind::Sample => ContinuousOutcome::Sample,
                ObservationKind::Crossing { event, transition } => {
                    let name = event_names.get(event.as_str()).ok_or_else(|| {
                        AnalysisError::new(
                            ErrorCode::Invalid,
                            "crossing has no linked event name",
                            Some(&definition.source),
                        )
                    })?;
                    ContinuousOutcome::Crossing {
                        event: (*name).into(),
                        transition: self.continuous_element(&transition, &definition.source)?,
                    }
                }
            };
            steps.push(ContinuousStep {
                at: observation.at,
                value: observation.value,
                state: state.element.clone(),
                state_name: state.name.clone(),
                state_output: state.output.clone(),
                outcome,
            });
        }
        Ok(ContinuousAnalysis {
            case: initial_rate.case,
            case_identity: case_element.identity,
            case_source: case_element.source,
            equation_source: initial_rate.equation_source,
            machine,
            definition,
            subject,
            initial,
            crossings: monitors,
            steps,
        })
    }

    fn continuous_value(
        &self,
        name: &str,
        label: &str,
        fallback: &Source,
    ) -> Result<AnalysisBinding, AnalysisError> {
        let feature = self
            .resolve_target(name)
            .map_err(|issue| self.analysis_error(issue, ErrorCode::Invalid, Some(fallback)))?;
        let element = self.analysis_element(feature)?;
        // Evaluate the selected declaration in its own context so a subject
        // redefinition cannot supply a different value under this identity.
        let owner = self
            .nav
            .owning_type(feature)
            .map_err(|issue| self.analysis_error(issue, ErrorCode::Invalid, Some(&element.source)))?
            .unwrap_or(feature);
        let value = self
            .linked_real_value(feature, owner, label)
            .map_err(|issue| {
                self.analysis_error(issue, ErrorCode::Unsupported, Some(&element.source))
            })?;
        Ok(AnalysisBinding {
            identity: element.identity,
            source: element.source,
            quantity: Quantity {
                value,
                unit: Unit::Scalar,
            },
        })
    }

    fn continuous_element(
        &self,
        identity: &str,
        source: &Source,
    ) -> Result<AnalysisElement, AnalysisError> {
        let object = self.nav.model.lookup(identity).ok_or_else(|| {
            AnalysisError::new(
                ErrorCode::Invalid,
                format!("continuous observation has no linked element {identity:?}"),
                Some(source),
            )
        })?;
        self.analysis_element(object.id())
    }
}

fn validate_continuous_inputs(
    analysis: &SolveDescription,
    inputs: &[AnalysisInput],
    binding: &ContinuousBinding,
    source: &Source,
) -> Result<(), AnalysisError> {
    let state = &binding.state_input;
    let output = &binding.state_machine.analysis_input;
    if state == output {
        return Err(AnalysisError::new(
            ErrorCode::Invalid,
            "the continuous coordinate and state output require distinct analysis inputs",
            Some(source),
        ));
    }
    if binding.state_machine.unit != Unit::Scalar {
        return Err(AnalysisError::new(
            ErrorCode::Dimension,
            "continuous state output requires a scalar Real binding",
            Some(source),
        ));
    }
    for name in [state, output] {
        if name == &analysis.case.result
            || !analysis.case.parameters.get(name).is_some_and(|parameter| {
                parameter.value.is_none() && parameter.unit == ScalarUnit::Scalar
            })
        {
            return Err(AnalysisError::new(
                ErrorCode::Unsupported,
                format!("continuous input {name:?} must be an open scalar Real input"),
                Some(source),
            ));
        }
        if inputs.iter().any(|input| &input.name == name) {
            return Err(AnalysisError::new(
                ErrorCode::Invalid,
                format!("input {name:?} is bound to continuous execution"),
                Some(source),
            ));
        }
    }
    if !analysis
        .case
        .parameters
        .get(&analysis.case.result)
        .is_some_and(|parameter| parameter.unit == ScalarUnit::Scalar)
    {
        return Err(AnalysisError::new(
            ErrorCode::Dimension,
            "continuous derivative must return a scalar Real",
            Some(source),
        ));
    }
    Ok(())
}
