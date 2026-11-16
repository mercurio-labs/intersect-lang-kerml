//! Focused original-grammar KerML rule probes; handwritten, not generated.
//! Milestone tier: the shared grammar and binding preparation takes about
//! 10 minutes cold. Moving them to the quick tier needs a language-pack load.
use intersect_kerml::native_pilot::NativePilotSession;
use intersect_model::{
    core::{
        frozen::{FeatureId, FrozenError, FrozenStore, LinkView, Metamodel, ObjId, Violation},
        model::Model,
        storage::{self, ModelRead, ModelStore},
    },
    definitions::{
        bundle::DefinitionBundle,
        conversion::{InvocationContext, InvocationObserver, ProviderId},
        rcore_scope::VerifiedScope,
    },
    frozen_scopes::{Execution, RcoreScopes},
};
use intersect_text::Source;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, OnceLock},
    time::Instant,
};

#[allow(dead_code)]
#[path = "../examples/support/kerml_scopes.rs"]
mod kerml_scopes;
#[allow(dead_code)]
#[path = "../examples/support/pilot_languages.rs"]
mod pilot_languages;

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
const LIBRARIES: [&str; 3] = [
    "sysml.library/Kernel Libraries/Kernel Semantic Library/Base.kerml",
    "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml",
    "sysml.library/Kernel Libraries/Kernel Semantic Library/Links.kerml",
];
const CASES: [(&str, &str); 17] = [
    (
        "chain_named_sibling",
        "package P {
 behavior B { step s; }
 behavior M {
 step p : B;
 feature f;
 feature subsets f chains p.s;
 }
}",
    ),
    (
        "featured_by_member",
        "package P {\n class A;\n feature f featured by g { member feature g : A; }\n}",
    ),
    (
        "global_qualification",
        "package P {\n package '$' { package Base { class Anything; } }\n class X specializes $::Base::Anything;\n}",
    ),
    (
        "recursive_import",
        "package P {\n package Q { package R { class D; } }\n package S { private import Q::**; class Y specializes D; }\n}",
    ),
    (
        "earlier_import",
        "package P {\n package Q { package R { class D; } }\n package S { private import Q::*; private import R::*; class Y specializes D; }\n}",
    ),
    (
        "invocation_result",
        "package P {\n behavior A { out y; }\n class C { feature z = A().y; }\n}",
    ),
    (
        "chain_through_own_subsetting",
        "package P {\n class Person { feature asPresident : Person; }\n class Country { feature president : Person subsets president.asPresident; }\n}",
    ),
    (
        "effective_name",
        "package P {\n class A { feature value; }\n class B specializes A { feature redefines value { feature local; } }\n feature selected : B::value::local;\n}",
    ),
    (
        "implicit_end",
        "package P {\n assoc A { end feature sourceEnd { feature local; } end feature targetEnd; }\n assoc B specializes A { end feature one { feature selected redefines local; } end feature two; }\n}",
    ),
    (
        "implicit_result",
        "package P {\n function A { return result { feature local; } }\n function B specializes A { return : Base::Anything; }\n feature selected : B::result;\n}",
    ),
    (
        "hidden_result",
        "package P {
 class T;
 function C { return T; }
 function D specializes C { return r : T; }
}",
    ),
    (
        "hidden_end",
        "package P {
 class e1;
 assoc A { end feature e1; end feature e2; }
 assoc B specializes A { end feature one : e1; end feature two; }
}",
    ),
    (
        "connector_end",
        "package P {\n class C {\n feature source; feature target;\n connector c : Links::BinaryLink {\n end feature transferSource references source;\n end feature transferTarget references target;\n }\n }\n}",
    ),
    (
        "chain_scope",
        "package P {\n class A { feature edges; feature that : A; }\n class B specializes A {\n feature redefines that;\n feature redefines edges subsets that.edges;\n }\n}",
    ),
    (
        "member_chain_ends",
        "package P {\n class A { feature b; feature d; }\n feature a : A; feature c : A;\n subset a.b subsets c.d;\n disjoint a.b from c.d;\n}",
    ),
    (
        "positional_parameter",
        "package P {\n behavior A { in index; }\n behavior Outer {\n in index;\n step s : A { in renamed = index; }\n }\n}",
    ),
    (
        "unresolved",
        "package P {\n feature selected : DeliberatelyMissing;\n}",
    ),
];
struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}
fn read(path: &str) -> Result<Source, String> {
    Ok(Source {
        uri: path.replace(' ', "%20"),
        text: std::fs::read_to_string(format!("{CORPUS}{path}")).map_err(|e| e.to_string())?,
    })
}
#[derive(Debug)]
struct Resolution {
    feature: String,
    spelling: String,
    targets: Vec<String>,
}
struct Probe {
    graph: Model,
    resolutions: Vec<Resolution>,
    unresolved: Vec<(String, String)>,
    seconds: f64,
}
type Probes = BTreeMap<&'static str, Result<Probe, String>>;
static SESSION: OnceLock<Result<NativePilotSession, String>> = OnceLock::new();
static ASSIGNMENTS: OnceLock<Result<Probes, String>> = OnceLock::new();
fn session() -> Result<&'static NativePilotSession, String> {
    SESSION
        .get_or_init(|| {
            let context = InvocationContext::new(Rc::new(Observer));
            let definitions = DEFINITIONS
                .iter()
                .map(|p| read(p))
                .collect::<Result<Vec<_>, _>>()?;
            NativePilotSession::prepare(&definitions, &context)
        })
        .as_ref()
        .map_err(Clone::clone)
}
static PROBES: OnceLock<Result<Probes, String>> = OnceLock::new();

// Preparation is shared once per suite binary. Per-probe timing includes
// construction, linking and defaults after the immutable programs and BSL
// construction have been prepared.
fn probes() -> Result<&'static Probes, String> {
    PROBES.get_or_init(prepare).as_ref().map_err(Clone::clone)
}
// Host-only cache, under the workspace target directory even though Cargo
// starts integration tests in the crate directory. Loading checks the exact
// compiler identity, bundle context and binding recipe, as the example does.
fn bindings(mm: &Metamodel) -> Result<(DefinitionBundle, Vec<VerifiedScope>), String> {
    use intersect_kerml::kerml_bindings;
    kerml_scopes::structure(mm.schema())?;
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/frozen-rule-probe-bindings.json");
    let bundle = kerml_bindings::bundle();
    if let Ok(text) = std::fs::read_to_string(&cache)
        && let Ok(scopes) = kerml_bindings::load(&text)
            .and_then(|scopes| kerml_bindings::check_recipe_results(&scopes).map(|()| scopes))
    {
        return Ok((bundle, scopes));
    }
    let (bundle, scopes) = kerml_scopes::compile(mm)?;
    let parent = cache.parent().ok_or("Binding cache has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = cache.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, kerml_bindings::save(&scopes)?).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, &cache).map_err(|e| e.to_string())?;
    Ok((bundle, scopes))
}

fn prepare() -> Result<Probes, String> {
    let start = Instant::now();
    let libraries = LIBRARIES
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;
    session()?.with_definitions(|definition, kerml_contracts, _, _| {
        let mm = intersect_kerml::language_pack::frozen_metamodel(definition)?;
        let (bundle, scopes) = bindings(&mm)?;
        let probes = pilot_languages::with_kerml(definition, kerml_contracts, |language| {
            // Immutable programs and the parsed BSL construction are preparation,
            // shared once. LinkView owns the per-model semantic memo tables.
            let resolver = RcoreScopes::new(&mm, &bundle, &scopes)
                .map_err(|e| e.to_string())?
                .with_execution(Execution::Fast);
            let mut library_store = FrozenStore::new(Arc::clone(&mm));
            for source in &libraries {
                let (evaluation, _, _) = language.parse(source, &mut library_store)?;
                if !evaluation.diagnostics.is_empty() {
                    return Err(format!("BSL: {:?}", evaluation.diagnostics));
                }
            }
            let library_graph = storage::export(&library_store)?;
            let library_records = library_store.resource_records()?;
            println!(
                "focused shared preparation (grammars, bindings, BSL): {:.3} s",
                start.elapsed().as_secs_f64()
            );
            let mut probes = BTreeMap::new();
            for (label, text) in CASES {
                assert!(text.lines().count() < 30);
                let result = (|| -> Result<Probe, String> {
                    let start = Instant::now();
                    let mut store = FrozenStore::new(Arc::clone(&mm));
                    // Restore the exact frozen-path construction, including source
                    // identities and resource records; no library is reparsed here.
                    storage::atomic(&mut store, |store| {
                        store.configure_schema(mm.schema())?;
                        for element in &library_graph.elements {
                            store.put(element.clone())?;
                        }
                        for record in library_records.values() {
                            store.set_resource_record(record.clone())?;
                        }
                        Ok(())
                    })?;
                    let source = Source {
                        uri: format!("probe/{label}.kerml"),
                        text: text.into(),
                    };
                    let (evaluation, _, _) = language.parse(&source, &mut store)?;
                    if !evaluation.diagnostics.is_empty() {
                        return Err(format!("{label}: {:?}", evaluation.diagnostics));
                    }
                    let mut graph = storage::export(&store)?;
                    let resolutions = RefCell::new(Vec::new());
                    let resolve = |view: &LinkView<'_>,
                                   owner: ObjId,
                                   feature: FeatureId,
                                   spelling: &str|
                     -> Result<Option<ObjId>, FrozenError> {
                        let candidates = resolver.candidates(view, owner, feature, spelling)?;
                        let ids = candidates
                            .iter()
                            .filter_map(|id| view.identity(*id).map(str::to_owned))
                            .collect::<Vec<_>>();
                        resolutions.borrow_mut().push((
                            view.identity(owner).unwrap_or_default().to_owned(),
                            mm.feature_name(feature).to_owned(),
                            spelling.to_owned(),
                            ids,
                        ));
                        Ok(if candidates.len() == 1 {
                            candidates.first().copied()
                        } else {
                            None
                        })
                    };
                    let linked = store
                        .into_builder()
                        .map_err(|e| e.to_string())?
                        .link(&resolve);
                    let unresolved = match linked {
                        Ok(linked) => {
                            // Exercise the same Rcore defaults and implicit construction
                            // as the example, rather than the legacy Rust policies.
                            let linked = linked
                                .complete_implicit(&resolver)
                                .map_err(|e| format!("{label} implicit: {e:?}"))?;
                            let linked = linked
                                .complete_defaults(&resolver)
                                .map_err(|e| format!("{label} defaults: {e:?}"))?;
                            match linked.validate() {
                                Ok(model) => {
                                    graph = model.to_model().map_err(|e| e.to_string())?;
                                }
                                Err(e) => return Err(format!("{label} validation: {e:?}")),
                            }
                            Vec::new()
                        }
                        Err(diagnostics) => {
                            let mut unresolved = Vec::new();
                            for diagnostic in diagnostics.0 {
                                match diagnostic {
                                    Violation::Unresolved {
                                        feature, spelling, ..
                                    } => unresolved.push((feature, spelling)),
                                    other => return Err(format!("{label}: {other:?}")),
                                }
                            }
                            unresolved
                        }
                    };
                    let resolutions = resolutions.into_inner();
                    let resolutions = resolutions
                        .into_iter()
                        .filter(|(owner, _, _, _)| {
                            graph
                                .elements
                                .iter()
                                .any(|e| e.id == *owner && e.resource.starts_with("probe/"))
                        })
                        .map(|(_, feature, spelling, targets)| Resolution {
                            feature,
                            spelling,
                            targets,
                        })
                        .collect();
                    let seconds = start.elapsed().as_secs_f64();
                    println!("focused KerML {label}: {seconds:.3} s");
                    Ok(Probe {
                        graph,
                        resolutions,
                        unresolved,
                        seconds,
                    })
                })();
                if let Err(error) = &result {
                    println!("{label}: {error}");
                }
                probes.insert(label, result);
            }
            Ok(probes)
        })?;
        Ok(probes)
    })
}
fn prepare_assignments() -> Result<Probes, String> {
    let context = InvocationContext::new(Rc::new(Observer));
    session()?.with_definitions(|_, _, sysml, contracts| {
        let mut probes = BTreeMap::new();
        let mm = intersect_kerml::language_pack::frozen_metamodel(sysml)?;
        pilot_languages::with_sysml(sysml, contracts, &context, |language| {
            for (label, target) in [("assignment_chain", "a.b"), ("assignment_index", "index")] {
                let result = (|| -> Result<Probe, String> {
                    let start = Instant::now();
                    let source = Source {
                        uri: format!("probe/{label}.sysml"),
                        text: format!("package P {{ action def A {{ assign {target} := 1; }} }}"),
                    };
                    let mut store = FrozenStore::new(Arc::clone(&mm));
                    let (evaluation, _, _) = language.parse(&source, &mut store)?;
                    if !evaluation.diagnostics.is_empty() {
                        return Err(format!("{label}: {:?}", evaluation.diagnostics));
                    }
                    let graph = storage::export(&store)?;
                    let seconds = start.elapsed().as_secs_f64();
                    println!("focused SysML {label}: {seconds:.3} s");
                    Ok(Probe {
                        graph,
                        resolutions: Vec::new(),
                        unresolved: Vec::new(),
                        seconds,
                    })
                })();
                if let Err(error) = &result {
                    println!("{label}: {error}");
                }
                probes.insert(label, result);
            }
            Ok(())
        })?;
        Ok(probes)
    })
}

pub(super) fn assignment_graphs() -> Result<[&'static Model; 2], String> {
    let graphs = ASSIGNMENTS
        .get_or_init(prepare_assignments)
        .as_ref()
        .map_err(Clone::clone)?;
    Ok([
        &checked_probe(graphs, "assignment_chain")?.graph,
        &checked_probe(graphs, "assignment_index")?.graph,
    ])
}

fn probe(label: &str) -> Result<&'static Probe, String> {
    checked_probe(probes()?, label)
}
fn checked_probe<'a>(probes: &'a Probes, label: &str) -> Result<&'a Probe, String> {
    let probe = probes
        .get(label)
        .ok_or_else(|| format!("Missing probe {label}"))?
        .as_ref()
        .map_err(Clone::clone)?;
    assert!(probe.seconds < 10.0, "{label}: {:.3} s", probe.seconds);
    Ok(probe)
}

// Test-only qualification of identities follows physical ownership, omitting
// relationship nodes. Unnamed redefining features use the redefined name;
// unnamed return parameters use the inherited result name in these fixtures.
fn qualified(graph: &Model, id: &str) -> Result<String, String> {
    fn walk(graph: &Model, id: &str, depth: usize) -> Result<Vec<String>, String> {
        if depth > 64 {
            return Err("QName containment cycle".into());
        }
        let element = graph
            .elements
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("Missing target {id}"))?;
        let parent = graph.elements.iter().find(|e| {
            ["ownedRelationship", "ownedRelatedElement"]
                .iter()
                .any(|f| {
                    e.values.get(*f).is_some_and(|v| {
                        v.as_array()
                            .is_some_and(|xs| xs.iter().any(|x| x.as_str() == Some(id)))
                            || v.as_str() == Some(id)
                    })
                })
        });
        let mut names = match parent {
            Some(p) => walk(graph, &p.id, depth + 1)?,
            None => Vec::new(),
        };
        if let Some(name) = element.values.get("declaredName").and_then(|v| v.as_str()) {
            names.push(name.into());
        } else if element.class.ends_with("Feature") {
            let redefinition = graph.elements.iter().find(|r| {
                r.class.ends_with("Redefinition")
                    && r.values.get("redefiningFeature").and_then(|v| v.as_str()) == Some(id)
            });
            if let Some(target) = redefinition
                .and_then(|r| r.values.get("redefinedFeature"))
                .and_then(|v| v.as_str())
            {
                let inherited = walk(graph, target, depth + 1)?;
                if let Some(name) = inherited.last() {
                    names.push(name.clone());
                }
            } else if parent.is_some_and(|p| p.class.ends_with("ReturnParameterMembership")) {
                names.push("result".into());
            }
        }
        Ok(names)
    }
    Ok(walk(graph, id, 0)?.join("::"))
}
fn resolves(probe: &Probe, feature: &str, spelling: &str, expected: &str) -> Result<(), String> {
    assert!(probe.unresolved.is_empty(), "{:?}", probe.unresolved);
    let matching = probe
        .resolutions
        .iter()
        .filter(|r| r.feature == feature && r.spelling == spelling)
        .collect::<Vec<_>>();
    assert!(
        !matching.is_empty(),
        "No {feature} '{spelling}': {:?}",
        probe.resolutions
    );
    for resolution in matching {
        assert_eq!(resolution.targets.len(), 1, "{resolution:?}");
        assert_eq!(
            qualified(&probe.graph, &resolution.targets[0])?,
            expected,
            "{resolution:?}"
        );
    }
    Ok(())
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn unnamed_redefinition_effective_name() -> Result<(), String> {
    resolves(
        probe("effective_name")?,
        "type",
        "B::value::local",
        "P::B::value::local",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn implicit_end_redefinition() -> Result<(), String> {
    resolves(
        probe("implicit_end")?,
        "redefinedFeature",
        "local",
        "P::A::sourceEnd::local",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn implicit_result_redefinition() -> Result<(), String> {
    resolves(
        probe("implicit_result")?,
        "type",
        "B::result",
        "P::B::result",
    )
}
// An owned result parameter or end without an owned redefinition redefines
// the general's at its position (the Pilot's computed redefinitions), which
// is then not inherited: `T` and `e1` name the package's class, as in the
// Pilot's CalculationUsage_Invalid2.sysml.
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn implicitly_redefined_result_hidden() -> Result<(), String> {
    resolves(probe("hidden_result")?, "type", "T", "P::T")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn implicitly_redefined_end_hidden() -> Result<(), String> {
    resolves(probe("hidden_end")?, "type", "e1", "P::e1")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn connector_end_scope() -> Result<(), String> {
    let p = probe("connector_end")?;
    resolves(p, "referencedFeature", "source", "P::C::source")?;
    resolves(p, "referencedFeature", "target", "P::C::target")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn feature_chain_scope() -> Result<(), String> {
    let p = probe("chain_scope")?;
    resolves(p, "chainingFeature", "that", "P::B::that")?;
    resolves(p, "chainingFeature", "edges", "P::A::edges")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn member_relationship_chain_ends() -> Result<(), String> {
    let p = probe("member_chain_ends")?;
    resolves(p, "chainingFeature", "a", "P::a")?;
    resolves(p, "chainingFeature", "b", "P::A::b")?;
    resolves(p, "chainingFeature", "c", "P::c")?;
    resolves(p, "chainingFeature", "d", "P::A::d")?;
    for (class, source, target) in [
        ("Subsetting", "subsettingFeature", "subsettedFeature"),
        ("Disjoining", "typeDisjoined", "disjoiningType"),
    ] {
        let relationship = p
            .graph
            .elements
            .iter()
            .find(|e| e.resource.starts_with("probe/") && e.class.ends_with(class))
            .ok_or_else(|| format!("Missing {class}"))?;
        for (feature, expected) in [(source, ["P::a", "P::A::b"]), (target, ["P::c", "P::A::d"])] {
            let id = relationship
                .values
                .get(feature)
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("{class}.{feature} missing: {relationship:?}"))?;
            let chain = p
                .graph
                .elements
                .iter()
                .find(|e| e.id == id)
                .ok_or("Missing chain")?;
            let memberships = chain
                .values
                .get("ownedRelationship")
                .and_then(|v| v.as_array())
                .ok_or("Chain has no relationships")?;
            let targets = memberships
                .iter()
                .filter_map(|m| {
                    p.graph
                        .elements
                        .iter()
                        .find(|e| Some(e.id.as_str()) == m.as_str())
                })
                .filter_map(|r| r.values.get("chainingFeature").and_then(|v| v.as_str()))
                .map(|id| qualified(&p.graph, id))
                .collect::<Result<Vec<_>, _>>()?;
            assert_eq!(targets, expected, "{class}.{feature}");
        }
    }
    Ok(())
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn parameter_redefined_by_position() -> Result<(), String> {
    resolves(
        probe("positional_parameter")?,
        "memberElement",
        "index",
        "P::Outer::index",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn featuring_type_declared_in_own_body() -> Result<(), String> {
    resolves(
        probe("featured_by_member")?,
        "featuringType",
        "g",
        "P::f::g",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn global_scope_qualification() -> Result<(), String> {
    resolves(
        probe("global_qualification")?,
        "superclassifier",
        "$::Base::Anything",
        "Base::Anything",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn recursive_import_reaches_nested_members() -> Result<(), String> {
    resolves(
        probe("recursive_import")?,
        "superclassifier",
        "D",
        "P::Q::R::D",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn import_resolved_through_earlier_import() -> Result<(), String> {
    let p = probe("earlier_import")?;
    resolves(p, "importedNamespace", "R", "P::Q::R")?;
    resolves(p, "superclassifier", "D", "P::Q::R::D")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn invocation_result_members() -> Result<(), String> {
    resolves(probe("invocation_result")?, "memberElement", "y", "P::A::y")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn chain_skips_its_own_subsetting() -> Result<(), String> {
    resolves(
        probe("chain_through_own_subsetting")?,
        "chainingFeature",
        "asPresident",
        "P::Person::asPresident",
    )
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn chain_named_sibling_is_not_matched_while_resolving() -> Result<(), String> {
    let p = probe("chain_named_sibling")?;
    resolves(p, "type", "B", "P::B")?;
    resolves(p, "chainingFeature", "s", "P::B::s")
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn unresolved_name_diagnostic() -> Result<(), String> {
    let p = probe("unresolved")?;
    assert_eq!(
        p.unresolved,
        [("type".into(), "DeliberatelyMissing".into())]
    );
    assert!(
        p.resolutions
            .iter()
            .any(|r| r.spelling == "DeliberatelyMissing" && r.targets.is_empty())
    );
    Ok(())
}
