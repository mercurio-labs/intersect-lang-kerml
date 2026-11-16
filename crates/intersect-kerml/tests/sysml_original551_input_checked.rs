//! Handwritten explicit conditional InputChecked request for unchanged original551 SysML sources.
//! Derived from the separate Strict LEGACY_10K carrier; no static-disjointness award.
//! Admission is asserted; rejected requests remain failed tests. No domain/runtime award.
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::bundle::DefinitionSource;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::schema::Diagnostic;
use intersect_text::compiler;
use intersect_text::grammar::Profile;
use intersect_text::grammar::SourceLimits;
use intersect_text::grammar::syntax;
use intersect_text::package::BundleCompilation;
use intersect_text::package::CompilationSourcePolicy;
use serde_json::{Value, json};
use std::error::Error;

const STRUCTURE: &str = "org.omg.sysml/model/SysML.ecore";
const EXPRESSIONS: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";

const USAGE: &str =
    "Usage: explicitly run the ignored original551 conditional InputChecked qualification carrier";
#[test]
#[ignore = "Full original SysML grammar admission remains unresolved; run explicitly for qualification"]
fn original_full551_sysml_input_checked_realistic_admission_succeeds() -> Result<(), Box<dyn Error>>
{
    let mut report = initial_report();
    report["frontend"] = json!(
        "intersect_text::ordinary::compile_with_runtime_guarded_policy_work_observed_and_budget"
    );
    report["language"] = json!("sysml");
    report["profile"] = json!(Profile::NativeV80.as_str());
    report["runtime_guarded_policy"] = json!("InputCheckedCompletionScopes");
    report["frontier_selection"] = json!({"order":"Historical","source":"literal existing runtime guarded compiler facade","new_policy_or_profile":false,"conditional_multiway_extension":true});
    report["budget_configuration"] = json!({"compiler_prediction_work_per_program":100000000,"named_existing_policy":"REALISTIC","cumulative_depth":128,"stack":"unchanged default","observed_remaining":null,"configuration_is_not_measured_consumption":true});
    report["runtime_candidate_observation"] = Value::Null;
    report["compiler_pool_diagnostic"] = Value::Null;
    report["first_queue_topology_observation"] = Value::Null;
    report["actual_compiler_result"] = Value::Null;
    report["typed_runtime_admission"] = Value::Null;
    report["actual_compile_invocations"] = json!(0);
    report["runtime_execution_performed"] = json!(false);
    let result = inspect_runtime(&mut report);
    // Complete owned compiler Result and observer data precede every assertion.
    // Host/setup errors are distinct from compiler rejection, never PASS.
    report["host_result"] = json!(
        result
            .as_ref()
            .map(|_| ())
            .map_err(|error| error.to_string())
    );
    println!("SYSML_ORIGINAL551_INPUT_CHECKED_CARGO_RAW {}", report);
    result?;
    assert_eq!(report["actual_compile_invocations"], json!(1));
    assert_eq!(
        report["syntax_inventory"][0]["authored_declarations"],
        json!(108)
    );
    assert_eq!(
        report["syntax_inventory"][1]["authored_declarations"],
        json!(443)
    );
    assert_eq!(report["observation_complete"], json!(true));
    assert_eq!(
        report["ok"],
        json!(true),
        "Original full551 admission must succeed; complete rejected Result is printed above"
    );
    assert!(report["actual_compiler_result"].get("Ok").is_some());
    assert!(!report["typed_runtime_admission"].is_null());
    assert_eq!(
        report["runtime_guarded_policy"],
        json!("InputCheckedCompletionScopes")
    );
    assert_eq!(
        report["budget_configuration"]["compiler_prediction_work_per_program"],
        json!(100_000_000)
    );
    assert_eq!(
        report["typed_runtime_admission"]["policy"],
        json!("InputCheckedCompletionScopes")
    );
    assert_eq!(
        report["typed_runtime_admission"]["requires_runtime_capability"],
        json!(true)
    );
    assert_eq!(
        report["typed_runtime_admission"]["strict_completion_obligation_count"],
        json!(0)
    );
    assert_eq!(
        report["typed_runtime_admission"]["input_checked_completion_obligation_count"],
        report["typed_runtime_admission"]["decision_count"]
    );
    assert!(
        report["typed_runtime_admission"]["decision_count"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    assert_eq!(report["runtime_execution_performed"], json!(false));
    assert_eq!(report["domain_semantics_verified"], json!(false));
    Ok(())
}

pub(crate) fn initial_report() -> Value {
    json!({
        "version": 1,
        "probe": "intersect.pinned_grammar_admission",
        "frontend": "intersect_text::ordinary::compile_with_context",
        "stage": "arguments",
        "observation_complete": false,
        "ok": false,
        "domain_semantics_verified": false,
        "first_blocker": null,
        "inputs": [],
    })
}

fn inspect_runtime(report: &mut Value) -> Result<(), Box<dyn Error>> {
    let language = "sysml";
    let profile = Profile::NativeV80;
    let (entry, entry_hash, entry_declarations) = match language {
        "kerml" => (
            "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
            "cc02be703955603d2daa91f878f299f950195fcd2e3c59563ae17643ba91dfaa",
            176,
        ),
        "sysml" => (
            "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
            "7d9142742c454f5810fe4db61a089abef5e0b8d61ad8bf552631d9136b04f298",
            443,
        ),
        _ => return Err(USAGE.into()),
    };
    report["stage"] = json!("source_pins");
    let mut read = |path: &str, kind, expected: &str| -> Result<DefinitionSource, Box<dyn Error>> {
        let source = DefinitionSource::new(path, kind, pinned_fixture(path)?);
        report["inputs"]
            .as_array_mut()
            .ok_or("Missing probe input inventory")?
            .push(json!({
                "source": source.id,
                "kind": source.kind,
                "bytes": source.text.len(),
                "expected_sha256": expected,
                "pin_exact": source.id.sha256 == expected,
            }));
        if source.id.sha256 != expected {
            return Err(format!("Pinned source digest mismatch: {path}").into());
        }
        Ok(source)
    };
    let ecore = read(
        STRUCTURE,
        SourceKind::Ecore,
        intersect_kerml::PILOT_SCHEMA_SHA256,
    )?;
    let mut base = read(
        EXPRESSIONS,
        SourceKind::Xtext,
        "7bea42879a913f7613ac5e32779174db01e8fd4fe75694f0845c495aa0191aa8",
    )?;
    let mut grammar = read(entry, SourceKind::Xtext, entry_hash)?;
    base.dependencies = vec![ecore.id.clone()];
    // Both independent entry grammars directly extend KerMLExpressions.
    grammar.dependencies = vec![ecore.id.clone(), base.id.clone()];
    let originals = [ecore.clone(), base.clone(), grammar.clone()];
    let policy = CompilationSourcePolicy {
        structure_bytes: ecore.text.len(),
        xtext_bytes: base.text.len().max(grammar.text.len()),
        // Physical authored declarations, including two shadowed KerML rules.
        xtext_declarations: 108 + entry_declarations,
    };
    report["source_policy"] = json!({"requested": policy, "host": policy});
    report["dependency_closure"] = json!(
        originals
            .iter()
            .map(|source| { json!({"source": source.id, "dependencies": source.dependencies}) })
            .collect::<Vec<_>>()
    );
    report["stage"] = json!("authored_syntax_inventory");
    let mut syntaxes = Vec::new();
    for (source, declarations) in [(&base, 108), (&grammar, entry_declarations)] {
        let parsed = syntax::parse(
            source,
            SourceLimits {
                source_bytes: policy.xtext_bytes,
                declarations: policy.xtext_declarations,
            },
        )
        .map_err(|diagnostic| {
            report["diagnostics"] = json!([diagnostic.clone()]);
            report["first_blocker"] = json!(diagnostic);
            "Pinned grammar syntax inventory failed"
        })?;
        if parsed.rules.len() != declarations {
            return Err(format!("Pinned declaration inventory changed: {}", source.id.uri).into());
        }
        syntaxes.push(parsed);
    }
    report["syntax_inventory"] = json!(
        syntaxes
            .iter()
            .map(|parsed| json!({
                "source": parsed.source,
                "name": parsed.name,
                "parents": parsed.parents,
                "authored_declarations": parsed.rules.len(),
            }))
            .collect::<Vec<_>>()
    );
    let mut semantics = DefinitionSource::new(
        "probe-behavior.rcore",
        SourceKind::RcoreBehavior,
        r#"rcore 1; package emptyRules version "1";"#,
    );
    semantics.dependencies = vec![ecore.id.clone()];
    report["behavior_context"] = json!({
        "source": semantics.id, "text": semantics.text,
        "dependencies": semantics.dependencies, "scope": "empty; no domain behavior installed",
    });
    let compilation = BundleCompilation {
        ecore: ecore.id.clone(),
        materialized_types: vec![],
        xtext: Some(grammar.id.clone()),
        rcore_behavior: semantics.id.clone(),
        namespace: "https://www.omg.org/spec/SysML/20250201".into(),
        bundle: DefinitionBundle {
            version: 1,
            sources: vec![ecore, base, grammar, semantics],
            namespaces: vec![],
        },
    };
    report["stage"] = json!("ecore_migration");
    let targets = [(compilation.ecore.clone(), format!("{STRUCTURE}.rcore"))].into();
    let migration = intersect_model::migration::migrate_ecore_bundle_with_limits(
        compilation.bundle.clone(),
        &targets,
        intersect_model::definitions::ecore::ParseLimits {
            source_bytes: policy.structure_bytes,
        },
    )?;
    report["migration"] = json!(
        migration
            .reports
            .iter()
            .map(|item| json!({
                "original": item.original.id,
                "converted": item.converted.as_ref().map(|source| &source.id),
                "correspondence": item.correspondence.len(), "issues": item.issues,
            }))
            .collect::<Vec<_>>()
    );
    let compilation = intersect_text::migration::remap_compilation(&migration, &compilation)?;
    report["migrated_sources"] = json!(
        compilation
            .bundle
            .sources
            .iter()
            .map(|source| json!({
                "source": source.id, "kind": source.kind, "bytes": source.text.len(),
                "dependencies": source.dependencies,
            }))
            .collect::<Vec<_>>()
    );
    report["stage"] = json!("structural_context");
    let mut schema = compiler::compile_structure_bundle_with_limits(
        &compilation.bundle,
        &compilation.ecore,
        &compilation.namespace,
        intersect_model::definitions::ecore::ParseLimits {
            source_bytes: policy.structure_bytes,
        },
    )?;
    intersect_text::grammar::prepare_ecore_reference_context(&mut schema)?;
    report["stage"] = json!("operand_provider_context");
    let mut contracts = Vec::new();
    for class in schema
        .classes
        .keys()
        .filter(|class| schema.conforms(class, "InvocationExpression"))
    {
        let contract = intersect_model::definitions::setting::ComputedSettingContract {
            version: 1,
            schema_revision: schema.revision.clone(),
            class: class.clone(),
            class_identity: schema.identity(class),
            feature: "operand".into(),
            member: schema.effective_feature(class, "operand")?.identity,
            provider: intersect_model::definitions::conversion::ProviderId {
                name: intersect_kerml::OPERAND_PROVIDER.into(),
                version: "1".into(),
            },
            sources: compilation
                .bundle
                .sources
                .iter()
                .map(|source| source.id.clone())
                .collect(),
            operations: vec![intersect_model::definitions::setting::SettingOperation::Append],
        };
        intersect_kerml::OperandSettings::new(&schema, &compilation.bundle, contract.clone())?;
        contracts.push(contract);
    }
    report["provider_context"] = json!({
        "computed_settings": contracts, "datatype_conversions": null,
        "scope": "source-checked append-only operand contracts; no model evaluation",
    });
    // Preserve the original seven operand contracts and their report whole.
    // This separate explicit opt-in context installs no behavior from metadata.
    if profile.has_computed_boolean_settings() {
        let mut usage_contracts = Vec::new();
        for class in schema
            .classes
            .keys()
            .filter(|class| schema.conforms(class, "Usage"))
        {
            let contract = intersect_model::definitions::setting::ComputedSettingContract {
                version: 1,
                schema_revision: schema.revision.clone(),
                class: class.clone(),
                class_identity: schema.identity(class),
                feature: "isReference".into(),
                member: schema.effective_feature(class, "isReference")?.identity,
                provider: intersect_model::definitions::conversion::ProviderId {
                    name: intersect_kerml::sysml_usage::USAGE_REFERENCE_PROVIDER.into(),
                    version: "1".into(),
                },
                sources: compilation
                    .bundle
                    .sources
                    .iter()
                    .map(|source| source.id.clone())
                    .collect(),
                operations: vec![intersect_model::definitions::setting::SettingOperation::Set],
            };
            intersect_kerml::sysml_usage::UsageReferenceSettings::new(
                &schema,
                &compilation.bundle,
                contract.clone(),
            )?;
            usage_contracts.push(contract);
        }
        report["usage_reference_provider_context"] = json!({
            "computed_settings": usage_contracts,
            "delegate_provenance": intersect_kerml::sysml_usage::usage_reference_delegate_source(),
            "requires_java_tooling_or_checkout": false,
            "scope": "registered handwritten Rust constructor source/schema checks; metadata admission only in this host observation, no model/read-session execution",
        });
        contracts.extend(usage_contracts);
    }
    report["stage"] = json!("ordinary_runtime_guarded_admission");
    report["actual_compile_invocations"] = json!(1);
    let prediction_observation =
        intersect_text::grammar::admission_observation::PredictionObservation::new();
    // Current R supplies existing phase/FIRST observations only. Optional
    // develop map diagnostics are unavailable here and are never fabricated.
    let outcome =
        intersect_text::ordinary::compile_with_runtime_guarded_policy_work_observed_and_budget(
            compilation,
            profile,
            &contracts,
            None,
            policy,
            policy,
            intersect_text::grammar::RuntimeGuardedPolicy::InputCheckedCompletionScopes,
            intersect_text::grammar::admission_observation::AnalysisObservers::phases(
                &prediction_observation,
            ),
            intersect_text::grammar::GrammarAnalysisWorkBudget::REALISTIC,
        );
    // RuntimeGuardedCompilation is not serializable. Its complete owned public
    // Package/Diagnostic Result is retained without truncation before checks.
    report["actual_compiler_result"] =
        serde_json::to_value(outcome.as_ref().map(|compiled| compiled.parts().0))?;
    report["runtime_candidate_observation"] = candidate_observation(&prediction_observation);
    match &outcome {
        Ok(compiled) => {
            let (package, admission) = compiled.parts();
            let program = package
                .grammar
                .as_ref()
                .and_then(|g| g.parser_prediction.as_ref());
            report["typed_runtime_admission"] = json!({
                "policy":format!("{:?}",admission.policy()),
                "decision_count":admission.decision_count(),
                "decisions":admission.decisions().collect::<Vec<_>>(),
                "requires_runtime_capability":program.map(|p|p.requires_runtime_capability()),
                "strict_completion_obligation_count":program.map(|p|p.strict_completion_obligation_count()),
                "input_checked_completion_obligation_count":program.map(|p|p.input_checked_completion_obligation_count()),
                "claim":"conditional source-bound runtime IR, not static disjointness or model execution",
                "runtime_consumed":false,
                "not_a_serialized_companion":true,
            });
            report["ok"] = json!(true);
            report["stage"] = json!("compiled_typed_runtime_ir_not_executed");
            report["classes"] = json!(package.schema.classes.len());
        }
        Err(diagnostics) => {
            if let Some(first) = diagnostics.first() {
                report["first_blocker"] = blocker(first, &originals, &syntaxes);
            }
            report["diagnostics"] = json!(diagnostics);
        }
    }
    report["observation_complete"] = json!(true);
    Ok(())
}

// The legacy frontend field retains the canonical policy-family identity.
// This sidecar records the actual diagnostic facade and only measured facts.
fn candidate_observation(
    observation: &intersect_text::grammar::admission_observation::PredictionObservation,
) -> Value {
    use intersect_text::grammar::admission_observation::Phase;
    let first = observation.first_failure();
    let shared = first.filter(|failure| {
        matches!(
            failure.phase,
            Phase::RuntimeNullableInventory
                | Phase::RuntimeCandidateBinding
                | Phase::RuntimeCandidateNonnullableFirst
        )
    });
    let charged = shared.and_then(|failure| failure.charged_work);
    json!({
        "facade":"intersect_text::ordinary::compile_with_runtime_guarded_policy_work_observed_and_budget",
        "coverage":format!("{:?}", observation.coverage()),
        "first_reuse_work":first_reuse_work_observation(observation),
        "first_map_cost":null,
        "first_map_cost_unavailable_reason":"Optional map diagnostics are not requested by this carrier; only phase/FIRST observations are recorded and no map counters measured",
        "first_failure":first.map(|failure| json!({
            "stage":format!("{:?}",failure.stage),
            "boundary":format!("{:?}",failure.phase),
            "source":failure.source.map(|source|json!({
                "uri":source.uri.as_str().ok(),"sha256":source.sha256.as_str().ok()
            })),
            "owner_rule":failure.owner_rule.and_then(|value|value.as_str().ok().map(str::to_owned)),
            "declaration_span":failure.declaration_span,
            "decision":failure.decision.and_then(|value|value.as_str().ok().map(str::to_owned)),
            "capture_unavailable":failure.capture_unavailable,
            "charged_work":failure.charged_work,
            "state_count":failure.state_count,
            "syntax_depth":failure.syntax_depth,"call_depth":failure.call_depth,
            "repeated_state_count":failure.repeated_state_count
        })),
        "shared_program_charged_work":charged,
        "remaining":charged.and_then(|spent|intersect_text::grammar::GrammarAnalysisWorkBudget::REALISTIC.units().checked_sub(spent)),
        "remaining_basis":"Derived only from directly measured cumulative balance at one of the three candidate boundaries; null otherwise.",
        "work_scope":"One existing shared program bucket including earlier graph/inventory work; not phase-only or whole compile consumption.",
        "proof_or_admission_authority":false
    })
}

fn first_reuse_work_observation(
    observation: &intersect_text::grammar::admission_observation::PredictionObservation,
) -> Value {
    use intersect_text::grammar::admission_observation::{
        FirstReuseCharge as C, FirstReuseEvent as E,
    };
    observation.first_reuse_work().map_or(Value::Null, |work| json!({
        "scope":"one actual runtime-guarded collector; successful logical debits only; diagnostic not authority",
        "stage":format!("{:?}",work.stage),
        "source":work.source.map(|source|json!({"uri":source.uri.as_str().ok(),"sha256":source.sha256.as_str().ok()})),
        "capture_unavailable":work.capture_unavailable,
        "entry_remaining":work.entry_remaining,"exit_remaining":work.exit_remaining,
        "precollect_shared_work":work.precollect_work,
        "precollect_scope":"Earlier shared program work before collect, not measured graph-only work.",
        "collector_windows":{
            "collect_total":work.collect_work,"nullable_inventory":work.inventory_work,
            "completed_header":work.completed_header_work,"companion_header":work.companion_header_work,
            "binding_inclusive_first":work.binding_inclusive_work,"first_inclusive":work.first_inclusive_work,
            "binding_exclusive_first":work.binding_exclusive_first(),"collector_outer":work.collector_outer_work()
        },
        "window_conservation":"collect=inventory+completed_header+companion_header+binding_inclusive_first+collector_outer; FIRST is nested inside binding and is never added twice. Global completed_header is excluded from per-query FIRST units.",
        "first_disjoint_units":{
            "canonical_visit":work.first_units[C::CanonicalVisit as usize],
            "affinity_and_state":work.first_units[C::AffinityAndState as usize],
            "visit_peak_updates":work.first_units[C::VisitPeakUpdates as usize],
            "source_key_and_active_checks":work.first_units[C::SourceKeyAndActiveChecks as usize],
            "ordered_map":work.first_units[C::OrderedMap as usize],
            "frame_retention_and_capacity":work.first_units[C::FrameRetentionAndCapacity as usize],
            "summary_inspection_and_clone":work.first_units[C::SummaryInspectionAndClone as usize],
            "publication_and_peak_transfer":work.first_units[C::PublicationAndPeakTransfer as usize]
        },
        "first_disjoint_units_total":work.first_units_total(),
        "first_units_match_inclusive_window":work.first_units_total().zip(work.first_inclusive_work).map(|(units,window)|units==window),
        "events":{
            "inventory_key":work.events[E::InventoryKey as usize],"inventory_repeat":work.events[E::InventoryRepeat as usize],
            "binding":work.events[E::Binding as usize],"eligible_record":work.events[E::EligibleRecord as usize],
            "first_query":work.events[E::FirstQuery as usize],"visit_attempt":work.events[E::VisitAttempt as usize],
            "visit_succeeded":work.events[E::VisitSucceeded as usize],"callee_entered":work.events[E::CalleeEntered as usize],
            "hit":work.events[E::Hit as usize],"publication":work.events[E::Publication as usize],
            "missing_source_or_range":work.events[E::MissingSourceOrRange as usize],"foreign_graph":work.events[E::ForeignGraph as usize],
            "active_call":work.events[E::ActiveCall as usize],"depth_or_capacity":work.events[E::DepthOrCapacity as usize],
            "cloned_symbols":work.events[E::ClonedSymbols as usize],"cloned_string_bytes":work.events[E::ClonedStringBytes as usize]
        },
        "cache_cumulative_logical_bytes":work.retained_cache_bytes,
        "memory_scope":"Existing cumulative cache quota only; neither total compiler memory nor RSS/peak.",
        "last_attempted_syntax_depth":work.last_syntax_depth,"last_attempted_call_depth":work.last_call_depth,
        "maximum_syntax_depth":null,"maximum_call_depth":null,"denied_charge_units":null,"topology":null,
        "finished":work.finished,"returned_error":work.returned_error,"overflow":work.overflow,"multiple_programs":work.multiple_programs,
        "unknown_rule":"Unavailable or overflowed values stay null; last attempted depths are not maxima or proof of denial cause. Multiple-program flags prohibit aggregate interpretation.",
        "proof_or_admission_authority":false
    }))
}

fn blocker(
    diagnostic: &Diagnostic,
    sources: &[DefinitionSource],
    syntaxes: &[syntax::Syntax],
) -> Value {
    let source = sources
        .iter()
        .find(|source| source.id.uri == diagnostic.source);
    let rule = syntaxes
        .iter()
        .find(|parsed| parsed.source.uri == diagnostic.source)
        .and_then(|parsed| {
            parsed
                .rules
                .iter()
                .find(|rule| rule.start <= diagnostic.start && diagnostic.end <= rule.end)
        });
    json!({
        "diagnostic": diagnostic,
        "source_identity": source.map(|source| &source.id),
        "source_span_text": source.and_then(|source| source.text.get(diagnostic.start..diagnostic.end)),
        "authored_rule": rule.map(|rule| &rule.value.name),
        "location_scope": if rule.is_some() { "authored_rule" } else { "unlocalized" },
    })
}

// Cargo-relative checked-in bytes; no external checkout, environment or target path.
fn pinned_fixture(path: &str) -> Result<&'static str, Box<dyn Error>> {
    match path {
        STRUCTURE => Ok(include_str!(
            "../../../fixtures/kerml-draft-pinned/SysML.ecore"
        )),
        EXPRESSIONS => Ok(include_str!(
            "../../../fixtures/kerml-draft-pinned/KerMLExpressions.xtext"
        )),
        "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext" => Ok(include_str!(
            "../../../fixtures/xtext/imported-assignment-v66/full-source-inputs/org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext"
        )),
        _ => Err(format!("Unexpected original551 fixture: {path}").into()),
    }
}
