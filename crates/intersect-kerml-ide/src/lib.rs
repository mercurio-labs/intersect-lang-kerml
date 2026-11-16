//! Handwritten KerML/SysML project IDE and visualization facade.
//!
//! This facade binds the existing pack, frozen SysML loader and KPAR resolver.
//! The bounded view/editing profile is documented in docs/visualization.md.
use base64::Engine;
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
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
mod editing;
mod evaluation;
mod views;

struct PreparedCommand {
    token: String,
    base_revision: u64,
    sources: BTreeMap<String, String>,
    model: Model,
    analysis: Value,
    baseline: ProjectRequest,
}

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
pub struct Host {
    documents: editing::Documents,
    loaded: Option<Loaded>,
    current: Option<Model>,
    sources: BTreeMap<String, String>,
    revision: u64,
    editable: BTreeSet<String>,
    baseline: Option<ProjectRequest>,
    last_analysis: Option<Value>,
    pending: Option<PreparedCommand>,
    next_transaction: u64,
}

struct Loaded {
    session: NativePilotSession,
    bundle: DefinitionBundle,
    scopes: Vec<VerifiedScope>,
    library: Model,
    texts: BTreeMap<String, String>,
    identity: String,
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
        let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)?;
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
        let payload = language_pack::pack_library_payload(&pack)
            .ok_or("Pack has no standard library. Build it with sysml_pack --library.")??;
        let texts: BTreeMap<String, String> = decode_library_part(payload)
            .map_err(|e| e.to_string())?
            .sources()
            .into_iter()
            .map(|s| (s.uri, s.text))
            .collect();
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
                let prepared = language_pack::prepare_library(
                    Some(payload),
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
                Ok(prepared)
            },
        )?;
        let nodes = if include_library_nodes {
            model_nodes(&prepared.model)
        } else {
            Vec::new()
        };
        let result = json!({
            "language": language_pack::LANGUAGE,
            "compiler": language_pack::expected_compiler(),
            "libraryKey": prepared.key,
            "libraryFiles": texts.keys().collect::<Vec<_>>(),
            "libraryObjects": prepared.model.len(), "nodes": nodes,
        });
        self.loaded = Some(Loaded {
            session,
            bundle,
            scopes,
            library: prepared.model,
            texts,
            identity,
        });
        self.current = None;
        self.sources.clear();
        self.baseline = None;
        self.pending = None;
        self.last_analysis = None;
        self.editable.clear();
        Ok(result)
    }

    pub fn dispatch(&mut self, request: Value) -> Result<Value, String> {
        let params = &request["params"];
        match request["method"].as_str().ok_or("Request has no method")? {
            "initialize" => Ok(json!({
                "capabilities": {"positionEncoding":"utf-16", "textDocumentSync":{"openClose":true,"change":1},
                    "hoverProvider":true, "documentSymbolProvider":true,
                    "definitionProvider":false, "completionProvider":{"resolveProvider":false}},
                "serverInfo":{"name":"Intersect KerML IDE","version":"0.1.0"},
                "limitations":["Completion offers lexical keywords and declaration snippets; scope-aware reference completion and definition locations await upstream APIs."],
            })),
            "textDocument/didOpen" => self.documents.open(params),
            "textDocument/didChange" => self.documents.change(params),
            "textDocument/didClose" => self.documents.close(params),
            "textDocument/completion" => self.documents.complete(params),
            "evaluation/sites" => self.evaluation_sites(params),
            "evaluation/evaluate" => self.evaluate_site(params),
            "workspace/analyze" => self.analyze(
                serde_json::from_value(params.clone())
                    .map_err(|e| format!("Invalid workspace request: {e}"))?,
            ),
            "views/catalog" => {
                self.check_revision(params)?;
                self.views_catalog()
            }
            "views/scene" => self.views_scene(params),
            "views/locate" => self.views_locate(params),
            "views/prepareCommand" => self.prepare_command(
                serde_json::from_value(params.clone())
                    .map_err(|e| format!("Invalid diagram command: {e}"))?,
            ),
            "views/commitCommand" => self.commit_command(params),
            "views/abortCommand" => {
                if self
                    .pending
                    .as_ref()
                    .is_some_and(|p| params["token"].as_str() == Some(p.token.as_str()))
                {
                    self.pending = None;
                }
                Ok(Value::Null)
            }
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
                self.current = None;
                self.loaded = None;
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
    fn model(&self) -> Result<&Model, String> {
        self.current
            .as_ref()
            .or_else(|| self.loaded.as_ref().map(|l| &l.library))
            .ok_or_else(|| "Load a compatible language pack first".to_owned())
    }
    fn source(&self, uri: &str) -> Option<&str> {
        self.sources
            .get(uri)
            .or_else(|| self.loaded.as_ref()?.texts.get(uri))
            .map(String::as_str)
    }
    fn range(&self, object: Obj<'_>) -> Value {
        let (start, end) = object.span();
        let text = self.source(object.resource()).unwrap_or("");
        json!({"start":byte_position(text,start),"end":byte_position(text,end)})
    }

    fn analyze(&mut self, request: ProjectRequest) -> Result<Value, String> {
        // Invalidate first: a failed edit must never leave the previous model
        // available to hover/inspection as if it described the new text.
        self.current = None;
        self.revision = request.revision;
        self.sources.clear();
        self.pending = None;
        self.baseline = None;
        self.editable = request.sources.iter().map(|s| s.uri.clone()).collect();
        let projection_resources = request.project_nodes_only.then(|| self.editable.clone());
        let baseline = request.clone();
        if request.sources.len() > 256
            || request.sources.iter().map(|s| s.text.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            return Err("Workspace exceeds 256 sources or 8 MiB of model text".into());
        }
        let mut seen = BTreeSet::new();
        for source in &request.sources {
            if !seen.insert(&source.uri) {
                return Err(format!("Duplicate source URI: {}", source.uri));
            }
        }
        let project = request
            .descriptor
            .as_ref()
            .map(|s| intersect_kpar::Project::parse(s.as_bytes()))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut archives = Vec::new();
        // A loose folder is a root candidate with the standard descriptor.
        if let Some(project) = project.as_ref() {
            archives.push(intersect_kpar::ProjectArchive {
                uri: "workspace:/.project.json".into(),
                root: String::new(),
                project: project.clone(),
                meta: intersect_kpar::Meta::default(),
                sources: vec![],
                notes: vec![],
                warnings: vec![],
            });
        }
        if request.archives.len() > 64 {
            return Err("Workspace exceeds 64 dependency archives".into());
        }
        for archive in request.archives {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(archive.base64)
                .map_err(|e| format!("Invalid archive transport: {e}"))?;
            archives.push(
                intersect_kpar::read(&archive.uri, &bytes, &Default::default())
                    .map_err(|e| e.to_string())?,
            );
        }
        let resolution = if project.is_some() {
            Some(intersect_kpar::resolve(&archives, &[0]).map_err(|errors| {
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?)
        } else {
            None
        };
        let mut sources = request.sources;
        let mut dependency_uris = BTreeSet::new();
        if let Some(resolution) = &resolution {
            for index in &resolution.order {
                for source in &archives[*index].sources {
                    dependency_uris.insert(source.uri.clone());
                    sources.push(Source {
                        uri: source.uri.clone(),
                        text: source.text.clone(),
                    });
                }
            }
        }
        if sources.len() > 256
            || sources.iter().map(|s| s.text.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            return Err("Resolved workspace exceeds 256 sources or 8 MiB of model text".into());
        }
        self.sources = sources
            .iter()
            .map(|s| (s.uri.clone(), s.text.clone()))
            .collect();
        let load = self.load_sources(&sources)?;
        let diagnostics: Vec<_> = load
            .diagnostics
            .iter()
            .map(|d| {
                let object = d
                    .object
                    .as_deref()
                    .and_then(|id| load.model.as_ref()?.lookup(id));
                let uri = d
                    .resource
                    .as_deref()
                    .or_else(|| object.map(|o| o.resource()));
                let span = object.map(|o| o.span());
                let text = uri.and_then(|uri| self.source(uri)).unwrap_or("");
                json!({"uri":uri,"message":d.message,"code":d.code,"phase":d.phase,
                "severity":d.severity,"range":{
                    "start":byte_position(text,d.start.or_else(||span.map(|s|s.0)).unwrap_or(0)),
                    "end":byte_position(text,d.end.or_else(||span.map(|s|s.1)).unwrap_or(0))}})
            })
            .collect();
        let valid_model = load.model.is_some();
        let nodes = load
            .model
            .as_ref()
            .map(|model| model_nodes_in_resources(model, projection_resources.as_ref()))
            .unwrap_or_default();
        let result = json!({"revision":self.revision,"nodes":nodes,"diagnostics":diagnostics,
            "hasModel":valid_model,"clean":load.is_clean(),"resolved":load.resolved,"references":load.references,
            "project":project.map(|p|p.to_value()), "dependencyUris":dependency_uris,
            "dependencies":resolution.as_ref().map(|r| r.order.iter().filter(|i| **i != 0)
                .map(|i| json!({"uri":archives[*i].uri,"name":archives[*i].project.name,
                    "version":archives[*i].project.version})).collect::<Vec<_>>()).unwrap_or_default(),
            "notes":archives.iter().flat_map(|a| a.notes.iter().chain(a.warnings.iter())).collect::<Vec<_>>()});
        if load.is_clean() {
            self.baseline = Some(baseline);
        }
        self.current = load.model;
        self.last_analysis = Some(result.clone());
        Ok(result)
    }

    fn load_sources(
        &self,
        sources: &[Source],
    ) -> Result<intersect_kerml::frozen_load::FrozenLoad, String> {
        let loaded = self
            .loaded
            .as_ref()
            .ok_or("Load a language pack to analyze SysML/KerML")?;
        let preparation = InvocationContext::new(Rc::new(Observer));
        with_sysml_host(
            &loaded.session,
            &preparation,
            SysmlBindings {
                bundle: &loaded.bundle,
                scopes: &loaded.scopes,
                identity: &loaded.identity,
            },
            RuntimePredictionMemoMode::On,
            |host| {
                host.load(
                    FrozenStore::from_model(loaded.library.clone()),
                    sources,
                    &mut |source, store| host.parse(source, store),
                    &|_| {},
                )
            },
        )
    }

    fn prepare_command(&mut self, command: views::Command) -> Result<Value, String> {
        let baseline = self
            .baseline
            .as_ref()
            .ok_or("Diagram editing requires a clean, current source model")?;
        let (patch, after, created) = views::prepare_patch(self, &command)?;
        let mut sources = self.sources.clone();
        sources.insert(patch.uri.clone(), after.clone());
        let batch: Vec<_> = sources
            .iter()
            .map(|(uri, text)| Source {
                uri: uri.clone(),
                text: text.clone(),
            })
            .collect();
        let load = self.load_sources(&batch)?;
        if !load.is_clean() {
            return Err(format!(
                "Proposed source edit was rejected: {}",
                load.diagnostics
                    .iter()
                    .map(|d| d.message.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        let model = load.model.ok_or("Proposed edit has no accepted model")?;
        let mut analysis = self.last_analysis.clone().ok_or("No accepted analysis")?;
        let next_revision = self.revision.checked_add(1).ok_or("Revision overflow")?;
        analysis["revision"] = json!(next_revision);
        analysis["nodes"] = json!(model_nodes_in_resources(
            &model,
            baseline.project_nodes_only.then_some(&self.editable)
        ));
        analysis["diagnostics"]=json!(load.diagnostics.iter().map(|d|json!({"uri":d.resource,"message":d.message,"severity":d.severity,"phase":d.phase,"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}})).collect::<Vec<_>>());
        analysis["clean"] = json!(true);
        analysis["hasModel"] = json!(true);
        analysis["resolved"] = json!(load.resolved);
        analysis["references"] = json!(load.references);
        let mut baseline = baseline.clone();
        baseline.revision = next_revision;
        for source in &mut baseline.sources {
            if source.uri == patch.uri {
                source.text = after.clone();
            }
        }
        self.next_transaction = self
            .next_transaction
            .checked_add(1)
            .ok_or("Transaction identity overflow")?;
        let token = format!("edit:{}:{}", self.revision, self.next_transaction);
        let result = PreparedEdit {
            schema_version: 1,
            token: token.clone(),
            base_revision: self.revision,
            next_revision,
            patches: vec![patch],
            created,
        };
        result.validate()?;
        self.pending = Some(PreparedCommand {
            token: token.clone(),
            base_revision: self.revision,
            sources,
            model,
            analysis,
            baseline,
        });
        serde_json::to_value(result).map_err(|e| e.to_string())
    }

    fn commit_command(&mut self, params: &Value) -> Result<Value, String> {
        self.check_revision(params)?;
        let pending = self.pending.as_ref().ok_or("No prepared diagram edit")?;
        if params["token"].as_str() != Some(pending.token.as_str())
            || pending.base_revision != self.revision
        {
            return Err("Prepared edit token is stale".into());
        }
        let received: Vec<Source> = serde_json::from_value(params["sources"].clone())
            .map_err(|e| format!("Invalid applied source receipt: {e}"))?;
        let mut seen = BTreeSet::new();
        if received.len() != self.editable.len()
            || received.iter().any(|s| {
                !seen.insert(&s.uri)
                    || !self.editable.contains(&s.uri)
                    || pending.sources.get(&s.uri) != Some(&s.text)
            })
        {
            return Err(
                "Editor buffers differ from the prepared source edit; nothing was published".into(),
            );
        }
        let pending = self.pending.take().ok_or("Prepared edit disappeared")?;
        self.revision = pending.baseline.revision;
        self.sources = pending.sources;
        self.current = Some(pending.model);
        self.baseline = Some(pending.baseline);
        self.last_analysis = Some(pending.analysis.clone());
        Ok(pending.analysis)
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
