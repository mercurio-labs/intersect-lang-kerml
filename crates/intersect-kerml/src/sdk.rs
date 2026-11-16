//! Handwritten UI-independent native SDK. Hosts own I/O and thread scheduling.
use crate::{
    language_pack,
    native_pilot::NativePilotSession,
    sysml_host::{SysmlBindings, with_sysml_host},
};
use intersect_model::{
    core::frozen::{Model, Obj, ValueRef},
    definitions::{
        bundle::DefinitionBundle, conversion::InvocationContext, pack::LanguagePack,
        rcore_scope::VerifiedScope,
    },
    python::GeneratedPython,
};
use intersect_text::Source;
use intersect_text::grammar::{RuntimeAdmissionLoadMode, RuntimePredictionMemoMode};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_SOURCES: usize = 256;
pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;

/// Check generated names over the pinned canonical structure before an expensive
/// pack compilation. Final artifacts still come from the actual prepared pack.
pub fn check_python_schema() -> Result<usize, String> {
    let source = crate::kerml_bindings::structure_source();
    let bundle = DefinitionBundle {
        version: 1,
        sources: vec![source.clone()],
        namespaces: vec![],
    };
    let schema = intersect_model::definitions::compiler::compile_structure_bundle_with_limits(
        &bundle,
        &source.id,
        "https://www.omg.org/spec/SysML/20250201",
        crate::kerml_bindings::SOURCE_LIMITS,
    )
    .map_err(|e| format!("Python schema preflight: {e:?}"))?;
    let generated = intersect_model::python::generate(&schema)?;
    Ok(generated.manifest["classes"]
        .as_object()
        .map_or(0, |classes| classes.len()))
}

/// Identity shared with the host-owned pack builder. This is a cache identity,
/// not authentication of a pack received from another party.
pub fn bindings_identity(bundle: &DefinitionBundle) -> String {
    let compiler = crate::sysml_bindings::compiler_identity();
    let mut digest = Sha256::new();
    for part in std::iter::once(compiler.as_str()).chain(
        bundle
            .sources
            .iter()
            .flat_map(|s| [s.id.uri.as_str(), s.id.sha256.as_str()]),
    ) {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    format!(
        "{}:{:x}",
        crate::sysml_bindings::BINDING_RECIPE,
        digest.finalize()
    )
}

pub fn python_artifacts(pack: &LanguagePack) -> Result<GeneratedPython, String> {
    let session = NativePilotSession::from_pack(pack, RuntimeAdmissionLoadMode::HostCache)?;
    let mut generated = session.with_definitions(|kerml, _, _, _| {
        let mm = language_pack::frozen_metamodel(kerml)?;
        intersect_model::python::generate(mm.schema())
    })?;
    generated.manifest["compiler"] = json!(language_pack::expected_compiler());
    generated.manifest["pack_sha256"] = json!(format!("{:x}", Sha256::digest(pack.bytes())));
    generated.manifest["pack_recipe"] = json!(pack.manifest().recipe);
    generated.manifest["pack_parts"] = json!(pack.manifest().parts);
    Ok(generated)
}

/// Thread-affine session. Construct/use/drop on one host-owned thread.
pub struct Engine {
    session: NativePilotSession,
    bundle: DefinitionBundle,
    scopes: Vec<VerifiedScope>,
    library: Model,
    bindings: String,
    pub manifest: Value,
}

#[derive(Serialize)]
pub struct Analysis {
    pub diagnostics: Vec<crate::frozen_load::LoadDiagnostic>,
    pub clean: bool,
    pub notes: Vec<String>,
    #[serde(skip)]
    pub model: Option<Snapshot>,
}

pub struct Snapshot {
    model: Model,
    project_uris: BTreeSet<String>,
}

impl Engine {
    pub fn from_trusted_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let pack = LanguagePack::read(bytes).map_err(|e| e.to_string())?;
        Self::from_trusted_pack(&pack)
    }
    /// The caller must establish that bytes came from its own matching build or
    /// an explicitly trusted release. Digest matching alone is not that proof.
    pub fn from_trusted_pack(pack: &LanguagePack) -> Result<Self, String> {
        let manifest = python_artifacts(pack)?.manifest;
        let session = NativePilotSession::from_pack(pack, RuntimeAdmissionLoadMode::HostCache)?;
        let (bundle, scopes) = language_pack::load_pack_sysml_bindings(pack)?;
        let bindings = bindings_identity(&bundle);
        let payload = language_pack::pack_library_payload(pack)
            .ok_or("Pack has no prepared library; build with --library")??;
        let library = with_sysml_host(
            &session,
            &InvocationContext::default(),
            SysmlBindings {
                bundle: &bundle,
                scopes: &scopes,
                identity: &bindings,
            },
            RuntimePredictionMemoMode::On,
            |host| {
                let library = language_pack::prepare_library(
                    Some(payload),
                    None,
                    host.context(),
                    Default::default(),
                    |_| {
                        Err(
                            "Stale library snapshot; rebuild with the matching SDK toolchain"
                                .into(),
                        )
                    },
                )?;
                if library.origin != language_pack::LibraryOrigin::Snapshot {
                    return Err("A matching prepared library is required".into());
                }
                Ok(library.model)
            },
        )?;
        Ok(Self {
            session,
            bundle,
            scopes,
            library,
            bindings,
            manifest,
        })
    }

    pub fn analyze(
        &self,
        project: BTreeMap<String, String>,
        descriptor: Option<&str>,
        archives: Vec<(String, Vec<u8>)>,
    ) -> Result<Analysis, String> {
        if archives.len() > 64 {
            return Err("At most 64 dependency archives are supported".into());
        }
        let project_uris = project.keys().cloned().collect();
        let mut sources: Vec<_> = project
            .into_iter()
            .map(|(uri, text)| Source { uri, text })
            .collect();
        let mut notes = Vec::new();
        if let Some(descriptor) = descriptor {
            let project =
                intersect_kpar::Project::parse(descriptor.as_bytes()).map_err(|e| e.to_string())?;
            let mut candidates = vec![intersect_kpar::ProjectArchive {
                uri: "workspace:/.project.json".into(),
                root: String::new(),
                project,
                meta: Default::default(),
                sources: vec![],
                notes: vec![],
                warnings: vec![],
            }];
            for (uri, bytes) in archives {
                candidates.push(
                    intersect_kpar::read(&uri, &bytes, &Default::default())
                        .map_err(|e| e.to_string())?,
                );
            }
            let resolution = intersect_kpar::resolve(&candidates, &[0]).map_err(|errors| {
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
            for index in resolution.order {
                for source in &candidates[index].sources {
                    sources.push(Source {
                        uri: source.uri.clone(),
                        text: source.text.clone(),
                    });
                }
            }
            notes.extend(
                candidates
                    .iter()
                    .flat_map(|a| a.notes.iter().chain(a.warnings.iter()))
                    .cloned(),
            );
        }
        if sources.len() > MAX_SOURCES
            || sources.iter().map(|s| s.text.len()).sum::<usize>() > MAX_SOURCE_BYTES
        {
            return Err("Resolved project exceeds 256 sources or 8 MiB of text".into());
        }
        let mut seen = BTreeSet::new();
        if sources.iter().any(|s| !seen.insert(&s.uri)) {
            return Err("Duplicate source URI".into());
        }
        let load = with_sysml_host(
            &self.session,
            &InvocationContext::default(),
            SysmlBindings {
                bundle: &self.bundle,
                scopes: &self.scopes,
                identity: &self.bindings,
            },
            RuntimePredictionMemoMode::On,
            |host| host.load_on_library(&self.library, &sources, &|_| {}),
        )?;
        let clean = load.is_clean();
        Ok(Analysis {
            diagnostics: load.diagnostics,
            clean,
            notes,
            model: if clean {
                load.model.map(|model| Snapshot {
                    model,
                    project_uris,
                })
            } else {
                None
            },
        })
    }
}

impl Snapshot {
    pub fn elements(
        &self,
        class: Option<&str>,
        include_libraries: bool,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>, String> {
        if limit > 4096 {
            return Err("Element page exceeds 4096 objects".into());
        }
        let expected = class
            .map(|c| {
                self.model
                    .metamodel()
                    .class(c)
                    .ok_or_else(|| format!("Unknown metaclass {c}"))
            })
            .transpose()?;
        Ok(self
            .model
            .objects()
            .filter(|o| {
                (include_libraries || self.project_uris.contains(o.resource()))
                    && expected.is_none_or(|c| self.model.metamodel().is_a(o.class(), c))
            })
            .skip(offset)
            .take(limit)
            .map(|o| o.identity().to_owned())
            .collect())
    }
    fn object(&self, id: &str) -> Result<Obj<'_>, String> {
        self.model
            .lookup(id)
            .ok_or_else(|| format!("Unknown element in this snapshot: {id}"))
    }
    pub fn inspect(&self, id: &str) -> Result<Value, String> {
        let o = self.object(id)?;
        let (start, end) = o.span();
        Ok(
            json!({"id":id,"class":o.class_key(),"uri":o.resource(),"start":start,"end":end,
            "container":o.container().map(|(parent,feature)|json!({"id":parent.identity(),
                "feature":self.model.metamodel().feature_name(feature)}))}),
        )
    }
    pub fn is_instance(&self, id: &str, class: &str) -> Result<bool, String> {
        let expected = self
            .model
            .metamodel()
            .class(class)
            .ok_or_else(|| format!("Unknown metaclass {class}"))?;
        Ok(self
            .model
            .metamodel()
            .is_a(self.object(id)?.class(), expected))
    }
    pub fn stored_properties(&self, id: &str) -> Result<Vec<String>, String> {
        let o = self.object(id)?;
        let class = self
            .model
            .metamodel()
            .class_info(o.class())
            .ok_or("Missing class metadata")?;
        Ok(class
            .stored
            .iter()
            .map(|f| self.model.metamodel().feature_name(*f).to_owned())
            .collect())
    }
    pub fn children(&self, id: &str) -> Result<Vec<String>, String> {
        let o = self.object(id)?;
        let class = self
            .model
            .metamodel()
            .class_info(o.class())
            .ok_or("Missing class metadata")?;
        Ok(class
            .stored
            .iter()
            .filter(|f| {
                self.model
                    .metamodel()
                    .feature_info(**f)
                    .is_some_and(|info| {
                        info.kind == intersect_model::core::frozen::FeatureKind::Containment
                    })
            })
            .flat_map(|f| o.refs(*f))
            .map(|child| child.identity().to_owned())
            .collect())
    }
    pub fn get(&self, id: &str, name: &str) -> Result<Value, String> {
        let o = self.object(id)?;
        let f = o
            .feature(name)
            .ok_or_else(|| format!("Unknown feature {name}"))?;
        let info = self
            .model
            .metamodel()
            .feature_info(f)
            .ok_or("Missing feature metadata")?;
        if info.derived {
            return Err(format!("Unsupported derived feature: {name}"));
        }
        let definition = self
            .model
            .metamodel()
            .definition(f)
            .ok_or("Missing feature definition")?;
        if definition.reference {
            let ids: Vec<_> = o.refs(f).map(|v| v.identity().to_owned()).collect();
            return Ok(json!({"reference":true,"many":info.many,"value":
                if info.many {json!(ids)} else {json!(ids.first())}}));
        }
        Ok(
            json!({"reference":false,"many":info.many,"target":definition.target,
            "value":self.value(o.get(f).ok_or("Feature is not stored")?)}),
        )
    }
    fn value(&self, value: ValueRef<'_>) -> Value {
        match value {
            ValueRef::Null => Value::Null,
            ValueRef::Bool(v) => json!(v),
            ValueRef::Int(v) => json!(v),
            ValueRef::Str(v) => json!(v),
            ValueRef::Default(v) | ValueRef::Json(v) => v.clone(),
            ValueRef::Ref(id) => self
                .model
                .obj(id)
                .map(|o| json!(o.identity()))
                .unwrap_or(Value::Null),
            ValueRef::Many(v) => Value::Array(v.iter().map(|v| self.value(v)).collect()),
        }
    }
}
