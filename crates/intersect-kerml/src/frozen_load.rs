//! The frozen-engine KerML/SysML text load as a library (handwritten, not
//! generated). It promotes what the `library_probe`, `frozen_kerml_load` and
//! `sysml_pack` examples did by hand:
//!
//! 1. bind a prepared KerML or SysML owner for parsing (`with_kerml`,
//!    `with_sysml`): the pinned source gate of `crate::text_capability`, the
//!    open grammar-driven construction, the KerML `Name` and comment
//!    conversions and the computed settings;
//! 2. parse each source into one `FrozenStore` (`load_kerml`);
//! 3. link it with the source-authored KerML name resolution (the verified
//!    Rcore bindings of `crate::kerml_bindings`), complete the implicit
//!    elements and the construction defaults, and validate;
//! 4. or, with `load_frozen`, do 2 and 3 with any bound languages and Rcore
//!    scopes into a given store, for example on top of a prepared standard
//!    library (`FrozenStore::from_model`, decision D10).
//!
//! Every problem is returned as a `LoadDiagnostic`; a load that fails a phase
//! stops there and returns no model. The bindings are an input: compiling
//! them takes minutes and needs deep stacks, which is a host concern (see the
//! examples' `support/kerml_scopes.rs` and `language_pack::load_pack_bindings`).
//!
//! This module performs no I/O and uses no clocks, threads or environment.
use crate::text_capability::{
    KerMLNonuniqueCapability, PinnedTextCapability, kerml_capability, kerml_string_rules,
    nonunique_draft_capability, sysml_capability, sysml_setting_providers,
};
use intersect_model::core::frozen::ClassId;
use intersect_model::core::frozen::FeatureId;
use intersect_model::core::frozen::FrozenError;
use intersect_model::core::frozen::FrozenStore;
use intersect_model::core::frozen::LinkView;
use intersect_model::core::frozen::Metamodel;
use intersect_model::core::frozen::Model as FrozenModel;
use intersect_model::core::frozen::ObjId;
use intersect_model::core::frozen::Severity;
use intersect_model::core::frozen::Validators;
use intersect_model::core::frozen::Violation;
use intersect_model::core::model::Model;
use intersect_model::core::query::QueryBudget;
use intersect_model::core::storage;
use intersect_model::core::storage::ModelStore;
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::rcore_scope::VerifiedScope;
use intersect_model::definitions::setting::ComputedSettingContract;
use intersect_model::frozen_scopes::Execution;
use intersect_model::frozen_scopes::RcoreScopes;
use intersect_model::setting::SourceSettingValues;
use intersect_text::preparation::PreparedLanguageDefinition;
use intersect_text::setting::CheckedDraftTextSettings;
use intersect_text::{
    EvaluationLimits, Source, StoredEvaluation,
    document::AnalysisLimits,
    draft::{DraftCapability, DraftLanguage, OpenDraftCapability},
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};

pub use crate::language_pack::frozen_metamodel;

/// Evaluation limits for library parsing: nested library expressions go
/// deeper than the default 64 rule levels (one per KerML operator precedence
/// level), and the live callers need a finite step pool far above the
/// library's needs.
pub fn limits() -> EvaluationLimits {
    EvaluationLimits {
        steps_per_resource: Some(4_000_000_000),
        nesting: 256,
        ..EvaluationLimits::native()
    }
}

/// The runtime prediction memo mode a host selected with the environment
/// variable `INTERSECT_PREDICTION_MEMO` (`off`, `on` or `verify`; unset is
/// `on`). For measurement and verification hosts; the library default is on.
pub fn prediction_memo_mode_from_env()
-> Result<intersect_text::grammar::RuntimePredictionMemoMode, String> {
    use intersect_text::grammar::RuntimePredictionMemoMode as Mode;
    match std::env::var("INTERSECT_PREDICTION_MEMO").as_deref() {
        Err(_) | Ok("on") => Ok(Mode::On),
        Ok("off") => Ok(Mode::Off),
        Ok("verify") => Ok(Mode::Verify),
        Ok(other) => Err(format!(
            "INTERSECT_PREDICTION_MEMO must be off, on or verify, not {other:?}"
        )),
    }
}

/// One bound language with its checked computed settings.
pub struct BoundLanguage<'d, 'i, C: DraftCapability> {
    pub language: DraftLanguage<'d, 'i, OpenDraftCapability<'d, C>>,
    pub settings: CheckedDraftTextSettings<'d>,
}

impl<C: DraftCapability> BoundLanguage<'_, '_, C> {
    /// Parse one source into `store` with `limits()` and these settings:
    /// the evaluation, the reference count and the consumed input count.
    pub fn parse<S: ModelStore>(
        &self,
        source: &Source,
        store: &mut S,
    ) -> Result<(StoredEvaluation, usize, usize), String> {
        self.parse_with(source, limits(), store, &mut QueryBudget::new(usize::MAX))
    }
    /// `parse` with explicit evaluation limits and the caller's work budget.
    pub fn parse_with<S: ModelStore>(
        &self,
        source: &Source,
        limits: EvaluationLimits,
        store: &mut S,
        caller: &mut QueryBudget,
    ) -> Result<(StoredEvaluation, usize, usize), String> {
        self.language.parse_runtime_into_with_settings(
            source,
            limits,
            Some(&self.settings),
            store,
            caller,
        )
    }
}

/// The inner capability of the bound KerML language.
pub type KerMLCapability = KerMLNonuniqueCapability<PinnedTextCapability>;
/// The inner capability of the bound SysML language.
pub type SysMLCapability = KerMLNonuniqueCapability<PinnedTextCapability, 2>;

/// Bind the prepared KerML owner and run `body` with it. The bound language
/// borrows its capability, so it is handed to a callback instead of returned.
pub fn with_kerml<'i, T>(
    definition: &PreparedLanguageDefinition<'i>,
    contracts: &[ComputedSettingContract],
    body: impl FnOnce(&BoundLanguage<'_, 'i, KerMLCapability>) -> Result<T, String>,
) -> Result<T, String> {
    let mut budget = QueryBudget::new(usize::MAX);
    let inner = kerml_capability(definition, contracts)?;
    let inner = nonunique_draft_capability(definition, inner, &mut budget)?;
    let names = kerml_string_rules(definition, &mut budget)?;
    let open = OpenDraftCapability::new(definition, &inner)?.with_string_rules(&names);
    let language = DraftLanguage::bind(definition, &open, AnalysisLimits::workbench())?;
    let operands = crate::operand_settings(definition, contracts)?;
    let providers: Vec<&dyn SourceSettingValues> = operands
        .iter()
        .map(|provider| provider as &dyn SourceSettingValues)
        .collect();
    let settings = CheckedDraftTextSettings::bind(definition, &providers)?;
    body(&BoundLanguage { language, settings })
}

/// Bind the prepared SysML owner and run `body` with it.
pub fn with_sysml<'i, T>(
    definition: &PreparedLanguageDefinition<'i>,
    contracts: &[ComputedSettingContract],
    preparation: &InvocationContext<'_>,
    body: impl FnOnce(&BoundLanguage<'_, 'i, SysMLCapability>) -> Result<T, String>,
) -> Result<T, String> {
    let mut budget = QueryBudget::new(usize::MAX);
    let inner = sysml_capability(definition, preparation, &mut budget)?;
    let providers = sysml_setting_providers(definition, contracts)?;
    let provider_refs: Vec<&dyn SourceSettingValues> =
        providers.iter().map(|provider| provider.as_ref()).collect();
    let settings = CheckedDraftTextSettings::bind(definition, &provider_refs)?;
    let names = kerml_string_rules(definition, &mut budget)?;
    let open = OpenDraftCapability::new(definition, &inner)?.with_string_rules(&names);
    let language = DraftLanguage::bind(definition, &open, AnalysisLimits::workbench())?;
    body(&BoundLanguage { language, settings })
}

/// The verified KerML Rcore bindings (`bind scope`, `bind default` and
/// `bind implicit`) of `crate::kerml_bindings`, with the definition bundle
/// they were compiled from.
pub struct KerMLBindings {
    bundle: DefinitionBundle,
    scopes: Vec<VerifiedScope>,
}

impl KerMLBindings {
    /// Bindings compiled or loaded by the host; they must be exactly the
    /// recipe jobs' results (`kerml_bindings::check_recipe_results`).
    pub fn new(scopes: Vec<VerifiedScope>) -> Result<Self, String> {
        crate::kerml_bindings::check_recipe_results(&scopes)?;
        Ok(Self {
            bundle: crate::kerml_bindings::bundle(),
            scopes,
        })
    }
    /// Bindings saved by `kerml_bindings::save` with this compiler identity.
    /// Like a language pack, the saved text is the host's own output; never
    /// load one received from a network, a browser or a model.
    pub fn load_saved(saved: &str) -> Result<Self, String> {
        Self::new(crate::kerml_bindings::load(saved)?)
    }
    pub fn scopes(&self) -> &[VerifiedScope] {
        &self.scopes
    }
}

/// What a load reports besides the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadPhase {
    /// Parsing and construction of one source.
    Parse,
    /// Name resolution of one cross reference, or a link violation.
    Link,
    /// Completion of the implicit elements.
    Implicit,
    /// Completion of the construction defaults.
    Defaults,
    /// Structural validation of the completed model.
    Validate,
    /// The language's validation rules (`bind validate`) over the objects
    /// this load added.
    Check,
}

/// One problem found by a load. `resource`, `start` and `end` locate it in a
/// source when it is known; `object` is the element identity it concerns.
///
/// `severity` is `Error` for every parse, link, completion and structural
/// problem; a validation rule (`LoadPhase::Check`) may also report warnings
/// and information, which do not make a load unclean (`is_error`). `code` is
/// the rule's issue code (for rules ported from the Pilot, the Pilot's code
/// verbatim), the `Violation` variant name for a structural problem
/// (`Multiplicity`, `TargetType`, ...), and none for parse and link problems.
/// `rule` identifies the rule where several share a code; `feature` names the
/// feature the problem concerns, when the rule names one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LoadDiagnostic {
    pub phase: LoadPhase,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<usize>,
}

impl LoadDiagnostic {
    /// An error without a code or a location: a parse, link or completion
    /// problem, before the caller locates it.
    pub fn error(phase: LoadPhase, message: String) -> Self {
        Self {
            phase,
            severity: Severity::Error,
            code: None,
            rule: None,
            feature: None,
            message,
            resource: None,
            object: None,
            start: None,
            end: None,
        }
    }
    /// Whether this diagnostic makes a load unclean: every severity `Error`.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

/// Choices of one load.
#[derive(Clone, Debug)]
pub struct LoadOptions {
    /// Parse limits for every source.
    pub limits: EvaluationLimits,
    /// The parse work of all sources together (`QueryBudget` units).
    pub work: usize,
    /// Features whose absent values are left to the language when validating
    /// (`LinkedModel::validate_delegating`): their bounds are not checked.
    pub delegated: Vec<String>,
}

impl Default for LoadOptions {
    fn default() -> Self {
        Self {
            limits: limits(),
            work: usize::MAX,
            delegated: Vec::new(),
        }
    }
}

/// Parse KerML `sources` with the prepared KerML owner into one frozen store
/// and export the construction, without linking: cross references stay
/// unresolved (`?spelling`). Returns the construction and the parse
/// diagnostics. This needs no Rcore bindings.
pub fn construct_kerml(
    definition: &PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
    sources: &[Source],
    limits: EvaluationLimits,
    caller: &mut QueryBudget,
) -> Result<(Model, Vec<LoadDiagnostic>), String> {
    let metamodel = frozen_metamodel(definition)?;
    let (store, diagnostics) = with_kerml(definition, contracts, |language| {
        Ok(construct(language, &metamodel, sources, limits, caller))
    })?;
    Ok((storage::export(&store)?, diagnostics))
}

/// The result of one load: the validated model when every phase passed, the
/// diagnostics of every phase that ran, and the reference counts.
#[derive(Debug, Serialize)]
pub struct KerMLLoad {
    pub model: Option<Model>,
    pub diagnostics: Vec<LoadDiagnostic>,
    /// Cross references the linker asked the Rcore scopes to resolve.
    pub references: usize,
    /// Of those, the ones that resolved to exactly one element.
    pub resolved: usize,
}

/// Parse every source with a bound language into one `FrozenStore` of
/// `metamodel` and return it with the parse diagnostics. A source whose parse
/// fails leaves the store unchanged. `caller` bounds the parse work of all
/// sources together.
pub fn construct<C: DraftCapability>(
    language: &BoundLanguage<'_, '_, C>,
    metamodel: &Arc<Metamodel>,
    sources: &[Source],
    limits: EvaluationLimits,
    caller: &mut QueryBudget,
) -> (FrozenStore, Vec<LoadDiagnostic>) {
    let mut store = FrozenStore::new(metamodel.clone());
    let mut diagnostics = Vec::new();
    for source in sources {
        match language.parse_with(source, limits, &mut store, caller) {
            Ok((evaluation, _, _)) => {
                diagnostics.extend(evaluation.diagnostics.into_iter().map(|d| LoadDiagnostic {
                    resource: Some(source.uri.clone()),
                    start: Some(d.start),
                    end: Some(d.end),
                    ..LoadDiagnostic::error(LoadPhase::Parse, d.message)
                }))
            }
            Err(error) => diagnostics.push(LoadDiagnostic {
                resource: Some(source.uri.clone()),
                ..LoadDiagnostic::error(LoadPhase::Parse, error)
            }),
        }
    }
    (store, diagnostics)
}

/// Load KerML `sources` with the prepared KerML owner (`definition` and its
/// `contracts`, for example from `NativeKerMLSession::with_definition`):
/// parse, link with `bindings`, complete implicit elements and defaults, and
/// validate. Parse diagnostics do not stop the load; a failing link,
/// completion or validation does. `Err` is reserved for setup failures (an
/// unsupported definition or bindings that do not fit its metamodel).
pub fn load_kerml(
    definition: &PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
    bindings: &KerMLBindings,
    sources: &[Source],
    options: &LoadOptions,
) -> Result<KerMLLoad, String> {
    let metamodel = frozen_metamodel(definition)?;
    let mut caller = QueryBudget::new(options.work);
    let (store, mut diagnostics) = with_kerml(definition, contracts, |language| {
        Ok(construct(
            language,
            &metamodel,
            sources,
            options.limits,
            &mut caller,
        ))
    })?;
    // Element locations, for diagnostics that name an element.
    let located: BTreeMap<String, (String, usize, usize)> = storage::export(&store)?
        .elements
        .into_iter()
        .map(|element| (element.id, (element.resource, element.start, element.end)))
        .collect();
    let locate = |phase: LoadPhase, object: Option<&str>, message: String| {
        let place = object.and_then(|id| located.get(id));
        LoadDiagnostic {
            resource: place.map(|(resource, ..)| resource.clone()),
            object: object.map(str::to_owned),
            start: place.map(|(_, start, _)| *start),
            end: place.map(|(.., end)| *end),
            ..LoadDiagnostic::error(phase, message)
        }
    };
    let violations = |phase: LoadPhase, found: &[Violation]| -> Vec<LoadDiagnostic> {
        found
            .iter()
            .map(|violation| LoadDiagnostic {
                code: Some(violation_code(violation).to_owned()),
                ..locate(phase, violation_object(violation), violation.to_string())
            })
            .collect()
    };
    let resolvers = RcoreScopes::new(&metamodel, &bindings.bundle, &bindings.scopes)
        .map_err(|error| error.to_string())?
        .with_execution(Execution::Fast);
    let outcomes = RefCell::new(Vec::<LoadDiagnostic>::new());
    let counts = RefCell::new((0usize, 0usize));
    let resolve = |view: &LinkView<'_>,
                   owner: ObjId,
                   feature: FeatureId,
                   spelling: &str|
     -> Result<Option<ObjId>, FrozenError> {
        let found = resolvers.candidates(view, owner, feature, spelling);
        let mut counts = counts.borrow_mut();
        counts.0 += 1;
        let unique = match &found {
            Ok(candidates) if candidates.len() == 1 => candidates.first().copied(),
            _ => None,
        };
        if unique.is_some() {
            counts.1 += 1;
        } else {
            let owner = view.identity(owner).unwrap_or("");
            let feature = metamodel.feature_name(feature);
            let message = match &found {
                Ok(candidates) if candidates.is_empty() => {
                    format!("{owner}.{feature}: no element named '{spelling}'")
                }
                Ok(candidates) => format!(
                    "{owner}.{feature}: '{spelling}' is ambiguous ({} candidates)",
                    candidates.len()
                ),
                Err(error) => format!("{owner}.{feature}: '{spelling}': {error}"),
            };
            outcomes
                .borrow_mut()
                .push(locate(LoadPhase::Link, Some(owner), message));
        }
        Ok(unique)
    };
    let builder = store.into_builder().map_err(|error| error.to_string())?;
    let linked = builder.link(&resolve);
    diagnostics.extend(outcomes.into_inner());
    let (references, resolved) = counts.into_inner();
    let result = |model: Option<Model>, diagnostics: Vec<LoadDiagnostic>| KerMLLoad {
        model,
        diagnostics,
        references,
        resolved,
    };
    let linked = match linked {
        Ok(linked) => linked,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Link, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    let linked = match linked.complete_implicit(&resolvers) {
        Ok(linked) => linked,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Implicit, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    let linked = match linked.complete_defaults(&resolvers) {
        Ok(linked) => linked,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Defaults, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    let delegated = |_: ClassId, feature: FeatureId| {
        options
            .delegated
            .iter()
            .any(|name| name == metamodel.feature_name(feature))
    };
    let validated = if options.delegated.is_empty() {
        linked.validate()
    } else {
        linked.validate_delegating(Some(&delegated))
    };
    match validated {
        Ok(model) => {
            diagnostics.extend(check(&model, &resolvers));
            let model = model.to_model().map_err(|error| error.to_string())?;
            Ok(result(Some(model), diagnostics))
        }
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Validate, &found.0));
            Ok(result(None, diagnostics))
        }
    }
}

/// The result of [`load_frozen`]: the validated frozen model when every
/// phase passed, the diagnostics of every phase that ran, and the reference
/// counts of the link phase.
pub struct FrozenLoad {
    pub model: Option<FrozenModel>,
    pub diagnostics: Vec<LoadDiagnostic>,
    /// Cross references the linker asked the Rcore scopes to resolve.
    pub references: usize,
    /// Of those, the ones that resolved to exactly one element.
    pub resolved: usize,
}

impl FrozenLoad {
    /// A model, no diagnostic of severity error and every reference
    /// resolved uniquely. Warnings and information do not count.
    pub fn is_clean(&self) -> bool {
        self.model.is_some()
            && !self.diagnostics.iter().any(LoadDiagnostic::is_error)
            && self.resolved == self.references
    }
}

/// Parse `sources` into `store` with `parse` (the host picks the bound
/// language of each source), then link the new cross references with
/// `resolvers`, complete implicit elements and defaults, and validate: the
/// whole-library load of `frozen_library_load` and the load of user files on
/// top of a prepared library (`store` from `FrozenStore::from_model`, whose
/// objects count as already linked and completed) alike. Parse diagnostics
/// do not stop the load; a failing link, completion or validation does.
///
/// After structural validation, the language's validation rules
/// (`bind validate` in `resolvers`) run over the objects this load added
/// (`check`); their issues are located at their objects, and only issues of
/// severity error make the load unclean (`FrozenLoad::is_clean`). The model
/// is returned whenever structural validation passed.
///
/// `observe` receives `parse_end`, `link_end`, `implicit_end`,
/// `defaults_end`, `validate_end` and `check_end` as each phase finishes, so
/// a host can time the phases (this library reads no clock). Unlike
/// `load_kerml`, structural diagnostics name the element they concern but are
/// not located in the source: locating them needs an export of the whole
/// store, which a library-sized load should not pay for.
pub fn load_frozen(
    mut store: FrozenStore,
    sources: &[Source],
    mut parse: impl FnMut(&Source, &mut FrozenStore) -> Result<(StoredEvaluation, usize, usize), String>,
    resolvers: &RcoreScopes<'_>,
    observe: &dyn Fn(&'static str),
) -> Result<FrozenLoad, String> {
    let metamodel = store.metamodel().clone();
    let mut diagnostics = Vec::new();
    for source in sources {
        match parse(source, &mut store) {
            Ok((evaluation, _, _)) => {
                diagnostics.extend(evaluation.diagnostics.into_iter().map(|d| LoadDiagnostic {
                    resource: Some(source.uri.clone()),
                    start: Some(d.start),
                    end: Some(d.end),
                    ..LoadDiagnostic::error(LoadPhase::Parse, d.message)
                }))
            }
            Err(error) => diagnostics.push(LoadDiagnostic {
                resource: Some(source.uri.clone()),
                ..LoadDiagnostic::error(LoadPhase::Parse, error)
            }),
        }
    }
    observe("parse_end");
    let unlocated = |phase: LoadPhase, object: Option<&str>, message: String| LoadDiagnostic {
        object: object.map(str::to_owned),
        ..LoadDiagnostic::error(phase, message)
    };
    let violations = |phase: LoadPhase, found: &[Violation]| -> Vec<LoadDiagnostic> {
        found
            .iter()
            .map(|violation| LoadDiagnostic {
                code: Some(violation_code(violation).to_owned()),
                ..unlocated(phase, violation_object(violation), violation.to_string())
            })
            .collect()
    };
    let outcomes = RefCell::new(Vec::<LoadDiagnostic>::new());
    let counts = RefCell::new((0usize, 0usize));
    let resolve = |view: &LinkView<'_>,
                   owner: ObjId,
                   feature: FeatureId,
                   spelling: &str|
     -> Result<Option<ObjId>, FrozenError> {
        let found = resolvers.candidates(view, owner, feature, spelling);
        let mut counts = counts.borrow_mut();
        counts.0 += 1;
        let unique = match &found {
            Ok(candidates) if candidates.len() == 1 => candidates.first().copied(),
            _ => None,
        };
        if unique.is_some() {
            counts.1 += 1;
        } else {
            let owner_id = owner;
            let owner = view.identity(owner).unwrap_or("");
            let feature = metamodel.feature_name(feature);
            let message = match &found {
                Ok(candidates) if candidates.is_empty() => {
                    format!("{owner}.{feature}: no element named '{spelling}'")
                }
                Ok(candidates) => format!(
                    "{owner}.{feature}: '{spelling}' is ambiguous ({} candidates)",
                    candidates.len()
                ),
                Err(error) => format!("{owner}.{feature}: '{spelling}': {error}"),
            };
            // Located at the referencing element: its resource and span.
            let mut diagnostic = unlocated(LoadPhase::Link, Some(owner), message);
            diagnostic.resource = view.resource(owner_id).map(str::to_owned);
            if let Some((start, end)) = view.span(owner_id).filter(|(s, e)| s < e) {
                diagnostic.start = Some(start);
                diagnostic.end = Some(end);
            }
            outcomes.borrow_mut().push(diagnostic);
        }
        Ok(unique)
    };
    let builder = store.into_builder().map_err(|error| error.to_string())?;
    let linked = builder.link(&resolve);
    observe("link_end");
    diagnostics.extend(outcomes.into_inner());
    let (references, resolved) = counts.into_inner();
    let result = |model: Option<FrozenModel>, diagnostics: Vec<LoadDiagnostic>| FrozenLoad {
        model,
        diagnostics,
        references,
        resolved,
    };
    let linked = match linked {
        Ok(linked) => linked,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Link, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    let linked = linked.complete_implicit(resolvers);
    observe("implicit_end");
    let linked = match linked {
        Ok(linked) => linked,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Implicit, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    let linked = linked.complete_defaults(resolvers);
    observe("defaults_end");
    let linked = match linked {
        Ok(linked) => linked,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Defaults, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    let validated = linked.validate();
    observe("validate_end");
    let model = match validated {
        Ok(model) => model,
        Err(found) => {
            diagnostics.extend(violations(LoadPhase::Validate, &found.0));
            return Ok(result(None, diagnostics));
        }
    };
    diagnostics.extend(check(&model, resolvers));
    observe("check_end");
    Ok(result(Some(model), diagnostics))
}

/// Run the language's validation rules (`bind validate`) over the objects of
/// `model` that its load added (`Model::check`) and locate each issue at its
/// object: resource and source span. The model is not changed.
pub fn check(model: &FrozenModel, validators: &dyn Validators) -> Vec<LoadDiagnostic> {
    model
        .check(validators)
        .into_iter()
        .map(|found| {
            let object = model.obj(found.object);
            let span = object.map(|object| object.span()).filter(|(s, e)| s < e);
            LoadDiagnostic {
                phase: LoadPhase::Check,
                severity: found.issue.severity,
                code: Some(found.issue.code),
                rule: found.issue.rule,
                feature: found.issue.feature,
                message: found.issue.message,
                resource: object.map(|object| object.resource().to_owned()),
                object: object.map(|object| object.identity().to_owned()),
                start: span.map(|(start, _)| start),
                end: span.map(|(_, end)| end),
            }
        })
        .collect()
}

/// The stable code of a structural violation: its variant name.
pub fn violation_code(violation: &Violation) -> &'static str {
    match violation {
        Violation::UnknownClass(_) => "UnknownClass",
        Violation::MultipleContainers(_) => "MultipleContainers",
        Violation::ContainmentCycle(_) => "ContainmentCycle",
        Violation::Abstract { .. } => "Abstract",
        Violation::Multiplicity { .. } => "Multiplicity",
        Violation::Kind { .. } => "Kind",
        Violation::Attribute { .. } => "Attribute",
        Violation::TargetType { .. } => "TargetType",
        Violation::Duplicate { .. } => "Duplicate",
        Violation::Unresolved { .. } => "Unresolved",
        Violation::ReferenceCycle { .. } => "ReferenceCycle",
        Violation::Opposite { .. } => "Opposite",
        Violation::Resolver(_) => "Resolver",
    }
}

/// The element a violation concerns.
fn violation_object(violation: &Violation) -> Option<&str> {
    match violation {
        Violation::UnknownClass(object)
        | Violation::MultipleContainers(object)
        | Violation::ContainmentCycle(object)
        | Violation::Abstract { object, .. }
        | Violation::Multiplicity { object, .. }
        | Violation::Kind { object, .. }
        | Violation::Attribute { object, .. }
        | Violation::TargetType { object, .. }
        | Violation::Duplicate { object, .. }
        | Violation::Unresolved { object, .. }
        | Violation::ReferenceCycle { object, .. }
        | Violation::Opposite { object, .. } => Some(object),
        Violation::Resolver(_) => None,
    }
}
