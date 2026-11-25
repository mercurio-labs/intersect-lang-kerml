//! Handwritten KerML/SysML project IDE and visualization facade.
//!
//! This facade binds the existing pack, frozen SysML loader and KPAR resolver.
//! The bounded view/editing profile is documented in docs/visualization.md.
use intersect_kerml::{
    language_pack::{self, LibraryOptions, LibraryOrigin},
    native_pilot::NativePilotSession,
    sysml_host::{SysmlBindings, with_sysml_host},
};
use intersect_model::{
    core::frozen::{FrozenStore, Model, Obj, SnapshotTrust, ValueRef},
    definitions::{
        bundle::DefinitionBundle,
        conversion::{InvocationContext, InvocationObserver, ProviderId},
        library_part::decode_library_part,
        pack::{LanguagePack, SectionCheck},
        rcore_scope::VerifiedScope,
    },
};
pub use intersect_text::Source;
use intersect_text::grammar::{RuntimeAdmissionLoadMode, RuntimePredictionMemoMode};
pub use intersect_views::{Palette, PrepareCommand, PreparedEdit, SceneResponse, ViewCatalog};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
mod completion;
#[cfg(feature = "components")]
mod components;
mod editing;
mod evaluation;
mod navigation;
mod view_commands;
mod view_loading;
#[path = "view_engine.rs"]
mod views;

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
pub struct Host {
    documents: editing::Documents,
    loaded: Option<Rc<Loaded>>,
    views: Option<intersect_view_host::Host>,
    view_presentation_fingerprint: String,
    /// Shipped package identities whose saved layouts this package accepts, oldest first.
    shipped_view_presentation_fingerprints: Vec<String>,
    project: Rc<RefCell<view_loading::ProjectEnvironment>>,
    #[cfg(test)]
    fixture_model: Option<Model>,
    sources: BTreeMap<String, String>,
    revision: u64,
    last_analysis: Option<Value>,
    #[cfg(feature = "components")]
    components: components::State,
    #[cfg(feature = "components")]
    mount_transaction: u64,
}

struct Loaded {
    session: NativePilotSession,
    bundle: DefinitionBundle,
    scopes: Vec<VerifiedScope>,
    library: Model,
    texts: BTreeMap<String, String>,
    identity: String,
    #[cfg(feature = "components")]
    library_info: Value,
    #[cfg(feature = "components")]
    standard_component_payload: Option<Vec<u8>>,
}

#[derive(Clone, Deserialize)]
struct ProjectRequest {
    sources: Vec<Source>,
    #[serde(default, rename = "projectNodesOnly")]
    project_nodes_only: bool,
    #[serde(default)]
    descriptor: Option<String>,
    #[serde(default)]
    archives: Vec<ArchiveInput>,
    revision: u64,
    #[cfg(feature = "components")]
    #[serde(default, rename = "componentUsages")]
    component_usages: Vec<String>,
}
#[derive(Clone, Deserialize)]
struct ArchiveInput {
    uri: String,
    base64: String,
}

impl Host {
    /// The same bounded provider used by `views/catalog`, available to native
    /// hosts without a JSON-RPC envelope. No filesystem or transport is owned.
    pub fn view_catalog(&self, revision: u64) -> Result<ViewCatalog, String> {
        self.check_revision(&json!({"revision":revision}))?;
        serde_json::from_value(self.views_catalog()?)
            .map_err(|e| format!("View catalog contract: {e}"))
    }
    pub fn view_scene(
        &self,
        revision: u64,
        view_key: &str,
        palette: Option<&Palette>,
    ) -> Result<SceneResponse, String> {
        serde_json::from_value(
            self.views_scene(&json!({"revision":revision,"viewKey":view_key,"palette":palette}))?,
        )
        .map_err(|e| format!("Scene contract: {e}"))
    }
    pub fn prepare_view_command(
        &mut self,
        command: PrepareCommand,
    ) -> Result<PreparedEdit, String> {
        serde_json::from_value(self.prepare_command(command)?)
            .map_err(|e| format!("Prepared edit contract: {e}"))
    }
    pub fn commit_view_command(
        &mut self,
        revision: u64,
        token: &str,
        sources: &[Source],
    ) -> Result<Value, String> {
        self.commit_command(&json!({"revision":revision,"token":token,"sources":sources}))
    }

    pub fn load_pack(&mut self, bytes: Vec<u8>) -> Result<Value, String> {
        self.load_pack_with_projection(bytes, true, &BTreeMap::new())
    }

    /// Where the sections this host reads lie in pack `bytes`:
    /// `[{"name", "start", "bytes"}]`, offsets from the start of the pack.
    /// An embedder with a faster SHA-256 than this build has (WebCrypto next
    /// to WebAssembly) hashes these ranges and passes the digests to
    /// `load_pack_with_digests`.
    pub fn pack_sections(bytes: &[u8]) -> Result<Value, String> {
        let ranges =
            LanguagePack::section_ranges(bytes).map_err(|e| format!("Language pack: {e}"))?;
        Ok(Value::Array(
            ranges
                .into_iter()
                .filter(|range| range.name != language_pack::BINDINGS_SECTION)
                .map(
                    |range| json!({"name": range.name, "start": range.start, "bytes": range.bytes}),
                )
                .collect(),
        ))
    }

    /// `load_pack` with the SHA-256 (lowercase hex) of sections, by name, that
    /// the embedder computed over exactly the ranges `pack_sections` gave of
    /// exactly these `bytes`. Each digest is compared with the pack manifest
    /// as a digest computed here would be; the embedder supplies the bytes
    /// and is trusted for their hash alike. A section without a digest is
    /// hashed here.
    pub fn load_pack_with_digests(
        &mut self,
        bytes: Vec<u8>,
        digests: &BTreeMap<String, String>,
    ) -> Result<Value, String> {
        self.load_pack_with_projection(bytes, true, digests)
    }

    /// Initialize an analysis-only host without building the library's optional
    /// presentation tree. Parsing, linking and the retained library are identical.
    pub fn load_pack_for_analysis(&mut self, bytes: Vec<u8>) -> Result<Value, String> {
        self.load_pack_with_projection(bytes, false, &BTreeMap::new())
    }

    fn load_pack_with_projection(
        &mut self,
        bytes: Vec<u8>,
        include_library_nodes: bool,
        digests: &BTreeMap<String, String>,
    ) -> Result<Value, String> {
        self.load_pack_reporting(bytes, include_library_nodes, digests, &mut |_| {})
    }

    /// Load with phase notifications for an explicit host-owned progress adapter.
    /// The callback supplies no schema, cache or model authority.
    pub fn load_pack_with_progress(
        &mut self,
        bytes: Vec<u8>,
        progress: &mut dyn FnMut(&'static str),
    ) -> Result<Value, String> {
        self.load_pack_reporting(bytes, true, &BTreeMap::new(), progress)
    }

    fn load_pack_reporting(
        &mut self,
        bytes: Vec<u8>,
        include_library_nodes: bool,
        digests: &BTreeMap<String, String>,
        progress: &mut dyn FnMut(&'static str),
    ) -> Result<Value, String> {
        let (loaded, result) = Loaded::from_pack(bytes, include_library_nodes, digests, progress)?;
        progress("view-functions");
        let project = Rc::new(RefCell::new(view_loading::ProjectEnvironment::default()));
        let (views, view_presentation_fingerprint, shipped_view_presentation_fingerprints) =
            views::host(loaded.clone(), project.clone())?;
        progress("publish");
        self.loaded = Some(loaded);
        self.views = Some(views);
        self.view_presentation_fingerprint = view_presentation_fingerprint;
        self.shipped_view_presentation_fingerprints = shipped_view_presentation_fingerprints;
        self.project = project;
        #[cfg(feature = "components")]
        {
            self.components = components::State::default();
        }
        self.sources.clear();
        self.last_analysis = None;
        Ok(result)
    }

    pub fn dispatch(&mut self, request: Value) -> Result<Value, String> {
        #[cfg(feature = "components")]
        if let Some(result) = self.component_dispatch(&request) {
            return result;
        }
        let method = request["method"]
            .as_str()
            .ok_or("Request has no method")?
            .to_owned();
        if method == "views/scene" {
            return self.views_scene(&request["params"]);
        }
        if method == "views/presentationCompatibility" {
            if self.views.is_none() {
                return Err("Load a language pack first".into());
            }
            // `previous` stays the Wave 1 identity for editors that only migrate
            // v4 layouts. `upgradable` lists every shipped identity.
            return Ok(json!({
                "previous": self.shipped_view_presentation_fingerprints.first(),
                "upgradable": self.shipped_view_presentation_fingerprints,
                "current": self.view_presentation_fingerprint,
            }));
        }
        if method.starts_with("views/") {
            return self.view_dispatch(request);
        }
        let params = &request["params"];
        match method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion":1,"language":{"id":"sysml","name":"KerML / SysML","fileExtensions":["sysml","kerml"]},
                "capabilities": {"positionEncoding":"utf-16", "textDocumentSync":{"openClose":true,"change":1},
                    "hoverProvider":true, "documentSymbolProvider":true,
                    "definitionProvider":true, "completionProvider":{"resolveProvider":false,"triggerCharacters":[".",":"]}},
                "serverInfo":{"name":"Intersect KerML IDE","version":"0.1.0"},
                "limitations":["Reference completion uses bounded parser probes and the language's existing scope semantics; unsupported grammar contexts retain lexical suggestions."],
            })),
            "textDocument/didOpen" => self.documents.open(params),
            "textDocument/didChange" => self.documents.change(params),
            "textDocument/didClose" => self.documents.close(params),
            "textDocument/completion" => self.complete(params),
            "textDocument/definition" => self.definition(params),
            "evaluation/sites" => self.evaluation_sites(params),
            "evaluation/evaluate" => self.evaluate_site(params),
            "workspace/analyze" => match serde_json::from_value(params.clone()) {
                Ok(request) => self.analyze(request),
                Err(error) => {
                    if let Some(views) = &mut self.views {
                        views.invalidate();
                    }
                    self.last_analysis = None;
                    Err(format!("Invalid workspace request: {error}"))
                }
            },
            "intersect/inspect" => {
                let id = params["id"].as_str().ok_or("Missing element id")?;
                let model = self.model()?;
                let object = model
                    .lookup(id)
                    .ok_or("Element no longer exists in the current model")?;
                let materialized = model
                    .metamodel()
                    .class_info(object.class())
                    .ok_or("Missing class metadata")?;
                let values: BTreeMap<_, _> = materialized
                    .stored
                    .iter()
                    .filter_map(|f| {
                        object
                            .get(*f)
                            .map(|v| (model.metamodel().feature_name(*f), value_json(model, v)))
                    })
                    .collect();
                Ok(
                    json!({"id":id,"class":object.class_key(),"resource":object.resource(),
                    "range":self.range(object),"values":values,"revision":self.revision}),
                )
            }
            "intersect/source" => {
                let uri = params["uri"].as_str().ok_or("Missing source URI")?;
                let text = self
                    .source(uri)
                    .ok_or_else(|| format!("No source text for {uri}"))?;
                Ok(json!({"uri":uri,"text":text}))
            }
            "textDocument/hover" => {
                self.check_revision(params)?;
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .ok_or("Missing document URI")?;
                let source = self.source(uri).ok_or("Unknown source")?;
                let offset = position_byte(source, &params["position"])?;
                let object = self
                    .model()?
                    .objects()
                    .filter(|o| {
                        let (start, end) = o.span();
                        o.resource() == uri && start <= offset && offset < end && name(*o).is_some()
                    })
                    .min_by_key(|o| o.span().1.saturating_sub(o.span().0));
                Ok(object.map_or(Value::Null, |o| json!({
                    "contents":{"kind":"plaintext","value":format!("{} {}\n{}",o.class_key(),name(o).unwrap_or(""),o.resource())},
                    "range":self.range(o),
                })))
            }
            "textDocument/documentSymbol" => {
                self.check_revision(params)?;
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .ok_or("Missing document URI")?;
                Ok(Value::Array(
                    self.model()?
                        .objects()
                        .filter(|o| o.resource() == uri)
                        .filter_map(|o| {
                            name(o).map(|n| {
                                json!({"name":n,"detail":o.class_key(),
                        "kind":5,"range":self.range(o),"selectionRange":self.range(o)})
                            })
                        })
                        .collect(),
                ))
            }
            "shutdown" => {
                self.documents = editing::Documents::default();
                self.views = None;
                self.loaded = None;
                #[cfg(feature = "components")]
                {
                    self.components = components::State::default();
                }
                self.sources.clear();
                self.last_analysis = None;
                Ok(Value::Null)
            }
            method => Err(format!("Unsupported language method: {method}")),
        }
    }

    fn check_revision(&self, params: &Value) -> Result<(), String> {
        if params["revision"].as_u64() != Some(self.revision) {
            return Err("Language model is stale; wait for the current analysis".into());
        }
        Ok(())
    }
    fn project_model(&self) -> Option<&Model> {
        #[cfg(test)]
        if let Some(model) = &self.fixture_model {
            return Some(model);
        }
        self.views.as_ref().and_then(|v| v.accepted_model())
    }
    fn model(&self) -> Result<&Model, String> {
        #[cfg(feature = "components")]
        if !self.components.usages.is_empty() {
            return Err(
                "This operation is not available for mounted component projects yet".into(),
            );
        }
        self.project_model()
            .or_else(|| self.loaded.as_ref().map(|l| &l.library))
            .ok_or_else(|| "Load a compatible language pack first".into())
    }
    fn source(&self, uri: &str) -> Option<&str> {
        self.sources
            .get(uri)
            .map(String::as_str)
            .or_else(|| self.views.as_ref()?.source(uri))
            .or_else(|| self.loaded.as_ref()?.texts.get(uri).map(String::as_str))
    }
    fn range(&self, object: Obj<'_>) -> Value {
        let (start, end) = object.span();
        let text = self.source(object.resource()).unwrap_or("");
        json!({"start":byte_position(text,start),"end":byte_position(text,end)})
    }

    fn analyze(&mut self, request: ProjectRequest) -> Result<Value, String> {
        #[cfg(feature = "components")]
        if !request.component_usages.is_empty() {
            return self.analyze_components(request);
        }
        #[cfg(feature = "components")]
        {
            self.components.clear_project();
        }
        if let Some(views) = &mut self.views {
            views.invalidate();
        }
        self.revision = request.revision;
        self.last_analysis = None;
        self.sources = request
            .sources
            .iter()
            .map(|s| (s.uri.clone(), s.text.clone()))
            .collect();
        *self.project.try_borrow_mut().map_err(|e| e.to_string())? =
            view_loading::ProjectEnvironment {
                descriptor: request.descriptor,
                archives: request.archives,
                #[cfg(feature = "components")]
                mounted: None,
            };
        if self.views.is_none() {
            // Resolve descriptor/archives through the same language-owned code,
            // preserving useful errors for editor sessions without a pack.
            let project = self.project.try_borrow().map_err(|e| e.to_string())?;
            view_loading::resolve_project(&project, &request.sources)?;
            return Err("Load a language pack to analyze SysML/KerML".into());
        }
        let mut result=self.views.as_mut().ok_or("Load a language pack first")?.dispatch(json!({
            "method":"workspace/analyze","params":{"revision":request.revision,"sources":request.sources,"projectNodesOnly":request.project_nodes_only}
        }))?;
        self.sync_analysis(&mut result)?;
        Ok(result)
    }
    fn sync_analysis(&mut self, analysis: &mut Value) -> Result<(), String> {
        #[cfg(feature = "components")]
        if let Some(mounted) = &self.project.borrow().mounted {
            mounted.enrich(analysis);
        }
        self.revision = analysis["revision"]
            .as_u64()
            .ok_or("Analysis has no revision")?;
        self.sources = view_loading::project_buffers(
            self.views.as_ref().ok_or("No shared view host")?,
            analysis,
        );
        self.last_analysis = Some(analysis.clone());
        Ok(())
    }
}

impl Loaded {
    fn from_pack(
        bytes: Vec<u8>,
        include_library_nodes: bool,
        digests: &BTreeMap<String, String>,
        progress: &mut dyn FnMut(&'static str),
    ) -> Result<(Rc<Self>, Value), String> {
        progress("pack-check");
        // This host never opens the KerML-only bindings: it runs the combined
        // KerML+SysML ones. Leaving that section unverified (and unreadable)
        // saves hashing a large part of the pack on every start.
        let pack = LanguagePack::read_checked(bytes, |section| {
            if section.name == language_pack::BINDINGS_SECTION {
                SectionCheck::Skip
            } else if let Some(digest) = digests.get(&section.name) {
                SectionCheck::HostDigest(digest.clone())
            } else {
                SectionCheck::Hash
            }
        })
        .map_err(|e| format!("Language pack: {e}"))?;
        progress("definition-owners");
        let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)?;
        progress("bindings");
        let (bundle, scopes) = language_pack::load_pack_sysml_bindings(&pack)?;
        // Host binding identity: matches upstream sysml_pack's
        // public binding recipe inputs (examples/support/sysml_scopes.rs).
        // The upstream CLI currently uses a different host-selected key.
        let compiler = intersect_kerml::sysml_bindings::compiler_identity();
        let mut digest = Sha256::new();
        for part in std::iter::once(compiler.as_str()).chain(
            bundle
                .sources
                .iter()
                .flat_map(|source| [source.id.uri.as_str(), source.id.sha256.as_str()]),
        ) {
            digest.update((part.len() as u64).to_le_bytes());
            digest.update(part.as_bytes());
        }
        let identity = format!(
            "{}:{:x}",
            intersect_kerml::sysml_bindings::BINDING_RECIPE,
            digest.finalize()
        );
        progress("library-sources");
        let payload = language_pack::pack_library_payload(&pack).transpose()?;
        #[cfg(not(feature = "components"))]
        let payload = Some(
            payload.ok_or("Pack has no standard library. Build it with sysml_pack --library.")?,
        );
        let texts: BTreeMap<String, String> = payload
            .map(decode_library_part)
            .transpose()
            .map_err(|e| e.to_string())?
            .map(|p| p.sources())
            .unwrap_or_default()
            .into_iter()
            .map(|s| (s.uri, s.text))
            .collect();
        progress("library-restore");
        let preparation = InvocationContext::new(Rc::new(Observer));
        let prepared = with_sysml_host(
            &session,
            &preparation,
            SysmlBindings {
                bundle: &bundle,
                scopes: &scopes,
                identity: &identity,
            },
            RuntimePredictionMemoMode::On,
            |host| {
                // Fail clearly on a stale snapshot. Rebuilding compiler caches is a
                // host build task, not an accidental multi-minute UI operation.
                if payload.is_none() {
                    let load = host.load(
                        FrozenStore::new(host.metamodel().clone()),
                        &[],
                        &mut |source, store| host.parse(source, store),
                        &|_| {},
                    )?;
                    return Ok((
                        load.model.ok_or("Empty library failed validation")?,
                        "no-standard-library".to_owned(),
                    ));
                }
                let prepared = language_pack::prepare_library(
                    payload,
                    None,
                    host.context(),
                    // The payload is a pack section this read verified.
                    LibraryOptions {
                        trust: SnapshotTrust::VerifiedHostCache,
                        ..LibraryOptions::default()
                    },
                    |_| {
                        Err("Library snapshot is stale. Rebuild the host-owned pack with --library.".into())
                    },
                )?;
                if prepared.origin != LibraryOrigin::Snapshot {
                    return Err("A prepared library snapshot is required".into());
                }
                Ok((prepared.model, prepared.key))
            },
        )?;
        #[cfg(feature = "components")]
        let standard_component_payload =
            language_pack::pack_standard_component_payload(&pack, &prepared.1)?.map(<[u8]>::to_vec);
        progress("library-tree");
        let nodes = if include_library_nodes {
            model_nodes(&prepared.0)
        } else {
            Vec::new()
        };
        let provenance: Value = serde_json::from_str(include_str!(
            "../../../fixtures/native-pilot/v1/manifest.json"
        ))
        .map_err(|e| format!("Invalid installed library provenance: {e}"))?;
        let library_info = json!({
            "name":"OMG standard library", "version":null,
            "uri":"urn:intersect:standard-library", "readOnly":true,
            "origin":"Language pack", "sourceRevision":provenance["pilot_commit"],
            "snapshotKey":prepared.1, "objects":prepared.0.len(),
            "files":texts.keys().collect::<Vec<_>>(),
        });
        #[cfg(feature = "components")]
        let library_info = {
            let mut info = library_info;
            info["componentCacheAvailable"] = json!(standard_component_payload.is_some());
            info
        };
        let result = json!({
            "language": language_pack::LANGUAGE,
            "compiler": language_pack::expected_compiler(),
            "libraryKey": prepared.1,
            "libraryFiles": texts.keys().collect::<Vec<_>>(),
            "libraryObjects": prepared.0.len(), "nodes": nodes,
            "libraryMount": library_info,
        });
        let loaded = Rc::new(Self {
            session,
            bundle,
            scopes,
            library: prepared.0,
            texts,
            identity,
            #[cfg(feature = "components")]
            library_info,
            #[cfg(feature = "components")]
            standard_component_payload,
        });
        Ok((loaded, result))
    }
}

fn name(object: Obj<'_>) -> Option<&str> {
    ["declaredName", "declaredShortName"]
        .into_iter()
        .find_map(|f| {
            object
                .feature(f)
                .and_then(|f| object.str(f))
                .filter(|s| !s.is_empty())
        })
}

// Presentation projection: skip unnamed relationship wrappers, following
// actual engine containers. No scope, ownership or language rules are inferred.
fn model_nodes(model: &Model) -> Vec<Value> {
    model_nodes_in_resources(model, None)
}

fn model_nodes_in_resources(model: &Model, resources: Option<&BTreeSet<String>>) -> Vec<Value> {
    model
        .objects()
        .filter(|object| resources.is_none_or(|resources| resources.contains(object.resource())))
        .filter_map(|object| {
            let name = name(object)?;
            let mut parent = object.container().map(|p| p.0);
            let mut remaining = model.len();
            while let Some(owner) = parent {
                if self::name(owner).is_some() {
                    break;
                }
                if remaining == 0 {
                    return None;
                }
                remaining -= 1;
                parent = owner.container().map(|p| p.0);
            }
            let (start, end) = object.span();
            Some(
                json!({"id":object.identity(),"name":name,"class":object.class_key(),
            "parent":parent.map(|p|p.identity()),"uri":object.resource(),"start":start,"end":end}),
            )
        })
        .collect()
}

fn value_json(model: &Model, value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Default(v) | ValueRef::Json(v) => v.clone(),
        ValueRef::Null => Value::Null,
        ValueRef::Bool(v) => json!(v),
        ValueRef::Int(v) => json!(v),
        ValueRef::Str(v) => json!(v),
        ValueRef::Ref(id) => model.obj(id).map_or(
            Value::Null,
            |o| json!({"$ref":o.identity(),"name":name(o),"class":o.class_key()}),
        ),
        ValueRef::Many(items) => Value::Array(items.iter().map(|v| value_json(model, v)).collect()),
    }
}

fn byte_position(text: &str, offset: usize) -> Value {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.bytes().filter(|c| *c == b'\n').count();
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    json!({"line":line,"character":before[start..].encode_utf16().count()})
}

fn position_byte(text: &str, position: &Value) -> Result<usize, String> {
    let line = position["line"].as_u64().ok_or("Invalid LSP line")? as usize;
    let character = position["character"]
        .as_u64()
        .ok_or("Invalid LSP character")? as usize;
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n').ok_or("LSP line outside source")? + 1;
    }
    let line_text = text[start..].split('\n').next().unwrap_or("");
    let mut units = 0;
    for (offset, c) in line_text.char_indices() {
        if units == character {
            return Ok(start + offset);
        }
        units += c.len_utf16();
        if units > character {
            return Err("LSP position splits a Unicode scalar".into());
        }
    }
    if units == character {
        Ok(start + line_text.len())
    } else {
        Err("LSP character outside line".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_lsp_positions_round_trip() -> Result<(), String> {
        let text = "a😀b\r\ncafé";
        for offset in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
            let position = byte_position(text, offset);
            assert_eq!(position_byte(text, &position)?, offset);
        }
        assert!(position_byte(text, &json!({"line":0,"character":2})).is_err());
        assert!(position_byte(text, &json!({"line":20,"character":0})).is_err());
        Ok(())
    }
    #[test]
    fn missing_dependency_and_bad_descriptor_fail_before_loading() {
        let mut host = Host::default();
        let bad = host.dispatch(json!({"method":"workspace/analyze","params":{
            "sources":[],"descriptor":"{}","revision":1}}));
        assert!(bad.is_err_and(|e| e.contains("name")));
        let missing = host.dispatch(json!({"method":"workspace/analyze","params":{
            "sources":[],"descriptor":"{\"name\":\"Test\",\"version\":\"1.0.0\",\"usage\":[{\"resource\":\"https://example.com/missing.kpar\"}]}","revision":2}}));
        assert!(missing.is_err_and(|e| e.contains("missing.kpar")));
    }
}

#[cfg(test)]
mod source_function_probe;

mod view_rendering;
