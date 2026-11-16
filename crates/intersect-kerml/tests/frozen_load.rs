//! `intersect_kerml::frozen_load`: the frozen-engine KerML load as a library
//! function. Both tests prepare the KerML owner and need the Rcore bindings,
//! so they are milestone-only.
use intersect_kerml::{
    frozen_load::{self, KerMLBindings, LoadOptions, LoadPhase},
    kerml_bindings,
    native_pilot::NativeKerMLSession,
};
use intersect_model::definitions::conversion::{InvocationContext, InvocationObserver, ProviderId};
use intersect_text::Source;
use std::rc::Rc;

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

const CORPUS: &str = "../../fixtures/native-pilot/v1/sources";

fn read(path: &str) -> Result<Source, String> {
    let text = std::fs::read_to_string(format!("{CORPUS}/{path}"))
        .map_err(|error| format!("{path}: {error}"))?;
    Ok(Source {
        uri: path.replace(' ', "%20"),
        text,
    })
}

/// The bindings a host saved (`target/kerml-bindings.json`, as the examples
/// write it) when they still fit this compiler, else compiled here on 64 MiB
/// stacks (minutes).
fn bindings() -> Result<KerMLBindings, String> {
    let cache = std::env::var("KERML_BINDING_CACHE")
        .unwrap_or_else(|_| "../../target/kerml-bindings.json".into());
    if let Ok(text) = std::fs::read_to_string(&cache)
        && let Ok(bindings) = KerMLBindings::load_saved(&text)
    {
        return Ok(bindings);
    }
    let bundle = kerml_bindings::bundle();
    let jobs = kerml_bindings::binding_jobs(&bundle)?;
    let scopes = std::thread::scope(|threads| {
        let handles = jobs
            .iter()
            .map(|job| {
                std::thread::Builder::new()
                    .stack_size(64 * 1024 * 1024)
                    .spawn_scoped(threads, || {
                        kerml_bindings::compile_job(&bundle, job, &|_| {})
                    })
                    .map_err(|error| error.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "binding compiler panicked".to_owned())?
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    KerMLBindings::new(scopes.into_iter().flatten().collect())
}

fn with_definition<T>(
    body: impl FnOnce(
        &intersect_text::preparation::PreparedLanguageDefinition<'_>,
        &[intersect_model::definitions::setting::ComputedSettingContract],
    ) -> Result<T, String>,
) -> Result<T, String> {
    let definitions = [
        "org.omg.sysml/model/SysML.ecore",
        "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
        "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    ]
    .iter()
    .map(|path| read(path))
    .collect::<Result<Vec<_>, _>>()?;
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativeKerMLSession::prepare(&definitions, &preparation)?;
    session.with_definition(body)
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn base_scalar_values_and_links_load_and_link() -> Result<(), String> {
    let bindings = bindings()?;
    let sources = [
        "sysml.library/Kernel Libraries/Kernel Semantic Library/Base.kerml",
        "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml",
        "sysml.library/Kernel Libraries/Kernel Semantic Library/Links.kerml",
    ]
    .iter()
    .map(|path| read(path))
    .collect::<Result<Vec<_>, _>>()?;
    with_definition(|definition, contracts| {
        // The two implicit values no construction rule supplies yet, as the
        // library probe delegates them.
        let options = LoadOptions {
            delegated: vec!["subsettingFeature".into(), "typeDisjoined".into()],
            ..LoadOptions::default()
        };
        let load = frozen_load::load_kerml(definition, contracts, &bindings, &sources, &options)?;
        assert!(load.references > 0);
        assert_eq!(load.resolved, load.references, "{:?}", load.diagnostics);
        // This subset leaves out Performances.kerml, which types literal
        // expressions (`literalEvaluations`), so the literal bounds of
        // multiplicities (`[1]`, `[1..*]`) have no type and the Pilot rule
        // validateFeatureHasType_ reports them. The whole library reports
        // no diagnostic. Every other diagnostic is a failure.
        for diagnostic in &load.diagnostics {
            let text = match (&diagnostic.resource, diagnostic.start, diagnostic.end) {
                (Some(resource), Some(start), Some(end)) => {
                    let path = format!("{CORPUS}/{}", resource.replace("%20", " "));
                    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
                    String::from_utf8_lossy(bytes.get(start..end).unwrap_or_default()).into_owned()
                }
                _ => String::new(),
            };
            assert!(
                diagnostic.phase == LoadPhase::Check
                    && diagnostic.code.as_deref() == Some("validateFeatureHasType_")
                    && (text == "*"
                        || (!text.is_empty() && text.chars().all(|c| c.is_ascii_digit()))),
                "unexpected diagnostic {diagnostic:?} on {text:?}"
            );
        }
        let model = load.model.ok_or("no validated model")?;
        assert!(model.elements.len() > 200);
        Ok(())
    })
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn unresolved_names_and_parse_errors_are_located_diagnostics() -> Result<(), String> {
    let bindings = bindings()?;
    let text = "package P { classifier A :> Missing; }";
    let sources = [
        Source {
            uri: "memory://p.kerml".into(),
            text: text.into(),
        },
        Source {
            uri: "memory://q.kerml".into(),
            text: "package Q { classifier C }".into(),
        },
    ];
    with_definition(|definition, contracts| {
        let load = frozen_load::load_kerml(
            definition,
            contracts,
            &bindings,
            &sources,
            &LoadOptions::default(),
        )?;
        assert!(load.model.is_none());
        assert!(
            load.diagnostics.iter().any(|d| d.phase == LoadPhase::Parse
                && d.resource.as_deref() == Some("memory://q.kerml"))
        );
        let link = load
            .diagnostics
            .iter()
            .find(|d| d.phase == LoadPhase::Link && d.message.contains("Missing"))
            .ok_or("no link diagnostic for Missing")?;
        assert_eq!(link.resource.as_deref(), Some("memory://p.kerml"));
        assert!(link.start.is_some_and(|start| start < text.len()));
        Ok(())
    })
}
