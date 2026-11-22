//! Language-owned library/KPAR loading behind the generic host's one candidate path.
use crate::{ArchiveInput, Loaded, Observer, Source};
use base64::Engine;
use intersect_kerml::sysml_host::{SysmlBindings, with_sysml_host};
use intersect_model::{core::frozen::FrozenStore, definitions::conversion::InvocationContext};
use intersect_text::grammar::RuntimePredictionMemoMode;
use intersect_view_host::{LoadedSources, ProjectMetadata, SourceLoader};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

#[derive(Default)]
pub(super) struct ProjectEnvironment {
    pub descriptor: Option<String>,
    pub archives: Vec<ArchiveInput>,
    #[cfg(feature = "components")]
    pub mounted: Option<crate::components::MountedSources>,
}
pub(super) struct Loader {
    pub loaded: Rc<Loaded>,
    pub project: Rc<RefCell<ProjectEnvironment>>,
}
pub(super) struct ResolvedSources {
    sources: Vec<Source>,
    reference_sources: BTreeMap<String, String>,
    metadata: ProjectMetadata,
}
pub(super) fn resolve_project(
    environment: &ProjectEnvironment,
    project_sources: &[Source],
) -> Result<ResolvedSources, String> {
    let project = environment
        .descriptor
        .as_ref()
        .map(|s| intersect_kpar::Project::parse(s.as_bytes()))
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut archives = vec![];
    if let Some(project) = &project {
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
    if environment.archives.len() > 64 {
        return Err("Workspace exceeds 64 dependency archives".into());
    }
    for archive in &environment.archives {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&archive.base64)
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
    let mut sources = project_sources.to_vec();
    let mut reference_sources = BTreeMap::new();
    let mut dependency_uris = BTreeSet::new();
    let mut uris: BTreeSet<_> = sources.iter().map(|s| s.uri.clone()).collect();
    for uri in &uris {
        if reference_sources.contains_key(uri) {
            return Err(format!(
                "Project source collides with a library resource: {uri}"
            ));
        }
    }
    if let Some(resolution) = &resolution {
        for index in &resolution.order {
            for source in &archives[*index].sources {
                if !uris.insert(source.uri.clone()) || reference_sources.contains_key(&source.uri) {
                    return Err(format!(
                        "Dependency source collides with a loaded resource: {}",
                        source.uri
                    ));
                }
                dependency_uris.insert(source.uri.clone());
                reference_sources.insert(source.uri.clone(), source.text.clone());
                sources.push(Source {
                    uri: source.uri.clone(),
                    text: source.text.clone(),
                });
            }
        }
    }
    if sources.len() > 256
        || sources
            .iter()
            .try_fold(0usize, |sum, s| sum.checked_add(s.text.len()))
            .is_none_or(|bytes| bytes > 8 * 1024 * 1024)
    {
        return Err("Resolved workspace exceeds 256 sources or 8 MiB of model text".into());
    }
    let dependencies = resolution.as_ref().map(|r| r.order.iter().filter(|i|**i!=0)
            .map(|i|json!({"uri":archives[*i].uri,"name":archives[*i].project.name,"version":archives[*i].project.version}))
            .collect()).unwrap_or_default();
    Ok(ResolvedSources {
        sources,
        reference_sources,
        metadata: ProjectMetadata {
            project: project.map(|p| p.to_value()),
            dependency_uris: dependency_uris.into_iter().collect(),
            dependencies,
            notes: archives
                .iter()
                .flat_map(|a| a.notes.iter().chain(a.warnings.iter()))
                .cloned()
                .collect(),
        },
    })
}
impl SourceLoader for Loader {
    fn load(&self, project_sources: &[Source]) -> Result<LoadedSources, String> {
        let environment = self.project.try_borrow().map_err(|e| e.to_string())?;
        #[cfg(feature = "components")]
        if let Some(mounted) = &environment.mounted {
            return mounted.load(&self.loaded, project_sources);
        }
        let resolved = resolve_project(&environment, project_sources)?;
        let mut reference_sources = self.loaded.texts.clone();
        for source in &resolved.sources {
            if reference_sources.contains_key(&source.uri) {
                return Err(format!(
                    "Project or dependency source collides with a library resource: {}",
                    source.uri
                ));
            }
        }
        reference_sources.extend(resolved.reference_sources);
        let loaded = &self.loaded;
        let preparation = InvocationContext::new(Rc::new(Observer));
        let load = with_sysml_host(
            &loaded.session,
            &preparation,
            SysmlBindings {
                bundle: &loaded.bundle,
                scopes: &loaded.scopes,
                identity: &loaded.identity,
            },
            RuntimePredictionMemoMode::On,
            |host| {
                host.load_sources(
                    FrozenStore::from_model(loaded.library.clone()),
                    &resolved.sources,
                    &|_| {},
                )
            },
        )?;
        Ok(LoadedSources {
            load: load.into(),
            reference_sources,
            metadata: resolved.metadata,
        })
    }
}

pub(super) fn project_buffers(
    host: &intersect_view_host::Host,
    analysis: &serde_json::Value,
) -> BTreeMap<String, String> {
    let mut sources = host.sources().clone();
    if let Some(uris) = analysis["dependencyUris"].as_array() {
        for uri in uris.iter().filter_map(|v| v.as_str()) {
            if let Some(text) = host.source(uri) {
                sources.insert(uri.into(), text.into());
            }
        }
    }
    sources
}
