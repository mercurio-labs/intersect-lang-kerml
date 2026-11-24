//! Handwritten projection of a linked SysML controller into Intersect Run.

use crate::{
    EvalError, Evaluator, ObjId, Outcome, Value,
    analysis_case::ScalarUnit,
    analysis_execution::{
        AnalysisBinding, AnalysisElement, AnalysisError, AnalysisInput, AnalysisSolution,
        ErrorCode, Quantity, Unit,
    },
    text::canonical_name,
};
use intersect_model::core::frozen::ValueRef;
use intersect_run::state_machine::{Chart, ChartSpec, Node, State, Transition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulBinding {
    pub machine: String,
    pub state_output: String,
    pub analysis_input: String,
    pub unit: Unit,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatefulEvent {
    /// Time on the scenario's millisecond clock.
    pub at: u64,
    /// The linked item's qualified name.
    pub event: String,
}

#[derive(Debug, Serialize)]
pub struct StatefulAnalysis {
    pub machine: AnalysisElement,
    pub definition: AnalysisElement,
    pub steps: Vec<StatefulStep>,
}

#[derive(Debug, Serialize)]
pub struct StatefulStep {
    pub at: u64,
    pub event: Option<String>,
    pub outcome: StatefulOutcome,
    pub state: AnalysisElement,
    pub state_name: String,
    pub state_output: AnalysisBinding,
    pub solution: AnalysisSolution,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatefulOutcome {
    Start,
    Transition { transition: AnalysisElement },
    Rejected { reason: String },
}

pub(crate) struct ProjectedState {
    pub(crate) element: AnalysisElement,
    pub(crate) name: String,
    pub(crate) output: AnalysisBinding,
}

pub(crate) struct LinkedChart {
    pub(crate) definition: ObjId,
    pub(crate) spec: ChartSpec,
    pub(crate) states: BTreeMap<String, ProjectedState>,
    pub(crate) signals: BTreeMap<String, String>,
}

impl Evaluator<'_> {
    /// Run a flat SysML state chart and feed each active state's numeric attribute
    /// into the selected analysis. Equal timestamps execute in request order.
    pub fn execute_stateful_analysis(
        &self,
        case: &str,
        inputs: &[AnalysisInput],
        binding: &StatefulBinding,
        events: &[StatefulEvent],
    ) -> Result<StatefulAnalysis, AnalysisError> {
        let resolve = |name| {
            self.resolve_target(name)
                .map_err(|issue| self.analysis_error(issue, ErrorCode::Invalid, None))
        };
        let case_id = resolve(case)?;
        let machine_id = resolve(&binding.machine)?;
        let machine = self.analysis_element(machine_id)?;
        let analysis = self.linked_scalar_analysis(case_id)?;
        let Some((_, Some(subject))) = analysis.subject else {
            return Err(AnalysisError::new(
                ErrorCode::Unsupported,
                "stateful execution requires a bound analysis subject",
                Some(&self.analysis_element(case_id)?.source),
            ));
        };
        if inputs
            .iter()
            .any(|input| input.name == binding.analysis_input)
        {
            return Err(AnalysisError::new(
                ErrorCode::Invalid,
                format!(
                    "input {} is bound to the controller",
                    binding.analysis_input
                ),
                Some(&machine.source),
            ));
        }
        validate_events(events, &machine)?;
        let LinkedChart {
            definition,
            spec,
            states,
            signals,
        } = self
            .project_linked_chart(machine_id, subject, binding)
            .map_err(|issue| {
                self.analysis_error(issue, ErrorCode::Unsupported, Some(&machine.source))
            })?;
        let definition = self.analysis_element(definition)?;
        let chart = Chart::new(spec).map_err(|issue| {
            AnalysisError::new(
                ErrorCode::Invalid,
                issue.to_string(),
                Some(&definition.source),
            )
        })?;
        let mut active = chart.initial();
        let mut solutions = BTreeMap::new();
        let mut observe = |active: &State, at, event, outcome| {
            let identity = chart.active_leaf(active);
            let state = states.get(identity).ok_or_else(|| {
                AnalysisError::new(
                    ErrorCode::Invalid,
                    "chart has no linked active state",
                    Some(&definition.source),
                )
            })?;
            let solution = match solutions.entry(state.element.identity.clone()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let mut bound = inputs.to_vec();
                    bound.push(AnalysisInput {
                        name: binding.analysis_input.clone(),
                        quantity: state.output.quantity,
                    });
                    entry.insert(self.solve_linked_scalar_analysis(case, &analysis, &bound)?)
                }
            };
            Ok::<_, AnalysisError>(StatefulStep {
                at,
                event,
                outcome,
                state: state.element.clone(),
                state_name: state.name.clone(),
                state_output: state.output.clone(),
                solution: solution.clone(),
            })
        };
        let mut steps = Vec::with_capacity(events.len() + 1);
        steps.push(observe(&active, 0, None, StatefulOutcome::Start)?);
        for event in events {
            let name = canonical_name(&event.event).ok_or_else(|| {
                AnalysisError::new(
                    ErrorCode::Invalid,
                    format!("invalid event qualified name {:?}", event.event),
                    Some(&machine.source),
                )
            })?;
            let accepted =
                if let Some(signal) = signals.get(&name).and_then(|id| chart.event_id(id)) {
                    chart
                        .dispatch(&mut active, signal, event.at, |_, _, _| Ok(true))
                        .map_err(|issue| {
                            AnalysisError::new(
                                ErrorCode::Invalid,
                                issue.to_string(),
                                Some(&definition.source),
                            )
                        })?
                } else {
                    false
                };
            let outcome = if accepted {
                let id = chart
                    .last_transition(&active)
                    .and_then(|id| self.nav.model.lookup(id))
                    .ok_or_else(|| {
                        AnalysisError::new(
                            ErrorCode::Invalid,
                            "chart accepted an event without a linked transition",
                            Some(&definition.source),
                        )
                    })?
                    .id();
                StatefulOutcome::Transition {
                    transition: self.analysis_element(id)?,
                }
            } else {
                StatefulOutcome::Rejected {
                    reason: "no transition from the active state".into(),
                }
            };
            steps.push(observe(&active, event.at, Some(name), outcome)?);
        }
        Ok(StatefulAnalysis {
            machine,
            definition,
            steps,
        })
    }

    pub(crate) fn project_linked_chart(
        &self,
        machine: ObjId,
        subject: ObjId,
        binding: &StatefulBinding,
    ) -> Result<LinkedChart, EvalError> {
        if self.class_name(machine)? != "ExhibitStateUsage"
            || self.class_name(subject)? != "PartUsage"
        {
            return Err(self.nav.error(
                machine,
                "stateful execution requires an exhibit state of the subject part",
            ));
        }
        let owner = self.nav.owning_type(machine)?.ok_or_else(|| {
            self.nav.error(
                machine,
                "exhibited state must be owned by the subject part or its definition",
            )
        })?;
        if owner != subject {
            let types = self.direct_chart_types(subject)?;
            if types.as_slice() != [owner] || self.class_name(owner)? != "PartDefinition" {
                return Err(self.nav.error(
                    machine,
                    "exhibited state is outside the analysis subject's part definition",
                ));
            }
        }
        if !self.nav.all_features(subject)?.contains(&machine) {
            return Err(self
                .nav
                .error(machine, "the subject does not exhibit the selected state"));
        }
        self.check_single_chart(machine, "exhibit state")?;
        let definition = self.single_chart_type(machine, "StateDefinition")?;
        if self.nav.generals(machine)?.as_slice() != [definition]
            || !self.nav.owned_features(machine)?.is_empty()
        {
            return Err(self
                .nav
                .error(machine, "exhibit state must use its definition directly"));
        }
        self.check_single_chart(definition, "state definition")?;
        if !self.nav.generals(definition)?.is_empty() {
            return Err(self
                .nav
                .error(definition, "state definition specialization is unsupported"));
        }

        let mut states = BTreeMap::new();
        let mut state_names = BTreeSet::new();
        let mut transition_ids = Vec::new();
        let mut entry = None;
        let mut succession = None;
        let mut previous = None;
        for rel in self.nav.relationships(definition)? {
            for member in self.nav.owned(rel)? {
                match (self.class_name(rel)?, self.class_name(member)?) {
                    ("StateSubactionMembership", "ActionUsage") if entry.is_none() => {
                        if !self.chart_kind(rel, "entry")?
                            || self
                                .nav
                                .string(member, "Element", "declaredName")?
                                .is_some()
                            || !self.nav.relationships(member)?.is_empty()
                        {
                            return Err(self.nav.error(
                                member,
                                "the only supported state action is an empty initial entry",
                            ));
                        }
                        entry = Some(member);
                    }
                    ("FeatureMembership", "StateUsage") => {
                        if states.len() == 128 {
                            return Err(self
                                .nav
                                .error(member, "state machine exceeds its state limit"));
                        }
                        let state = self.project_state(member, binding)?;
                        if !state_names.insert(state.name.clone()) {
                            return Err(self.nav.error(member, "ambiguous linked state name"));
                        }
                        states.insert(state.element.identity.clone(), state);
                    }
                    ("FeatureMembership", "TransitionUsage") => transition_ids.push(member),
                    ("FeatureMembership", "SuccessionAsUsage")
                        if succession.is_none() && entry.is_some() && previous == entry =>
                    {
                        succession = Some(member);
                    }
                    (_, "Documentation" | "Comment") => continue,
                    _ => {
                        return Err(self.nav.error(
                            member,
                            "unsupported owned member in a flat state definition",
                        ));
                    }
                }
                previous = Some(member);
            }
        }
        let succession = succession.ok_or_else(|| {
            self.nav.error(
                definition,
                "state definition needs one initial entry succession",
            )
        })?;
        let initial = self.chart_succession_target(succession)?;
        let mut signals = BTreeMap::new();
        let mut transitions = Vec::with_capacity(transition_ids.len());
        for id in transition_ids {
            transitions.push(self.project_transition(id, &mut signals)?);
        }
        let spec = ChartSpec {
            initial: self.nav.object(initial)?.identity().into(),
            states: states
                .keys()
                .map(|id| Node {
                    id: id.clone(),
                    parent: None,
                    initial: None,
                })
                .collect(),
            transitions,
            immediate: true,
            clock_state: None,
        };
        Ok(LinkedChart {
            definition,
            spec,
            states,
            signals,
        })
    }

    fn project_state(
        &self,
        state: ObjId,
        binding: &StatefulBinding,
    ) -> Result<ProjectedState, EvalError> {
        self.check_single_chart(state, "state")?;
        if !self.nav.generals(state)?.is_empty() {
            return Err(self.nav.error(state, "state specialization is unsupported"));
        }
        let mut attribute = None;
        for rel in self.nav.relationships(state)? {
            for member in self.nav.owned(rel)? {
                match (self.class_name(rel)?, self.class_name(member)?) {
                    ("FeatureMembership", "AttributeUsage") => {
                        self.check_state_attribute(member)?;
                        if self.nav.string(member, "Element", "declaredName")?
                            == Some(binding.state_output.as_str())
                            && attribute.replace(member).is_some()
                        {
                            return Err(self.nav.error(member, "ambiguous state output attribute"));
                        }
                    }
                    (_, "Documentation" | "Comment" | "Multiplicity") => {}
                    _ => {
                        return Err(self
                            .nav
                            .error(member, "only numeric bindings on flat states are supported"));
                    }
                }
            }
        }
        let attribute = attribute.ok_or_else(|| {
            self.nav.error(
                state,
                format!(
                    "state is missing output attribute {:?}",
                    binding.state_output
                ),
            )
        })?;
        let value = self.linked_real_value(attribute, state, "state output")?;
        let identity = |id| {
            self.analysis_element(id)
                .map_err(|issue| self.nav.error(id, issue.message))
        };
        let name = self
            .qualified_name(state)?
            .ok_or_else(|| self.nav.error(state, "state must have a qualified name"))?;
        let element = identity(state)?;
        let source = identity(attribute)?;
        Ok(ProjectedState {
            element,
            name,
            output: AnalysisBinding {
                identity: source.identity,
                source: source.source,
                quantity: Quantity {
                    value,
                    unit: binding.unit,
                },
            },
        })
    }

    pub(crate) fn linked_real_value(
        &self,
        attribute: ObjId,
        target: ObjId,
        label: &str,
    ) -> Result<f64, EvalError> {
        self.check_scalar_multiplicity(&[attribute])?;
        if self.class_name(attribute)? != "AttributeUsage"
            || self.analysis_unit(attribute)? != ScalarUnit::Scalar
            || self.nav.feature_value(attribute)?.is_none()
        {
            return Err(self.nav.error(
                attribute,
                format!("{label} must be a bound, scalar Real attribute"),
            ));
        }
        let Outcome::Values(values) = self.evaluate_feature(attribute, Some(target))? else {
            return Err(self
                .nav
                .error(attribute, format!("{label} could not be evaluated")));
        };
        match values.as_slice() {
            [Value::Int(value)] if value.unsigned_abs() <= (1_u64 << 53) => Ok(*value as f64),
            [Value::Real(value)] if value.is_finite() => Ok(*value),
            _ => Err(self.nav.error(
                attribute,
                format!("{label} must evaluate to one finite number"),
            )),
        }
    }

    fn check_state_attribute(&self, attribute: ObjId) -> Result<(), EvalError> {
        for rel in self.nav.relationships(attribute)? {
            match self.class_name(rel)? {
                "FeatureTyping" | "FeatureValue" => {}
                "OwningMembership" => {
                    for child in self.nav.owned(rel)? {
                        if !matches!(
                            self.class_name(child)?,
                            "Documentation" | "Comment" | "Multiplicity" | "MultiplicityRange"
                        ) {
                            return Err(self
                                .nav
                                .error(child, "unsupported state attribute member"));
                        }
                    }
                }
                _ => {
                    return Err(self
                        .nav
                        .error(rel, "unsupported state attribute relationship"));
                }
            }
        }
        Ok(())
    }

    fn project_transition(
        &self,
        transition: ObjId,
        signals: &mut BTreeMap<String, String>,
    ) -> Result<Transition, EvalError> {
        let mut sources = Vec::new();
        let mut successions = Vec::new();
        let mut triggers = Vec::new();
        for rel in self.nav.relationships(transition)? {
            match self.class_name(rel)? {
                "Membership" => {
                    sources.extend(self.nav.refs(rel, "Membership", "memberElement")?)
                }
                "TransitionFeatureMembership" => {
                    if !self.chart_kind(rel, "trigger")? {
                        return Err(self
                            .nav
                            .error(rel, "transition guards and effects are unsupported"));
                    }
                    triggers.extend(self.nav.owned(rel)?);
                }
                "OwningMembership" => {
                    for child in self.nav.owned(rel)? {
                        match self.class_name(child)? {
                            "SuccessionAsUsage" => successions.push(child),
                            "Documentation" | "Comment" => {}
                            _ => {
                                return Err(self
                                    .nav
                                    .error(child, "unsupported owned transition member"));
                            }
                        }
                    }
                }
                "ParameterMembership" | "FeatureMembership" => {
                    // The language bookkeeper stores anonymous transition slots.
                    for child in self.nav.owned(rel)? {
                        if self.class_name(child)? != "ReferenceUsage"
                            || !self.nav.relationships(child)?.is_empty()
                        {
                            return Err(self
                                .nav
                                .error(child, "unsupported transition parameter or effect"));
                        }
                    }
                }
                _ => return Err(self.nav.error(rel, "unsupported transition relationship")),
            }
        }
        let ([from], [succession], [trigger]) = (
            sources.as_slice(),
            successions.as_slice(),
            triggers.as_slice(),
        ) else {
            return Err(self.nav.error(
                transition,
                "expected one linked transition source, target, and trigger",
            ));
        };
        if self.class_name(*trigger)? != "AcceptActionUsage" {
            return Err(self
                .nav
                .error(*trigger, "expected a typed item accept trigger"));
        }
        let mut payload = Vec::new();
        for rel in self.nav.relationships(*trigger)? {
            if self.class_name(rel)? != "ParameterMembership" {
                return Err(self
                    .nav
                    .error(rel, "only item payload triggers are supported"));
            }
            payload.extend(self.nav.owned(rel)?);
        }
        let [payload] = payload.as_slice() else {
            return Err(self
                .nav
                .error(*trigger, "expected exactly one trigger payload and no port"));
        };
        if self.class_name(*payload)? != "ReferenceUsage"
            || self.nav.feature_value(*payload)?.is_some()
        {
            return Err(self
                .nav
                .error(*payload, "expected an unbound item trigger payload"));
        }
        let signal = self.single_chart_type(*payload, "ItemDefinition")?;
        let name = self
            .qualified_name(signal)?
            .ok_or_else(|| self.nav.error(signal, "trigger item has no qualified name"))?;
        let identity = self.nav.object(signal)?.identity().to_owned();
        match signals.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(identity.clone());
            }
            Entry::Occupied(entry) if entry.get() != &identity => {
                return Err(self.nav.error(signal, "ambiguous linked trigger item name"));
            }
            Entry::Occupied(_) => {}
        }
        Ok(Transition {
            id: Some(self.nav.object(transition)?.identity().into()),
            from: self.nav.object(*from)?.identity().into(),
            to: self
                .nav
                .object(self.chart_succession_target(*succession)?)?
                .identity()
                .into(),
            event: identity,
        })
    }

    fn chart_succession_target(&self, succession: ObjId) -> Result<ObjId, EvalError> {
        let mut ends = Vec::new();
        for rel in self.nav.relationships(succession)? {
            if self.class_name(rel)? != "EndFeatureMembership" {
                return Err(self
                    .nav
                    .error(rel, "unsupported state succession relationship"));
            }
            ends.extend(self.nav.owned(rel)?);
        }
        let [source, target] = ends.as_slice() else {
            return Err(self
                .nav
                .error(succession, "state succession needs two ends"));
        };
        if self.class_name(*source)? != "ReferenceUsage"
            || self.class_name(*target)? != "ReferenceUsage"
        {
            return Err(self
                .nav
                .error(succession, "expected reference ends of a state succession"));
        }
        for rel in self.nav.relationships(*source)? {
            self.check_chart_end_member(rel, "expected an implicit state succession source")?;
        }
        let mut references = Vec::new();
        for rel in self.nav.relationships(*target)? {
            match self.class_name(rel)? {
                "ReferenceSubsetting" => {
                    let object = self.nav.object(rel)?;
                    let field = self
                        .nav
                        .model
                        .metamodel()
                        .feature(object.class(), "referencedFeature")
                        .ok_or_else(|| self.nav.error(rel, "expected a linked state reference"))?;
                    references.extend(object.refs(field).map(|target| target.id()));
                }
                _ => {
                    self.check_chart_end_member(rel, "expected a direct state target reference")?
                }
            }
        }
        let [target] = references.as_slice() else {
            return Err(self
                .nav
                .error(succession, "expected one linked succession target"));
        };
        Ok(*target)
    }

    fn check_chart_end_member(&self, relation: ObjId, message: &str) -> Result<(), EvalError> {
        if self.class_name(relation)? != "OwningMembership" {
            return Err(self.nav.error(relation, message));
        }
        for child in self.nav.owned(relation)? {
            if self.class_name(child)? != "Multiplicity" {
                return Err(self.nav.error(child, message));
            }
        }
        Ok(())
    }

    fn single_chart_type(&self, feature: ObjId, expected: &str) -> Result<ObjId, EvalError> {
        let types = self.direct_chart_types(feature)?;
        if let [ty] = types.as_slice()
            && self.class_name(*ty)? == expected
        {
            return Ok(*ty);
        }
        Err(self
            .nav
            .error(feature, format!("expected one direct {expected} type")))
    }

    fn direct_chart_types(&self, feature: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let mut types = Vec::new();
        for relationship in self.nav.relationships(feature)? {
            if self.class_name(relationship)? == "FeatureTyping" {
                types.extend(self.nav.refs(relationship, "FeatureTyping", "type")?);
            }
        }
        Ok(types)
    }

    fn check_single_chart(&self, id: ObjId, label: &str) -> Result<(), EvalError> {
        self.check_scalar_multiplicity(&[id])?;
        let object = self.nav.object(id)?;
        let mm = self.nav.model.metamodel();
        let flag = |name| {
            let value = mm
                .feature(object.class(), name)
                .and_then(|field| object.get(field));
            matches!(value, Some(ValueRef::Bool(true)))
                || matches!(value, Some(ValueRef::Default(v) | ValueRef::Json(v)) if v.as_bool() == Some(true))
        };
        if flag("isParallel") || flag("isAbstract") {
            return Err(self
                .nav
                .error(id, format!("parallel or abstract {label} is unsupported")));
        }
        Ok(())
    }

    fn chart_kind(&self, relationship: ObjId, expected: &str) -> Result<bool, EvalError> {
        let object = self.nav.object(relationship)?;
        let mm = self.nav.model.metamodel();
        let field = mm
            .feature(object.class(), "kind")
            .ok_or_else(|| self.nav.error(relationship, "missing membership kind"))?;
        let enumeration = mm
            .definition(field)
            .and_then(|field| mm.schema().enumerations.get(&field.target))
            .ok_or_else(|| {
                self.nav
                    .error(relationship, "membership kind has no enumeration")
            })?;
        let expected = enumeration
            .parse(expected)
            .map_err(|issue| self.nav.error(relationship, issue))?;
        let expected = serde_json::to_value(expected)
            .map_err(|issue| self.nav.error(relationship, issue.to_string()))?;
        Ok(
            matches!(object.get(field), Some(ValueRef::Default(v) | ValueRef::Json(v)) if v == &expected),
        )
    }
}

fn validate_events(
    events: &[StatefulEvent],
    machine: &AnalysisElement,
) -> Result<(), AnalysisError> {
    if events.len() > 1_000 || events.iter().any(|event| event.at > 86_400_000) {
        return Err(AnalysisError::new(
            ErrorCode::Budget,
            "stateful scenario exceeds time or event limits",
            Some(&machine.source),
        ));
    }
    if events.windows(2).any(|pair| pair[0].at > pair[1].at) {
        return Err(AnalysisError::new(
            ErrorCode::Invalid,
            "stateful scenario events must be ordered by time",
            Some(&machine.source),
        ));
    }
    Ok(())
}
