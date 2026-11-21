//! Handwritten host core (not generated): SysML v2 with its standard library
//! on the frozen engine, shared by the `intersect` CLI and the measurement
//! examples.
//!
//! [`with_sysml_host`](crate::sysml_host::with_sysml_host) binds the KerML and
//! SysML languages of a [`NativePilotSession`](crate::native_pilot::NativePilotSession)
//! (from a language pack or prepared from sources) and the combined KerML+SysML
//! Rcore bindings (from the pack's `rcore/sysml-bindings` part or a host cache).
//! It hands a [`SysmlHost`](crate::sysml_host::SysmlHost) to the caller. The host
//! parses KerML and SysML into a frozen store, then links, completes and validates
//! the model with `frozen_load::load_frozen`. This works for the whole library
//! from an empty store or for user files on top of a prepared library.
//!
//! This module performs no I/O and reads no clock: hosts read files and the
//! pack, and time phases through the `observe` callback.
use crate::{
    frozen_load::{self, FrozenLoad, LoadDiagnostic, LoadPhase},
    language_pack::{LibraryContext, PreparedLibrary, library_parse_order},
    native_pilot::NativePilotSession,
    standard_library::{CombinedCatalog, StdlibCatalog, implicit_stdlib_usage, package_for_model},
};
use intersect_model::core::query::QueryBudget;
use intersect_model::{
    core::{
        frozen::{FrozenStore, Metamodel, Model},
        package_load::{PackagePayload, load_against_lock, validate_lock_resource_records},
        project_usages::{DependencyLock, PackageCatalog, Usage, UsageResolver},
        resource_set::ResourceContext,
        storage::ModelRead,
    },
    definitions::{
        bundle::DefinitionBundle, conversion::InvocationContext, library_part::LibrarySource,
        rcore_scope::VerifiedScope,
    },
    frozen_scopes::{Execution, RcoreScopes},
};
use intersect_text::EvaluationLimits;
use intersect_text::grammar::RuntimePredictionMemoMode;
use intersect_text::{Source, StoredEvaluation};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// The combined KerML+SysML Rcore bindings a host binds: the bundle they were
/// compiled against, the verified scopes, and the identity that keys a
/// prepared library on them (`LibraryContext::bindings`).
pub struct SysmlBindings<'b> {
    pub bundle: &'b DefinitionBundle,
    pub scopes: &'b [VerifiedScope],
    pub identity: &'b str,
}

/// One parse: the evaluation, the reference count and the consumed input.
type Parsed = Result<(StoredEvaluation, usize, usize), String>;

/// Explicit additional project dependencies. Acquisition remains a host concern;
/// each resolved identity needs a supplied prepared payload at load admission.
#[derive(Default)]
pub struct ProjectDependencies<'a> {
    pub usages: &'a [Usage],
    pub catalog: Option<&'a dyn PackageCatalog>,
    pub payloads: &'a [PackagePayload<'a>],
}

/// A project load together with the exact dependency snapshot and context.
pub struct ProjectLoad {
    pub load: FrozenLoad,
    pub lock: DependencyLock,
    pub context: ResourceContext,
}

/// Both languages bound, the frozen metamodel and the combined bindings.
pub struct SysmlHost<'h> {
    metamodel: Arc<Metamodel>,
    bindings: &'h str,
    resolvers: &'h RcoreScopes<'h>,
    parse: &'h dyn Fn(&Source, &mut FrozenStore, EvaluationLimits, &mut QueryBudget) -> Parsed,
}

impl SysmlHost<'_> {
    pub fn metamodel(&self) -> &Arc<Metamodel> {
        &self.metamodel
    }
    /// The key context of a prepared library on this host.
    pub fn context(&self) -> LibraryContext<'_> {
        LibraryContext {
            metamodel: &self.metamodel,
            bindings: self.bindings,
        }
    }
    /// Parse one source into `store` with its language (`.kerml` URIs with
    /// KerML, the rest with SysML), without linking.
    pub fn parse(&self, source: &Source, store: &mut FrozenStore) -> Parsed {
        self.parse_with(
            source,
            store,
            frozen_load::limits(),
            &mut QueryBudget::new(usize::MAX),
        )
    }
    /// Construction-only parsing with explicit limits and a shared caller budget.
    /// Editing probes use this without publishing their speculative model.
    pub fn parse_with(
        &self,
        source: &Source,
        store: &mut FrozenStore,
        limits: EvaluationLimits,
        caller: &mut QueryBudget,
    ) -> Parsed {
        (self.parse)(source, store, limits, caller)
    }
    /// Parse `sources` into `store` in order, link, complete and validate
    /// (`frozen_load::load_frozen`) with `parse` (so a host can time each
    /// source). `observe` receives the phase markers `parse_end`, `link_end`,
    /// `implicit_end`, `defaults_end`, `validate_end` and `check_end`.
    pub fn load(
        &self,
        store: FrozenStore,
        sources: &[Source],
        parse: &mut dyn FnMut(&Source, &mut FrozenStore) -> Parsed,
        observe: &dyn Fn(&'static str),
    ) -> Result<FrozenLoad, String> {
        frozen_load::load_frozen(store, sources, parse, self.resolvers, observe)
    }
    /// `load` with this host's own parse.
    pub fn load_sources(
        &self,
        store: FrozenStore,
        sources: &[Source],
        observe: &dyn Fn(&'static str),
    ) -> Result<FrozenLoad, String> {
        self.load(
            store,
            sources,
            &mut |source, store| self.parse(source, store),
            observe,
        )
    }
    /// User files on top of a prepared library: the library's objects count
    /// as already linked and completed.
    pub fn load_on_library(
        &self,
        library: &Model,
        sources: &[Source],
        observe: &dyn Fn(&'static str),
    ) -> Result<FrozenLoad, String> {
        let (library, catalog) = package_for_model(library, self.context())?;
        self.load_package_project(
            &library,
            &catalog,
            sources,
            ProjectDependencies::default(),
            observe,
        )
        .map(|project| project.load)
    }

    /// Resolve project usages (including the implicit prepared-library root),
    /// assemble payloads against that lock, then use the existing language load.
    pub fn load_project(
        &self,
        library: &PreparedLibrary,
        sources: &[Source],
        dependencies: ProjectDependencies<'_>,
        observe: &dyn Fn(&'static str),
    ) -> Result<ProjectLoad, String> {
        let catalog = StdlibCatalog::new(library)?;
        self.load_package_project(&library.model, &catalog, sources, dependencies, observe)
    }

    fn load_package_project(
        &self,
        library: &Model,
        stdlib: &StdlibCatalog,
        sources: &[Source],
        dependencies: ProjectDependencies<'_>,
        observe: &dyn Fn(&'static str),
    ) -> Result<ProjectLoad, String> {
        let mut catalogs: Vec<&dyn PackageCatalog> = vec![stdlib];
        if let Some(catalog) = dependencies.catalog {
            catalogs.push(catalog);
        }
        let catalog = CombinedCatalog(&catalogs);
        let mut usages = dependencies.usages.to_vec();
        usages.push(implicit_stdlib_usage());
        let lock = UsageResolver::resolve(&usages, &catalog).map_err(|e| e.to_string())?;
        let mut payloads = vec![PackagePayload {
            identity: stdlib.identity(),
            model: library,
        }];
        payloads.extend(dependencies.payloads.iter().map(|payload| PackagePayload {
            identity: payload.identity,
            model: payload.model,
        }));
        let project = intersect_model::core::model::Model {
            schema_revision: self.metamodel.revision().into(),
            elements: Vec::new(),
            resources: BTreeMap::new(),
        };
        let loaded = load_against_lock(self.metamodel.clone(), &lock, &payloads, &project)
            .map_err(|e| e.to_string())?;
        let mut uris = BTreeSet::new();
        for source in sources {
            if !uris.insert(source.uri.as_str())
                || loaded.store.resource_record(&source.uri)?.is_some()
            {
                return Err(format!(
                    "Project source collides with a loaded resource: {}",
                    source.uri
                ));
            }
        }
        let load = self.load_sources(loaded.store, sources, observe)?;
        if let Some(model) = &load.model {
            validate_lock_resource_records(
                &lock,
                &model.resource_records().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(ProjectLoad {
            load,
            lock,
            context: loaded.context,
        })
    }
    /// The whole library in parse order (KerML, then SysML) from an empty
    /// store, with `parse` (so a host can time each source); an error unless
    /// every phase passed and every reference resolved uniquely.
    pub fn parse_library_with(
        &self,
        sources: &[LibrarySource],
        parse: &mut dyn FnMut(&Source, &mut FrozenStore) -> Parsed,
        observe: &dyn Fn(&'static str),
    ) -> Result<(Model, FrozenLoad), String> {
        let ordered: Vec<Source> = library_parse_order(sources)?
            .into_iter()
            .map(|source| Source {
                uri: source.uri.clone(),
                text: source.text.clone(),
            })
            .collect();
        let mut load = self.load(
            FrozenStore::new(self.metamodel.clone()),
            &ordered,
            parse,
            observe,
        )?;
        match load.model.take() {
            Some(model)
                if !load.diagnostics.iter().any(is_error) && load.resolved == load.references =>
            {
                Ok((model, load))
            }
            _ => Err(format!(
                "library load incomplete: {} of {} references resolved, {} problems, first {:?}",
                load.resolved,
                load.references,
                load.diagnostics.len(),
                load.diagnostics.first().map(|d| d.message.clone())
            )),
        }
    }
    /// `parse_library_with` with this host's own parse.
    pub fn parse_library(
        &self,
        sources: &[LibrarySource],
        observe: &dyn Fn(&'static str),
    ) -> Result<(Model, FrozenLoad), String> {
        self.parse_library_with(
            sources,
            &mut |source, store| self.parse(source, store),
            observe,
        )
    }
}

/// Bind both languages of `session` and the combined `bindings`, then run
/// `body` with the host. `memo` selects the runtime prediction memo mode of
/// both languages (`frozen_load::prediction_memo_mode_from_env` reads it from
/// the environment for hosts that allow that).
pub fn with_sysml_host<T>(
    session: &NativePilotSession,
    preparation: &InvocationContext<'_>,
    bindings: SysmlBindings<'_>,
    memo: RuntimePredictionMemoMode,
    body: impl FnOnce(&SysmlHost<'_>) -> Result<T, String>,
) -> Result<T, String> {
    // One metamodel for both languages: the KerML and SysML owners construct
    // the same combined SysML.ecore. The session freezes it once.
    let metamodel = session.frozen_metamodel()?;
    session.with_definitions(|kerml, kerml_contracts, sysml, sysml_contracts| {
        let resolvers = RcoreScopes::new(&metamodel, bindings.bundle, bindings.scopes)
            .map_err(|e| e.to_string())?
            .with_limits(RcoreScopes::default_limits())
            .with_execution(Execution::Fast);
        frozen_load::with_kerml(kerml, kerml_contracts, |kerml| {
            frozen_load::with_sysml(sysml, sysml_contracts, preparation, |sysml| {
                kerml.language.prediction_memo().set_mode(memo);
                sysml.language.prediction_memo().set_mode(memo);
                let parse = |source: &Source,
                             store: &mut FrozenStore,
                             limits: EvaluationLimits,
                             caller: &mut QueryBudget| {
                    if source.uri.ends_with(".kerml") {
                        kerml.parse_with(source, limits, store, caller)
                    } else {
                        sysml.parse_with(source, limits, store, caller)
                    }
                };
                body(&SysmlHost {
                    metamodel: metamodel.clone(),
                    bindings: bindings.identity,
                    resolvers: &resolvers,
                    parse: &parse,
                })
            })
        })
    })
}

/// The interchange formats a host exports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// The Intersect model JSON (`core::model::Model`).
    Json,
    /// XMI 2.0 (`intersect_model::xmi`), one document per resource.
    Xmi,
}

/// Export the resources `selected` of `model` (all when empty) in `format`:
/// one `Resource` per selected resource for XMI (named by its URI), one JSON
/// document holding them all for JSON. References into resources that are
/// not selected (the standard library) stay references by identity.
pub fn export(
    model: &Model,
    selected: &[String],
    format: ExportFormat,
) -> Result<Vec<intersect_model::core::interchange::Resource>, String> {
    use intersect_model::core::interchange::Resource;
    match format {
        // The frozen XMI path: validated with the frozen rules, which read a
        // redefined feature (SysML.ecore's MOF redefinitions) through its
        // redefining one; resources are named by their URIs.
        ExportFormat::Xmi => intersect_model::xmi::XmiFormat
            .encode_frozen(model, (!selected.is_empty()).then_some(selected)),
        ExportFormat::Json => {
            let mut materialized = model.to_model().map_err(|e| e.to_string())?;
            if !selected.is_empty() {
                let keep: std::collections::BTreeSet<&str> =
                    selected.iter().map(String::as_str).collect();
                materialized
                    .elements
                    .retain(|element| keep.contains(element.resource.as_str()));
                materialized
                    .resources
                    .retain(|uri, _| keep.contains(uri.as_str()));
            }
            let bytes = serde_json::to_vec_pretty(&materialized).map_err(|e| e.to_string())?;
            Ok(vec![Resource {
                uri: "model.json".into(),
                bytes,
            }])
        }
    }
}

/// Whether a diagnostic is an error: every parse, link, completion and
/// structural problem, and the validation-rule issues of severity error.
/// Warnings and information do not make a load unclean.
pub fn is_error(diagnostic: &LoadDiagnostic) -> bool {
    diagnostic.is_error()
}

/// The 1-based line and column (in characters) of byte `offset` in `text`;
/// an offset past the end or inside a character points at the end of the
/// line it falls in.
pub fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let before = &text[..end];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    (line, before[start..].chars().count() + 1)
}

/// The phase name of a diagnostic, as hosts print it.
pub fn phase_name(phase: LoadPhase) -> &'static str {
    match phase {
        LoadPhase::Parse => "parse",
        LoadPhase::Link => "link",
        LoadPhase::Implicit => "implicit",
        LoadPhase::Defaults => "defaults",
        LoadPhase::Validate => "validate",
        LoadPhase::Check => "check",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_column_counts_lines_and_characters() {
        let text = "ab\ncä d\n";
        assert_eq!(line_column(text, 0), (1, 1));
        assert_eq!(line_column(text, 2), (1, 3));
        assert_eq!(line_column(text, 3), (2, 1));
        // 'ä' is two bytes: the space after it is column 3.
        assert_eq!(line_column(text, 6), (2, 3));
        // Inside 'ä' points at its start.
        assert_eq!(line_column(text, 5), (2, 2));
        assert_eq!(line_column(text, 100), (3, 1));
    }
}
