//! Handwritten host transport for the frozen-engine KerML load
//! (`intersect_kerml::frozen_load`). KerML owns the definition preparation,
//! the parse, the linking and the diagnostics; this module only bounds the
//! JSON envelope and reports what the load returned.
//!
//! `draft/kerml/analyze` parses one source; `draft/kerml/project/analyze`
//! (mode `frozen-kerml-load.v1`) parses up to eight sources into one store.
//! The result is the unlinked construction (references stay `?spelling`)
//! with the parse diagnostics. Linking needs the verified KerML Rcore
//! bindings (`intersect_kerml::frozen_load::load_kerml`); they take minutes
//! to compile and their saved form (about 13 MB) exceeds this host's request
//! envelope, so this transport does not link yet.
use intersect_kerml::{
    frozen_load::{self, LoadDiagnostic},
    native_pilot::NativeKerMLSession,
};
use intersect_model::{
    core::query::QueryBudget,
    definitions::conversion::{InvocationContext, InvocationObserver, ProviderId},
};
use intersect_text::Source;
use intersect_text::package::BundleCompilation;
use serde::Deserialize;
use serde_json::{Value, json};
use std::rc::Rc;

pub(crate) const ANALYZE: &str = "draft/kerml/analyze";
pub(crate) const PROJECT: &str = "draft/kerml/project/analyze";
pub(crate) const PROJECT_MODE: &str = "frozen-kerml-load.v1";
const ORIGINAL_BYTES: usize = 1_000_000;
const MODEL_BYTES: usize = 256_000;
const RESOURCES: usize = 8;
const RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// Default and largest parse work of one request, all sources together.
const WORK: usize = 4_000_000_000;
/// The three original definitions the KerML owner is prepared from; the
/// fourth original source (`probe-behavior.rcore`) is the empty behavior the
/// preparation supplies itself.
const DEFINITIONS: [&str; 3] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzeRequest {
    method: String,
    original: BundleCompilation,
    source: Source,
    #[serde(default)]
    work: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectRequest {
    method: String,
    version: u32,
    mode: String,
    original: BundleCompilation,
    sources: Vec<Source>,
    #[serde(default)]
    work: Option<usize>,
}

/// Preparation compiles the original grammar and migration; it invokes only
/// the definition compiler's own providers.
struct Preparation;
impl InvocationObserver for Preparation {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

/// Bound the borrowed original definitions before deserializing them.
fn check_original(value: &Value) -> Result<(), String> {
    let sources = value["original"]["bundle"]["sources"]
        .as_array()
        .ok_or("Missing original definition sources")?;
    if sources.len() != 4 {
        return Err("KerML load requires exactly four original definition sources".into());
    }
    let mut bytes = 0usize;
    for source in sources {
        let text = source["text"]
            .as_str()
            .ok_or("Missing original source text")?;
        bytes = bytes
            .checked_add(text.len())
            .ok_or("Original source byte overflow")?;
        if bytes > ORIGINAL_BYTES {
            return Err("KerML load original source byte budget exceeded".into());
        }
    }
    Ok(())
}

/// Bound the borrowed model sources before deserializing them.
fn check_sources(sources: &[Value]) -> Result<(), String> {
    if sources.is_empty() || sources.len() > RESOURCES {
        return Err("KerML load resource count budget exceeded".into());
    }
    let mut bytes = 0usize;
    for (position, source) in sources.iter().enumerate() {
        let uri = source["uri"]
            .as_str()
            .filter(|uri| !uri.is_empty())
            .ok_or("Missing KerML source URI")?;
        if sources[..position]
            .iter()
            .any(|previous| previous["uri"].as_str() == Some(uri))
        {
            return Err("Duplicate KerML source URI".into());
        }
        bytes = bytes
            .checked_add(
                source["text"]
                    .as_str()
                    .ok_or("Missing KerML source text")?
                    .len(),
            )
            .ok_or("KerML source byte count overflow")?;
        if bytes > MODEL_BYTES {
            return Err("KerML load model source byte budget exceeded".into());
        }
    }
    Ok(())
}

fn work(requested: Option<usize>) -> Result<usize, String> {
    let work = requested.unwrap_or(WORK);
    if work > WORK {
        return Err("KerML load work grant exceeds host policy".into());
    }
    Ok(work)
}

/// The three definition sources of `original`, by their original URIs.
fn definitions(original: &BundleCompilation) -> Result<Vec<Source>, String> {
    DEFINITIONS
        .iter()
        .map(|uri| {
            original
                .bundle
                .sources
                .iter()
                .find(|source| source.id.uri == *uri)
                .map(|source| Source {
                    uri: source.id.uri.clone(),
                    text: source.text.clone(),
                })
                .ok_or_else(|| format!("Missing original definition source {uri}"))
        })
        .collect()
}

/// Prepare the KerML owner from `original` and construct `sources`.
fn construct(
    original: &BundleCompilation,
    sources: &[Source],
    grant: usize,
) -> Result<(intersect_model::core::model::Model, Vec<LoadDiagnostic>), String> {
    let definitions = definitions(original)?;
    let preparation = InvocationContext::new(Rc::new(Preparation));
    let session = NativeKerMLSession::prepare(&definitions, &preparation)?;
    session.with_definition(|definition, contracts| {
        let mut caller = QueryBudget::new(grant);
        frozen_load::construct_kerml(
            definition,
            contracts,
            sources,
            frozen_load::limits(),
            &mut caller,
        )
    })
}

fn respond(
    extra: Value,
    sources: &[Source],
    grant: usize,
    (model, diagnostics): (intersect_model::core::model::Model, Vec<LoadDiagnostic>),
) -> Result<Value, String> {
    let mut response = json!({
        "version": 1,
        "kind": "frozen_kerml_load",
        "sources": sources.iter().map(|source| &source.uri).collect::<Vec<_>>(),
        "linked": false,
        "language_validated": false,
        "publication_permitted": false,
        "model": model,
        "diagnostics": diagnostics,
        "work_ceiling": grant,
    });
    if let (Some(object), Value::Object(extra)) = (response.as_object_mut(), extra) {
        object.extend(extra);
    }
    if serde_json::to_vec(&response)
        .map_err(|error| error.to_string())?
        .len()
        > RESPONSE_BYTES
    {
        return Err("KerML load encoded response byte budget exceeded".into());
    }
    Ok(response)
}

/// `draft/kerml/analyze`: the construction of one source.
pub(crate) fn analyze(value: &Value) -> Result<Value, String> {
    check_original(value)?;
    check_sources(std::slice::from_ref(&value["source"]))?;
    let request: AnalyzeRequest = serde_json::from_value(value.clone())
        .map_err(|error| format!("Invalid KerML load request: {error}"))?;
    if request.method != ANALYZE {
        return Err("Unsupported KerML load method".into());
    }
    let grant = work(request.work)?;
    let sources = [request.source];
    let construction = construct(&request.original, &sources, grant)?;
    respond(json!({ "method": ANALYZE }), &sources, grant, construction)
}

/// `draft/kerml/project/analyze`: the construction of several sources into
/// one store.
pub(crate) fn analyze_project(value: &Value) -> Result<Value, String> {
    if value["version"].as_u64() != Some(1) || value["mode"].as_str() != Some(PROJECT_MODE) {
        return Err("Unsupported KerML project version or mode".into());
    }
    check_original(value)?;
    check_sources(
        value["sources"]
            .as_array()
            .ok_or("Missing KerML project sources")?,
    )?;
    let request: ProjectRequest = serde_json::from_value(value.clone())
        .map_err(|error| format!("Invalid KerML project request: {error}"))?;
    if request.method != PROJECT || request.version != 1 || request.mode != PROJECT_MODE {
        return Err("KerML project context changed during decoding".into());
    }
    let grant = work(request.work)?;
    let construction = construct(&request.original, &request.sources, grant)?;
    respond(
        json!({ "method": PROJECT, "mode": PROJECT_MODE }),
        &request.sources,
        grant,
        construction,
    )
}
