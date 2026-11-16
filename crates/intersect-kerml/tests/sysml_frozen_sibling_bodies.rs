//! Sibling SysML members with bodies parse on the frozen text path in work
//! linear in their number. Each member body is a live nested decision that
//! reads to the end of input; without shared nested selections, deferred
//! chains and reused end-of-input verdicts, 20 such siblings exhausted the
//! prediction budget (the ISQ and units library files).
use intersect_kerml::native_pilot::NativePilotSession;
use intersect_model::{
    core::{
        frozen::{FrozenStore, Metamodel},
        query::QueryBudget,
        storage,
    },
    definitions::conversion::{InvocationContext, InvocationObserver, ProviderId},
};
use intersect_text::{
    EvaluationLimits, Source,
    document::AnalysisLimits,
    draft::{DraftLanguage, OpenDraftCapability},
};
use std::{rc::Rc, sync::Arc};

#[allow(dead_code)]
#[path = "../examples/support/kerml_scopes.rs"]
mod kerml_scopes;

const CORPUS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-pilot/v1/sources/"
);
const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];
const SIBLINGS: usize = 20;
const ASSIGNMENTS: &str = "package P {
	action def A {
		attribute index;
		attribute seq;
		action body;
		private action initialization
			assign index := 1;
		then private action whileLoop
			while index <= size(seq) {
				assign index := seq#(index);
				then perform body;
				then assign index := index + 1;
			}
	}
}
";

const SENDS: &str = "package P {
	action a {
		send x via p;
		send x.y;
		action snd2 send via this to aa.target;
		then send x.y via p.q to r.s;
		action s send es via vehicle.statusPort { }
	}
	state st {
		entry send new S() via apisp.APIS_HTTP;
		then idle;
		state idle;
			accept r : S via getTopic.APIS_MQTT
			do send r via giveTopic.APIS_MQTT
		then idle;
	}
}
";

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

fn read(path: &str) -> Result<Source, String> {
    Ok(Source {
        uri: path.into(),
        text: std::fs::read_to_string(format!("{CORPUS}{path}"))
            .map_err(|error| format!("{path}: {error}"))?,
    })
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn sysml_prediction_controls_parse_within_the_budget() -> Result<(), String> {
    std::thread::Builder::new()
        .name("sysml-sibling-bodies".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(parse_siblings)
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "Sibling body parse panicked".to_owned())?
}

fn parse_siblings() -> Result<(), String> {
    let definitions = DEFINITIONS
        .iter()
        .map(|path| read(path))
        .collect::<Result<Vec<_>, _>>()?;
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativePilotSession::prepare(&definitions, &preparation)?;
    session.with_definitions(|_, _, sysml, contracts| {
        let mut budget = QueryBudget::new(usize::MAX);
        let inner = intersect_kerml::draft::sysml_frozen_inner_capability(
            sysml,
            &preparation,
            &mut budget,
        )?;
        let providers = intersect_kerml::draft::sysml_setting_providers(sysml, contracts)?;
        let provider_refs: Vec<&dyn intersect_model::setting::SourceSettingValues> =
            providers.iter().map(|p| p.as_ref()).collect();
        let settings =
            intersect_text::setting::CheckedDraftTextSettings::bind(sysml, &provider_refs)?;
        let names = intersect_kerml::draft::kerml_string_rules(sysml, &mut budget)?;
        let open = OpenDraftCapability::new(sysml, &inner)?.with_string_rules(&names);
        let language = DraftLanguage::bind(sysml, &open, AnalysisLimits::workbench())?;
        let redefinitions = kerml_scopes::redefinitions(sysml.schema())?;
        let metamodel = Arc::new(
            Metamodel::freeze(sysml.schema())
                .and_then(|mm| mm.with_redefinitions(&redefinitions))
                .map_err(|error| error.to_string())?,
        );
        let members = (0..SIBLINGS)
            .map(|i| {
                format!(
                    "    attribute a{i} : Real {{ :>> b = 1.0; }}
"
                )
            })
            .collect::<String>();
        let cases = [
            (
                "control/sibling-bodies.sysml",
                format!(
                    "package P {{
{members}}}
"
                ),
                // The package, its membership, and per member the attribute,
                // its typing, its membership and the redefining body feature.
                SIBLINGS * 6,
            ),
            (
                // `( => TargetBinding '.' )?` guards only TargetBinding: `index`
                // parses as one, but `:=` follows, so the optional binding is
                // skipped instead of committed (Actions.sysml, ForLoopAction).
                "control/assignment-target.sysml",
                ASSIGNMENTS.to_owned(),
                20,
            ),
            (
                // `=> ( NodeParameterMember SenderReceiverPart? | ... )` ends
                // after `x`, where the plain reference is complete while the
                // reference, metadata and chained-invocation readings of
                // `x.y` all continue: greedy bounded acceptance reads on
                // (ActionTest, AHFNorwayTopics and the Annex A examples).
                "control/send-chains.sysml",
                SENDS.to_owned(),
                20,
            ),
        ];
        for (uri, text, minimum) in cases {
            let source = Source {
                uri: uri.into(),
                text,
            };
            let mut store = FrozenStore::new(metamodel.clone());
            let mut caller = QueryBudget::new(usize::MAX);
            let limits = EvaluationLimits {
                // Linear work for 20 siblings stays far below this; the former
                // exponential prediction exhausted it.
                steps_per_resource: Some(400_000_000),
                nesting: 256,
                ..EvaluationLimits::native()
            };
            let (evaluation, _, _) = language
                .parse_runtime_into_with_settings(
                    &source,
                    limits,
                    Some(&settings),
                    &mut store,
                    &mut caller,
                )
                .map_err(|error| format!("{uri}: {error:?}"))?;
            assert!(
                evaluation.diagnostics.is_empty(),
                "{uri}: {:?}",
                evaluation.diagnostics
            );
            let model = storage::export(&store).map_err(|error| error.to_string())?;
            assert!(
                model.elements.len() > minimum,
                "{uri}: {}",
                model.elements.len()
            );
        }
        Ok(())
    })
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn assignment_target_predicate_constructs_binding_and_member() -> Result<(), String> {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(parse_assignment_targets)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "Assignment target probe panicked".to_owned())?
}

fn parse_assignment_targets() -> Result<(), String> {
    use intersect_model::core::model::{Element, Model};
    fn children<'a>(
        graph: &'a Model,
        element: &Element,
        field: &str,
    ) -> Result<Vec<&'a Element>, String> {
        let ids = element
            .values
            .get(field)
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Missing {field}: {element:?}"))?;
        ids.iter()
            .map(|id| {
                graph
                    .elements
                    .iter()
                    .find(|e| Some(e.id.as_str()) == id.as_str())
                    .ok_or_else(|| format!("Missing child {id}"))
            })
            .collect()
    }
    let graphs = super::frozen_rule_probes::assignment_graphs()?;
    for ((target, binding, member), graph) in [("a.b", Some("?a"), "?b"), ("index", None, "?index")]
        .into_iter()
        .zip(graphs)
    {
        let assignments = graph
            .elements
            .iter()
            .filter(|e| e.class == "AssignmentActionUsage")
            .collect::<Vec<_>>();
        assert_eq!(assignments.len(), 1, "{target}");
        let relationships = children(graph, assignments[0], "ownedRelationship")?;
        assert_eq!(relationships.len(), 3, "{target}: {relationships:?}");
        assert_eq!(relationships[0].class, "ParameterMembership");
        let parameters = children(graph, relationships[0], "ownedRelatedElement")?;
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].class, "ReferenceUsage");
        let parameter_relationships = children(graph, parameters[0], "ownedRelationship")?;
        let bindings = parameter_relationships
            .iter()
            .filter(|e| e.class == "FeatureValue")
            .collect::<Vec<_>>();
        assert_eq!(
            bindings.len(),
            usize::from(binding.is_some()),
            "{target}: {parameter_relationships:?}"
        );
        if let Some(expected) = binding {
            let expressions = children(graph, bindings[0], "ownedRelatedElement")?;
            assert_eq!(expressions.len(), 1);
            assert_eq!(
                expressions[0].class, "FeatureReferenceExpression",
                "Binding must be a, not a.b"
            );
            let members = children(graph, expressions[0], "ownedRelationship")?;
            assert_eq!(members.len(), 1);
            assert_eq!(
                members[0]
                    .values
                    .get("memberElement")
                    .and_then(|v| v.as_str()),
                Some(expected)
            );
        }
        assert_eq!(relationships[1].class, "Membership");
        assert_eq!(
            relationships[1]
                .values
                .get("memberElement")
                .and_then(|v| v.as_str()),
            Some(member),
            "{target}"
        );
    }
    Ok(())
}
