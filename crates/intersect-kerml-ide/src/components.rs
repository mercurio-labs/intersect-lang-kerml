//! Opt-in IDE transport for closed, immutable component usages. Acquisition is
//! host-owned. Preparation never publishes a usage or changes a document buffer;
//! each analysis supplies its complete usage set and latest editable sources.
use super::*;
use intersect_kerml::components::with_shared_component_language;
use intersect_model::{
    components::{ComponentImage, MountedModel, SharedSchema},
    storage::{self, ModelRead},
};
use intersect_text::components::ComponentLanguage;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct State {
    context: Option<Arc<SharedSchema>>,
    pool: BTreeMap<String, Prepared>,
    standard: Option<Prepared>,
    pub usages: Vec<String>,
    pub model: Option<MountedModel>,
    navigation: Vec<intersect_text::Reference>,
    pub texts: BTreeMap<String, String>,
    paths: BTreeMap<String, String>,
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
    nodes: Vec<Value>,
}

impl Host {
    pub(super) fn component_dispatch(&mut self, request: &Value) -> Option<Result<Value, String>> {
        let params = &request["params"];
        match request["method"].as_str()? {
            "intersect/source"
                if self
                    .components
                    .texts
                    .contains_key(params["uri"].as_str().unwrap_or("")) =>
            {
                Some((|| {
                    let uri = params["uri"].as_str().ok_or("Missing source URI")?;
                    Ok(
                        json!({"uri":uri,"text":self.components.texts.get(uri),"path":self.components.paths.get(uri),"readOnly":true}),
                    )
                })())
            }
            "components/prepareUsage" => Some(self.prepare_usage(params)),
            "components/checkUsageSet" => Some((|| {
                let request: ProjectRequest = serde_json::from_value(params.clone())
                    .map_err(|e| format!("Invalid usage request: {e}"))?;
                self.check_component_profile(&request)?;
                Ok(Value::Null)
            })()),
            "intersect/inspect" if !self.components.usages.is_empty() => Some((|| {
                self.check_revision(params)?;
                let model = self
                    .components
                    .model
                    .as_ref()
                    .ok_or("No current mounted model")?;
                let id = params["id"].as_str().ok_or("Missing element id")?;
                let mut result = Value::Null;
                if !model.inspect(id, &mut |object| {
                    let text = self.source(object.resource()).unwrap_or("");
                    let values: BTreeMap<_, _> = object.fields().map(|(k,v)| (k, v.to_json())).collect();
                    result = json!({"id":id,"class":object.class(),"resource":object.resource(),
                        "range":{"start":byte_position(text,object.start()),"end":byte_position(text,object.end())},
                        "values":values,"revision":self.revision});
                    Ok(())
                })? { return Err("Element no longer exists in the current model".into()); }
                Ok(result)
            })()),
            "textDocument/definition" if !self.components.usages.is_empty() => {
                Some(self.component_definition(params))
            }
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

    fn component_definition(&self, params: &Value) -> Result<Value, String> {
        self.check_revision(params)?;
        if !self.documents.matches_sources(|uri| self.source(uri)) {
            return Err("Open documents differ from the current analysis".into());
        }
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or("Missing document URI")?;
        let text = self.source(uri).ok_or("Unknown source")?;
        if !self
            .documents
            .matches(uri, text, params["textDocument"]["version"].as_i64())
        {
            return Err("Document differs from the current analysis".into());
        }
        let offset = position_byte(text, &params["position"])?;
        let Some(reference) = self
            .components
            .navigation
            .iter()
            .filter(|r| r.uri == uri && r.start <= offset && offset <= r.end)
            .min_by_key(|r| r.end - r.start)
        else {
            return Ok(Value::Null);
        };
        let model = self
            .components
            .model
            .as_ref()
            .ok_or("No current mounted model")?;
        let mut result = Value::Null;
        model.inspect(&reference.target,&mut |object| {
            let target=self.source(object.resource()).ok_or("Target source unavailable")?;
            let range=json!({"start":byte_position(target,object.start()),"end":byte_position(target,object.end())});
            result=json!([{"originSelectionRange":{"start":byte_position(text,reference.start),"end":byte_position(text,reference.end)},"targetUri":object.resource(),"targetRange":range,"targetSelectionRange":range}]);Ok(())
        })?;
        Ok(result)
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
        let nodes = component_nodes(&image, &model, &paths)?;
        let info = json!({"key":key,"uri":uri,"name":archive.project.name,"version":archive.project.version,
            "url":url,"sha256":digest,"readOnly":true,"cacheHit":false,"objects":image.object_count(),
            "files":texts.keys().collect::<Vec<_>>(),"mountKey":image.key(),"identity":image.identity(),
            "origin":"Hosted KPAR",
            "notes":archive.notes.iter().chain(&archive.warnings).collect::<Vec<_>>()});
        self.components.context = Some(context);
        self.components.pool.insert(
            key,
            Prepared {
                image,
                texts,
                paths,
                info: info.clone(),
                nodes,
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
        // Invalidate before attempting analysis, including operational errors.
        self.current = None;
        self.pending = None;
        self.baseline = None;
        self.last_analysis = None;
        self.components.model = None;
        self.components.navigation.clear();
        self.navigation.clear();
        self.components.usages = request.component_usages.clone();
        self.revision = request.revision;
        self.sources = request
            .sources
            .iter()
            .map(|s| (s.uri.clone(), s.text.clone()))
            .collect();
        self.editable = self.sources.keys().cloned().collect();
        if self.sources.len() != request.sources.len()
            || request.sources.len() > 256
            || request.sources.iter().map(|s| s.text.len()).sum::<usize>() > 8 * 1024 * 1024
        {
            return Err("Project requires distinct sources, at most 256 files and 8 MiB".into());
        }
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
        let mut dependency_nodes = Vec::new();
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
            dependency_nodes.extend(prepared.nodes.iter().cloned());
        }
        let mut library_files = Vec::new();
        if !loaded.library.is_empty() {
            if self.components.standard.is_none() {
                self.components.standard = Some(prepare_standard_component(&context, loaded)?);
            }
            let standard = self
                .components
                .standard
                .as_ref()
                .ok_or("Missing standard library")?;
            library_files.extend(standard.texts.keys().cloned());
            texts.extend(standard.texts.clone());
            paths.extend(standard.paths.clone());
            images.push(standard.image.clone());
            dependency_nodes.extend(standard.nodes.iter().cloned());
        }
        self.components.texts = texts;
        self.components.paths = paths;
        let preparation = InvocationContext::new(Rc::new(Observer));
        let analyzed = with_shared_component_language(
            &loaded.session,
            &preparation,
            SysmlBindings {
                bundle: &loaded.bundle,
                scopes: &loaded.scopes,
                identity: &loaded.identity,
            },
            context,
            |language| language.analyze(&request.sources, &images),
        )?;
        let mut nodes = analyzed
            .model
            .as_ref()
            .map(|model| mounted_nodes(model, true))
            .transpose()?
            .unwrap_or_default();
        if analyzed.model.is_some() {
            nodes.extend(dependency_nodes);
            nodes.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        }
        let diagnostics: Vec<_> = analyzed
            .diagnostics
            .iter()
            .map(|d| {
                let text = self.source(&d.source).unwrap_or("");
                json!({"uri":d.source,"message":d.message,"phase":"component","severity":"Error",
                "range":{"start":byte_position(text,d.start),"end":byte_position(text,d.end)}})
            })
            .collect();
        let result = json!({"revision":self.revision,"nodes":nodes,"diagnostics":diagnostics,
            "hasModel":analyzed.model.is_some(),"clean":analyzed.diagnostics.is_empty() && analyzed.model.is_some(),
            "resolved":analyzed.references.len(),"references":analyzed.references.len(),"project":project.map(|p|p.to_value()),
            "libraryFiles":library_files,"libraryOrigin":self.components.standard.as_ref().map(|standard|&standard.info),
            "libraryMount":self.components.standard.as_ref().map(|standard|&standard.info),
            "dependencyUris":dependency_uris,"dependencyDocuments":self.components.paths.iter().map(|(uri,path)|json!({"uri":uri,"path":path})).collect::<Vec<_>>(),"dependencies":dependencies,"notes":["Hosted usages are read only and retained for this open session. Diagram and evaluation services for mounted projects are not yet available."]});
        self.components.navigation = analyzed.references;
        self.components.model = analyzed.model;
        self.last_analysis = Some(result.clone());
        Ok(result)
    }
}

/// Adapt the exact, already validated closed standard-library package into the
/// component representation once per worker. The original frozen package and
/// all its resource provenance remain in Loaded; the component reports that
/// origin explicitly rather than pretending its representation key is the
/// original library package identity. A metadata-only frozen-store operation
/// preserves native slots and assignment evidence. No source is reparsed.
fn prepare_standard_component(
    context: &Arc<SharedSchema>,
    loaded: &Loaded,
) -> Result<Prepared, String> {
    use intersect_model::{frozen::ById, storage::ModelStore};
    let records = loaded
        .library
        .resource_records()
        .map_err(|e| e.to_string())?;
    let original_packages: BTreeMap<_, _> = records
        .iter()
        .map(|(uri, record)| (uri.clone(), record.package.clone()))
        .collect();
    let mut store = FrozenStore::from_model(loaded.library.clone());
    storage::atomic(&mut store, |store| {
        for mut record in records.into_values() {
            record.package = None;
            store.set_resource_record(record)?;
        }
        Ok(())
    })?;
    // Keep native assignment evidence and all physical slots intact. An owned
    // JSON export/import would introduce legacy default-assignment markers.
    let closed = store
        .into_builder()
        .map_err(|e| e.to_string())?
        .link(&ById)
        .map_err(|e| format!("Closed standard library relink failed: {e:?}"))?
        .validate()
        .map_err(|e| format!("Closed standard library validation failed: {e:?}"))?;
    let image = ComponentImage::prepare(context.clone(), "urn:intersect:standard-library", closed)?;
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
    let nodes = component_nodes(&image, &loaded.library, &paths)?;
    let mut info = loaded.library_info.clone();
    info["mountKey"] = json!(image.key());
    info["identity"] = json!(image.identity());
    info["originalPackages"] = json!(original_packages);
    info["files"] = json!(texts.keys().collect::<Vec<_>>());
    Ok(Prepared {
        image,
        texts,
        paths,
        info,
        nodes,
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
