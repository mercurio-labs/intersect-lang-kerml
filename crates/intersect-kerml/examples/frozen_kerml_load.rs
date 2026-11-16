//! Handwritten measurement host: a thin Base/ScalarValues/Links load.
//!
//! The original KerML grammar (NativeV80, runtime-guarded) parses each library
//! through a language-neutral open draft capability into a `FrozenStore`. The
//! construction is then linked with the source-authored KerML name resolution
//! and validated. Usage: `frozen_kerml_load ROOT [ADMITTED_DIR]`; with the
//! `fixtures/frozen-bsl` (the admitted graph), resolutions are compared with the
//! admitted graph. `FROZEN_KERML_DUMP=PATH` writes the frozen construction;
//! `FROZEN_KERML_LINKED_DUMP=PATH` writes the validated model plus diagnostics;
//! validation failure writes diagnostics with the labeled construction graph.
//!
//! The metamodel's redefinitions and the Rcore construction defaults
//! (`semantics/kerml-construction.rcore`) supply the implicit values.
use intersect_kerml::native_pilot::NativeKerMLSession;
use intersect_model::{
    core::{
        frozen::{FeatureId, FrozenError, FrozenStore, LinkView, Metamodel, ObjId},
        model::Model,
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
use serde_json::Value;
use std::{path::PathBuf, rc::Rc, sync::Arc, time::Instant};

#[path = "support/kerml_scopes.rs"]
mod kerml_scopes;

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

const CORPUS: &str = "fixtures/native-pilot/v1/sources";
const DEFINITIONS: [&str; 3] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
];
const LIBRARIES: [&str; 3] = [
    "sysml.library/Kernel Libraries/Kernel Semantic Library/Base.kerml",
    "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml",
    "sysml.library/Kernel Libraries/Kernel Semantic Library/Links.kerml",
];

fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

fn main() -> Result<(), String> {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let read = |path: &str| -> Result<Source, String> {
        let text = std::fs::read_to_string(root.join(CORPUS).join(path))
            .map_err(|e| format!("{path}: {e}"))?;
        Ok(Source {
            // Logical resource URIs may not contain spaces; the admitted graph does.
            uri: path.replace(' ', "%20"),
            text,
        })
    };
    let definitions = DEFINITIONS
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;
    let libraries = LIBRARIES
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;

    // Optional: the admitted graph, `fixtures/frozen-bsl`.
    let admitted: Option<Model> = match std::env::args().nth(2) {
        Some(dir) => {
            let path = PathBuf::from(dir).join("graph.json");
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            Some(serde_json::from_slice(&bytes).map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let clock = Instant::now();
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativeKerMLSession::prepare(&definitions, &preparation)?;
    println!("KerML definition preparation {:.0} ms", ms(clock));

    session.with_definition(|definition, contracts| {
        // KerML supplies only its value converters and assignment routing; the
        // grammar and metamodel decide what is constructed.
        let kerml =
            intersect_kerml::text_capability::kerml_capability(definition, contracts)?;
        let kerml = intersect_kerml::text_capability::nonunique_draft_capability(
            definition,
            kerml,
            &mut QueryBudget::new(usize::MAX),
        )?;
        let capability = OpenDraftCapability::new(definition, &kerml)?;
        let names = intersect_kerml::text_capability::kerml_string_rules(definition, &mut QueryBudget::new(usize::MAX))?;
        let capability = capability.with_string_rules(&names);
        let language = DraftLanguage::bind(definition, &capability, AnalysisLimits::workbench())?;
        // The Pilot `operand` parsing extension, as the grammar's computed settings.
        let operands = intersect_kerml::operand_settings(definition, contracts)?;
        let providers: Vec<&dyn intersect_model::setting::SourceSettingValues> = operands
            .iter()
            .map(|p| p as &dyn intersect_model::setting::SourceSettingValues)
            .collect();
        let settings = intersect_text::setting::CheckedDraftTextSettings::bind(definition, &providers)?;
        // Live callers need a finite step pool; this one is far above BSL's needs.
        let limits = EvaluationLimits {
            steps_per_resource: Some(4_000_000_000),
            ..EvaluationLimits::native()
        };
        let clock = Instant::now();
        // MOF property redefinitions come from the metamodel's own annotations
        // (read from the stored SysML structure).
        let redefinitions = kerml_scopes::redefinitions(definition.schema())?;
        println!(
            "read {} redefinitions {:.0} ms",
            redefinitions.len(),
            ms(clock)
        );
        let clock = Instant::now();
        let mm = Arc::new(
            Metamodel::freeze(definition.schema())
                .and_then(|mm| mm.with_redefinitions(&redefinitions))
                .map_err(|e| e.to_string())?,
        );
        println!("freeze metamodel {:.1} ms", ms(clock));
        let mut frozen = FrozenStore::new(mm.clone());
        let clock = Instant::now();
        let mut diagnostics = 0;
        let mut proxies = 0;
        for source in &libraries {
            let name = source.uri.rsplit('/').next().unwrap_or("");
            let mut caller = QueryBudget::new(usize::MAX);
            let result = language.parse_runtime_into_with_settings(
                source, limits, Some(&settings), &mut frozen, &mut caller,
            );
            match result {
                Ok((evaluation, written, _)) => {
                    proxies += written;
                    diagnostics += evaluation.diagnostics.len();
                    for d in evaluation.diagnostics.iter().take(3) {
                        println!("  FrozenStore {name}: {}", d.message.chars().take(200).collect::<String>());
                    }
                }
                Err(failure) => println!("  FrozenStore {name}: {}", failure.chars().take(300).collect::<String>()),
            }
        }
        println!(
            "parse into FrozenStore: {:.1} ms, diagnostics {diagnostics}, reference proxies {proxies}",
            ms(clock)
        );
        let actual = storage::export(&frozen)?;
        if let Ok(path) = std::env::var("FROZEN_KERML_DUMP") {
            let json = serde_json::to_vec(&actual).map_err(|e| e.to_string())?;
            std::fs::write(&path, json).map_err(|e| format!("{path}: {e}"))?;
        }
        println!("objects: FrozenStore {}", actual.elements.len());
        link_and_validate(&mm, frozen, &actual, admitted.as_ref())?;
        Ok(())
    })
}

/// Link the construction with the source-authored KerML name resolution on the
/// direct executor, compare each resolution with the admitted graph when given,
/// and validate. Missing KerML construction rules show up as violations.
fn link_and_validate(
    mm: &Arc<Metamodel>,
    frozen: FrozenStore,
    construction: &Model,
    admitted: Option<&Model>,
) -> Result<(), String> {
    use intersect_model::frozen_scopes::{Execution, RcoreScopes};
    use std::{cell::RefCell, collections::HashMap};
    let clock = Instant::now();
    let (bundle, scopes, cached) = kerml_scopes::compile_cached(mm)?;
    let defaults = scopes.iter().filter(|s| s.is_default()).count();
    println!(
        "{} {} Rcore scopes and {defaults} defaults {:.0} ms",
        if cached { "load saved" } else { "compile" },
        scopes.len() - defaults,
        ms(clock)
    );
    let resolvers = RcoreScopes::new(mm, &bundle, &scopes)
        .map_err(|e| e.to_string())?
        .with_execution(Execution::Fast);
    // Admitted identities use spaces where logical URIs use %20.
    let normal = |id: &str| id.replace("%20", " ");
    let expected: HashMap<(String, String), Vec<String>> = admitted
        .map(|graph| {
            graph
                .elements
                .iter()
                .flat_map(|e| {
                    e.values.iter().map(move |(k, v)| {
                        let targets = match v {
                            Value::String(s) => vec![s.clone()],
                            Value::Array(items) => items
                                .iter()
                                .filter_map(|i| i.as_str().map(str::to_owned))
                                .collect(),
                            _ => vec![],
                        };
                        ((e.id.clone(), k.clone()), targets)
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    // (owner, feature, spelling, Rcore candidates)
    type Outcome = (String, String, String, Result<Vec<String>, String>);
    let outcomes: RefCell<Vec<Outcome>> = RefCell::new(Vec::new());
    let resolve = |view: &LinkView<'_>,
                   owner: ObjId,
                   feature: FeatureId,
                   spelling: &str|
     -> Result<Option<ObjId>, FrozenError> {
        let found = resolvers.candidates(view, owner, feature, spelling);
        let names = found.as_ref().map_err(|e| e.to_string()).map(|c| {
            c.iter()
                .filter_map(|o| view.identity(*o))
                .map(normal)
                .collect::<Vec<_>>()
        });
        outcomes.borrow_mut().push((
            normal(view.identity(owner).unwrap_or("")),
            mm.feature_name(feature).to_owned(),
            spelling.to_owned(),
            names,
        ));
        Ok(match found {
            Ok(c) if c.len() == 1 => c.first().copied(),
            _ => None,
        })
    };
    let clock = Instant::now();
    let builder = frozen.into_builder().map_err(|e| e.to_string())?;
    let linked = builder.link(&resolve);
    let link = ms(clock);
    let outcomes = outcomes.into_inner();
    let unique = outcomes
        .iter()
        .filter(|(.., found)| found.as_ref().is_ok_and(|f| f.len() == 1))
        .count();
    println!(
        "link {link:.1} ms: {} references, {unique} resolved uniquely, executor {:?}",
        outcomes.len(),
        resolvers.stats()
    );
    if admitted.is_some() {
        let mut agree = 0;
        for (owner, feature, spelling, found) in &outcomes {
            let want = expected.get(&(owner.clone(), feature.clone()));
            match (found, want) {
                (Ok(f), Some(w)) if f.len() == 1 && w.contains(&f[0]) => agree += 1,
                _ => println!(
                    "  {} .{feature} '{spelling}': admitted {want:?} got {found:?}",
                    owner.rsplit('/').next().unwrap_or(owner)
                ),
            }
        }
        println!(
            "agree with the admitted graph on {agree} of {}",
            outcomes.len()
        );
    }
    let linked = match linked {
        Ok(linked) => linked,
        Err(diagnostics) => {
            summarize("link", &diagnostics);
            return Ok(());
        }
    };
    let clock = Instant::now();
    let linked = match linked.complete_implicit(&resolvers) {
        Ok(linked) => linked,
        Err(diagnostics) => {
            summarize("implicit", &diagnostics);
            return Ok(());
        }
    };
    let linked = match linked.complete_defaults(&resolvers) {
        Ok(linked) => linked,
        Err(diagnostics) => {
            summarize("defaults", &diagnostics);
            return Ok(());
        }
    };
    println!("defaults {:.1} ms", ms(clock));
    let clock = Instant::now();
    let model = match linked.validate() {
        Ok(model) => model,
        Err(diagnostics) => {
            println!("validate {:.1} ms", ms(clock));
            summarize("validate", &diagnostics);
            // LinkedModel::validate consumes its input, including on failure.
            // Keep the construction graph for identity pairing and explicitly
            // label its stage; it is not a validated or completed graph.
            write_linked_dump(construction, &diagnostics, "construction")?;
            return Ok(());
        }
    };
    println!(
        "validate {:.1} ms: valid, {} objects",
        ms(clock),
        model.len()
    );
    if std::env::var_os("FROZEN_KERML_LINKED_DUMP").is_some() {
        let exported = model.to_model().map_err(|e| e.to_string())?;
        write_linked_dump(
            &exported,
            &intersect_model::core::frozen::Diagnostics(Vec::new()),
            "validated",
        )?;
    }
    if let Some(graph) = admitted {
        compare(&model.to_model().map_err(|e| e.to_string())?, graph);
    }
    Ok(())
}

/// Oracle-only envelope; the shared model serialization stays unchanged.
fn write_linked_dump(
    model: &Model,
    diagnostics: &intersect_model::core::frozen::Diagnostics,
    graph_stage: &str,
) -> Result<(), String> {
    use intersect_model::core::frozen::Violation;
    let Ok(path) = std::env::var("FROZEN_KERML_LINKED_DUMP") else {
        return Ok(());
    };
    let entries: Vec<_> = diagnostics
        .0
        .iter()
        .map(|violation| {
            let (code, object) = match violation {
                Violation::UnknownClass(o) => ("UnknownClass", Some(o)),
                Violation::Abstract { object, .. } => ("Abstract", Some(object)),
                Violation::Multiplicity { object, .. } => ("Multiplicity", Some(object)),
                Violation::Kind { object, .. } => ("Kind", Some(object)),
                Violation::Attribute { object, .. } => ("Attribute", Some(object)),
                Violation::TargetType { object, .. } => ("TargetType", Some(object)),
                Violation::Duplicate { object, .. } => ("Duplicate", Some(object)),
                Violation::Unresolved { object, .. } => ("Unresolved", Some(object)),
                Violation::ReferenceCycle { object, .. } => ("ReferenceCycle", Some(object)),
                Violation::MultipleContainers(o) => ("MultipleContainers", Some(o)),
                Violation::ContainmentCycle(o) => ("ContainmentCycle", Some(o)),
                Violation::Opposite { object, .. } => ("Opposite", Some(object)),
                Violation::Resolver(_) => ("Resolver", None),
            };
            let element = object.and_then(|id| model.elements.iter().find(|e| &e.id == id));
            serde_json::json!({
                "severity": "error", "code": code, "message": violation.to_string(),
                "target": object, "resource": element.map(|e| &e.resource),
                "start": element.map(|e| e.start), "end": element.map(|e| e.end),
            })
        })
        .collect();
    let mut exported = serde_json::to_value(model).map_err(|e| e.to_string())?;
    exported["diagnostics"] = serde_json::json!(entries);
    exported["graph_stage"] = serde_json::json!(graph_stage);
    let json = serde_json::to_vec(&exported).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{path}: {e}"))
}

/// Element-by-element comparison with the admitted graph. Logical URIs use
/// %20 where admitted identities use spaces, and the admitted resources carry
/// a `pilot:/` prefix; element identities are compared after decoding both
/// hex parts and applying the same normalization.
fn compare(model: &Model, graph: &Model) {
    fn normal(value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(s.replace("%20", " ")),
            Value::Array(items) => Value::Array(items.iter().map(normal).collect()),
            other => other.clone(),
        }
    }
    fn element_id(value: Option<&Value>) -> Option<(String, String)> {
        let text = value?.as_str()?;
        let (resource, identity) = text
            .strip_prefix("urn:intersect:native-element:v1:")?
            .split_once(':')?;
        let decode = |hex: &str| -> Option<String> {
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
                .collect::<Option<Vec<u8>>>()?;
            let text = String::from_utf8(bytes).ok()?;
            Some(text.trim_start_matches("pilot:/").replace("%20", " "))
        };
        Some((decode(resource)?, decode(identity)?))
    }
    let expected: std::collections::BTreeMap<_, _> = graph
        .elements
        .iter()
        .filter(|e| !e.resource.contains("native-bench"))
        .map(|e| (e.id.clone(), e))
        .collect();
    let mut differences = std::collections::BTreeMap::<String, (usize, String)>::new();
    let mut matched = 0;
    for element in &model.elements {
        let id = element.id.replace("%20", " ");
        let Some(admitted) = expected.get(&id) else {
            *differences
                .entry("only in the frozen model".into())
                .or_default() = (differences.len(), id);
            continue;
        };
        matched += 1;
        let keys: std::collections::BTreeSet<&String> = element
            .values
            .keys()
            .chain(admitted.values.keys())
            .collect();
        for key in keys {
            if key == "elementId" {
                let ours = element_id(element.values.get(key));
                if ours.is_some() && ours == element_id(admitted.values.get(key)) {
                    continue;
                }
            }
            let ours = element.values.get(key).map(normal);
            let theirs = admitted.values.get(key).cloned();
            if ours != theirs {
                let entry = differences
                    .entry(format!("{}.{key}", admitted.class))
                    .or_insert((0, String::new()));
                entry.0 += 1;
                if entry.1.is_empty() {
                    entry.1 = format!("admitted {theirs:?}, frozen {ours:?}")
                        .chars()
                        .take(160)
                        .collect();
                }
            }
        }
    }
    println!(
        "compared {matched} of {} admitted objects (control resource excluded); {} differing fields",
        expected.len(),
        differences.len()
    );
    for (field, (count, example)) in differences {
        println!("  {count:4} {field}: {example}");
    }
}

/// Count violations by kind and feature, with one example each.
fn summarize(phase: &str, diagnostics: &intersect_model::core::frozen::Diagnostics) {
    use intersect_model::core::frozen::Violation;
    use std::collections::BTreeMap;
    let mut kinds: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for violation in &diagnostics.0 {
        let text = violation.to_string();
        let key = match violation {
            Violation::Multiplicity {
                feature,
                count,
                lower,
                upper,
                ..
            } => format!("{feature}: {count} values outside [{lower},{upper:?}]"),
            // Other kinds: the violation text with object identities removed.
            _ => text
                .split_whitespace()
                .filter(|w| !w.contains(".kerml#"))
                .collect::<Vec<_>>()
                .join(" "),
        };
        kinds.entry(key).or_insert((0, text)).0 += 1;
    }
    println!(
        "{phase}: {} violations in {} groups",
        diagnostics.0.len(),
        kinds.len()
    );
    for (key, (count, example)) in kinds {
        let example: String = example.chars().take(160).collect();
        println!("  {count:4} {key}  (e.g. {example})");
    }
}
