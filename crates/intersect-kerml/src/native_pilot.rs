//! Handwritten native Pilot session. Hosts own files and clocks; the session owns
//! the two genuine original grammar compilations and their migration provenance.
//! Construction/linking, selected checks and whole-language validity are distinct.
//! A session is either prepared from the original sources or restored from a
//! HOST-BUILT language pack (`from_pack`, see `language_pack`): the pack holds
//! each owner's saved compiled draft and origin, a trusted host cache under the
//! `VerifiedExecutable::load_trusted` convention, not cryptography.
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::bundle::DefinitionSource;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::pack::LanguagePack;
use intersect_model::definitions::pack::PackHook;
use intersect_model::definitions::schema::Schema;
use intersect_model::definitions::setting::ComputedSettingContract;
use intersect_text::grammar::Profile;
use intersect_text::grammar::RuntimeAdmissionLoadMode;
use intersect_text::grammar::RuntimeGuardedPolicy;
use intersect_text::package::BundleCompilation;
use intersect_text::package::CompilationSourcePolicy;
use intersect_text::preparation::PreparedLanguageDefinition;
use intersect_text::preparation::SharedDraftRuntimeLanguageDefinition;
use intersect_text::preparation::SourceBoundDraftRuntimeDefinition;
use intersect_text::{
    Source,
    ordinary::{OrdinaryPackExpectation, SavedSourceBoundDefinition},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::sysml_native_inputs::SYSML;
const EMPTY_BEHAVIOR: &str = r#"rcore 1; package emptyRules version "1";"#;
/// KerML's expressions climb one grammar rule per operator precedence level,
/// so nested parenthesized expressions in the standard library (for example
/// VectorFunctions) need a deeper prediction call stack than the default.
pub const PREDICTION_CALL_DEPTH: usize = 512;

// Only the sealed native result can avoid replay. Arbitrary reports still
// enter the existing replaying bind_origin path.
enum DefinitionOrigin {
    Report(intersect_model::migration::BundleMigrationReport),
    Completed(intersect_model::migration::CompletedBundleMigration),
}

/// Which original program (grammar compilation) a session owner or a pack
/// part holds. Each role fixes its entry grammar, source policy, contract
/// derivation and pack section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgramRole {
    KerML,
    SysML,
}
impl ProgramRole {
    /// The original entry grammar URI.
    pub fn entry(self) -> &'static str {
        match self {
            Self::KerML => super::KERML,
            Self::SysML => SYSML,
        }
    }
    /// The pinned requested (and host) source policy of this recipe.
    pub fn policy(self) -> CompilationSourcePolicy {
        match self {
            Self::KerML => super::entry::KERML_POLICY,
            Self::SysML => super::sysml_native_inputs::SYSML_POLICY,
        }
    }
    /// Number of computed-setting contracts this program's grammar carries.
    pub fn contract_count(self) -> usize {
        match self {
            Self::KerML => 7,
            Self::SysML => 57,
        }
    }
    /// The language-pack section (and part) holding this program.
    pub fn section(self) -> &'static str {
        match self {
            Self::KerML => "program/kerml",
            Self::SysML => "program/sysml",
        }
    }
    fn validate_original(self, original: &BundleCompilation) -> Result<(), String> {
        match self {
            Self::KerML => super::entry::validate_kerml_original(original),
            Self::SysML => super::sysml_native_inputs::validate_original(original),
        }
    }
    /// The recipe's exact contract derivation, the one preparation runs.
    fn contracts(
        self,
        schema: &Schema,
        bundle: &DefinitionBundle,
    ) -> Result<Vec<ComputedSettingContract>, String> {
        match self {
            Self::KerML => super::entry::operand_contracts(schema, bundle),
            Self::SysML => super::sysml_native_inputs::sysml_contracts(schema, bundle),
        }
    }
}

/// One program part of a host-built language pack: the saved source-bound
/// definition of one session owner. Private fields and no public constructor;
/// deserializing grants nothing: only `NativePilotSession::from_pack` and
/// `NativeKerMLSession::from_pack` turn it back into an owner, after the
/// checks of `DefinitionOwner::from_saved`. The requested policy and the
/// computed-setting contracts are NOT stored a second time: they live once in
/// `definition` (restored policy, grammar `computed_settings`) and are checked
/// against the recipe on load.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedDefinitionPart {
    version: u32,
    role: ProgramRole,
    definition: SavedSourceBoundDefinition,
}
const SAVED_PART_VERSION: u32 = 1;
/// One owner's saved part with its role and in-memory contracts.
pub(super) type SavedOwner<'a> = (
    ProgramRole,
    SavedDefinitionPart,
    &'a [ComputedSettingContract],
);

/// One program owner restored from a previous pack's part through the
/// `HostCache` checks of `DefinitionOwner::from_saved` (the part-level half of
/// `NativePilotSession::from_pack`; the caller checks the manifest part key
/// and section digests first). Only `NativePilotSession::assemble_with_progress`
/// consumes it, for the same role, and then runs `inventory`.
pub(super) struct RestoredOwner {
    role: ProgramRole,
    owner: DefinitionOwner,
}
impl RestoredOwner {
    pub(super) fn load(pack: &LanguagePack, role: ProgramRole) -> Result<Self, String> {
        Ok(Self {
            role,
            owner: DefinitionOwner::from_pack(pack, role, RuntimeAdmissionLoadMode::HostCache)?,
        })
    }
    fn into_owner(self, role: ProgramRole) -> Result<DefinitionOwner, String> {
        if self.role != role {
            return Err(format!(
                "Restored {:?} owner offered as the {role:?} owner",
                self.role
            ));
        }
        Ok(self.owner)
    }
}

struct DefinitionOwner {
    bound: SourceBoundDraftRuntimeDefinition,
    contracts: Vec<ComputedSettingContract>,
    policy: CompilationSourcePolicy,
}
impl DefinitionOwner {
    fn prepare_with_progress(
        original: &BundleCompilation,
        role: ProgramRole,
        observe: &impl Fn(&'static str),
    ) -> Result<Self, String> {
        observe("input_recipe_begin");
        let (input, origin, contracts, policy) = if role == ProgramRole::SysML {
            let (input, migration, contracts, policy) =
                super::sysml_native_inputs::prepare_native_sysml_inputs(original, observe)?;
            (
                input,
                DefinitionOrigin::Report(migration),
                contracts,
                policy,
            )
        } else {
            let (input, migration, contracts, policy) =
                super::entry::prepare_minimal_inputs_completed_with_progress(original, observe)?;
            (
                input,
                DefinitionOrigin::Completed(migration),
                contracts,
                policy,
            )
        };
        observe("input_recipe_end");
        observe("runtime_compile_begin");
        let owner = intersect_text::ordinary::prepare_with_runtime_guarded_policy_and_call_depth(
            input,
            Profile::NativeV80,
            &contracts,
            None,
            policy,
            policy,
            RuntimeGuardedPolicy::InputCheckedCompletionScopes,
            None,
            PREDICTION_CALL_DEPTH,
        )
        .map_err(|errors| format!("Original native definition preparation failed: {errors:?}"))?;
        observe("runtime_compile_end");
        observe("origin_binding_begin");
        let shared = SharedDraftRuntimeLanguageDefinition::new(Arc::new(owner));
        let bound = match origin {
            DefinitionOrigin::Report(migration) => shared.bind_origin(migration, policy),
            DefinitionOrigin::Completed(migration) => {
                shared.bind_completed_origin(migration, policy)
            }
        }
        .map_err(|errors| format!("Original native origin binding failed: {errors:?}"))?;
        observe("origin_binding_end");
        Ok(Self {
            bound,
            contracts,
            policy,
        })
    }
    fn view(&self) -> Result<PreparedLanguageDefinition<'_>, String> {
        self.bound
            .prepare(self.policy)
            .map_err(|errors| format!("Original native definition view failed: {errors:?}"))
    }
    fn save(&self, role: ProgramRole) -> Result<SavedDefinitionPart, String> {
        Ok(SavedDefinitionPart {
            version: SAVED_PART_VERSION,
            role,
            definition: self.bound.save()?,
        })
    }
    fn from_pack(
        pack: &LanguagePack,
        role: ProgramRole,
        mode: RuntimeAdmissionLoadMode,
    ) -> Result<Self, String> {
        let part: SavedDefinitionPart = pack
            .section_json(role.section())
            .map_err(|error| format!("Language pack {}: {error}", role.section()))?;
        Self::from_saved(part, role, mode, &pack.manifest().requires_hooks)
    }
    /// Restore one owner from its pack part. Order:
    /// 1. part version 1 and `part.role == role` (a SysML payload in the KerML
    ///    slot is refused);
    /// 2. `ordinary::load_from_pack` with the role policy as host and as the
    ///    EXACT requested policy, and the NativeV80 InputCheckedCompletionScopes
    ///    admission at `PREDICTION_CALL_DEPTH` (Revalidate also re-verifies the
    ///    admission and replays the migration origin there);
    /// 3. the original selection is rebuilt from the restored original
    ///    sources by the same `original_compilation`, must pass the recipe's
    ///    own original check, EQUAL the restored original bundle, and remap
    ///    through the restored migration to EXACTLY the restored converted
    ///    selection (bundle, roles, namespace, no materialized types);
    /// 4. the grammar's computed settings EQUAL the recipe's contract
    ///    derivation over the restored schema and converted bundle (each
    ///    contract re-validated by its provider constructor);
    /// 5. every contract provider is a hook the pack manifest declares.
    fn from_saved(
        part: SavedDefinitionPart,
        role: ProgramRole,
        mode: RuntimeAdmissionLoadMode,
        hooks: &[PackHook],
    ) -> Result<Self, String> {
        let SavedDefinitionPart {
            version,
            role: saved_role,
            definition,
        } = part;
        if version != SAVED_PART_VERSION {
            return Err(format!("Unsupported saved program part version {version}"));
        }
        if saved_role != role {
            return Err(format!(
                "Language pack part {} holds the {saved_role:?} program",
                role.section()
            ));
        }
        let policy = role.policy();
        let bound = intersect_text::ordinary::load_from_pack(
            definition,
            policy,
            mode,
            OrdinaryPackExpectation {
                requested: policy,
                policy: RuntimeGuardedPolicy::InputCheckedCompletionScopes,
                call_depth: PREDICTION_CALL_DEPTH,
            },
        )
        .map_err(|errors| format!("Language pack {} load failed: {errors:?}", role.section()))?;
        let contracts = {
            let view = bound
                .prepare(policy)
                .map_err(|errors| format!("Loaded native definition view failed: {errors:?}"))?;
            check_original_selection(role, &view)?;
            let contracts = view
                .grammar()
                .ok_or("Loaded native definition has no grammar")?
                .computed_settings
                .clone();
            check_contracts(role, view.schema(), view.sources(), &contracts)?;
            contracts
        };
        if let Some(contract) = contracts
            .iter()
            .find(|contract| !hooks.iter().any(|hook| hook.provider == contract.provider))
        {
            return Err(format!(
                "Language pack does not declare the setting provider {}@{}",
                contract.provider.name, contract.provider.version
            ));
        }
        Ok(Self {
            bound,
            contracts,
            policy,
        })
    }
}

/// The recipe's contract derivation over `schema` and its converted `bundle`
/// must reproduce `contracts` exactly. Preparation produces its contracts by
/// that same derivation, so this is the shared check, not a second one.
fn check_contracts(
    role: ProgramRole,
    schema: &Schema,
    bundle: &DefinitionBundle,
    contracts: &[ComputedSettingContract],
) -> Result<(), String> {
    let expected = role.contracts(schema, bundle)?;
    if expected.len() != role.contract_count() || contracts != expected.as_slice() {
        return Err(format!(
            "Loaded {role:?} computed-setting contracts differ from the recipe derivation"
        ));
    }
    Ok(())
}

/// Tie a restored definition to the recipe: rebuild the original selection
/// from its own original sources and require the exact converted selection.
fn check_original_selection(
    role: ProgramRole,
    definition: &PreparedLanguageDefinition<'_>,
) -> Result<(), String> {
    let restored = definition.original_sources();
    let sources: Vec<_> = restored
        .sources
        .iter()
        .map(|source| Source {
            uri: source.id.uri.clone(),
            text: source.text.clone(),
        })
        .collect();
    let original = original_compilation(&sources, role.entry())?;
    role.validate_original(&original)?;
    if original.bundle != *restored {
        return Err(format!(
            "Loaded {role:?} original sources differ from the recipe's original selection"
        ));
    }
    let expected = intersect_text::migration::remap_compilation(definition.migration(), &original)?;
    let actual = definition.selection();
    if !actual.materialized_types.is_empty()
        || &expected.bundle != actual.bundle
        || &expected.ecore != actual.ecore
        || expected.namespace != actual.namespace
        || expected.xtext.as_ref() != actual.xtext
        || &expected.rcore_behavior != actual.rcore_behavior
    {
        return Err(format!(
            "Loaded {role:?} converted selection differs from the recipe's remapped selection"
        ));
    }
    Ok(())
}

fn original_compilation(definitions: &[Source], entry: &str) -> Result<BundleCompilation, String> {
    let select = |uri: &str, kind| -> Result<DefinitionSource, String> {
        let mut selected = definitions.iter().filter(|source| source.uri == uri);
        let source = selected
            .next()
            .ok_or_else(|| format!("Missing original definition: {uri}"))?;
        if selected.next().is_some() {
            return Err(format!("Duplicate original definition: {uri}"));
        }
        Ok(DefinitionSource::new(uri, kind, source.text.clone()))
    };
    let ecore = select(super::SCHEMA, SourceKind::Ecore)?;
    let mut expressions = select(super::EXPRESSIONS, SourceKind::Xtext)?;
    expressions.dependencies = vec![ecore.id.clone()];
    let mut grammar = select(entry, SourceKind::Xtext)?;
    grammar.dependencies = vec![ecore.id.clone(), expressions.id.clone()];
    let mut behavior = DefinitionSource::new(
        "probe-behavior.rcore",
        SourceKind::RcoreBehavior,
        EMPTY_BEHAVIOR,
    );
    behavior.dependencies = vec![ecore.id.clone()];
    Ok(BundleCompilation {
        ecore: ecore.id.clone(),
        materialized_types: vec![],
        xtext: Some(grammar.id.clone()),
        rcore_behavior: behavior.id.clone(),
        namespace: "https://www.omg.org/spec/SysML/20250201".into(),
        bundle: DefinitionBundle {
            version: 1,
            sources: vec![ecore, expressions, grammar, behavior],
            namespaces: vec![],
        },
    })
}

/// Owns each original grammar compilation once per batch. It retains no host I/O,
/// provider registry or mutable graph. An owner restored by `from_pack` holds the
/// same immutable compiled draft, loaded from the host's own saved output
/// instead of compiled again; it is never an executable package.
pub struct NativePilotSession {
    kerml: DefinitionOwner,
    sysml: DefinitionOwner,
    /// The frozen metamodel of the owned definitions, frozen on first use.
    /// The owners are immutable, so one freeze serves every load.
    metamodel: std::sync::OnceLock<
        Result<std::sync::Arc<intersect_model::core::frozen::Metamodel>, String>,
    >,
}

/// KerML-only immutable preparation over the complete original metamodel and
/// original expressions/KerML grammar closure. No SysML grammar is compiled.
/// This owns no graph, resource roster or executable-provider authority.
pub struct NativeKerMLSession {
    kerml: DefinitionOwner,
}
impl NativeKerMLSession {
    pub fn prepare(
        definitions: &[Source],
        preparation: &InvocationContext<'_>,
    ) -> Result<Self, String> {
        Self::prepare_with_progress(definitions, preparation, |_| {})
    }
    /// Report at most twelve fixed setup boundaries. Hosts supply clocks and retain
    /// only bounded observations. Inner compiler work is not interruptible through
    /// these callbacks; callers needing a hard deadline must supervise the process.
    pub fn prepare_with_progress(
        definitions: &[Source],
        preparation: &InvocationContext<'_>,
        observe: impl Fn(&'static str),
    ) -> Result<Self, String> {
        if !preparation.has_observer() || definitions.len() != 3 {
            return Err(
                "KerML-only preparation requires an observer and three original definitions".into(),
            );
        }
        preparation.checkpoint()?;
        let kerml = DefinitionOwner::prepare_with_progress(
            &original_compilation(definitions, super::KERML)?,
            ProgramRole::KerML,
            &observe,
        )?;
        preparation.checkpoint()?;
        Ok(Self { kerml })
    }
    /// Restore the KerML owner from a host-built language pack (see
    /// `NativePilotSession::from_pack`); only the `program/kerml` part loads.
    pub fn from_pack(pack: &LanguagePack, mode: RuntimeAdmissionLoadMode) -> Result<Self, String> {
        super::language_pack::check_pack(pack, &[ProgramRole::KerML])?;
        Ok(Self {
            kerml: DefinitionOwner::from_pack(pack, ProgramRole::KerML, mode)?,
        })
    }
    pub fn with_definition<R>(
        &self,
        consume: impl FnOnce(
            &PreparedLanguageDefinition<'_>,
            &[ComputedSettingContract],
        ) -> Result<R, String>,
    ) -> Result<R, String> {
        let definition = self.kerml.view()?;
        consume(&definition, &self.kerml.contracts)
    }
}

#[derive(Debug, Serialize)]
pub struct NativePilotDefinitionInventory {
    pub classes: usize,
    pub enumerations: usize,
    pub concrete_classes: usize,
    pub kerml_parser_rules: usize,
    pub sysml_parser_rules: usize,
    pub kerml_setting_contracts: usize,
    pub sysml_setting_contracts: usize,
    pub original_definitions_prepared: bool,
    pub whole_language_semantics_verified: bool,
}

impl NativePilotSession {
    /// Accept exactly the four unmodified original definition files. Both
    /// language recipes independently validate their pinned closure and providers.
    pub fn prepare(
        definitions: &[Source],
        preparation: &InvocationContext<'_>,
    ) -> Result<Self, String> {
        Self::prepare_with_progress(definitions, preparation, |_| {})
    }

    /// Same two-owner preparation with finite host-selected progress markers.
    /// Clocks, I/O and hard deadlines remain outside this language library.
    pub fn prepare_with_progress(
        definitions: &[Source],
        preparation: &InvocationContext<'_>,
        observe: impl Fn(&'static str),
    ) -> Result<Self, String> {
        Self::assemble_with_progress(definitions, preparation, None, None, observe)
    }

    /// The one assembly path of a session (language-pack step 8): each owner
    /// is either a restored one (`RestoredOwner`, from a previous pack's part)
    /// or prepared from `definitions` (`original_compilation` for its entry,
    /// then `DefinitionOwner::prepare_with_progress` with its own recipe),
    /// then `inventory` checks the two owners agree. The owners are
    /// independent: neither reads the other, so one can be rebuilt while the
    /// other is restored.
    ///
    /// A restored owner is not compared with `definitions` here (only their
    /// count is checked): the caller must have pinned both to the same
    /// inputs, as `build_language_pack` does with `mismatched_definitions`
    /// and the part keys, and as `from_pack` does with the pack's original
    /// source identities.
    pub(super) fn assemble_with_progress(
        definitions: &[Source],
        preparation: &InvocationContext<'_>,
        kerml: Option<RestoredOwner>,
        sysml: Option<RestoredOwner>,
        observe: impl Fn(&'static str),
    ) -> Result<Self, String> {
        if !preparation.has_observer() {
            return Err("Native Pilot preparation requires an explicit observer".into());
        }
        preparation.checkpoint()?;
        if definitions.len() != 4 {
            return Err("Native Pilot requires the four original definition files".into());
        }
        let kerml = match kerml {
            Some(restored) => restored.into_owner(ProgramRole::KerML)?,
            None => {
                observe("kerml_owner_begin");
                let owner = DefinitionOwner::prepare_with_progress(
                    &original_compilation(definitions, super::KERML)?,
                    ProgramRole::KerML,
                    &observe,
                )?;
                observe("kerml_owner_end");
                owner
            }
        };
        preparation.checkpoint()?;
        let sysml = match sysml {
            Some(restored) => restored.into_owner(ProgramRole::SysML)?,
            None => {
                observe("sysml_owner_begin");
                let owner = DefinitionOwner::prepare_with_progress(
                    &original_compilation(definitions, SYSML)?,
                    ProgramRole::SysML,
                    &observe,
                )?;
                observe("sysml_owner_end");
                owner
            }
        };
        preparation.checkpoint()?;
        let session = Self {
            kerml,
            sysml,
            metamodel: std::sync::OnceLock::new(),
        };
        session.inventory()?;
        Ok(session)
    }

    /// Restore both owners from a HOST-BUILT language pack (bytes already
    /// verified by `LanguagePack::read`) without compiling either grammar or
    /// replaying migration (`Revalidate` re-verifies each runtime admission and
    /// replays each migration origin). Order: `language_pack::check_pack`
    /// (format, recipe, compiler identity, constants, pinned inputs, hooks,
    /// recomputed part keys and output digests); each owner's checks (see
    /// `DefinitionOwner::from_saved`); then `inventory`. Trust: the
    /// `VerifiedExecutable::load_trusted` convention, not cryptography. Any
    /// failure is an error; the host then rebuilds the pack from sources.
    pub fn from_pack(pack: &LanguagePack, mode: RuntimeAdmissionLoadMode) -> Result<Self, String> {
        super::language_pack::check_pack(pack, &[ProgramRole::KerML, ProgramRole::SysML])?;
        let kerml = DefinitionOwner::from_pack(pack, ProgramRole::KerML, mode)?;
        let sysml = DefinitionOwner::from_pack(pack, ProgramRole::SysML, mode)?;
        let session = Self {
            kerml,
            sysml,
            metamodel: std::sync::OnceLock::new(),
        };
        session.inventory()?;
        Ok(session)
    }

    /// The saved program parts, KerML then SysML, with each owner's contracts.
    pub(super) fn saved_parts(&self) -> Result<[SavedOwner<'_>; 2], String> {
        Ok([
            (
                ProgramRole::KerML,
                self.kerml.save(ProgramRole::KerML)?,
                self.kerml.contracts.as_slice(),
            ),
            (
                ProgramRole::SysML,
                self.sysml.save(ProgramRole::SysML)?,
                self.sysml.contracts.as_slice(),
            ),
        ])
    }

    pub fn inventory(&self) -> Result<NativePilotDefinitionInventory, String> {
        self.with_definitions(|kerml, _, sysml, _| {
            if kerml.schema().revision != sysml.schema().revision
                || kerml.schema().classes != sysml.schema().classes
            {
                return Err("Original language owners disagree on their combined metamodel".into());
            }
            Ok(NativePilotDefinitionInventory {
                classes: kerml.schema().classes.len(),
                enumerations: kerml.schema().enumerations.len(),
                concrete_classes: kerml
                    .schema()
                    .classes
                    .values()
                    .filter(|class| !class.abstract_type)
                    .count(),
                kerml_parser_rules: kerml
                    .grammar()
                    .ok_or("Missing original KerML grammar")?
                    .rules
                    .len(),
                sysml_parser_rules: sysml
                    .grammar()
                    .ok_or("Missing original SysML grammar")?
                    .rules
                    .len(),
                kerml_setting_contracts: self.kerml.contracts.len(),
                sysml_setting_contracts: self.sysml.contracts.len(),
                original_definitions_prepared: true,
                whole_language_semantics_verified: false,
            })
        })
    }

    /// The one frozen metamodel of both languages (the KerML and SysML owners
    /// construct the same combined SysML.ecore), as
    /// `language_pack::frozen_metamodel` freezes it from the KerML owner.
    pub fn frozen_metamodel(
        &self,
    ) -> Result<std::sync::Arc<intersect_model::core::frozen::Metamodel>, String> {
        self.metamodel
            .get_or_init(|| {
                let kerml = self.kerml.view()?;
                crate::language_pack::frozen_metamodel(&kerml)
            })
            .clone()
    }

    /// Borrow the prepared SysML definition from its immutable owner.
    pub fn sysml_definition(&self) -> Result<PreparedLanguageDefinition<'_>, String> {
        self.sysml.view()
    }

    /// Scoped views share the real immutable owners. The callback may bind the
    /// existing language services without recompiling either original grammar.
    pub fn with_definitions<R>(
        &self,
        consume: impl FnOnce(
            &PreparedLanguageDefinition<'_>,
            &[ComputedSettingContract],
            &PreparedLanguageDefinition<'_>,
            &[ComputedSettingContract],
        ) -> Result<R, String>,
    ) -> Result<R, String> {
        let kerml = self.kerml.view()?;
        let sysml = self.sysml.view()?;
        consume(&kerml, &self.kerml.contracts, &sysml, &self.sysml.contracts)
    }
}
