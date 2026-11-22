//! Opt-in IDE transport for closed, immutable component usages. Acquisition is
//! host-owned. Preparation never publishes a usage or changes a document buffer;
//! each analysis supplies its complete usage set and latest editable sources.
use super::*;
use base64::Engine as _;
use intersect_kerml::components::with_shared_component_language;
use intersect_model::{
    components::{ComponentImage, MountedModel, SharedSchema},
    storage::ModelRead,
};
use intersect_text::components::ComponentLanguage;
use intersect_view_host::{
    LoadedModel, LoadedSources, PreparedProject, ProjectLoad, ProjectMetadata,
    load::{LoadDiagnostic, LoadPhase},
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct State {
    context: Option<Arc<SharedSchema>>,
    pool: BTreeMap<String, Prepared>,
    standard: Option<Prepared>,
    pub usages: Vec<String>,
    pending: Option<PendingMount>,
}

struct PendingMount {
    token: String,
    candidate: PreparedMount,
}

struct PreparedMount {
    revision: u64,
    usages: Vec<String>,
    environment: view_loading::ProjectEnvironment,
    project: PreparedProject,
}

impl State {
    pub(super) fn clear_project(&mut self) {
        self.usages.clear();
        self.pending = None;
    }
}
struct ComponentSources {
    texts: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
}

struct Prepared {
    image: Arc<ComponentImage>,
    texts: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
    info: Value,
}

pub(super) struct MountedSources {
    context: Arc<SharedSchema>,
    images: Vec<Arc<ComponentImage>>,
    texts: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
    metadata: ProjectMetadata,
    library_files: Vec<String>,
    library_info: Option<Value>,
}

impl MountedSources {
    pub(super) fn load(
        &self,
        loaded: &Loaded,
        sources: &[Source],
    ) -> Result<LoadedSources, String> {
        let preparation = InvocationContext::new(Rc::new(Observer));
        let analyzed = with_shared_component_language(
            &loaded.session,
            &preparation,
            SysmlBindings {
                bundle: &loaded.bundle,
                scopes: &loaded.scopes,
                identity: &loaded.identity,
            },
            self.context.clone(),
            |language| language.analyze(sources, &self.images),
        )?;
        let diagnostics = analyzed
            .diagnostics
            .into_iter()
            .map(|d| LoadDiagnostic {
                resource: Some(d.source),
                start: Some(d.start),
                end: Some(d.end),
                ..LoadDiagnostic::error(LoadPhase::Validate, d.message)
            })
            .collect();
        let model = analyzed
            .model
            .map(MountedModel::into_read_model)
            .transpose()?
            .map(|model| LoadedModel::Mounted(Rc::new(model)));
        Ok(LoadedSources {
            load: ProjectLoad {
                model,
                diagnostics,
                references: analyzed.references.len(),
                resolved: analyzed.references.len(),
                navigation: analyzed.references,
            },
            reference_sources: self.texts.clone(),
            metadata: ProjectMetadata {
                project: self.metadata.project.clone(),
                dependency_uris: self.metadata.dependency_uris.clone(),
                dependencies: self.metadata.dependencies.clone(),
                notes: self.metadata.notes.clone(),
            },
        })
    }

    pub(super) fn enrich(&self, analysis: &mut Value) {
        analysis["libraryFiles"] = json!(self.library_files);
        analysis["libraryOrigin"] = json!(self.library_info);
        analysis["libraryMount"] = json!(self.library_info);
        analysis["dependencyDocuments"] = json!(
            self.paths
                .iter()
                .map(|(uri, path)| json!({"uri": uri, "path": path}))
                .collect::<Vec<_>>()
        );
    }
}

impl Host {
    pub(super) fn component_dispatch(&mut self, request: &Value) -> Option<Result<Value, String>> {
        let params = &request["params"];
        match request["method"].as_str()? {
            "intersect/source" => {
                let uri = params["uri"].as_str()?;
                let project = self.project.borrow();
                let mounted = project.mounted.as_ref()?;
                let path = mounted.paths.get(uri)?;
                let text = self.source(uri)?;
                Some(Ok(
                    json!({"uri": uri, "text": text, "path": path, "readOnly": true}),
                ))
            }
            "components/prepareUsage" => Some(self.prepare_usage(params)),
            "components/prepareProject" => Some(self.prepare_component_project(params)),
            "components/adoptProject" => Some(self.adopt_component_project(params)),
            "components/abortProject" => {
                if self
                    .components
                    .pending
                    .as_ref()
                    .is_some_and(|pending| params["token"].as_str() == Some(pending.token.as_str()))
                {
                    self.components.pending = None;
                }
                Some(Ok(Value::Null))
            }
            "components/checkUsageSet" => Some((|| {
                let request: ProjectRequest = serde_json::from_value(params.clone())
                    .map_err(|e| format!("Invalid usage request: {e}"))?;
                self.check_component_profile(&request)?;
                Ok(Value::Null)
            })()),
            "intersect/inspect" if !self.components.usages.is_empty() => Some((|| {
                self.check_revision(params)?;
                self.views
                    .as_mut()
                    .ok_or("No current mounted model")?
                    .dispatch(request.clone())
            })()),
            "textDocument/completion" if !self.components.usages.is_empty() => {
                Some(self.documents.complete(params))
            }
            // Never answer these using a previous single-arena project.
            "textDocument/hover" | "textDocument/documentSymbol"
                if !self.components.usages.is_empty() =>
            {
                Some(Ok(Value::Null))
            }
            _ => None,
        }
    }

    fn prepare_usage(&mut self, params: &Value) -> Result<Value, String> {
        let uri = params["uri"].as_str().ok_or("Missing usage URI")?;
        intersect_model::resources::validate_resource_uri(uri)?;
        if uri.len() > 2048 {
            return Err("Usage URI is too long".into());
        }
        let url = params["url"].as_str().ok_or("Missing acquisition URL")?;
        if url.len() > 8192 {
            return Err("Acquisition URL is too long".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(params["base64"].as_str().ok_or("Missing KPAR bytes")?)
            .map_err(|e| format!("Invalid KPAR transport: {e}"))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("KPAR exceeds 8 MiB".into());
        }
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let key = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(uri, &digest)).map_err(|e| e.to_string())?)
        );
        if let Some(prepared) = self.components.pool.get(&key) {
            let mut info = prepared.info.clone();
            info["cacheHit"] = json!(true);
            return Ok(info);
        }
        if self.components.pool.len() >= 32 {
            return Err(
                "This worker has reached its 32 prepared-usage limit. Reconnect to clear it."
                    .into(),
            );
        }
        let archive_uri = format!("urn:intersect:kpar:sha256:{digest}");
        let archive = intersect_kpar::read(&archive_uri, &bytes, &Default::default())
            .map_err(|e| e.to_string())?;
        if !archive.project.usage.is_empty() {
            return Err("Hosted usages currently require a self-contained textual KPAR without transitive usages".into());
        }
        let sources: Vec<_> = archive
            .sources
            .iter()
            .map(|s| Source {
                uri: s.uri.clone(),
                text: s.text.clone(),
            })
            .collect();
        if sources.is_empty()
            || sources.len() > 256
            || sources.iter().map(|s| s.text.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            return Err("KPAR must contain 1–256 textual sources, totaling at most 8 MiB".into());
        }
        let loaded = self
            .loaded
            .as_ref()
            .ok_or("Load a compatible language pack first")?;
        if !loaded.library.is_empty() && loaded.standard_component_payload.is_none() {
            return Err("This language pack has no prepared standard-library component. Rebuild the host-owned pack with components and --library before adding hosted usages.".into());
        }
        let context = match &self.components.context {
            Some(context) => context.clone(),
            None => SharedSchema::from_metamodel(loaded.session.frozen_metamodel()?)?,
        };
        let preparation = InvocationContext::new(Rc::new(Observer));
        let model = with_shared_component_language(
            &loaded.session,
            &preparation,
            SysmlBindings {
                bundle: &loaded.bundle,
                scopes: &loaded.scopes,
                identity: &loaded.identity,
            },
            context.clone(),
            |language| language.prepare_model(&sources),
        )?;
        let image = ComponentImage::prepare(context.clone(), uri, model.clone())?;
        let ComponentSources { texts, paths } =
            component_texts(&context, &image, &model, &sources)?;
        let standard = if self.components.standard.is_none() && !loaded.library.is_empty() {
            Some(prepare_standard_component(&context, loaded)?)
        } else {
            None
        };
        let info = json!({"key":key,"uri":uri,"name":archive.project.name,"version":archive.project.version,
            "url":url,"sha256":digest,"readOnly":true,"cacheHit":false,"objects":image.object_count(),
            "files":texts.keys().collect::<Vec<_>>(),"mountKey":image.key(),"identity":image.identity(),
            "origin":"Hosted KPAR",
            "notes":archive.notes.iter().chain(&archive.warnings).collect::<Vec<_>>()});
        self.components.context = Some(context);
        if let Some(standard) = standard {
            self.components.standard = Some(standard);
        }
        self.components.pool.insert(
            key,
            Prepared {
                image,
                texts,
                paths,
                info: info.clone(),
            },
        );
        Ok(info)
    }

    fn check_component_profile(
        &self,
        request: &ProjectRequest,
    ) -> Result<Option<intersect_kpar::Project>, String> {
        if !request.archives.is_empty() {
            return Err("This hosted-usage prototype does not yet combine local KPAR archives with mounted hosted usages".into());
        }
        let project = request
            .descriptor
            .as_ref()
            .map(|s| intersect_kpar::Project::parse(s.as_bytes()))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut seen = BTreeSet::new();
        let mut candidates = Vec::new();
        if let Some(root) = &project {
            candidates.push(intersect_kpar::ProjectArchive {
                uri: "workspace:/.project.json".into(),
                root: String::new(),
                project: root.clone(),
                meta: Default::default(),
                sources: vec![],
                notes: vec![],
                warnings: vec![],
            });
        }
        for key in &request.component_usages {
            let prepared = self
                .components
                .pool
                .get(key)
                .ok_or("Prepared usage is unavailable; add its hosted KPAR again")?;
            if !seen.insert(prepared.image.uri()) {
                return Err("Duplicate or conflicting usage URI".into());
            }
            if project.is_some() {
                let descriptor = json!({"name":prepared.info["name"],"version":prepared.info["version"],"usage":[]});
                candidates.push(intersect_kpar::ProjectArchive {
                    uri: prepared.image.uri().into(),
                    root: String::new(),
                    project: intersect_kpar::Project::parse(
                        &serde_json::to_vec(&descriptor).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?,
                    meta: Default::default(),
                    sources: vec![],
                    notes: vec![],
                    warnings: vec![],
                });
            }
        }
        if let Some(root) = &project {
            intersect_kpar::resolve(&candidates, &[0]).map_err(|errors| {
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
            if root
                .usage
                .iter()
                .any(|usage| intersect_kpar::resolve::is_standard_library(&usage.resource))
                && self.loaded.as_ref().is_none_or(|l| l.library.is_empty())
            {
                return Err("This project requires a language pack containing its prepared standard library".into());
            }
        }
        Ok(project)
    }

    pub(super) fn analyze_components(&mut self, request: ProjectRequest) -> Result<Value, String> {
        let candidate = self.prepare_mount(request)?;
        self.publish_mount(candidate)
    }

    fn prepare_component_project(&mut self, params: &Value) -> Result<Value, String> {
        let request: ProjectRequest = serde_json::from_value(params.clone())
            .map_err(|e| format!("Invalid component project request: {e}"))?;
        if request.component_usages.is_empty() || request.revision <= self.revision {
            return Err(
                "Preparing a mount requires an exact usage set and a newer revision".into(),
            );
        }
        let candidate = self.prepare_mount(request)?;
        self.mount_transaction = self
            .mount_transaction
            .checked_add(1)
            .ok_or("Mount transaction limit exceeded")?;
        let token = format!("mount:{}", self.mount_transaction);
        let receipt = json!({ "token": token, "revision": candidate.revision });
        self.components.pending = Some(PendingMount { token, candidate });
        Ok(receipt)
    }

    fn adopt_component_project(&mut self, params: &Value) -> Result<Value, String> {
        let pending = self
            .components
            .pending
            .as_ref()
            .ok_or("No prepared component project")?;
        if params["token"].as_str() != Some(pending.token.as_str())
            || params["revision"].as_u64() != Some(pending.candidate.revision)
        {
            return Err("Prepared component project is stale".into());
        }
        let candidate = self
            .components
            .pending
            .take()
            .ok_or("No prepared component project")?
            .candidate;
        self.publish_mount(candidate)
    }

    fn prepare_mount(&mut self, request: ProjectRequest) -> Result<PreparedMount, String> {
        let project = self.check_component_profile(&request)?;
        let context = self
            .components
            .context
            .clone()
            .ok_or("No prepared component context")?;
        let loaded = self
            .loaded
            .as_ref()
            .ok_or("Load a compatible language pack first")?;
        let mut images = Vec::new();
        let mut dependency_uris = Vec::new();
        let mut dependencies = Vec::new();
        let mut texts = BTreeMap::new();
        let mut paths = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut logical = BTreeSet::new();
        for key in &request.component_usages {
            let prepared = self
                .components
                .pool
                .get(key)
                .ok_or("Prepared usage is unavailable; add its hosted KPAR again")?;
            if !seen.insert(key) || !logical.insert(prepared.image.uri()) {
                return Err("Duplicate or conflicting usage URI".into());
            }
            images.push(prepared.image.clone());
            dependency_uris.extend(prepared.texts.keys().cloned());
            texts.extend(prepared.texts.clone());
            paths.extend(prepared.paths.clone());
            dependencies.push(prepared.info.clone());
        }
        let mut library_files = Vec::new();
        let prepared_standard = if !loaded.library.is_empty() && self.components.standard.is_none()
        {
            Some(prepare_standard_component(&context, loaded)?)
        } else {
            None
        };
        let standard = self
            .components
            .standard
            .as_ref()
            .or(prepared_standard.as_ref());
        if let Some(standard) = standard {
            library_files.extend(standard.texts.keys().cloned());
            texts.extend(standard.texts.clone());
            paths.extend(standard.paths.clone());
            images.push(standard.image.clone());
        }
        let mounted = MountedSources {
            context,
            images,
            texts,
            paths,
            metadata: ProjectMetadata {
                project: project.map(|p| p.to_value()),
                dependency_uris,
                dependencies,
                notes: vec![
                    "Hosted usages are read only and retained for this open session.".into(),
                ],
            },
            library_files,
            library_info: standard.map(|standard| standard.info.clone()),
        };
        let views = self
            .views
            .as_mut()
            .ok_or("Load a compatible language pack first")?;
        let previous = self.project.replace(view_loading::ProjectEnvironment {
            mounted: Some(mounted),
            ..Default::default()
        });
        let prepared = views.prepare_project(request.revision, request.sources, false);
        let environment = self.project.replace(previous);
        let project = prepared?;
        if let Some(standard) = prepared_standard {
            self.components.standard = Some(standard);
        }
        Ok(PreparedMount {
            revision: request.revision,
            usages: request.component_usages,
            environment,
            project,
        })
    }

    fn publish_mount(&mut self, candidate: PreparedMount) -> Result<Value, String> {
        let mut result = self
            .views
            .as_mut()
            .ok_or("Load a compatible language pack first")?
            .adopt_project(candidate.project)?;
        self.project.replace(candidate.environment);
        self.components.usages = candidate.usages;
        self.components.pending = None;
        self.sync_analysis(&mut result)?;
        Ok(result)
    }
}

/// Restore the build-time admitted component. This path never reparses,
/// rehashes a materialized graph, relinks or runs component preparation. The
/// enclosing pack section was verified before its bytes entered Loaded.
fn prepare_standard_component(
    context: &Arc<SharedSchema>,
    loaded: &Loaded,
) -> Result<Prepared, String> {
    let records = loaded
        .library
        .resource_records()
        .map_err(|e| e.to_string())?;
    let original_packages: BTreeMap<_, _> = records
        .iter()
        .map(|(uri, record)| (uri.clone(), record.package.clone()))
        .collect();
    let payload = loaded
        .standard_component_payload
        .as_deref()
        .ok_or("Rebuild the host-owned pack with its prepared standard-library component")?;
    let library_key = loaded.library_info["snapshotKey"]
        .as_str()
        .ok_or("Missing standard-library key")?;
    let image = intersect_kerml::language_pack::load_standard_component_cache(
        context.clone(),
        payload,
        library_key,
        SnapshotTrust::VerifiedHostCache,
    )?;
    let sources: Vec<_> = loaded
        .texts
        .iter()
        .map(|(uri, text)| Source {
            uri: uri.clone(),
            text: text.clone(),
        })
        .collect();
    let ComponentSources { texts, paths } =
        component_texts(context, &image, &loaded.library, &sources)?;
    let mut info = loaded.library_info.clone();
    info["mountKey"] = json!(image.key());
    info["identity"] = json!(image.identity());
    info["originalPackages"] = json!(original_packages);
    info["files"] = json!(texts.keys().collect::<Vec<_>>());
    info["componentOrigin"] = json!("Pack snapshot");
    Ok(Prepared {
        image,
        texts,
        paths,
        info,
    })
}

// Build source correspondence from actual original and mounted objects. The
// transport never decodes or invents the component runtime's opaque identities.
fn component_texts(
    context: &Arc<SharedSchema>,
    image: &Arc<ComponentImage>,
    original: &Model,
    sources: &[Source],
) -> Result<ComponentSources, String> {
    let view = MountedModel::new(context.clone(), vec![image.clone()])?;
    let source_map: BTreeMap<_, _> = sources
        .iter()
        .map(|s| (s.uri.as_str(), s.text.as_str()))
        .collect();
    let mut texts = BTreeMap::new();
    let mut paths = BTreeMap::new();
    let mut mapped = BTreeSet::new();
    for object in original.objects() {
        // Resource qualification belongs to the image, so all objects in one
        // original resource have the same mounted resource. Verify that actual
        // correspondence once per source rather than once per library object.
        if mapped.contains(object.resource()) {
            continue;
        }
        if let Some(text) = source_map.get(object.resource()) {
            view.inspect(image.object_id(object.identity())?, &mut |mounted| {
                texts
                    .entry(mounted.resource().to_owned())
                    .or_insert_with(|| (*text).to_owned());
                paths.insert(mounted.resource().to_owned(), object.resource().to_owned());
                Ok(())
            })?;
            mapped.insert(object.resource().to_owned());
        }
    }
    Ok(ComponentSources { texts, paths })
}

// Names, source spans and containment of closed images are immutable. Qualify
// the ordinary native presentation once, retaining actual runtime identities.
// Reciprocal project overlays cannot change any of these presentation fields.
#[cfg(test)]
fn component_nodes(
    image: &ComponentImage,
    original: &Model,
    paths: &BTreeMap<String, String>,
) -> Result<Vec<Value>, String> {
    let resources: BTreeMap<_, _> = paths
        .iter()
        .map(|(mounted, local)| (local.as_str(), mounted.as_str()))
        .collect();
    let mut nodes = model_nodes(original);
    for node in &mut nodes {
        node["id"] = json!(
            image.object_id(
                node["id"]
                    .as_str()
                    .ok_or("Missing component node identity")?
            )?
        );
        if let Some(parent) = node["parent"].as_str() {
            node["parent"] = json!(image.object_id(parent)?);
        }
        let uri = node["uri"].as_str().ok_or("Missing component source URI")?;
        node["uri"] = json!(
            resources
                .get(uri)
                .ok_or("Missing component source correspondence")?
        );
    }
    Ok(nodes)
}

#[cfg(test)]
fn mounted_nodes(model: &MountedModel, project_only: bool) -> Result<Vec<Value>, String> {
    let mut nodes = BTreeMap::new();
    let mut parents = BTreeMap::<String, String>::new();
    let mut containment = BTreeMap::new();
    for (_, id) in model.id_page(None, usize::MAX)? {
        if project_only && model.component_target(&id).is_some() {
            continue;
        }
        model.inspect(&id, &mut |object| {
        let features = if let Some(features) = containment.get(object.class()) {
            features
        } else {
            let fields: Vec<_> = model
                .schema()
                .features(object.class())?
                .into_values()
                .filter(|f| f.containment && !f.derived)
                .map(|f| f.name)
                .collect();
            containment
                .entry(object.class().to_owned())
                .or_insert(fields)
        };
        for feature in features {
            if let Some(value) = object.value(feature) {
                let value = value.to_json();
                for item in value
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(std::slice::from_ref(&value))
                {
                    if let Some(id) = item.as_str() {
                        parents.insert(id.to_owned(), object.id().to_owned());
                    }
                }
            }
        }
        let name = ["declaredName", "declaredShortName"].iter().find_map(|f| {
            object
                .value(f)
                .map(|v| v.to_json())
                .and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
        });
        if let Some(name) = name {
            nodes.insert(object.id().to_owned(),json!({"id":object.id(),"name":name,"class":object.class(),"uri":object.resource(),"start":object.start(),"end":object.end(),"parent":null}));
        }
        Ok(())
        })?;
    }
    let named: BTreeSet<_> = nodes.keys().cloned().collect();
    for (id, node) in &mut nodes {
        let mut parent = parents.get(id);
        let mut seen = BTreeSet::new();
        while let Some(owner) = parent {
            if !seen.insert(owner) {
                break;
            }
            if named.contains(owner) {
                node["parent"] = json!(owner);
                break;
            }
            parent = parents.get(owner);
        }
    }
    Ok(nodes.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use intersect_model::frozen::{ById, ModelBuilder};

    #[test]
    fn rejected_component_analysis_preserves_the_accepted_revision() {
        let uri = "memory:project.sysml";
        let mut host = Host {
            revision: 4,
            sources: BTreeMap::from([(uri.into(), "accepted source".into())]),
            last_analysis: Some(json!({"revision":4})),
            ..Default::default()
        };
        host.components.usages = vec!["accepted".into()];
        let rejected = host.dispatch(json!({
            "method":"workspace/analyze",
            "params":{
                "revision":5,
                "sources":[{"uri":uri,"text":"candidate source"}],
                "componentUsages":["missing"]
            }
        }));
        assert!(rejected.is_err());
        assert_eq!(host.revision, 4);
        assert_eq!(host.source(uri), Some("accepted source"));
        assert_eq!(host.components.usages, ["accepted"]);
        assert_eq!(host.last_analysis, Some(json!({"revision":4})));
    }

    #[test]
    fn standard_component_pack_cache_is_bound_checked_and_invalidated_with_library()
    -> Result<(), String> {
        use intersect_kerml::language_pack::*;
        use intersect_model::definitions::{
            library_part::{LibraryManifest, LibrarySource},
            pack::{FileDigest, PackBuilder, PackHeader, PackSectionFlags, canonical_json},
        };
        let package = intersect_model::compile_rcore(
            r#"rcore 1;
package cache version "1";
metamodel "urn:cache" prefix c { class Node { attr name: String [0,1]; } }"#,
        )
        .map_err(|e| e.to_string())?;
        let context = SharedSchema::new(package.schema())?;
        let mm = context.metamodel();
        let class = mm
            .class(package.schema().classes.keys().next().ok_or("Node")?)
            .ok_or("Node")?;
        let mut builder = ModelBuilder::new(mm.clone());
        builder
            .create("node", class, "memory:/Base.kerml")
            .map_err(|e| e.to_string())?;
        let native = builder
            .link(&ById)
            .map_err(|e| format!("{e:?}"))?
            .validate()
            .map_err(|e| format!("{e:?}"))?;
        let sources = [LibrarySource {
            uri: "memory:/Base.kerml".into(),
            language: KERML_SOURCE.into(),
            text: "package Base;".into(),
        }];
        let library = prepare_library(
            None,
            Some(&sources),
            LibraryContext {
                metamodel: &mm,
                bindings: "test-bindings",
            },
            LibraryOptions::default(),
            |_| Ok(native.clone()),
        )?;
        assert_eq!(
            library.manifest,
            LibraryManifest::of_sources(&sources).map_err(|e| e.to_string())?
        );
        let mut parts = PartKeyInputs::current()?.parts()?;
        let placeholder = canonical_json(&json!({})).map_err(|e| e.to_string())?;
        let output = FileDigest::of(&placeholder).sha256;
        for part in &mut parts {
            part.output_digest = output.clone();
        }
        let mut builder = PackBuilder::new(PackHeader {
            language: LANGUAGE.into(),
            recipe: recipe(),
            compiler: expected_compiler(),
            recipe_constants: recipe_constants()?,
            inputs: pinned_inputs(),
            parts: parts.clone(),
            requires_hooks: vec![],
            depends_on: vec![],
        });
        for part in &parts {
            builder = builder.section(
                part.name.clone(),
                "placeholder",
                PackSectionFlags::REQUIRED,
                placeholder.clone(),
                part.input_digest.clone(),
                output.clone(),
            );
        }
        let base = LanguagePack::read(builder.build().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let library_payload = library.payload.as_ref().ok_or("Library payload")?.clone();
        let classic = LanguagePack::read(with_library_part(&base, library_payload.clone())?)
            .map_err(|e| e.to_string())?;
        assert!(pack_standard_component_payload(&classic, &library.key)?.is_none());
        let payload =
            prepare_standard_component_cache(context.clone(), &library.key, &library_payload)?;
        assert_eq!(
            payload,
            prepare_standard_component_cache(context.clone(), &library.key, &library_payload)?
        );
        let packed = LanguagePack::read(with_standard_component_part(&classic, payload.clone())?)
            .map_err(|e| e.to_string())?;
        let stored =
            pack_standard_component_payload(&packed, &library.key)?.ok_or("Component cache")?;
        assert_eq!(stored, payload);
        for trust in [
            SnapshotTrust::Revalidate,
            SnapshotTrust::HostCache,
            SnapshotTrust::VerifiedHostCache,
        ] {
            let image =
                load_standard_component_cache(context.clone(), stored, &library.key, trust)?;
            assert_eq!(image.uri(), STANDARD_COMPONENT_URI);
            assert_eq!(image.object_count(), native.len());
            MountedModel::new(context.clone(), vec![image])?.validate()?;
        }
        assert!(pack_standard_component_payload(&packed, &"00".repeat(32)).is_err());
        assert!(
            load_standard_component_cache(
                context.clone(),
                stored,
                &"00".repeat(32),
                SnapshotTrust::HostCache
            )
            .is_err()
        );
        let mut corrupt = payload.clone();
        *corrupt.last_mut().ok_or("Snapshot byte")? ^= 1;
        assert!(
            load_standard_component_cache(
                context.clone(),
                &corrupt,
                &library.key,
                SnapshotTrust::HostCache
            )
            .is_err()
        );
        assert!(
            load_standard_component_cache(
                context,
                &corrupt,
                &library.key,
                SnapshotTrust::VerifiedHostCache
            )
            .is_err()
        );
        assert!(
            load_standard_component_cache(
                SharedSchema::new(package.schema())?,
                &payload[..10],
                &library.key,
                SnapshotTrust::HostCache
            )
            .is_err()
        );
        assert_eq!(without_standard_component_part(&packed)?, classic.bytes());
        assert_eq!(without_library_part(&packed)?, base.bytes());
        let replaced = LanguagePack::read(with_library_part(&packed, library_payload)?)
            .map_err(|e| e.to_string())?;
        assert!(pack_standard_component_payload(&replaced, &library.key)?.is_none());
        assert!(with_standard_component_part(&base, payload).is_err());
        Ok(())
    }

    #[test]
    fn cached_component_nodes_match_full_mounted_projection_after_project_changes()
    -> Result<(), String> {
        let package = intersect_model::compile_rcore(
            r#"rcore 1;
package presentation version "1";
metamodel "urn:presentation" prefix p {
 class Node {
  attr declaredName: String [0,1];
  attr declaredShortName: String [0,1];
  ref children: Node [0,*] containment opposite Node.parent;
  ref parent: Node [0,1] opposite Node.children;
  ref out: Node [0,*] opposite Node.incoming;
  ref incoming: Node [0,*] opposite Node.out;
 }
}"#,
        )
        .map_err(|e| e.to_string())?;
        let context = SharedSchema::new(package.schema())?;
        let mm = context.metamodel();
        let class = mm
            .class(package.schema().classes.keys().next().ok_or("Node class")?)
            .ok_or("Node class")?;
        let name = mm.feature(class, "declaredName").ok_or("Name")?;
        let short = mm.feature(class, "declaredShortName").ok_or("Short name")?;
        let children = mm.feature(class, "children").ok_or("Children")?;
        let mut builder = ModelBuilder::new(mm.clone());
        let root = builder
            .create("root", class, "memory:/library.sysml")
            .map_err(|e| e.to_string())?;
        let wrapper = builder
            .create("wrapper", class, "memory:/library.sysml")
            .map_err(|e| e.to_string())?;
        let child = builder
            .create("child", class, "memory:/library.sysml")
            .map_err(|e| e.to_string())?;
        builder
            .set_str(root, name, "Library")
            .map_err(|e| e.to_string())?;
        builder
            .set_str(child, short, "C")
            .map_err(|e| e.to_string())?;
        builder
            .add_ref(root, children, wrapper)
            .map_err(|e| e.to_string())?;
        builder
            .add_ref(wrapper, children, child)
            .map_err(|e| e.to_string())?;
        let native = builder
            .link(&ById)
            .map_err(|e| format!("{e:?}"))?
            .validate()
            .map_err(|e| format!("{e:?}"))?;
        let first = ComponentImage::prepare(context.clone(), "urn:library:first", native.clone())?;
        let second =
            ComponentImage::prepare(context.clone(), "urn:library:second", native.clone())?;
        let sources = [Source {
            uri: "memory:/library.sysml".into(),
            text: "library text".into(),
        }];
        let mut cached = Vec::new();
        for image in [&first, &second] {
            let correspondence = component_texts(&context, image, &native, &sources)?;
            cached.extend(component_nodes(image, &native, &correspondence.paths)?);
        }
        let mut model = MountedModel::new(context, vec![first.clone(), second])?;
        let check = |model: &MountedModel| -> Result<(), String> {
            let mut combined = mounted_nodes(model, true)?;
            combined.extend(cached.iter().cloned());
            combined.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            assert_eq!(combined, mounted_nodes(model, false)?);
            Ok(())
        };
        check(&model)?;
        model.create("project", mm.class_key(class), "memory:/project.sysml")?;
        model.set("project", "declaredName", json!("Project"))?;
        model.set("project", "out", json!([first.object_id("child")?]))?;
        check(&model)?;
        model.set("project", "declaredName", json!("Edited project"))?;
        check(&model)?;
        Ok(())
    }
}
