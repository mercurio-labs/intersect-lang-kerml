//! Host-only pinned-source admission through the ordinary Xtext frontend.
//! A successful compilation does not qualify KerML or SysML domain semantics.
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
use std::{error::Error, path::Path, process::ExitCode};

const USAGE: &str =
    "Usage: inspect_pinned_grammar_admission PILOT_ROOT kerml|sysml PROFILE [--observe]";
const STRUCTURE: &str = "org.omg.sysml/model/SysML.ecore";
const EXPRESSIONS: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";

pub(crate) fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let mut report = initial_report();
    let result = (|| -> Result<(), Box<dyn Error>> {
        let measured =
            args.len() == 5 && args[3] == "--observe" && args[4] == "--measure-recursive-work";
        if args.len() != 3 && !(args.len() == 4 && args[3] == "--observe") && !measured {
            return Err(USAGE.into());
        }
        let profile: Profile = serde_json::from_value(json!(args[2]))?;
        report["profile"] = json!(profile.as_str());
        report["language"] = json!(args[1]);
        inspect(
            Path::new(&args[0]),
            &args[1],
            profile,
            args.len() >= 4,
            measured,
            &mut report,
        )
    })();
    let exit = if let Err(error) = result {
        if report["first_blocker"].is_null() {
            report["first_blocker"] = json!({"message": error.to_string()});
        }
        report["error"] = json!(error.to_string());
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    };
    println!("{report}");
    exit
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

fn inspect(
    root: &Path,
    language: &str,
    profile: Profile,
    observe: bool,
    measure_recursive_work: bool,
    report: &mut Value,
) -> Result<(), Box<dyn Error>> {
    inspect_with_prefix(
        root,
        language,
        profile,
        observe,
        measure_recursive_work,
        report,
        None,
    )
}

pub(crate) fn inspect_with_prefix(
    root: &Path,
    language: &str,
    profile: Profile,
    observe: bool,
    measure_recursive_work: bool,
    report: &mut Value,
    prefix: Option<&intersect_text::grammar::SourcePrefixDiagnostic<'_>>,
) -> Result<(), Box<dyn Error>> {
    inspect_with_optional_frontier(
        root,
        language,
        profile,
        observe,
        measure_recursive_work,
        report,
        prefix,
        None,
    )
}

// Same setup and result projection; only the additive host selects this option.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inspect_with_optional_frontier(
    root: &Path,
    language: &str,
    profile: Profile,
    observe: bool,
    measure_recursive_work: bool,
    report: &mut Value,
    prefix: Option<&intersect_text::grammar::SourcePrefixDiagnostic<'_>>,
    frontier: Option<intersect_text::grammar::RecursiveFrontierOrder>,
) -> Result<(), Box<dyn Error>> {
    inspect_with_optional_frontier_pool(
        root,
        language,
        profile,
        observe,
        measure_recursive_work,
        report,
        prefix,
        frontier,
        None,
    )
}

// Host-only opt-in; every legacy/default entry still passes literal None.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inspect_with_optional_frontier_pool(
    root: &Path,
    language: &str,
    profile: Profile,
    observe: bool,
    measure_recursive_work: bool,
    report: &mut Value,
    prefix: Option<&intersect_text::grammar::SourcePrefixDiagnostic<'_>>,
    frontier: Option<intersect_text::grammar::RecursiveFrontierOrder>,
    pool: Option<
        &intersect_text::grammar::admission_observation::pool_blocks::CompilerPoolDiagnostic,
    >,
) -> Result<(), Box<dyn Error>> {
    if pool.is_some()
        && (language != "sysml"
            || profile != Profile::NativeV80
            || !matches!(frontier, Some(
                intersect_text::grammar::RecursiveFrontierOrder::BreadthFirstTransparentPorts
                | intersect_text::grammar::RecursiveFrontierOrder::BreadthFirstComposedTransparentPorts
                | intersect_text::grammar::RecursiveFrontierOrder::BreadthFirstSharedCallPorts))
            || prefix.is_some()
            || !observe
            || !measure_recursive_work)
    {
        return Err("Compiler pool host requires the full original SysML Native80 transparent-port observation".into());
    }

    if frontier.is_some() && (prefix.is_some() || !observe || !measure_recursive_work) {
        return Err(
            "The selected frontier host requires phase/work observation and no prefix capture"
                .into(),
        );
    }
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
        let source = DefinitionSource::new(path, kind, std::fs::read_to_string(root.join(path))?);
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
    report["stage"] = json!("ordinary_admission");
    let outcome = if let Some(prefix) = prefix {
        intersect_text::ordinary::compile_with_context_prefix_inspected(
            compilation,
            profile,
            &contracts,
            None,
            policy,
            policy,
            None,
            prefix,
        )
    } else if observe {
        use intersect_text::grammar::admission_observation::PredictionObservation;
        let observation = PredictionObservation::new();
        let counters = measure_recursive_work.then(|| {
            Box::new(
                intersect_text::grammar::admission_observation::RecursiveAnalysisCounters::new(),
            )
        });
        let result = if let Some(pool) = pool {
            // Explicit opt-in only. The old pool selector calls the same old
            // facade with the same arguments; None/default branches stay exact.
            let compile = if frontier == Some(intersect_text::grammar::RecursiveFrontierOrder::BreadthFirstComposedTransparentPorts) {
                intersect_text::ordinary::compile_with_context_composed_ports_pool_observed
            } else if frontier == Some(intersect_text::grammar::RecursiveFrontierOrder::BreadthFirstSharedCallPorts) {
                intersect_text::ordinary::compile_with_context_shared_call_ports_pool_observed
            } else {
                intersect_text::ordinary::compile_with_context_pool_observed
            };
            let result = compile(
                compilation,
                profile,
                &contracts,
                None,
                policy,
                policy,
                intersect_text::grammar::admission_observation::AnalysisObservers {
                    phases: &observation,
                    recursive_work: counters.as_deref(),
                }
                .into(),
                pool,
            );
            // Preserve the whole actual Result before any fallible projection.
            report["actual_compiler_result"] = serde_json::to_value(&result)?;
            result
        } else if let Some(order) = frontier {
            let result = intersect_text::ordinary::compile_with_context_frontier_selected(
                compilation,
                profile,
                &contracts,
                None,
                policy,
                policy,
                Some(
                    intersect_text::grammar::admission_observation::AnalysisObservers {
                        phases: &observation,
                        recursive_work: counters.as_deref(),
                    }
                    .into(),
                ),
                order,
            );
            // Preserve the COMPLETE real Result before any fallible observation
            // projection or outcome check. This never asserts admission/failure.
            report["actual_compiler_result"] = serde_json::to_value(&result)?;
            result
        } else {
            if let Some(work) = counters.as_ref() {
                intersect_text::ordinary::compile_with_context_measured(
                    compilation,
                    profile,
                    &contracts,
                    None,
                    policy,
                    policy,
                    intersect_text::grammar::admission_observation::AnalysisObservers {
                        phases: &observation,
                        recursive_work: Some(work.as_ref()),
                    },
                )
            } else {
                intersect_text::ordinary::compile_with_context_observed(
                    compilation,
                    profile,
                    &contracts,
                    None,
                    policy,
                    policy,
                    &observation,
                )
            }
        };
        let failure = observation.first_failure().map(|failure| -> Result<Value, Box<dyn Error>> {
            Ok(json!({
                "stage": format!("{:?}", failure.stage),
                "phase": format!("{:?}", failure.phase),
                "source": failure.source.map(|source| -> Result<Value, Box<dyn Error>> {
                    Ok(json!({"uri": source.uri.as_str()?, "sha256": source.sha256.as_str()?}))
                }).transpose()?,
                "owner_rule": failure.owner_rule.map(|value| value.as_str().map(str::to_owned)).transpose()?,
                "declaration_span": failure.declaration_span,
                "decision": failure.decision.map(|value| value.as_str().map(str::to_owned)).transpose()?,
                "capture_unavailable": failure.capture_unavailable,
                "charged_work": failure.charged_work,
                "syntax_depth": failure.syntax_depth,
                "call_depth": failure.call_depth,
                "state_count": failure.state_count,
                "repeated_state_count": failure.repeated_state_count,
            }))
        }).transpose()?;
        report["prediction_observation"] = json!({
            "coverage": format!("{:?}", observation.coverage()),
            "first_failure": failure,
            "scope": "supplemental returned phase only; authoritative result and limits unchanged",
        });
        if let Some(work) = counters.as_ref() {
            report["recursive_analysis_metrics"] = recursive_metrics(work)?;
            if profile == Profile::NativeV80 {
                report["recursive_analysis_metrics"]["scope"] = json!(
                    "partial seven-category canonical worker counters; Native80 source/template/growth/frontier/range fees are uncategorized; no whole10000 conservation, timing or allocator bound"
                );
                report["recursive_analysis_metrics"]["native80_partial_pool"] =
                    partial_native80_pool(work);
            }
        }
        result
    } else {
        intersect_text::ordinary::compile_with_context(
            compilation,
            profile,
            &contracts,
            None,
            policy,
            policy,
        )
    };
    if prefix.is_some() {
        // Preserve the COMPLETE actual Result before projection/consumption.
        // This is host-only output; no diagnostic record is executable authority.
        report["actual_compiler_result"] = serde_json::to_value(&outcome)?;
    }
    match outcome {
        Ok(package) => {
            report["ok"] = json!(true);
            report["stage"] = json!("compiled");
            report["classes"] = json!(package.schema.classes.len());
        }
        Err(diagnostics) => {
            if let Some(first) = diagnostics.first() {
                report["first_blocker"] = blocker(first, &originals, &syntaxes);
            }
            report["diagnostics"] = json!(diagnostics);
        }
    }
    // A completed rejected observation is distinct from a failed probe process.
    report["observation_complete"] = json!(true);
    Ok(())
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

// Host-only JSON projection; the core observer owns no I/O or source lifetime.
fn recursive_metrics(
    work: &intersect_text::grammar::admission_observation::RecursiveAnalysisCounters,
) -> Result<Value, Box<dyn Error>> {
    use intersect_text::grammar::admission_observation::{
        RecursiveDecisionCounts, RecursiveProgramCounts,
    };
    let decision = |row: RecursiveDecisionCounts| -> Result<Value, Box<dyn Error>> {
        Ok(json!({
            "program_ordinal": row.program_ordinal, "decision_ordinal": row.decision_ordinal,
            "stage": format!("{:?}", row.stage),
            "source": row.source.map(|source| -> Result<Value, Box<dyn Error>> {
                Ok(json!({"uri":source.uri.as_str()?,"sha256":source.sha256.as_str()?}))
            }).transpose()?,
            "owner_rule": row.owner_rule.map(|value|value.as_str().map(str::to_owned)).transpose()?,
            "declaration_span": row.declaration_span,
            "decision": row.decision.map(|value|value.as_str().map(str::to_owned)).transpose()?,
            "capture_unavailable": row.capture_unavailable,
            "failed": row.failed,"completed": row.completed,"work": row.work,
        }))
    };
    let program = |row: RecursiveProgramCounts| {
        json!({
            "ordinal":row.ordinal,"stage":format!("{:?}",row.stage),"initial":row.initial,
            "remaining":row.remaining,"granted":row.granted,"denied":row.denied,
            "completed":row.completed,"conserved":row.conserved,
        })
    };
    let snapshot = work.snapshot();
    let completed = snapshot
        .heaviest_completed
        .into_iter()
        .map(|row| row.map(decision).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"coverage":format!("{:?}",snapshot.coverage),
        "categories":["FrontierPop","ClosureVisit","ClosureReservation","CallerlessFollowEdge","TransitionEdge","WitnessState","WitnessEdge"],
        "schedule_sites":["Seed","Return","Follow","Successor"],
        "schedule_columns":["absent_candidates","retained_unique","duplicate_candidates"],
        "stage_order":["Parent","TerminalParent","Executable"],"stages":snapshot.stages,
        "current_program":snapshot.current_program.map(program),"last_program":snapshot.last_program.map(program),
        "current":snapshot.current.map(decision).transpose()?,"heaviest_completed":completed,
        "overflowed":snapshot.overflowed,"inconsistent":snapshot.inconsistent,
        "observer_bytes":std::mem::size_of::<intersect_text::grammar::admission_observation::RecursiveAnalysisCounters>(),
        "scope":"actual existing work/key/cache operations; stage totals aggregate; context work is not descendant source/cost attribution; no timing or allocator-byte bound",
    }))
}

// Host-only reconciliation of measured pool boundaries. The difference is an
// explicitly UNATTRIBUTED quantity, never a manufactured work category. A denied
// debit outside the seven old sites is unavailable here, not asserted to be zero.
fn partial_native80_pool(
    work: &intersect_text::grammar::admission_observation::RecursiveAnalysisCounters,
) -> Value {
    let snapshot = work.snapshot();
    let program = snapshot.last_program.or(snapshot.current_program);
    let boundaries = program.map(|program| {
        let counted = program.granted.iter().try_fold(0u64, |sum, n| sum.checked_add(*n));
        let after_entry = program.initial.checked_sub(program.remaining);
        let unattributed = after_entry.and_then(|used| u64::try_from(used).ok())
            .zip(counted).and_then(|(used, counted)| used.checked_sub(counted));
        json!({
            "original_parser_analysis_pool":10000,
            "program_ordinal":program.ordinal,"stage":format!("{:?}",program.stage),
            "observed_counter_entry_remaining":program.initial,
            "observed_exit_remaining":program.remaining,
            "pool_consumed_before_counter_entry":10000usize.checked_sub(program.initial),
            "pool_consumed_after_counter_entry":after_entry,
            "seven_category_successful_units":counted,
            "uncategorized_after_counter_entry_units":unattributed,
            "uncategorized_is_a_boundary_difference_not_a_category":true,
            "denied_attempts_outside_seven_categories":"unavailable",
            "old_conserved_field_meaning":"equality of only seven category totals to this program interval; never complete Native80 coverage",
        })
    });
    json!({"boundaries":boundaries,"whole_pool_category_coverage":false,
        "v76_exact_sidecar_support":false,"overflowed":snapshot.overflowed,
        "inconsistent":snapshot.inconsistent})
}
