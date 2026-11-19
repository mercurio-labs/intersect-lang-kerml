//! Handwritten KerML/SysML language-pack recipe (language-pack steps 6a, 7
//! and 8). Not generated.
//!
//! A pack holds the two original program owners of a `NativePilotSession`
//! (`program/kerml`, `program/sysml`), each the saved source-bound compiled
//! draft with its migration origin, and two verified Rcore binding sets,
//! each the binding bundle and the verbatim `save_bindings` text of one
//! recipe: `rcore/bindings` (the `kerml_bindings` recipe, KerML alone, for
//! the KerML-only host) and `rcore/sysml-bindings` (the `sysml_bindings`
//! recipe, the KerML behaviors layered with the SysML ones, for the SysML
//! hosts). Loading one skips both grammar compiles, both migrations and the
//! binding compiles; see `NativePilotSession::from_pack`,
//! `NativeKerMLSession::from_pack`, `load_pack_bindings` and
//! `load_pack_sysml_bindings` for the checks that run.
//!
//! `build_language_pack` is incremental per part (step 8): each part has a
//! part key whose direct entries are only what it reads (`RecipePart`; the
//! programs are keyed on intersect-kerml's chained `SOURCE_IDENTITY`, each
//! binding part only on its binding recipe digest and intersect-model, see
//! docs/architecture.md), and a previous pack's
//! part is reused when its key matches, its section passes that part's own
//! load checks and its dependencies are reused (`plan_parts`); every other
//! part is rebuilt.
//!
//! # Trust
//!
//! A pack is the host's OWN output, trusted by convention exactly like
//! `VerifiedExecutable::load_trusted`. The digests and recomputed part keys
//! detect corruption, stale inputs and a different compiler; they are not a
//! cryptographic signature. Never load a pack received from a network, a
//! browser or a model.
//!
//! The compiler identity of each crate (`SOURCE_IDENTITY`) hashes its enabled
//! cargo features too, so a pack built by a binary with other unified
//! features (for example a workspace or `--all-features` build) is refused.
//! That fails closed: the host rebuilds the pack.
//!
//! # Oracles
//!
//! `HostCache` trusts the saved compiled drafts, migration drafts, choice
//! bindings and verified binding images. `Revalidate` additionally re-verifies
//! each runtime admission and replays each migration origin; it does NOT
//! cover the Rcore bindings (see `load_pack_bindings`). For the programs
//! (grammar and migration), only rebuilding from sources (two independent
//! preparations producing identical pack bytes) checks the compiler output
//! itself and is the determinism oracle; packing one session twice only
//! proves the encoding is deterministic. The bindings compiler output is
//! checked only by recompiling the bindings (a host's choice; `sysml_pack
//! check` does it in every mode).
//!
//! This module performs no I/O and uses no clocks, threads or environment.
use super::native_pilot::{NativePilotSession, PREDICTION_CALL_DEPTH, RestoredOwner};
pub use super::native_pilot::{ProgramRole, SavedDefinitionPart};
use crate::kerml_bindings::{self, BindingJob};
use crate::sysml_bindings;
use intersect_model::core::frozen::Metamodel;
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::bundle::SourceId;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::conversion::ProviderId;
use intersect_model::definitions::ecore::ParseLimits;
use intersect_model::definitions::pack::FileDigest;
use intersect_model::definitions::pack::LanguagePack;
use intersect_model::definitions::pack::PackBuilder;
use intersect_model::definitions::pack::PackHeader;
use intersect_model::definitions::pack::PackHook;
use intersect_model::definitions::pack::PackManifest;
use intersect_model::definitions::pack::PackPart;
use intersect_model::definitions::pack::PackRecipe;
use intersect_model::definitions::pack::PackSectionFlag;
use intersect_model::definitions::pack::PackSectionFlags;
use intersect_model::definitions::pack::PartKeyInput;
use intersect_model::definitions::pack::canonical_json;
use intersect_model::definitions::pack::part_key;
use intersect_model::definitions::rcore::feature_redefinitions;
use intersect_model::definitions::rcore_scope::VerifiedScope;
use intersect_model::definitions::resources::DeclarationId;
use intersect_model::definitions::setting::ComputedSettingContract;
use intersect_text::Source;
use intersect_text::grammar::Profile;
use intersect_text::grammar::RuntimeAdmissionLoadMode;
use intersect_text::grammar::RuntimeGuardedPolicy;
use intersect_text::preparation::PreparedLanguageDefinition;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[path = "language_pack_library.rs"]
mod library;
pub(crate) use library::identity_key;
pub use library::{
    KERML_SOURCE, LIBRARY_KIND, LIBRARY_SECTION, LibraryContext, LibraryOptions, LibraryOrigin,
    PreparedLibrary, SYSML_SOURCE, library_key, library_package_identity, library_parse_order,
    load_library_snapshot, pack_library_payload, payload_key, plan_library, prepare_library,
    with_library_part, without_kerml_bindings, without_library_part,
};

/// Optional host-built standard-library component admission cache.
pub const STANDARD_COMPONENT_SECTION: &str = "library/sysml-component";
#[cfg(feature = "components")]
#[path = "language_pack_component.rs"]
mod component_cache;
#[cfg(feature = "components")]
pub use component_cache::{
    STANDARD_COMPONENT_URI, load_standard_component_cache, pack_standard_component_payload,
    prepare_standard_component_cache, with_standard_component_part,
    without_standard_component_part,
};

/// The pack's language identity.
pub const LANGUAGE: &str = "org.omg.sysml.kerml+sysml";
/// The recipe name and version written into every pack.
pub const RECIPE_NAME: &str = "intersect.kerml.pilot";
/// Version 2 adds the required `rcore/bindings` part; version 3 adds the
/// implicit-generalization source to its inputs; version 4 adds the required
/// `rcore/sysml-bindings` part (the combined KerML+SysML bindings, which the
/// SysML hosts previously compiled into a host-side cache), its two SysML
/// behavior inputs, its compiler component and its constants. A version-3
/// pack lacks that part, so every part is rebuilt. Version 5 adds the two
/// validation-rule sources (`kerml-validation`, `sysml-validation`) to the
/// binding inputs, whose bindings now include `bind validate`. Version 6
/// stores a binding part's saved text as nested JSON (section kinds `.v2`)
/// and makes the `rcore/bindings` part optional in a pack: a build always
/// produces it, and `without_kerml_bindings` derives the pack of a host that
/// runs only the combined KerML+SysML bindings.
pub const RECIPE_VERSION: &str = "6";
/// Section kind of both program parts.
pub const PROGRAM_KIND: &str = "intersect.kerml.program.v1";
/// The section (and part) holding the verified KerML Rcore bindings.
pub const BINDINGS_SECTION: &str = "rcore/bindings";
/// Section kind of the bindings part.
pub const BINDINGS_KIND: &str = "intersect.kerml.rcore-bindings.v2";
/// Pinned input roles of the bindings part (the `kerml_bindings` bundle).
const BINDING_INPUTS: [&str; 5] = [
    "binding-structure",
    "kerml-construction",
    "kerml-implicit-generalizations",
    "kerml-name-resolution",
    "kerml-validation",
];
/// Compiler component of the KerML binding recipe: `kerml_bindings::recipe_digest`.
pub const BINDINGS_COMPONENT: &str = "intersect-kerml-bindings";
/// Compiler components the bindings part is keyed on: the binding recipe
/// (`BINDINGS_COMPONENT`, the only intersect-kerml code that decides what
/// compiles) and intersect-model, which compiles and saves the images. Not
/// intersect-kerml's or intersect-xtext's `SOURCE_IDENTITY`, so an edit there
/// keeps the bindings (see `kerml_bindings::compiler_identity`).
const BINDING_COMPONENTS: [&str; 2] = [BINDINGS_COMPONENT, "intersect-model"];
/// The section (and part) holding the verified combined KerML+SysML Rcore
/// bindings (the `sysml_bindings` recipe).
pub const SYSML_BINDINGS_SECTION: &str = "rcore/sysml-bindings";
/// Section kind of the SysML bindings part.
pub const SYSML_BINDINGS_KIND: &str = "intersect.kerml.rcore-sysml-bindings.v2";
/// Pinned input roles of the SysML bindings part (the `sysml_bindings`
/// bundle: the KerML bundle's roles and the three SysML behaviors).
const SYSML_BINDING_INPUTS: [&str; 8] = [
    "binding-structure",
    "kerml-construction",
    "kerml-implicit-generalizations",
    "kerml-name-resolution",
    "kerml-validation",
    "sysml-construction",
    "sysml-name-resolution",
    "sysml-validation",
];
/// Compiler component of the SysML binding recipe:
/// `sysml_bindings::recipe_digest`, which also covers `kerml_bindings.rs`
/// (the shared limits, compiler call and job check).
pub const SYSML_BINDINGS_COMPONENT: &str = "intersect-sysml-bindings";
/// Compiler components the SysML bindings part is keyed on, as for the KerML
/// bindings part (in manifest order: sorted).
const SYSML_BINDING_COMPONENTS: [&str; 2] = ["intersect-model", SYSML_BINDINGS_COMPONENT];
/// Compiler components the program parts are keyed on: the crates that
/// compile or load a program.
const PROGRAM_COMPONENTS: [&str; 3] = ["intersect-kerml", "intersect-model", "intersect-xtext"];
/// Pinned input roles that are original definition files (the others are
/// compiled into this crate).
const DEFINITION_INPUTS: [&str; 4] = ["expressions", "kerml-grammar", "schema", "sysml-grammar"];
/// Provenance of every hook this recipe declares: compiled into this crate.
pub const HOOK_PROVENANCE: &str = "intersect-kerml";
const PART_VERSION: u32 = 1;
const ROLES: [ProgramRole; 2] = [ProgramRole::KerML, ProgramRole::SysML];
/// Bound for reading the converted SysML structure for its redefinitions.
const STRUCTURE_LIMITS: ParseLimits = ParseLimits {
    source_bytes: 2_000_000,
};

pub fn recipe() -> PackRecipe {
    PackRecipe {
        name: RECIPE_NAME.into(),
        version: RECIPE_VERSION.into(),
    }
}

/// The compiler components the parts are keyed on: the source identity of
/// each crate that compiles or loads a program (`PROGRAM_COMPONENTS`) and the
/// binding recipe digests (`BINDINGS_COMPONENT`, `SYSML_BINDINGS_COMPONENT`).
pub fn expected_compiler() -> BTreeMap<String, String> {
    [
        ("intersect-kerml", crate::SOURCE_IDENTITY.to_owned()),
        (BINDINGS_COMPONENT, kerml_bindings::recipe_digest()),
        (SYSML_BINDINGS_COMPONENT, sysml_bindings::recipe_digest()),
        (
            "intersect-model",
            intersect_model::SOURCE_IDENTITY.to_owned(),
        ),
        (
            "intersect-xtext",
            intersect_text::compiler_identity().to_owned(),
        ),
    ]
    .into_iter()
    .map(|(name, identity)| (name.to_owned(), identity))
    .collect()
}

/// The pinned original inputs by role. The standard `Ecore.ecore` is
/// embedded in intersect-model and covered by its source identity. The
/// binding inputs are the `kerml_bindings` and `sysml_bindings` bundles
/// (which share the structure and the KerML roles), embedded in this crate:
/// the structure is pinned to the native migration output
/// (`PILOT_RCORE_SCHEMA_SHA256`), the behaviors to their embedded text.
pub fn pinned_inputs() -> BTreeMap<String, SourceId> {
    let bindings = kerml_bindings::input_ids()
        .into_iter()
        .chain(sysml_bindings::input_ids())
        .map(|(role, id)| match role {
            "binding-structure" => (
                role.to_owned(),
                SourceId {
                    uri: id.uri,
                    sha256: crate::PILOT_RCORE_SCHEMA_SHA256.into(),
                },
            ),
            _ => (role.to_owned(), id),
        });
    [
        ("expressions", super::EXPRESSIONS, super::EXPRESSION_SHA),
        ("kerml-grammar", super::KERML, super::KERML_SHA),
        (
            "probe-behavior",
            "probe-behavior.rcore",
            super::EMPTY_BEHAVIOR_SHA,
        ),
        ("schema", super::SCHEMA, crate::PILOT_SCHEMA_SHA256),
        (
            "sysml-grammar",
            super::sysml_native_inputs::SYSML,
            super::sysml_native_inputs::SYSML_SHA,
        ),
    ]
    .into_iter()
    .map(|(role, uri, sha256)| {
        (
            role.to_owned(),
            SourceId {
                uri: uri.into(),
                sha256: sha256.into(),
            },
        )
    })
    .chain(bindings)
    .collect()
}

fn grammar_input(role: ProgramRole) -> &'static str {
    match role {
        ProgramRole::KerML => "kerml-grammar",
        ProgramRole::SysML => "sysml-grammar",
    }
}

fn role_prefix(role: ProgramRole) -> &'static str {
    match role {
        ProgramRole::KerML => "kerml",
        ProgramRole::SysML => "sysml",
    }
}

/// The recipe constants a fresh preparation passes to the compiler. Keys
/// prefixed `kerml.` / `sysml.` belong to one program, keys prefixed
/// `bindings.` to the bindings part only, keys prefixed `sysml-bindings.` to
/// the SysML bindings part only; the rest to both programs.
pub fn recipe_constants() -> Result<BTreeMap<String, Value>, String> {
    let value = |value: Result<Value, serde_json::Error>| value.map_err(|e| e.to_string());
    let mut constants = BTreeMap::new();
    constants.insert(
        "grammar_profile".to_owned(),
        Value::from(Profile::NativeV80.as_str()),
    );
    constants.insert(
        "runtime_guarded_policy".to_owned(),
        value(serde_json::to_value(
            RuntimeGuardedPolicy::InputCheckedCompletionScopes,
        ))?,
    );
    constants.insert(
        "prediction_call_depth".to_owned(),
        Value::from(PREDICTION_CALL_DEPTH),
    );
    constants.insert(
        "namespace".to_owned(),
        Value::from("https://www.omg.org/spec/SysML/20250201"),
    );
    let providers: Vec<_> = hook_catalog()
        .into_iter()
        .map(|hook| hook.provider)
        .collect();
    constants.insert(
        "setting_providers".to_owned(),
        value(serde_json::to_value(providers))?,
    );
    for role in ROLES {
        let prefix = role_prefix(role);
        constants.insert(
            format!("{prefix}.source_policy"),
            value(serde_json::to_value(role.policy()))?,
        );
        constants.insert(
            format!("{prefix}.setting_contracts"),
            Value::from(role.contract_count()),
        );
    }
    constants.insert(
        "bindings.recipe".to_owned(),
        Value::from(kerml_bindings::BINDING_RECIPE),
    );
    constants.insert(
        "bindings.source_bytes".to_owned(),
        Value::from(kerml_bindings::SOURCE_LIMITS.source_bytes),
    );
    constants.insert(
        "sysml-bindings.recipe".to_owned(),
        Value::from(sysml_bindings::BINDING_RECIPE),
    );
    constants.insert(
        "sysml-bindings.source_bytes".to_owned(),
        Value::from(kerml_bindings::SOURCE_LIMITS.source_bytes),
    );
    Ok(constants)
}

/// One part of this recipe. Each part's key lists directly only what it
/// reads, so a change of another direct entry does not invalidate it:
/// - a program part on `PROGRAM_COMPONENTS`, the shared and its own
///   constants (not the other program's, not `bindings.`), and the inputs
///   `schema`, `expressions`, `probe-behavior` and its own grammar;
/// - the bindings part on `BINDING_COMPONENTS`, the `bindings.` constants, the
///   five `kerml_bindings` bundle sources and `schema` (the bundle's
///   structure is the native migration of that Ecore file, so an Ecore change
///   must rebuild the bindings even before the pinned structure is updated);
/// - the SysML bindings part likewise on `SYSML_BINDING_COMPONENTS`, the
///   `sysml-bindings.` constants, the eight `sysml_bindings` bundle sources and
///   `schema`.
///
/// The program keys are coupled in practice: intersect-kerml's
/// `SOURCE_IDENTITY` chains the sources of intersect-xtext and
/// intersect-model, and the grammar pins and recipe constants are
/// intersect-kerml source, so a real source edit in any of those crates
/// changes both program keys. The binding keys are not keyed on
/// intersect-kerml's or intersect-xtext's identity, so they change only on an
/// edit of intersect-model or of their recipe digest (`kerml_bindings.rs`
/// moves both binding parts, `sysml_bindings.rs` only the SysML one), of a
/// pinned binding source they read or `SysML.ecore`, or of their own
/// constants (see the part-key caveat in docs/architecture.md).
///
/// No part depends on another part's output, so every `depends` list is
/// empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecipePart {
    Program(ProgramRole),
    Bindings(BindingRecipe),
}
const PARTS: [RecipePart; 4] = [
    RecipePart::Program(ProgramRole::KerML),
    RecipePart::Program(ProgramRole::SysML),
    RecipePart::Bindings(BindingRecipe::KerML),
    RecipePart::Bindings(BindingRecipe::SysML),
];
impl RecipePart {
    fn name(self) -> &'static str {
        match self {
            Self::Program(role) => role.section(),
            Self::Bindings(recipe) => recipe.section(),
        }
    }
    fn named(name: &str) -> Option<Self> {
        PARTS.into_iter().find(|part| part.name() == name)
    }
    fn uses_input(self, role: &str) -> bool {
        match self {
            Self::Program(program) => {
                matches!(role, "expressions" | "probe-behavior" | "schema")
                    || role == grammar_input(program)
            }
            Self::Bindings(recipe) => role == "schema" || recipe.inputs().contains(&role),
        }
    }
    fn uses_component(self, component: &str) -> bool {
        match self {
            Self::Program(_) => PROGRAM_COMPONENTS.contains(&component),
            Self::Bindings(recipe) => recipe.components().contains(&component),
        }
    }
    fn uses_constant(self, key: &str) -> bool {
        match self {
            Self::Program(role) => key
                .split_once('.')
                .is_none_or(|(prefix, _)| prefix == role_prefix(role)),
            Self::Bindings(recipe) => key
                .split_once('.')
                .is_some_and(|(prefix, _)| prefix == recipe.constant_prefix()),
        }
    }
    fn kind(self) -> &'static str {
        match self {
            Self::Program(_) => PROGRAM_KIND,
            Self::Bindings(BindingRecipe::KerML) => BINDINGS_KIND,
            Self::Bindings(BindingRecipe::SysML) => SYSML_BINDINGS_KIND,
        }
    }
    /// The observer marker for this part's planned action.
    fn marker(self, reused: bool) -> &'static str {
        use BindingRecipe::{KerML, SysML};
        match (self, reused) {
            (Self::Program(ProgramRole::KerML), true) => "pack_part_reused:program/kerml",
            (Self::Program(ProgramRole::KerML), false) => "pack_part_rebuilt:program/kerml",
            (Self::Program(ProgramRole::SysML), true) => "pack_part_reused:program/sysml",
            (Self::Program(ProgramRole::SysML), false) => "pack_part_rebuilt:program/sysml",
            (Self::Bindings(KerML), true) => "pack_part_reused:rcore/bindings",
            (Self::Bindings(KerML), false) => "pack_part_rebuilt:rcore/bindings",
            (Self::Bindings(SysML), true) => "pack_part_reused:rcore/sysml-bindings",
            (Self::Bindings(SysML), false) => "pack_part_rebuilt:rcore/sysml-bindings",
        }
    }
}

/// The two binding recipes a pack holds, one part each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingRecipe {
    /// `kerml_bindings`, in `rcore/bindings` (the KerML-only host).
    KerML,
    /// `sysml_bindings`, in `rcore/sysml-bindings` (the SysML hosts).
    SysML,
}
impl BindingRecipe {
    /// The section (and part) name of this recipe's bindings.
    pub fn section(self) -> &'static str {
        match self {
            Self::KerML => BINDINGS_SECTION,
            Self::SysML => SYSML_BINDINGS_SECTION,
        }
    }
    fn inputs(self) -> &'static [&'static str] {
        match self {
            Self::KerML => &BINDING_INPUTS,
            Self::SysML => &SYSML_BINDING_INPUTS,
        }
    }
    fn components(self) -> &'static [&'static str] {
        match self {
            Self::KerML => &BINDING_COMPONENTS,
            Self::SysML => &SYSML_BINDING_COMPONENTS,
        }
    }
    fn constant_prefix(self) -> &'static str {
        match self {
            Self::KerML => "bindings",
            Self::SysML => "sysml-bindings",
        }
    }
    /// The recipe's definition bundle.
    pub fn bundle(self) -> DefinitionBundle {
        match self {
            Self::KerML => kerml_bindings::bundle(),
            Self::SysML => sysml_bindings::bundle(),
        }
    }
    /// The recipe's jobs over `bundle` (which must be `self.bundle()`).
    pub fn binding_jobs(self, bundle: &DefinitionBundle) -> Result<Vec<BindingJob>, String> {
        match self {
            Self::KerML => kerml_bindings::binding_jobs(bundle),
            Self::SysML => sysml_bindings::binding_jobs(bundle),
        }
    }
    /// Compile one of the recipe's jobs (see `kerml_bindings::compile_job`;
    /// run it on a thread with a large stack).
    pub fn compile_job(
        self,
        bundle: &DefinitionBundle,
        job: &BindingJob,
        observe: &impl Fn(&'static str),
    ) -> Result<Vec<VerifiedScope>, String> {
        match self {
            Self::KerML => kerml_bindings::compile_job(bundle, job, observe),
            Self::SysML => sysml_bindings::compile_job(bundle, job, observe),
        }
    }
    fn check_recipe_results(self, scopes: &[VerifiedScope]) -> Result<(), String> {
        match self {
            Self::KerML => kerml_bindings::check_recipe_results(scopes),
            Self::SysML => sysml_bindings::check_recipe_results(scopes),
        }
    }
    fn compiler_identity(self) -> String {
        match self {
            Self::KerML => kerml_bindings::compiler_identity(),
            Self::SysML => sysml_bindings::compiler_identity(),
        }
    }
    fn save(self, scopes: &[VerifiedScope]) -> Result<String, String> {
        match self {
            Self::KerML => kerml_bindings::save(scopes),
            Self::SysML => sysml_bindings::save(scopes),
        }
    }
    /// Loads the saved text of a binding part. Parts are read only from pack
    /// sections (`read_bindings_section`), and a pack hands out a section
    /// only after verifying its digest, so the saved text is not hashed a
    /// second time here.
    fn load(self, saved: &str) -> Result<Vec<VerifiedScope>, String> {
        match self {
            Self::KerML => kerml_bindings::load_verified(saved),
            Self::SysML => sysml_bindings::load_verified(saved),
        }
    }
}

/// One binding part `build_language_pack` asks its host to compile: the
/// recipe, its bundle and its jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingCompile {
    pub recipe: BindingRecipe,
    pub bundle: DefinitionBundle,
    pub jobs: Vec<BindingJob>,
}
impl BindingCompile {
    /// The recipe's bundle and jobs.
    pub fn new(recipe: BindingRecipe) -> Result<Self, String> {
        let bundle = recipe.bundle();
        let jobs = recipe.binding_jobs(&bundle)?;
        Ok(Self {
            recipe,
            bundle,
            jobs,
        })
    }
    /// Compile `job` (one of `self.jobs`) with the recipe's compiler call.
    pub fn compile_job(
        &self,
        job: &BindingJob,
        observe: &impl Fn(&'static str),
    ) -> Result<Vec<VerifiedScope>, String> {
        self.recipe.compile_job(&self.bundle, job, observe)
    }
}

/// Everything the part keys are computed from: the pinned inputs by role,
/// the compiler components and the recipe constants. `current()` is this
/// binary's; a test may change any entry to see which part keys move.
#[derive(Clone, Debug, PartialEq)]
pub struct PartKeyInputs {
    pub inputs: BTreeMap<String, SourceId>,
    pub compiler: BTreeMap<String, String>,
    pub constants: BTreeMap<String, Value>,
}
impl PartKeyInputs {
    /// `pinned_inputs`, `expected_compiler` and `recipe_constants`.
    pub fn current() -> Result<Self, String> {
        Ok(Self {
            inputs: pinned_inputs(),
            compiler: expected_compiler(),
            constants: recipe_constants()?,
        })
    }
    /// The manifest entry of `part` over these inputs, with `output_digest`.
    fn part(&self, part: RecipePart, output_digest: String) -> Result<PackPart, String> {
        fn select<V: Clone>(
            map: &BTreeMap<String, V>,
            keep: impl Fn(&str) -> bool,
        ) -> BTreeMap<String, V> {
            map.iter()
                .filter(|(key, _)| keep(key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()
        }
        let components = select(&self.compiler, |name| part.uses_component(name));
        let constants = select(&self.constants, |key| part.uses_constant(key));
        let inputs = select(&self.inputs, |role| part.uses_input(role));
        let input_digest = part_key(PartKeyInput {
            part: part.name(),
            part_version: PART_VERSION,
            recipe: &recipe(),
            components: &components,
            recipe_constants: &constants,
            inputs: &inputs,
            dependencies: &BTreeMap::new(),
        })
        .map_err(|error| error.to_string())?;
        Ok(PackPart {
            name: part.name().into(),
            components: components.into_keys().collect(),
            inputs: inputs.into_keys().collect(),
            depends: Vec::new(),
            input_digest,
            output_digest,
        })
    }
    /// Every part of this recipe in pack order, keyed over these inputs. The
    /// output digests are unknown before a build and left empty.
    pub fn parts(&self) -> Result<Vec<PackPart>, String> {
        PARTS
            .into_iter()
            .map(|part| self.part(part, String::new()))
            .collect()
    }
}

/// The pinned inputs one program part is keyed on.
fn role_inputs(role: ProgramRole) -> BTreeMap<String, SourceId> {
    pinned_inputs()
        .into_iter()
        .filter(|(name, _)| RecipePart::Program(role).uses_input(name))
        .collect()
}

/// The pins of `recipe`'s bundle sources.
#[cfg(test)]
fn binding_inputs(recipe: BindingRecipe) -> BTreeMap<String, SourceId> {
    pinned_inputs()
        .into_iter()
        .filter(|(name, _)| recipe.inputs().contains(&name.as_str()))
        .collect()
}

/// A `save_bindings` text stored in its part as the JSON it is, not as a
/// string holding JSON: a string is escaped when the section is written and
/// unescaped on every load, which for these parts was most of their size.
/// `save_bindings` writes canonical JSON, so the canonical section holds the
/// text byte for byte and reading it back gives the text again.
mod nested_json {
    use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::Error};
    use serde_json::value::RawValue;

    pub fn serialize<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serde_json::from_str::<&RawValue>(text)
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
        Box::<RawValue>::deserialize(deserializer).map(|raw| Box::<str>::from(raw).into_string())
    }
}

/// The `rcore/bindings` part: the bundle the bindings were compiled against
/// and the verbatim `save_bindings` text (its envelope unchanged: format,
/// compiler identity, digest, scopes), nested as JSON. Private fields: `new` builds one only
/// from the recipe jobs' results (`kerml_bindings::check_recipe_results`).
/// A part deserialized from a section is unchecked data; `pack_session` and
/// `load_pack_bindings` restore its bindings and check them the same way
/// before using it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RcoreBindingsPart {
    bundle: DefinitionBundle,
    #[serde(with = "nested_json")]
    saved: String,
}
impl RcoreBindingsPart {
    /// Save `scopes`, which must be the `kerml_bindings` recipe jobs' results
    /// over its `bundle()` in job order, under
    /// `kerml_bindings::compiler_identity()`.
    pub fn new(scopes: &[VerifiedScope]) -> Result<Self, String> {
        let (bundle, saved) = save_part(BindingRecipe::KerML, scopes)?;
        Ok(Self { bundle, saved })
    }
    /// The checked bindings of this part: the bundle equals the recipe's, the
    /// saved text's compiler identity is the current one, `load_bindings`
    /// accepts it, and the bindings are the recipe jobs' results. Trusted
    /// images (see `load_pack_bindings`).
    fn restore(&self) -> Result<Vec<VerifiedScope>, String> {
        restore_part(BindingRecipe::KerML, &self.bundle, &self.saved)
    }
    /// The `save_bindings` text.
    pub fn saved(&self) -> &str {
        &self.saved
    }
    pub fn bundle(&self) -> &DefinitionBundle {
        &self.bundle
    }
}

/// The `rcore/sysml-bindings` part: as `RcoreBindingsPart`, for the
/// `sysml_bindings` recipe (the combined KerML+SysML bindings).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SysmlBindingsPart {
    bundle: DefinitionBundle,
    #[serde(with = "nested_json")]
    saved: String,
}
impl SysmlBindingsPart {
    /// Save `scopes`, which must be the `sysml_bindings` recipe jobs' results
    /// over its `bundle()` in job order, under
    /// `sysml_bindings::compiler_identity()`.
    pub fn new(scopes: &[VerifiedScope]) -> Result<Self, String> {
        let (bundle, saved) = save_part(BindingRecipe::SysML, scopes)?;
        Ok(Self { bundle, saved })
    }
    /// The checked bindings of this part (see `RcoreBindingsPart::restore`).
    fn restore(&self) -> Result<Vec<VerifiedScope>, String> {
        restore_part(BindingRecipe::SysML, &self.bundle, &self.saved)
    }
    /// The `save_bindings` text.
    pub fn saved(&self) -> &str {
        &self.saved
    }
    pub fn bundle(&self) -> &DefinitionBundle {
        &self.bundle
    }
}

/// The bundle and saved text of a binding part: `scopes` must be `recipe`'s
/// jobs' results in job order.
fn save_part(
    recipe: BindingRecipe,
    scopes: &[VerifiedScope],
) -> Result<(DefinitionBundle, String), String> {
    recipe.check_recipe_results(scopes)?;
    Ok((recipe.bundle(), recipe.save(scopes)?))
}

/// The checked bindings of a binding part of `recipe`: the bundle equals the
/// recipe's, the saved text's compiler identity is the current one,
/// `load_bindings` accepts it, and the bindings are the recipe jobs' results.
fn restore_part(
    recipe: BindingRecipe,
    bundle: &DefinitionBundle,
    saved: &str,
) -> Result<Vec<VerifiedScope>, String> {
    let section = recipe.section();
    if *bundle != recipe.bundle() {
        return Err(format!(
            "Language pack {section} bundle differs from the recipe's pinned sources"
        ));
    }
    let compiler = saved_compiler(section, saved)?;
    if compiler != recipe.compiler_identity() {
        return Err(format!(
            "Language pack {section} was saved by another binding compiler identity ({compiler})"
        ));
    }
    let scopes = recipe
        .load(saved)
        .map_err(|error| format!("Language pack {section} load failed: {error}"))?;
    recipe.check_recipe_results(&scopes).map_err(|error| {
        format!("Language pack {section} does not hold the recipe's bindings: {error}")
    })?;
    Ok(scopes)
}

/// The current manifest entry of `part` with `output_digest`.
fn expected(part: RecipePart, output_digest: String) -> Result<PackPart, String> {
    PartKeyInputs::current()?.part(part, output_digest)
}

/// The hooks a pack of this recipe may require: the computed-setting
/// providers its saved contracts reference. The keyword-boolean rule provider
/// is installed by the host capability at bind time and is not referenced by
/// the payload, so it is not listed.
pub fn hook_catalog() -> Vec<PackHook> {
    [
        crate::OPERAND_PROVIDER,
        crate::sysml_usage::USAGE_REFERENCE_PROVIDER,
    ]
    .into_iter()
    .map(|name| PackHook {
        provider: ProviderId {
            name: name.into(),
            version: "1".into(),
        },
        provenance: HOOK_PROVENANCE.into(),
    })
    .collect()
}

/// Every required hook must be in `hook_catalog` with the same name, version
/// and provenance.
pub fn check_hooks(required: &[PackHook]) -> Result<(), String> {
    let catalog = hook_catalog();
    match required.iter().find(|hook| !catalog.contains(hook)) {
        Some(hook) => Err(format!(
            "Language pack requires an unknown hook {}@{} ({})",
            hook.provider.name, hook.provider.version, hook.provenance
        )),
        None => Ok(()),
    }
}

fn contract_hooks<'a>(
    contracts: impl IntoIterator<Item = &'a ComputedSettingContract>,
) -> BTreeSet<PackHook> {
    contracts
        .into_iter()
        .map(|contract| PackHook {
            provider: contract.provider.clone(),
            provenance: HOOK_PROVENANCE.into(),
        })
        .collect()
}

fn original_ids(definition: &PreparedLanguageDefinition<'_>) -> BTreeSet<SourceId> {
    definition
        .original_sources()
        .sources
        .iter()
        .map(|source| source.id.clone())
        .collect()
}

/// Encode a prepared (or loaded) session and its two Rcore binding parts as
/// pack bytes. Deterministic: the same session state and bindings always give
/// the same bytes. The bindings are restored and checked first (as
/// `load_pack_bindings` and `load_pack_sysml_bindings` do), so a part that
/// would not load is not encoded.
pub fn pack_session(
    session: &NativePilotSession,
    bindings: &RcoreBindingsPart,
    sysml_bindings: &SysmlBindingsPart,
) -> Result<Vec<u8>, String> {
    // Both owners must have been prepared from exactly the pinned inputs, and
    // their schemas must name the stored structure the bindings compile on.
    session.with_definitions(|kerml, _, sysml, _| {
        for (role, definition) in [(ProgramRole::KerML, kerml), (ProgramRole::SysML, sysml)] {
            let expected: BTreeSet<_> = role_inputs(role).into_values().collect();
            if original_ids(definition) != expected {
                return Err(format!(
                    "{role:?} owner was not prepared from the pinned original inputs"
                ));
            }
            kerml_bindings::structure(definition.schema())?;
        }
        Ok(())
    })?;
    drop(bindings.restore()?);
    drop(sysml_bindings.restore()?);
    let mut parts = Vec::new();
    let mut hooks = BTreeSet::new();
    let mut sections = Vec::new();
    for (role, part, contracts) in session.saved_parts()? {
        let payload = canonical_json(&part).map_err(|error| error.to_string())?;
        let output = FileDigest::of(&payload).sha256;
        let part = expected(RecipePart::Program(role), output.clone())?;
        hooks.extend(contract_hooks(contracts));
        sections.push((
            role.section(),
            PROGRAM_KIND,
            payload,
            part.input_digest.clone(),
            output,
        ));
        parts.push(part);
    }
    for (recipe, payload) in [
        (BindingRecipe::KerML, canonical_json(bindings)),
        (BindingRecipe::SysML, canonical_json(sysml_bindings)),
    ] {
        let payload = payload.map_err(|error| error.to_string())?;
        let output = FileDigest::of(&payload).sha256;
        let part = RecipePart::Bindings(recipe);
        let entry = expected(part, output.clone())?;
        sections.push((
            recipe.section(),
            part.kind(),
            payload,
            entry.input_digest.clone(),
            output,
        ));
        parts.push(entry);
    }
    let hooks: Vec<_> = hooks.into_iter().collect();
    check_hooks(&hooks)?;
    let mut builder = PackBuilder::new(PackHeader {
        language: LANGUAGE.into(),
        recipe: recipe(),
        compiler: expected_compiler(),
        recipe_constants: recipe_constants()?,
        inputs: pinned_inputs(),
        parts,
        requires_hooks: hooks,
        depends_on: Vec::new(),
    });
    for (name, kind, payload, key, output) in sections {
        builder = builder.section(name, kind, PackSectionFlags::REQUIRED, payload, key, output);
    }
    builder.build().map_err(|error| error.to_string())
}

/// Check a structurally verified pack against this recipe before loading the
/// `roles` it needs: language, recipe, compiler identity, constants, pinned
/// inputs (manifest-wide), then `check_layout` (no layered dependencies, known
/// hooks, exactly the two program parts and the two binding parts with their
/// sections), and for each needed part `check_part` (the recomputed part key
/// and the output digests: `part.output_digest == section.output_digest ==
/// section.digest.sha256`).
pub fn check_pack(pack: &LanguagePack, roles: &[ProgramRole]) -> Result<(), String> {
    let manifest = pack.manifest();
    let refuse = |what: &str| Err(format!("Language pack {what} differs from this recipe"));
    if manifest.language != LANGUAGE {
        return refuse("language");
    }
    if manifest.recipe != recipe() {
        return refuse("recipe");
    }
    let compiler = expected_compiler();
    if manifest.compiler != compiler {
        let differing: Vec<_> = compiler
            .keys()
            .chain(manifest.compiler.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter(|name| manifest.compiler.get(*name) != compiler.get(*name))
            .map(String::as_str)
            .collect();
        return Err(format!(
            "Language pack compiler identity differs from this recipe (built by other              {}); rebuild the pack with this build",
            differing.join(", ")
        ));
    }
    if manifest.recipe_constants != recipe_constants()? {
        return refuse("recipe constants");
    }
    if manifest.inputs != pinned_inputs() {
        return refuse("inputs");
    }
    check_layout(manifest)?;
    for role in roles {
        check_part(pack, RecipePart::Program(*role))?;
    }
    Ok(())
}

/// The manifest checks that hold for every part alike: language, no layered
/// dependencies, known hooks, and exactly this recipe's parts and sections in
/// pack order, plus at most the optional prepared-library part
/// (`LIBRARY_SECTION`, decision D10). The KerML bindings part
/// (`BINDINGS_SECTION`) may be absent, part and section together
/// (`without_kerml_bindings`): the pack of a host that runs only the combined
/// KerML+SysML bindings; `load_pack_bindings` refuses such a pack. Unlike `check_pack` it does not compare
/// the manifest-wide compiler, constants and inputs: each part key covers the
/// subset its part reads, which is what per-part reuse relies on.
fn check_layout(manifest: &PackManifest) -> Result<(), String> {
    let refuse = |what: &str| Err(format!("Language pack {what} differs from this recipe"));
    if manifest.language != LANGUAGE {
        return refuse("language");
    }
    if !manifest.depends_on.is_empty() {
        return refuse("layering");
    }
    check_hooks(&manifest.requires_hooks)?;
    let cached_part = manifest
        .parts
        .iter()
        .any(|p| p.name == STANDARD_COMPONENT_SECTION);
    let cached_section = manifest
        .sections
        .iter()
        .any(|s| s.name == STANDARD_COMPONENT_SECTION);
    if cached_part != cached_section
        || (cached_part
            && (!manifest.parts.iter().any(|p| p.name == LIBRARY_SECTION)
                || !manifest.sections.iter().any(|s| s.name == LIBRARY_SECTION)))
    {
        return refuse("standard component dependency");
    }
    let expected: Vec<_> = PARTS.iter().map(|part| part.name()).collect();
    // The optional library part sorts before the programs; the recipe
    // checks it apart (`pack_library_payload`).
    let parts: Vec<_> = manifest
        .parts
        .iter()
        .map(|part| part.name.as_str())
        .filter(|name| *name != LIBRARY_SECTION && *name != STANDARD_COMPONENT_SECTION)
        .collect();
    let sections: Vec<_> = manifest
        .sections
        .iter()
        .map(|section| section.name.as_str())
        .filter(|name| *name != LIBRARY_SECTION && *name != STANDARD_COMPONENT_SECTION)
        .collect();
    if parts != sections {
        return refuse("part layout");
    }
    let without_kerml_bindings = || {
        expected
            .iter()
            .filter(|name| **name != BINDINGS_SECTION)
            .eq(parts.iter())
    };
    if parts != expected && !without_kerml_bindings() {
        return refuse("part layout");
    }
    Ok(())
}

/// The manifest entry of `part` equals the recomputed one (part key,
/// components, inputs, no dependencies) and its section has the part's kind,
/// is required and carries the part's digests.
fn check_part(pack: &LanguagePack, part: RecipePart) -> Result<(), String> {
    let name = part.name();
    let entry = pack
        .manifest()
        .parts
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("Language pack has no {name} part"))?;
    if *entry != expected(part, entry.output_digest.clone())? {
        return Err(format!(
            "Language pack {name} part key differs from this recipe"
        ));
    }
    check_section(pack, entry, part.kind())
}

/// The section of `part` has `kind`, is required, and its digests are the
/// part's (`part.output_digest == section.output_digest ==
/// section.digest.sha256`).
fn check_section(pack: &LanguagePack, part: &PackPart, kind: &str) -> Result<(), String> {
    let name = &part.name;
    let section = pack
        .section_entry(name)
        .ok_or_else(|| format!("Language pack has no {name} section"))?;
    if section.kind != kind || !section.flags.contains(PackSectionFlag::Required) {
        return Err(format!(
            "Language pack {name} section kind differs from this recipe"
        ));
    }
    if section.part_digest != part.input_digest
        || section.output_digest != part.output_digest
        || section.digest.sha256 != part.output_digest
    {
        return Err(format!(
            "Language pack {name} section digest differs from this recipe"
        ));
    }
    Ok(())
}

/// Restore the verified KerML Rcore bindings from a HOST-BUILT pack (bytes
/// already verified by `LanguagePack::read`) without compiling them. Order:
/// 1. `check_pack` (format, recipe, compiler identity, constants, pinned
///    inputs including the three binding sources, part layout);
/// 2. the recomputed `rcore/bindings` part key and its section digests;
/// 3. the stored bundle EQUALS `kerml_bindings::bundle()`, rebuilt from the
///    pinned embedded sources;
/// 4. the saved text's compiler identity EQUALS
///    `kerml_bindings::compiler_identity()`;
/// 5. `load_bindings` over the rebuilt bundle (format, identity again, the
///    scopes digest, and each image's context hash against the bundle);
/// 6. the loaded bindings are the recipe jobs' results: same count and, in
///    job order, the same kind for the same declaration
///    (`kerml_bindings::check_recipe_results`).
///
/// Steps 2 to 6 are the part's own load checks; `build_language_pack` runs
/// them (after `check_layout`) to decide whether a previous pack's bindings
/// are reused.
///
/// There is one trust mode: the saved images are trusted
/// (`VerifiedExecutable::load_trusted`: types and control flow are not
/// rechecked). No `Revalidate` variant exists because the only re-verification
/// available is a full recompile, which costs as much as compiling the
/// bindings (minutes in release, see the step 7 measurements), not seconds. A
/// host that must check the images recompiles them with `kerml_bindings` and
/// compares `save_bindings` text (`sysml_pack check` does). Trust is a
/// convention, not cryptography; on any error the host rebuilds the pack.
pub fn load_pack_bindings(pack: &LanguagePack) -> Result<Vec<VerifiedScope>, String> {
    check_pack(pack, &[])?;
    restore_bindings_part(pack).map(|(_, scopes)| scopes)
}

/// Restore the verified combined KerML+SysML Rcore bindings
/// (`rcore/sysml-bindings`, the `sysml_bindings` recipe) from a HOST-BUILT
/// pack without compiling them, with the checks of `load_pack_bindings`
/// against the `sysml_bindings` recipe (its bundle, compiler identity and
/// jobs). Returns `sysml_bindings::bundle()`, which the stored bundle equals,
/// and the bindings in job order: what a host passes to `RcoreScopes::new`.
/// The same single trust mode as `load_pack_bindings`.
pub fn load_pack_sysml_bindings(
    pack: &LanguagePack,
) -> Result<(DefinitionBundle, Vec<VerifiedScope>), String> {
    check_pack(pack, &[])?;
    let (_, scopes) = restore_sysml_bindings_part(pack)?;
    Ok((sysml_bindings::bundle(), scopes))
}

/// Steps 2 to 6 of `load_pack_bindings`: the part, and its checked bindings.
fn restore_bindings_part(
    pack: &LanguagePack,
) -> Result<(RcoreBindingsPart, Vec<VerifiedScope>), String> {
    let saved: RcoreBindingsPart = read_bindings_section(pack, BindingRecipe::KerML)?;
    let scopes = saved.restore()?;
    Ok((saved, scopes))
}

/// Steps 2 to 6 of `load_pack_sysml_bindings`: the part, and its checked
/// bindings.
fn restore_sysml_bindings_part(
    pack: &LanguagePack,
) -> Result<(SysmlBindingsPart, Vec<VerifiedScope>), String> {
    let saved: SysmlBindingsPart = read_bindings_section(pack, BindingRecipe::SysML)?;
    let scopes = saved.restore()?;
    Ok((saved, scopes))
}

/// The recomputed part key and section digests of `recipe`'s part, then its
/// section decoded (unchecked data until restored).
fn read_bindings_section<T: serde::de::DeserializeOwned>(
    pack: &LanguagePack,
    recipe: BindingRecipe,
) -> Result<T, String> {
    check_part(pack, RecipePart::Bindings(recipe))?;
    let section = recipe.section();
    pack.section_json(section)
        .map_err(|error| format!("Language pack {section}: {error}"))
}

/// The `compiler` field of a `save_bindings` envelope, read without trusting
/// the rest (which `load_bindings` checks).
fn saved_compiler(section: &str, saved: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Envelope {
        compiler: String,
    }
    serde_json::from_str::<Envelope>(saved)
        .map(|envelope| envelope.compiler)
        .map_err(|error| format!("Language pack {section} saved text: {error}"))
}

/// The definition-file roles whose file in `definitions` is missing,
/// duplicated or not the pinned text.
fn mismatched_definitions(definitions: &[Source]) -> Vec<String> {
    pinned_inputs()
        .into_iter()
        .filter(|(role, _)| DEFINITION_INPUTS.contains(&role.as_str()))
        .filter(|(_, id)| {
            let mut found = definitions.iter().filter(|source| source.uri == id.uri);
            !matches!(
                (found.next(), found.next()),
                (Some(source), None) if SourceId::for_text(&source.uri, &source.text) == *id
            )
        })
        .map(|(role, _)| role)
        .collect()
}

/// Why `build_language_pack` rebuilds a part instead of reusing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebuildReason {
    /// No previous pack was supplied.
    NoPrevious,
    /// The previous pack has no such part, or its manifest entry (part key,
    /// components, inputs or dependencies) differs from the recomputed one.
    KeyChanged,
    /// A part it depends on is rebuilt, or is not an earlier part.
    DependencyRebuilt,
    /// The key matched, but the previous part failed its own load checks.
    LoadRefused,
}

/// What `build_language_pack` does with one part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartDecision {
    Reuse,
    Rebuild(RebuildReason),
}

/// The pure per-part reuse decision of language-pack step 8. For each
/// `expected` part (the recomputed manifest entries in pack order; output
/// digests ignored), in order:
/// - no `previous` parts: rebuild (`NoPrevious`);
/// - the previous entry of that name is missing or differs in anything but
///   its output digest: rebuild (`KeyChanged`);
/// - a part it `depends` on is not an earlier part decided `Reuse`: rebuild
///   (`DependencyRebuilt`). A rebuilt dependency always rebuilds its
///   dependents, even if it would rebuild to the same output digest: the
///   dependent's key covers that digest, which is unknown until the rebuild;
/// - the part is not in `loaded`, the parts whose previous section passed its
///   own load checks: rebuild (`LoadRefused`);
/// - otherwise reuse.
///
/// So a rebuild propagates to every dependent and a reuse needs its whole
/// dependency closure reused. Fail closed: when in doubt, rebuild. Calling it
/// with every part `loaded` gives the candidates worth load-checking.
pub fn plan_parts(
    previous: Option<&[PackPart]>,
    expected: &[PackPart],
    loaded: &BTreeSet<String>,
) -> Vec<(String, PartDecision)> {
    let mut plan: Vec<(String, PartDecision)> = Vec::new();
    for part in expected {
        let decision = match previous {
            None => PartDecision::Rebuild(RebuildReason::NoPrevious),
            Some(previous) => match previous.iter().find(|entry| entry.name == part.name) {
                Some(entry) if same_key(entry, part) => {
                    let reused = |name: &String| {
                        plan.iter().any(|(planned, decision)| {
                            planned == name && *decision == PartDecision::Reuse
                        })
                    };
                    if !part.depends.iter().all(reused) {
                        PartDecision::Rebuild(RebuildReason::DependencyRebuilt)
                    } else if !loaded.contains(&part.name) {
                        PartDecision::Rebuild(RebuildReason::LoadRefused)
                    } else {
                        PartDecision::Reuse
                    }
                }
                _ => PartDecision::Rebuild(RebuildReason::KeyChanged),
            },
        };
        plan.push((part.name.clone(), decision));
    }
    plan
}

/// Equal manifest entries except for the output digest.
fn same_key(previous: &PackPart, expected: &PackPart) -> bool {
    previous.name == expected.name
        && previous.components == expected.components
        && previous.inputs == expected.inputs
        && previous.depends == expected.depends
        && previous.input_digest == expected.input_digest
}

/// The plan for a previous pack, with the restored content of every part it
/// reuses and the load-check error of every key-matching part it refused.
struct PreviousParts {
    plan: Vec<(String, PartDecision)>,
    refusals: BTreeMap<String, String>,
    kerml: Option<RestoredOwner>,
    sysml: Option<RestoredOwner>,
    bindings: Option<RcoreBindingsPart>,
    sysml_bindings: Option<SysmlBindingsPart>,
}

/// Plan `expected` against `previous`: `plan_parts` with every part assumed
/// loadable gives the candidates; each candidate then runs its own load
/// checks (`check_layout` first, for all parts; for a program part
/// `check_part` and `DefinitionOwner::from_saved` in `HostCache` mode, as
/// `NativePilotSession::from_pack` does; for a binding part steps 2 to 6
/// of `load_pack_bindings` or `load_pack_sysml_bindings`); `plan_parts` with
/// the parts that loaded is the plan.
fn previous_parts(previous: Option<&LanguagePack>, expected: &[PackPart]) -> PreviousParts {
    let mut parts = PreviousParts {
        plan: plan_parts(None, expected, &BTreeSet::new()),
        refusals: BTreeMap::new(),
        kerml: None,
        sysml: None,
        bindings: None,
        sysml_bindings: None,
    };
    let Some(previous) = previous else {
        return parts;
    };
    let entries = previous.manifest().parts.as_slice();
    let mut loaded = BTreeSet::new();
    match check_layout(previous.manifest()) {
        Err(error) => {
            for part in expected {
                parts.refusals.insert(part.name.clone(), error.clone());
            }
        }
        Ok(()) => {
            let all: BTreeSet<_> = expected.iter().map(|part| part.name.clone()).collect();
            for (name, decision) in plan_parts(Some(entries), expected, &all) {
                if decision != PartDecision::Reuse {
                    continue;
                }
                let result = match RecipePart::named(&name) {
                    Some(RecipePart::Program(role)) => {
                        check_part(previous, RecipePart::Program(role))
                            .and_then(|()| RestoredOwner::load(previous, role))
                            .map(|owner| match role {
                                ProgramRole::KerML => parts.kerml = Some(owner),
                                ProgramRole::SysML => parts.sysml = Some(owner),
                            })
                    }
                    Some(RecipePart::Bindings(BindingRecipe::KerML)) => {
                        restore_bindings_part(previous).map(|(part, _)| parts.bindings = Some(part))
                    }
                    Some(RecipePart::Bindings(BindingRecipe::SysML)) => {
                        restore_sysml_bindings_part(previous)
                            .map(|(part, _)| parts.sysml_bindings = Some(part))
                    }
                    None => Err(format!("Language pack part {name} is not in this recipe")),
                };
                match result {
                    Ok(()) => {
                        loaded.insert(name);
                    }
                    Err(error) => {
                        parts.refusals.insert(name, error);
                    }
                }
            }
        }
    }
    parts.plan = plan_parts(Some(entries), expected, &loaded);
    // Drop the restored content of a part the final plan rebuilds.
    let reuse = |part: RecipePart| {
        parts
            .plan
            .iter()
            .any(|(name, decision)| name == part.name() && *decision == PartDecision::Reuse)
    };
    let (kerml, sysml, bindings, sysml_bindings) = (
        reuse(RecipePart::Program(ProgramRole::KerML)),
        reuse(RecipePart::Program(ProgramRole::SysML)),
        reuse(RecipePart::Bindings(BindingRecipe::KerML)),
        reuse(RecipePart::Bindings(BindingRecipe::SysML)),
    );
    if !kerml {
        parts.kerml = None;
    }
    if !sysml {
        parts.sysml = None;
    }
    if !bindings {
        parts.bindings = None;
    }
    if !sysml_bindings {
        parts.sysml_bindings = None;
    }
    parts
}

/// For a host's report: each part of this recipe in pack order, what
/// `build_language_pack` would do with it given `previous`, and for a part
/// whose key matched but that was refused, the error of its load checks
/// (for every part when the layout check refused the whole pack). Runs the
/// same checks the builder runs (no prepare and no compile).
pub fn explain_previous(
    previous: &LanguagePack,
) -> Result<Vec<(String, PartDecision, Option<String>)>, String> {
    let expected = PartKeyInputs::current()?.parts()?;
    let PreviousParts { plan, refusals, .. } = previous_parts(Some(previous), &expected);
    Ok(plan
        .into_iter()
        .map(|(name, decision)| {
            let refusal = refusals.get(&name).cloned();
            (name, decision, refusal)
        })
        .collect())
}

/// Build pack bytes for the four original definitions and the two Rcore
/// binding parts (KerML, and combined KerML+SysML), reusing what it can of `previous` part by part (language-pack
/// step 8). The definitions must be exactly the pinned files.
///
/// A part of `previous` is reused only when (`plan_parts`) its recomputed
/// part key equals the previous manifest's entry, the previous section
/// passes that part's own load checks (a program part through the
/// `NativePilotSession::from_pack` `HostCache` path, a binding part through
/// `load_pack_bindings`'s or `load_pack_sysml_bindings`' part checks), and
/// every part it depends on is reused too. Every other part is rebuilt: a
/// program owner is prepared from the definitions, a binding part compiled. The two program owners
/// are prepared independently (see
/// `NativePilotSession::assemble_with_progress`), so either can be rebuilt
/// alone; the bindings never depend on the programs. Each part's key lists
/// directly only what it reads (see `RecipePart`): intersect-kerml,
/// intersect-xtext and the grammars are not entries of the binding keys, so
/// an edit there rebuilds the programs and reuses the bindings; an edit of
/// intersect-model or `kerml_bindings.rs` rebuilds both binding parts, an
/// edit of `sysml_bindings.rs` or a SysML behavior only the SysML one (see
/// the caveat in docs/architecture.md).
///
/// The pack is always encoded again from the assembled session and bindings
/// (a reused part re-encodes to its previous section: save after load is a
/// fixed point, and a different output digest is an error, never silently
/// accepted), read back and self-checked (`from_pack` in `HostCache` mode
/// and both binding loads) before it is returned, so a pack that cannot
/// load is never produced. The manifest-wide header (compiler summary,
/// recipe constants, inputs, hooks) is always written for the current
/// recipe and is not part of any part key, so the result equals the previous
/// bytes only when every part is reused and that header also matches;
/// `pack_reused` reports exactly that case.
///
/// `compile_bindings` compiles the rebuilt binding parts: it receives one
/// `BindingCompile` (recipe, bundle, jobs) per rebuilt binding part in pack
/// order and returns, for each, its jobs' `BindingCompile::compile_job`
/// results concatenated in job order (checked by `RcoreBindingsPart::new` /
/// `SysmlBindingsPart::new`). The host chooses how the jobs run (for example
/// every job of every request at once, one thread with a large stack each);
/// this library starts no threads. It is called once, and only when at least
/// one binding part is rebuilt.
///
/// `observe` receives, in order: one `pack_part_reused:<part>` or
/// `pack_part_rebuilt:<part>` marker per part in pack order; the preparation
/// progress markers of each rebuilt program owner; `bindings_compile_begin`
/// / `bindings_compile_end` when any binding part is rebuilt;
/// `pack_encode_begin`, `pack_self_check_begin`, `pack_self_check_end`; and
/// `pack_reused` last when the result equals `previous`. A host reports why
/// parts were rebuilt with `explain_previous`.
pub fn build_language_pack(
    definitions: &[Source],
    previous: Option<&LanguagePack>,
    preparation: &InvocationContext<'_>,
    compile_bindings: impl FnOnce(&[BindingCompile]) -> Result<Vec<Vec<VerifiedScope>>, String>,
    observe: impl Fn(&'static str),
) -> Result<Vec<u8>, String> {
    let mismatched = mismatched_definitions(definitions);
    if !mismatched.is_empty() {
        return Err(format!(
            "Original definitions differ from the recipe's pinned inputs: {}",
            mismatched.join(", ")
        ));
    }
    let expected = PartKeyInputs::current()?.parts()?;
    let PreviousParts {
        plan,
        kerml,
        sysml,
        bindings,
        sysml_bindings,
        ..
    } = previous_parts(previous, &expected);
    for (name, decision) in &plan {
        let part = RecipePart::named(name)
            .ok_or_else(|| format!("Language pack part {name} is not in this recipe"))?;
        observe(part.marker(*decision == PartDecision::Reuse));
    }
    let session = NativePilotSession::assemble_with_progress(
        definitions,
        preparation,
        kerml,
        sysml,
        &observe,
    )?;
    let (bindings, sysml_bindings) =
        compile_binding_parts(bindings, sysml_bindings, compile_bindings, &observe)?;
    observe("pack_encode_begin");
    let bytes = pack_session(&session, &bindings, &sysml_bindings)?;
    drop(session);
    drop(bindings);
    drop(sysml_bindings);
    observe("pack_self_check_begin");
    let pack = LanguagePack::read(bytes).map_err(|error| error.to_string())?;
    if let Some(previous) = previous {
        let output = |manifest: &PackManifest, name: &str| {
            manifest
                .parts
                .iter()
                .find(|part| part.name == name)
                .map(|part| part.output_digest.clone())
        };
        for (name, decision) in &plan {
            if *decision == PartDecision::Reuse
                && output(pack.manifest(), name) != output(previous.manifest(), name)
            {
                return Err(format!(
                    "Reused language pack part {name} re-encoded to another output digest"
                ));
            }
        }
    }
    NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)?;
    load_pack_bindings(&pack)?;
    load_pack_sysml_bindings(&pack)?;
    observe("pack_self_check_end");
    if previous.is_some_and(|previous| previous.bytes() == pack.bytes()) {
        observe("pack_reused");
    }
    Ok(pack.bytes().to_vec())
}

/// The reused binding parts, and the missing ones compiled with one
/// `compile_bindings` call (see `build_language_pack`).
fn compile_binding_parts(
    bindings: Option<RcoreBindingsPart>,
    sysml_bindings: Option<SysmlBindingsPart>,
    compile_bindings: impl FnOnce(&[BindingCompile]) -> Result<Vec<Vec<VerifiedScope>>, String>,
    observe: &impl Fn(&'static str),
) -> Result<(RcoreBindingsPart, SysmlBindingsPart), String> {
    let (mut bindings, mut sysml_bindings) = match (bindings, sysml_bindings) {
        (Some(bindings), Some(sysml_bindings)) => return Ok((bindings, sysml_bindings)),
        parts => parts,
    };
    observe("bindings_compile_begin");
    let mut requests = Vec::new();
    if bindings.is_none() {
        requests.push(BindingCompile::new(BindingRecipe::KerML)?);
    }
    if sysml_bindings.is_none() {
        requests.push(BindingCompile::new(BindingRecipe::SysML)?);
    }
    let compiled = compile_bindings(&requests)?;
    if compiled.len() != requests.len() {
        return Err(format!(
            "Binding compile returned {} results for {} requests",
            compiled.len(),
            requests.len()
        ));
    }
    for (request, scopes) in requests.iter().zip(compiled) {
        match request.recipe {
            BindingRecipe::KerML => bindings = Some(RcoreBindingsPart::new(&scopes)?),
            BindingRecipe::SysML => sysml_bindings = Some(SysmlBindingsPart::new(&scopes)?),
        }
    }
    observe("bindings_compile_end");
    match (bindings, sysml_bindings) {
        (Some(bindings), Some(sysml_bindings)) => Ok((bindings, sysml_bindings)),
        _ => Err("A binding part was neither reused nor compiled".into()),
    }
}

/// The MOF property redefinitions of the converted SysML structure this
/// definition was compiled from (the Rcore source its schema declarations
/// name, read from the definition's own converted bundle).
pub fn metamodel_redefinitions(
    definition: &PreparedLanguageDefinition<'_>,
) -> Result<Vec<(DeclarationId, DeclarationId)>, String> {
    let id = definition
        .schema()
        .declarations
        .values()
        .next()
        .map(|declaration| declaration.source.clone())
        .ok_or("Schema has no declarations")?;
    if &id != definition.selection().ecore {
        return Err("Schema declarations do not name the selected structure source".into());
    }
    let structure = definition.sources().source(&id)?;
    if structure.kind != SourceKind::Rcore {
        return Err("Selected structure source is not converted Rcore".into());
    }
    feature_redefinitions(structure, STRUCTURE_LIMITS).map_err(|error| error.to_string())
}

/// The frozen metamodel of a definition: its schema frozen with the
/// redefinitions of `metamodel_redefinitions`. Computed at load (no pack
/// part): it costs milliseconds and needs no trusted list.
pub fn frozen_metamodel(
    definition: &PreparedLanguageDefinition<'_>,
) -> Result<Arc<Metamodel>, String> {
    let redefinitions = metamodel_redefinitions(definition)?;
    Metamodel::freeze(definition.schema())
        .and_then(|metamodel| metamodel.with_redefinitions(&redefinitions))
        .map(Arc::new)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_pins_are_the_recipe_bundle() {
        for recipe in [BindingRecipe::KerML, BindingRecipe::SysML] {
            let pins = binding_inputs(recipe);
            let bundle: BTreeSet<_> = recipe
                .bundle()
                .sources
                .into_iter()
                .map(|source| source.id)
                .collect();
            assert_eq!(pins.values().cloned().collect::<BTreeSet<_>>(), bundle);
            assert_eq!(
                pins.keys().map(String::as_str).collect::<Vec<_>>(),
                recipe.inputs()
            );
            // The program parts never see the binding inputs, and vice versa.
            for role in ROLES {
                assert!(
                    role_inputs(role)
                        .keys()
                        .all(|name| !pins.contains_key(name))
                );
            }
        }
        // The SysML bundle extends the KerML one's roles.
        let kerml = binding_inputs(BindingRecipe::KerML);
        let sysml = binding_inputs(BindingRecipe::SysML);
        assert!(kerml.iter().all(|(role, id)| sysml.get(role) == Some(id)));
        assert_eq!(sysml.len(), kerml.len() + 3);
    }

    #[test]
    fn part_constants_are_partitioned() -> Result<(), String> {
        let constants = recipe_constants()?;
        let reads = |part: RecipePart| -> Vec<&str> {
            constants
                .keys()
                .map(String::as_str)
                .filter(|key| part.uses_constant(key))
                .collect()
        };
        assert_eq!(
            reads(RecipePart::Bindings(BindingRecipe::KerML)),
            ["bindings.recipe", "bindings.source_bytes"]
        );
        assert_eq!(
            reads(RecipePart::Bindings(BindingRecipe::SysML)),
            ["sysml-bindings.recipe", "sysml-bindings.source_bytes"]
        );
        assert_eq!(
            constants.get("sysml-bindings.recipe"),
            Some(&Value::from(sysml_bindings::BINDING_RECIPE))
        );
        for role in ROLES {
            let own = reads(RecipePart::Program(role));
            assert!(own.iter().all(|key| !key.contains("bindings.")));
            let other = role_prefix(match role {
                ProgramRole::KerML => ProgramRole::SysML,
                ProgramRole::SysML => ProgramRole::KerML,
            });
            assert!(own.iter().all(|key| !key.starts_with(other)));
            assert!(own.contains(&format!("{}.source_policy", role_prefix(role)).as_str()));
        }
        // Every constant and every pinned input is read by some part.
        assert!(
            constants
                .keys()
                .all(|key| PARTS.iter().any(|part| part.uses_constant(key)))
        );
        assert!(
            pinned_inputs()
                .keys()
                .all(|role| PARTS.iter().any(|part| part.uses_input(role)))
        );
        Ok(())
    }

    #[test]
    fn bindings_part_is_keyed_apart_from_the_programs() -> Result<(), String> {
        let parts = PartKeyInputs::current()?.parts()?;
        let names: Vec<_> = parts.iter().map(|part| part.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "program/kerml",
                "program/sysml",
                BINDINGS_SECTION,
                SYSML_BINDINGS_SECTION
            ]
        );
        assert_eq!(parts, PartKeyInputs::current()?.parts()?, "deterministic");
        let bindings = &parts[2];
        let sysml_bindings = &parts[3];
        assert_eq!(bindings.components, BINDING_COMPONENTS);
        assert_eq!(sysml_bindings.components, SYSML_BINDING_COMPONENTS);
        for program in &parts[..2] {
            // The programs never key on a binding recipe digest, and the
            // bindings never on intersect-kerml or intersect-xtext.
            assert_eq!(program.components, PROGRAM_COMPONENTS);
            assert!(
                !program
                    .components
                    .iter()
                    .any(|name| name == BINDINGS_COMPONENT || name == SYSML_BINDINGS_COMPONENT)
            );
        }
        for binding in [bindings, sysml_bindings] {
            assert!(
                !binding
                    .components
                    .iter()
                    .any(|name| name == "intersect-kerml" || name == "intersect-xtext")
            );
        }
        // Every compiler component is read by some part.
        assert!(
            expected_compiler()
                .keys()
                .all(|name| PARTS.iter().any(|part| part.uses_component(name)))
        );
        assert_eq!(
            expected_compiler().get(BINDINGS_COMPONENT),
            Some(&kerml_bindings::recipe_digest())
        );
        assert_eq!(
            expected_compiler().get(SYSML_BINDINGS_COMPONENT),
            Some(&sysml_bindings::recipe_digest())
        );
        assert_eq!(
            bindings.inputs,
            [
                "binding-structure",
                "kerml-construction",
                "kerml-implicit-generalizations",
                "kerml-name-resolution",
                "kerml-validation",
                "schema"
            ]
        );
        assert_eq!(
            sysml_bindings.inputs,
            [
                "binding-structure",
                "kerml-construction",
                "kerml-implicit-generalizations",
                "kerml-name-resolution",
                "kerml-validation",
                "schema",
                "sysml-construction",
                "sysml-name-resolution",
                "sysml-validation"
            ]
        );
        assert!(bindings.depends.is_empty());
        assert!(sysml_bindings.depends.is_empty());
        assert_ne!(bindings.input_digest, sysml_bindings.input_digest);
        for program in &parts[..2] {
            assert_ne!(program.input_digest, bindings.input_digest);
            assert_ne!(program.input_digest, sysml_bindings.input_digest);
            assert!(program.depends.is_empty());
        }
        assert_ne!(parts[0].input_digest, parts[1].input_digest);
        // `expected` is the same entry with the output digest set.
        let entry = expected(RecipePart::Bindings(BindingRecipe::KerML), "0".repeat(64))?;
        assert_eq!(entry.input_digest, bindings.input_digest);
        assert_eq!(entry.output_digest, "0".repeat(64));
        Ok(())
    }

    /// The parts of `keys` as a previous pack records them (output digests
    /// are synthetic: they are not part of the key).
    fn built(keys: &PartKeyInputs) -> Result<Vec<PackPart>, String> {
        Ok(keys
            .parts()?
            .into_iter()
            .enumerate()
            .map(|(index, mut part)| {
                part.output_digest = format!("{index:064}");
                part
            })
            .collect())
    }

    fn every_part() -> BTreeSet<String> {
        PARTS.iter().map(|part| part.name().to_owned()).collect()
    }

    /// The parts `plan_parts` rebuilds when every previous section loads.
    fn rebuilt(previous: &[PackPart], current: &PartKeyInputs) -> Result<Vec<String>, String> {
        Ok(plan_parts(Some(previous), &current.parts()?, &every_part())
            .into_iter()
            .filter(|(_, decision)| *decision != PartDecision::Reuse)
            .map(|(name, _)| name)
            .collect())
    }

    /// `keys` with the pinned input `role` given another SourceId (a
    /// synthetic source edit: no real source changes).
    fn with_input(keys: &PartKeyInputs, role: &str) -> Result<PartKeyInputs, String> {
        let mut changed = keys.clone();
        let id = changed
            .inputs
            .get_mut(role)
            .ok_or_else(|| format!("no input {role}"))?;
        id.sha256 = "f".repeat(64);
        Ok(changed)
    }

    const KERML: &str = "program/kerml";
    const SYSML: &str = "program/sysml";
    const BINDINGS: &str = BINDINGS_SECTION;
    const SYSML_BINDINGS: &str = SYSML_BINDINGS_SECTION;

    #[test]
    fn unchanged_keys_reuse_every_part() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        assert!(rebuilt(&previous, &keys)?.is_empty());
        let plan = plan_parts(Some(&previous), &keys.parts()?, &every_part());
        assert!(
            plan.iter()
                .all(|(_, decision)| *decision == PartDecision::Reuse)
        );
        Ok(())
    }

    #[test]
    fn a_source_change_rebuilds_only_the_parts_that_read_it() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        for (role, expected) in [
            // The SysML.xtext SourceId: only program/sysml (the programs are
            // prepared independently).
            ("sysml-grammar", &[SYSML][..]),
            ("kerml-grammar", &[KERML]),
            ("expressions", &[KERML, SYSML]),
            ("probe-behavior", &[KERML, SYSML]),
            // A KerML Rcore source or the structure: both binding parts.
            ("kerml-construction", &[BINDINGS, SYSML_BINDINGS]),
            (
                "kerml-implicit-generalizations",
                &[BINDINGS, SYSML_BINDINGS],
            ),
            ("kerml-name-resolution", &[BINDINGS, SYSML_BINDINGS]),
            ("kerml-validation", &[BINDINGS, SYSML_BINDINGS]),
            ("binding-structure", &[BINDINGS, SYSML_BINDINGS]),
            // A SysML Rcore source: only rcore/sysml-bindings.
            ("sysml-construction", &[SYSML_BINDINGS]),
            ("sysml-name-resolution", &[SYSML_BINDINGS]),
            ("sysml-validation", &[SYSML_BINDINGS]),
            // The Ecore file: everything.
            ("schema", &[KERML, SYSML, BINDINGS, SYSML_BINDINGS]),
        ] {
            assert_eq!(
                rebuilt(&previous, &with_input(&keys, role)?)?,
                expected,
                "{role}"
            );
        }
        Ok(())
    }

    /// `keys` with each of `components` given another identity.
    fn with_components(keys: &PartKeyInputs, components: &[&str]) -> PartKeyInputs {
        let mut changed = keys.clone();
        for component in components {
            changed
                .compiler
                .insert((*component).to_owned(), "0".repeat(64));
        }
        changed
    }

    /// Plan-level: each component identity is changed alone. A real edit of
    /// intersect-xtext or intersect-model also changes intersect-kerml's
    /// chained identity (see the `a_real_..._edit_...` tests below).
    #[test]
    fn a_component_change_rebuilds_only_the_parts_it_compiles() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        for (component, expected) in [
            // intersect-xtext compiles the grammars, not the bindings.
            ("intersect-xtext", &[KERML, SYSML][..]),
            ("intersect-model", &[KERML, SYSML, BINDINGS, SYSML_BINDINGS]),
            // The bindings are keyed on the recipe digests, not on the rest
            // of intersect-kerml.
            ("intersect-kerml", &[KERML, SYSML]),
            (BINDINGS_COMPONENT, &[BINDINGS]),
            (SYSML_BINDINGS_COMPONENT, &[SYSML_BINDINGS]),
        ] {
            let changed = with_components(&keys, &[component]);
            assert_eq!(rebuilt(&previous, &changed)?, expected, "{component}");
        }
        Ok(())
    }

    /// An intersect-kerml edit outside `kerml_bindings.rs` changes only
    /// intersect-kerml's identity; an intersect-xtext edit also changes it
    /// (build.rs chains intersect-xtext's sources). Neither changes the recipe
    /// digest, so only the programs are rebuilt and the bindings are reused.
    #[test]
    fn a_kerml_or_xtext_edit_without_a_recipe_change_reuses_the_bindings() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        for components in [
            &["intersect-kerml"][..],
            &["intersect-kerml", "intersect-xtext"],
        ] {
            let changed = with_components(&keys, components);
            assert_eq!(
                changed.compiler.get(BINDINGS_COMPONENT),
                keys.compiler.get(BINDINGS_COMPONENT)
            );
            assert_eq!(
                rebuilt(&previous, &changed)?,
                [KERML, SYSML],
                "{components:?}"
            );
            let plan = plan_parts(Some(&previous), &changed.parts()?, &every_part());
            assert!(plan.contains(&(BINDINGS.to_owned(), PartDecision::Reuse)));
            assert!(plan.contains(&(SYSML_BINDINGS.to_owned(), PartDecision::Reuse)));
        }
        Ok(())
    }

    /// A real intersect-xtext edit changes intersect-xtext's and (chained)
    /// intersect-kerml's identities but not the recipe digest: the programs
    /// are rebuilt, the bindings reused.
    #[test]
    fn a_real_intersect_xtext_edit_rebuilds_only_the_programs() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        let changed = with_components(&keys, &["intersect-xtext", "intersect-kerml"]);
        assert_eq!(rebuilt(&previous, &changed)?, [KERML, SYSML]);
        Ok(())
    }

    /// A recipe-digest change alone rebuilds only its bindings part. A real
    /// edit of `kerml_bindings.rs` changes both recipe digests (the SysML one
    /// covers it) and intersect-kerml's identity (it is an intersect-kerml
    /// source), so it rebuilds every part; a real edit of `sysml_bindings.rs`
    /// rebuilds the programs and the SysML bindings, not the KerML bindings.
    #[test]
    fn a_recipe_digest_change_rebuilds_only_the_bindings() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        let changed = with_components(&keys, &[BINDINGS_COMPONENT]);
        assert_eq!(rebuilt(&previous, &changed)?, [BINDINGS]);
        let changed = with_components(&keys, &[SYSML_BINDINGS_COMPONENT]);
        assert_eq!(rebuilt(&previous, &changed)?, [SYSML_BINDINGS]);
        let changed = with_components(
            &keys,
            &[
                BINDINGS_COMPONENT,
                SYSML_BINDINGS_COMPONENT,
                "intersect-kerml",
            ],
        );
        assert_eq!(
            rebuilt(&previous, &changed)?,
            [KERML, SYSML, BINDINGS, SYSML_BINDINGS]
        );
        let changed = with_components(&keys, &[SYSML_BINDINGS_COMPONENT, "intersect-kerml"]);
        assert_eq!(
            rebuilt(&previous, &changed)?,
            [KERML, SYSML, SYSML_BINDINGS]
        );
        Ok(())
    }

    /// A real intersect-model edit changes intersect-model's and (chained)
    /// intersect-kerml's identities: every part is rebuilt.
    #[test]
    fn a_real_intersect_model_edit_rebuilds_every_part() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        for components in [
            &["intersect-model"][..],
            &["intersect-model", "intersect-kerml"],
        ] {
            let changed = with_components(&keys, components);
            assert_eq!(
                rebuilt(&previous, &changed)?,
                [KERML, SYSML, BINDINGS, SYSML_BINDINGS],
                "{components:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_constant_change_rebuilds_only_the_parts_that_read_it() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let previous = built(&keys)?;
        for (constant, expected) in [
            ("sysml.setting_contracts", &[SYSML][..]),
            ("kerml.source_policy", &[KERML]),
            ("prediction_call_depth", &[KERML, SYSML]),
            ("bindings.recipe", &[BINDINGS]),
            ("bindings.source_bytes", &[BINDINGS]),
            ("sysml-bindings.recipe", &[SYSML_BINDINGS]),
            ("sysml-bindings.source_bytes", &[SYSML_BINDINGS]),
        ] {
            let mut changed = keys.clone();
            changed
                .constants
                .insert(constant.to_owned(), Value::from("changed"));
            assert_eq!(rebuilt(&previous, &changed)?, expected, "{constant}");
        }
        Ok(())
    }

    #[test]
    fn plan_fails_closed() -> Result<(), String> {
        let keys = PartKeyInputs::current()?;
        let expected = keys.parts()?;
        let previous = built(&keys)?;
        // No previous pack: everything is built.
        assert!(
            plan_parts(None, &expected, &every_part())
                .iter()
                .all(|(_, decision)| *decision == PartDecision::Rebuild(RebuildReason::NoPrevious))
        );
        // A key match whose previous section did not load is rebuilt.
        let mut loaded = every_part();
        loaded.remove(SYSML_BINDINGS);
        let plan = plan_parts(Some(&previous), &expected, &loaded);
        assert_eq!(
            plan[3],
            (
                SYSML_BINDINGS.to_owned(),
                PartDecision::Rebuild(RebuildReason::LoadRefused)
            )
        );
        assert_eq!(plan[0].1, PartDecision::Reuse);
        assert_eq!(plan[2].1, PartDecision::Reuse);
        // A missing previous part, or one whose entry differs in anything but
        // the output digest, is rebuilt; the output digest alone is no key.
        let mut edited = previous.clone();
        edited.remove(1);
        edited[0].output_digest = "e".repeat(64);
        edited[1].inputs.pop();
        let plan = plan_parts(Some(&edited), &expected, &every_part());
        let decisions: Vec<_> = plan.iter().map(|(_, decision)| *decision).collect();
        assert_eq!(
            decisions,
            [
                PartDecision::Reuse,
                PartDecision::Rebuild(RebuildReason::KeyChanged),
                PartDecision::Rebuild(RebuildReason::KeyChanged),
                PartDecision::Reuse,
            ]
        );
        Ok(())
    }

    #[test]
    fn a_rebuilt_dependency_rebuilds_its_dependents() {
        let part = |name: &str, depends: &[&str]| PackPart {
            name: name.into(),
            components: vec!["c".into()],
            inputs: vec![name.into()],
            depends: depends.iter().map(|name| (*name).to_owned()).collect(),
            input_digest: format!("key-{name}"),
            output_digest: String::new(),
        };
        let expected = [
            part("a", &[]),
            part("b", &["a"]),
            part("c", &["b"]),
            part("d", &[]),
            // Depends on a later part: never reused.
            part("e", &["f"]),
            part("f", &[]),
        ];
        let all: BTreeSet<String> = expected.iter().map(|part| part.name.clone()).collect();
        let decisions = |previous: &[PackPart], loaded: &BTreeSet<String>| {
            plan_parts(Some(previous), &expected, loaded)
                .into_iter()
                .map(|(_, decision)| decision)
                .collect::<Vec<_>>()
        };
        use PartDecision::{Rebuild, Reuse};
        use RebuildReason::{DependencyRebuilt, KeyChanged, LoadRefused};
        assert_eq!(
            decisions(&expected, &all),
            [
                Reuse,
                Reuse,
                Reuse,
                Reuse,
                Rebuild(DependencyRebuilt),
                Reuse
            ]
        );
        let mut changed = expected.clone();
        changed[0].input_digest = "old".into();
        assert_eq!(
            decisions(&changed, &all),
            [
                Rebuild(KeyChanged),
                Rebuild(DependencyRebuilt),
                Rebuild(DependencyRebuilt),
                Reuse,
                Rebuild(DependencyRebuilt),
                Reuse
            ]
        );
        let mut loaded = all.clone();
        loaded.remove("b");
        assert_eq!(
            decisions(&expected, &loaded),
            [
                Reuse,
                Rebuild(LoadRefused),
                Rebuild(DependencyRebuilt),
                Reuse,
                Rebuild(DependencyRebuilt),
                Reuse
            ]
        );
    }

    #[test]
    fn every_part_has_its_markers() {
        for part in PARTS {
            assert_eq!(RecipePart::named(part.name()), Some(part));
            for reused in [true, false] {
                let marker = part.marker(reused);
                assert!(marker.ends_with(&format!(":{}", part.name())), "{marker}");
                assert_eq!(marker.starts_with("pack_part_reused:"), reused);
            }
        }
        assert_eq!(RecipePart::named("program/other"), None);
    }

    #[test]
    fn mismatched_definitions_names_the_roles() {
        assert_eq!(
            mismatched_definitions(&[]),
            ["expressions", "kerml-grammar", "schema", "sysml-grammar"]
        );
    }

    #[test]
    fn saved_compiler_reads_only_the_envelope() -> Result<(), String> {
        assert_eq!(
            saved_compiler(
                BINDINGS_SECTION,
                r#"{"compiler":"c","digest":"d","format":4,"scopes":[]}"#
            )?,
            "c"
        );
        let error = saved_compiler(SYSML_BINDINGS_SECTION, "[]")
            .err()
            .ok_or("an array was read as an envelope")?;
        assert!(error.contains(SYSML_BINDINGS_SECTION), "{error}");
        Ok(())
    }

    /// A part's section holds the saved text as nested JSON, byte for byte,
    /// and reading the section gives the same part and text back.
    #[test]
    fn a_binding_part_nests_its_saved_text() -> Result<(), String> {
        let part = RcoreBindingsPart {
            bundle: kerml_bindings::bundle(),
            saved: kerml_bindings::save(&[])?,
        };
        let section = canonical_json(&part).map_err(|error| error.to_string())?;
        let text = std::str::from_utf8(&section).map_err(|error| error.to_string())?;
        assert!(
            text.ends_with(&format!("\"saved\":{}}}", part.saved())),
            "the saved text is not nested verbatim"
        );
        let read: RcoreBindingsPart =
            serde_json::from_slice(&section).map_err(|error| error.to_string())?;
        assert_eq!(read, part);
        assert_eq!(read.saved(), part.saved());
        // Text that is not JSON cannot be stored as a part.
        let broken = RcoreBindingsPart {
            saved: "{".into(),
            ..part
        };
        assert!(canonical_json(&broken).is_err());
        Ok(())
    }

    #[test]
    fn bindings_part_requires_the_recipe_results() -> Result<(), String> {
        // `new` refuses bindings that are not the recipe jobs' results.
        let error = RcoreBindingsPart::new(&[])
            .err()
            .ok_or("empty part was built")?;
        assert!(error.contains("not the recipe jobs' results"), "{error}");
        // A deserialized part holding no bindings, under the current identity
        // and the recipe's bundle, is refused by `restore` (and so by
        // `pack_session` and `load_pack_bindings`).
        let empty = RcoreBindingsPart {
            bundle: kerml_bindings::bundle(),
            saved: kerml_bindings::save(&[])?,
        };
        assert_eq!(
            saved_compiler(BINDINGS_SECTION, empty.saved())?,
            kerml_bindings::compiler_identity()
        );
        let error = empty.restore().err().ok_or("empty part restored")?;
        assert!(
            error.contains("does not hold the recipe's bindings"),
            "{error}"
        );
        // A foreign bundle is refused before the saved text is read.
        let mut foreign = empty.clone();
        foreign.bundle.sources.swap(1, 2);
        let error = foreign.restore().err().ok_or("foreign bundle restored")?;
        assert!(error.contains("bundle differs"), "{error}");
        Ok(())
    }

    #[test]
    fn sysml_bindings_part_requires_the_recipe_results() -> Result<(), String> {
        let error = SysmlBindingsPart::new(&[])
            .err()
            .ok_or("empty part was built")?;
        assert!(
            error.contains("Compiled SysML bindings")
                && error.contains("not the recipe jobs' results"),
            "{error}"
        );
        let empty = SysmlBindingsPart {
            bundle: sysml_bindings::bundle(),
            saved: sysml_bindings::save(&[])?,
        };
        let error = empty.restore().err().ok_or("empty part restored")?;
        assert!(
            error.contains("rcore/sysml-bindings does not hold the recipe's bindings"),
            "{error}"
        );
        // The KerML recipe's saved text is refused by its compiler identity,
        // and the KerML bundle as a foreign bundle.
        let kerml_text = SysmlBindingsPart {
            bundle: sysml_bindings::bundle(),
            saved: kerml_bindings::save(&[])?,
        };
        let error = kerml_text.restore().err().ok_or("KerML text restored")?;
        assert!(
            error.contains("another binding compiler identity"),
            "{error}"
        );
        let kerml_bundle = SysmlBindingsPart {
            bundle: kerml_bindings::bundle(),
            saved: empty.saved,
        };
        let error = kerml_bundle
            .restore()
            .err()
            .ok_or("KerML bundle restored")?;
        assert!(error.contains("bundle differs"), "{error}");
        Ok(())
    }

    #[test]
    fn binding_compiles_are_the_recipes() -> Result<(), String> {
        for recipe in [BindingRecipe::KerML, BindingRecipe::SysML] {
            let compile = BindingCompile::new(recipe)?;
            assert_eq!(compile.bundle, recipe.bundle());
            assert_eq!(compile.jobs, recipe.binding_jobs(&compile.bundle)?);
            assert_eq!(
                RecipePart::named(recipe.section()),
                Some(RecipePart::Bindings(recipe))
            );
        }
        // Both parts reused: nothing is compiled, no marker.
        let kerml = RcoreBindingsPart {
            bundle: kerml_bindings::bundle(),
            saved: String::new(),
        };
        let sysml = SysmlBindingsPart {
            bundle: sysml_bindings::bundle(),
            saved: String::new(),
        };
        let markers = std::cell::RefCell::new(Vec::new());
        let observe = |marker| markers.borrow_mut().push(marker);
        compile_binding_parts(
            Some(kerml.clone()),
            Some(sysml),
            |_| Err("compile reached".to_owned()),
            &observe,
        )?;
        assert!(markers.borrow().is_empty());
        // One part missing: one request, for that recipe; a result list of
        // the wrong length is refused.
        let error = compile_binding_parts(
            Some(kerml),
            None,
            |requests| {
                if requests.len() != 1 || requests[0].recipe != BindingRecipe::SysML {
                    return Err("unexpected requests".to_owned());
                }
                Ok(Vec::new())
            },
            &observe,
        )
        .err()
        .ok_or("a missing result was accepted")?;
        assert!(error.contains("0 results for 1 requests"), "{error}");
        assert_eq!(*markers.borrow(), ["bindings_compile_begin"]);
        Ok(())
    }
}
