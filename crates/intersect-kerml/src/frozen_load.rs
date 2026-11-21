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
use intersect_model::core::frozen::ObjId;
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

// Compatibility exports: one language-neutral loader and diagnostic implementation.
use intersect_load::violation_object;
pub use intersect_load::{
    FrozenLoad, LoadDiagnostic, LoadPhase, check, load_frozen, violation_code,
};

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
