//! Handwritten KerML Rcore binding recipe (language-pack step 7). Not
//! generated.
//!
//! The source-authored KerML name resolution
//! (`semantics/kerml-name-resolution.rcore`) and construction defaults
//! (`semantics/kerml-construction.rcore`) compile against the stored SysML
//! structure (`metamodel/SysML.ecore.rcore`, the migration output of the
//! pinned Pilot `SysML.ecore`) into one verified binding per `bind scope`,
//! `bind default` and `bind implicit`, and the validation rules
//! (`semantics/kerml-validation.rcore`) into one per `bind validate`. This
//! module owns WHICH sources,
//! limits, declarations and binding kinds compile; the compiler itself is
//! intersect-model's `compile_binding_kinds_with_progress`, and the saved form
//! is its `save_bindings` / `load_bindings`.
//!
//! The recipe is a list of independent jobs, one per behavior source. A host
//! may run them on its own threads (each needs a large stack; the examples use
//! 64 MiB), concatenates the results in job order and checks them with
//! `check_job_results`. This module performs no I/O and uses no clocks,
//! threads or environment.
use intersect_model::definitions::{
    bundle::{DefinitionBundle, DefinitionSource, SourceId, SourceKind, SourceSpan},
    ecore::ParseLimits,
    rcore::feature_redefinitions,
    rcore_behavior::{self, ir_v2::Limits},
    rcore_scope::{
        BindingKind, VerifiedScope, compile_binding_kinds_with_progress, load_bindings,
        load_verified_bindings, save_bindings, source_default_bindings, source_implicit_bindings,
        source_scope_bindings, source_validate_bindings,
    },
    resources::DeclarationId,
    schema::Schema,
};
use sha2::{Digest, Sha256};

const STRUCTURE: &str = include_str!("../metamodel/SysML.ecore.rcore");
const IMPLICIT: &str = include_str!("../semantics/implicit-generalizations.rcore");
const RESOLUTION: &str = include_str!("../semantics/kerml-name-resolution.rcore");
const CONSTRUCTION: &str = include_str!("../semantics/kerml-construction.rcore");
const VALIDATION: &str = include_str!("../semantics/kerml-validation.rcore");

/// URI of the stored SysML structure; equal to the converted structure source
/// that the Pilot programs' schema declarations name.
pub const STRUCTURE_URI: &str = "org.omg.sysml/model/SysML.ecore.rcore";
/// URI of the shared implicit-generalization map, which the name resolution
/// imports. It holds no bindings of its own.
pub const IMPLICIT_URI: &str = "package:/kerml/implicit-generalizations.rcore";
/// URI of the KerML name-resolution behavior (`bind scope`).
pub const RESOLUTION_URI: &str = "package:/kerml/kerml-name-resolution.rcore";
/// URI of the KerML construction behavior (`bind default`, `bind implicit`).
pub const CONSTRUCTION_URI: &str = "package:/kerml/kerml-construction.rcore";
/// URI of the KerML validation rules (`bind validate`).
pub const VALIDATION_URI: &str = "package:/kerml/kerml-validation.rcore";

/// Version of this binding recipe (sources, limits, job split, kinds and the
/// format of `compiler_identity`). Version 4 narrows the identity to
/// `recipe_digest` and intersect-model's `SOURCE_IDENTITY`; version 5 adds the
/// implicit-generalization source to the bundle; version 6 adds the
/// validation source and its `bind validate` job. Bump it when the recipe
/// changes in a way `compiler_identity` does not already capture.
pub const BINDING_RECIPE: &str = "intersect.kerml.bindings.v6";

/// The text of this recipe module, embedded to digest it (`recipe_digest`).
const RECIPE_SOURCE: &str = include_str!("kerml_bindings.rs");

/// Per-source parse bound for the structure and the behaviors.
pub const SOURCE_LIMITS: ParseLimits = ParseLimits {
    source_bytes: 2_000_000,
};

/// Function IR limits for a binding compile: the defaults, with room for
/// 32,768 parameters instead of 4,096. Each job verifies every function its
/// bindings reach as one program, and every function and closure counts its
/// parameters against this one total; the KerML and SysML name resolution
/// (2026-11-06) exceeds the default. It bounds verification only.
///
/// The lowering work is also raised, from 1,048,576 to 16,777,216 steps:
/// lowering each lambda scans every expression of its source for the
/// enclosing lambda, so the work grows with lambdas times expressions, and
/// the name resolution with the Pilot example rules (2026-11-08) exceeds the
/// default.
pub fn compile_limits() -> Limits {
    let mut limits = Limits::default();
    limits.scalar.max_parameters = 32_768;
    limits.scalar.max_work = 16_777_216;
    limits
}

/// SHA-256 (lowercase hex) of this recipe module's own text, with CRLF
/// normalized to LF so it is the same on every platform and checkout.
pub fn recipe_digest() -> String {
    let mut hasher = Sha256::new();
    hash_lf_normalized(&mut hasher, RECIPE_SOURCE);
    format!("{:x}", hasher.finalize())
}

/// Feed `text` to `hasher` with CRLF normalized to LF. Shared with
/// `sysml_bindings::recipe_digest`.
pub(crate) fn hash_lf_normalized(hasher: &mut Sha256, text: &str) {
    for (index, line) in text.split("\r\n").enumerate() {
        if index > 0 {
            hasher.update(b"\n");
        }
        hasher.update(line.as_bytes());
    }
}

/// The identity written into, and required from, saved bindings:
/// `BINDING_RECIPE`, `recipe_digest()` and intersect-model's
/// `SOURCE_IDENTITY`, joined by `+`.
///
/// Covered: this module (`recipe_digest`), which is the only intersect-kerml
/// code that decides what compiles (the embedded sources, limits, jobs,
/// declarations and binding kinds), and intersect-model, which compiles,
/// saves and loads the images (its identity hashes its sources AND its
/// enabled features). The three embedded Rcore sources are not in the
/// digest: `load_bindings` checks each image's context hash against
/// `bundle()`, whose source ids hash them, and a pack pins them as inputs of
/// the bindings part. The other intersect-kerml code that touches bindings
/// (`check_job_results` here, `RcoreBindingsPart` and `load_pack_bindings` in
/// language_pack.rs) only checks or encodes results, so it cannot change
/// what a given identity compiles to.
///
/// Not covered: the rest of intersect-kerml and intersect-xtext, so an edit
/// there keeps saved bindings valid. An edit to the bindings ENCODING in
/// language_pack.rs is guarded by the bindings section kind and the part
/// version, which must be bumped by hand.
///
/// Equal identities mean the same compiler inputs; it is a cache key for the
/// host's own output, not a signature.
pub fn compiler_identity() -> String {
    format!(
        "{BINDING_RECIPE}+{}+{}",
        recipe_digest(),
        intersect_model::SOURCE_IDENTITY
    )
}

/// The stored SysML structure source, without a schema check.
pub fn structure_source() -> DefinitionSource {
    DefinitionSource::new(STRUCTURE_URI, SourceKind::Rcore, STRUCTURE)
}

/// The stored SysML structure source, required to be the source that
/// `schema`'s declarations name.
pub fn structure(schema: &Schema) -> Result<DefinitionSource, String> {
    let structure = structure_source();
    let expected = schema
        .declarations
        .values()
        .next()
        .map(|declaration| declaration.source.clone())
        .ok_or("Schema has no declarations")?;
    if structure.id != expected {
        return Err(format!(
            "Stored structure {:?} does not match the schema's {expected:?}",
            structure.id
        ));
    }
    Ok(structure)
}

/// The metamodel's MOF property redefinitions, for `Metamodel::with_redefinitions`.
pub fn redefinitions(schema: &Schema) -> Result<Vec<(DeclarationId, DeclarationId)>, String> {
    feature_redefinitions(&structure(schema)?, SOURCE_LIMITS).map_err(|error| error.to_string())
}

/// The definition bundle every binding compiles against: the stored structure
/// and the four behaviors, each depending on the structure; the name
/// resolution also depends on the implicit-generalization map it imports, and
/// the validation rules on both (they read derived information through them).
pub fn bundle() -> DefinitionBundle {
    let structure = structure_source();
    let behavior = |uri: &str, text: &str| {
        let mut source = DefinitionSource::new(uri, SourceKind::RcoreBehavior, text);
        source.dependencies.push(structure.id.clone());
        source
    };
    let implicit = behavior(IMPLICIT_URI, IMPLICIT);
    let mut resolution = behavior(RESOLUTION_URI, RESOLUTION);
    resolution.dependencies.push(implicit.id.clone());
    let construction = behavior(CONSTRUCTION_URI, CONSTRUCTION);
    let mut validation = behavior(VALIDATION_URI, VALIDATION);
    validation
        .dependencies
        .extend([implicit.id.clone(), resolution.id.clone()]);
    DefinitionBundle {
        version: 1,
        sources: vec![structure, implicit, resolution, construction, validation],
        namespaces: vec![],
    }
}

/// The source identities of `bundle()`, by pack input role.
pub fn input_ids() -> [(&'static str, SourceId); 5] {
    [
        ("binding-structure", structure_source().id),
        (
            "kerml-implicit-generalizations",
            SourceId::for_text(IMPLICIT_URI, IMPLICIT),
        ),
        (
            "kerml-name-resolution",
            SourceId::for_text(RESOLUTION_URI, RESOLUTION),
        ),
        (
            "kerml-construction",
            SourceId::for_text(CONSTRUCTION_URI, CONSTRUCTION),
        ),
        (
            "kerml-validation",
            SourceId::for_text(VALIDATION_URI, VALIDATION),
        ),
    ]
}

/// One independent compile: the declarations of one behavior source, by
/// index in that source, with the selector kind each implements.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingJob {
    pub source: SourceId,
    pub declarations: Vec<(usize, BindingKind)>,
}

/// The recipe's jobs over `bundle` (which must be `bundle()`): every
/// `bind scope` of the name resolution, then every `bind default` and
/// `bind implicit` of the construction source, then every `bind validate` of
/// the validation source. Bindings are concatenated in this job order.
pub fn binding_jobs(bundle: &DefinitionBundle) -> Result<Vec<BindingJob>, String> {
    let (resolution, construction, validation) = match bundle.sources.as_slice() {
        [_, implicit, resolution, construction, validation]
            if implicit.id.uri == IMPLICIT_URI
                && resolution.id.uri == RESOLUTION_URI
                && construction.id.uri == CONSTRUCTION_URI
                && validation.id.uri == VALIDATION_URI =>
        {
            (resolution, construction, validation)
        }
        _ => return Err("Unexpected KerML binding bundle".into()),
    };
    let scopes = source_scope_bindings(resolution, SOURCE_LIMITS)
        .map_err(|error| format!("{error:?}"))?
        .iter()
        .map(|binding| (binding.declaration, BindingKind::Scope))
        .collect();
    let mut defaults: Vec<_> = source_default_bindings(construction, SOURCE_LIMITS)
        .map_err(|error| format!("{error:?}"))?
        .iter()
        .map(|binding| (binding.declaration, BindingKind::Default))
        .collect();
    defaults.extend(
        source_implicit_bindings(construction, SOURCE_LIMITS)
            .map_err(|error| format!("{error:?}"))?
            .iter()
            .map(|binding| (binding.declaration, BindingKind::Implicit)),
    );
    let validations = source_validate_bindings(validation, SOURCE_LIMITS)
        .map_err(|error| format!("{error:?}"))?
        .iter()
        .map(|binding| (binding.declaration, BindingKind::Validate))
        .collect();
    Ok(vec![
        BindingJob {
            source: resolution.id.clone(),
            declarations: scopes,
        },
        BindingJob {
            source: construction.id.clone(),
            declarations: defaults,
        },
        BindingJob {
            source: validation.id.clone(),
            declarations: validations,
        },
    ])
}

/// Compile one job as one batch (one source check for lowering, one for
/// verification). `observe` receives the compiler's phase markers; any clock
/// belongs to the host. Deep recursion: run it on a thread with a large stack.
pub fn compile_job(
    bundle: &DefinitionBundle,
    job: &BindingJob,
    observe: &impl Fn(&'static str),
) -> Result<Vec<VerifiedScope>, String> {
    compile_recipe_job("KerML", bundle, job, observe)
}

/// `compile_job` for the recipe named `recipe` in error text (`KerML`,
/// `SysML`): the same limits and compiler for every binding recipe of this
/// crate (`sysml_bindings::compile_job` uses it).
pub(crate) fn compile_recipe_job(
    recipe: &str,
    bundle: &DefinitionBundle,
    job: &BindingJob,
    observe: &impl Fn(&'static str),
) -> Result<Vec<VerifiedScope>, String> {
    compile_binding_kinds_with_progress(
        bundle,
        &job.source,
        &job.declarations,
        &compile_limits(),
        SOURCE_LIMITS,
        observe,
    )
    .map_err(|errors| {
        format!(
            "{recipe} binding compile of {} failed: {errors:?}",
            job.source.uri
        )
    })
}

/// The span of every job declaration with its kind, in job order: what the
/// verified binding at that position records as its `declaration_span`.
fn job_selectors(
    recipe: &str,
    bundle: &DefinitionBundle,
    jobs: &[BindingJob],
) -> Result<Vec<(SourceSpan, BindingKind)>, String> {
    let mut selectors = Vec::new();
    for job in jobs {
        let source = bundle
            .sources
            .iter()
            .find(|source| source.id == job.source)
            .ok_or_else(|| {
                format!(
                    "{recipe} binding job source {} is not in the bundle",
                    job.source.uri
                )
            })?;
        // The public parse applies the default source bound; require it so a
        // larger source is an explicit error, not a parse diagnostic.
        if source.text.len() > ParseLimits::default().source_bytes {
            return Err(format!(
                "{recipe} binding source {} exceeds the declaration-check parse bound",
                job.source.uri
            ));
        }
        let document = rcore_behavior::parse(source.clone());
        if !document.diagnostics.is_empty() {
            return Err(format!(
                "{recipe} binding source {} does not parse: {:?}",
                job.source.uri, document.diagnostics
            ));
        }
        for (index, kind) in &job.declarations {
            let declaration = document.declarations.get(*index).ok_or_else(|| {
                format!(
                    "{recipe} binding job names declaration {index} beyond {}",
                    job.source.uri
                )
            })?;
            selectors.push((declaration.span.clone(), *kind));
        }
    }
    Ok(selectors)
}

/// Require `scopes` to be the jobs' results in job order: the same count and,
/// position by position, the same binding kind for the same declaration (the
/// binding's recorded `declaration_span`, which names its source, equals the
/// span of the job's declaration at that position). The executable images are
/// not rechecked.
pub fn check_job_results(
    bundle: &DefinitionBundle,
    jobs: &[BindingJob],
    scopes: &[VerifiedScope],
) -> Result<(), String> {
    check_recipe_job_results("KerML", bundle, jobs, scopes)
}

/// `check_job_results` for the recipe named `recipe` in error text
/// (`sysml_bindings::check_job_results` uses it).
pub(crate) fn check_recipe_job_results(
    recipe: &str,
    bundle: &DefinitionBundle,
    jobs: &[BindingJob],
    scopes: &[VerifiedScope],
) -> Result<(), String> {
    let expected = job_selectors(recipe, bundle, jobs)?;
    let refuse = |what: String| {
        Err(format!(
            "Compiled {recipe} bindings ({} bindings) are not the recipe jobs' results \
             ({} bindings) in job order: {what}",
            scopes.len(),
            expected.len()
        ))
    };
    if expected.len() != scopes.len() {
        return refuse("the counts differ".into());
    }
    for (position, ((span, kind), scope)) in expected.iter().zip(scopes).enumerate() {
        if scope.kind() != *kind {
            return refuse(format!(
                "binding {position} is a {:?}, the job's is a {kind:?}",
                scope.kind()
            ));
        }
        if scope.binding().declaration_span != *span {
            return refuse(format!(
                "binding {position} implements another declaration than the job's"
            ));
        }
    }
    Ok(())
}

/// `check_job_results` against the recipe's own jobs over `bundle()`.
pub fn check_recipe_results(scopes: &[VerifiedScope]) -> Result<(), String> {
    let bundle = bundle();
    check_job_results(&bundle, &binding_jobs(&bundle)?, scopes)
}

/// `save_bindings` under `compiler_identity()`.
pub fn save(scopes: &[VerifiedScope]) -> Result<String, String> {
    save_bindings(scopes, &compiler_identity())
}

/// `load_bindings` over `bundle()` under `compiler_identity()`. Trusted input
/// only: the host's own saved output (see `VerifiedExecutable::load_trusted`).
pub fn load(saved: &str) -> Result<Vec<VerifiedScope>, String> {
    load_bindings(saved, &bundle(), &compiler_identity())
}

/// `load` for saved text taken from a language-pack section whose digest the
/// pack read already verified (`load_verified_bindings`): the saved scopes
/// digest, a second hash of the same text, is not recomputed.
pub fn load_verified(saved: &str) -> Result<Vec<VerifiedScope>, String> {
    load_verified_bindings(saved, &bundle(), &compiler_identity())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_is_the_pinned_structure_and_the_behaviors() {
        let bundle = bundle();
        let ids: Vec<_> = bundle.sources.iter().map(|source| &source.id).collect();
        let pinned = input_ids();
        assert_eq!(ids, pinned.iter().map(|(_, id)| id).collect::<Vec<_>>());
        assert_eq!(bundle.sources[0].kind, SourceKind::Rcore);
        let structure = bundle.sources[0].id.clone();
        let implicit = bundle.sources[1].id.clone();
        for behavior in &bundle.sources[1..] {
            assert_eq!(behavior.kind, SourceKind::RcoreBehavior);
        }
        assert_eq!(
            bundle.sources[1].dependencies,
            std::slice::from_ref(&structure)
        );
        assert_eq!(
            bundle.sources[2].dependencies,
            [structure.clone(), implicit.clone()]
        );
        assert_eq!(
            bundle.sources[3].dependencies,
            std::slice::from_ref(&structure)
        );
        let resolution = bundle.sources[2].id.clone();
        assert_eq!(
            bundle.sources[4].dependencies,
            [structure, implicit, resolution]
        );
        // Rebuilding gives the same bundle (no hidden state).
        assert_eq!(bundle, super::bundle());
    }

    #[test]
    fn stored_structure_is_the_native_migration_output() {
        assert_eq!(
            structure_source().id.sha256,
            crate::PILOT_RCORE_SCHEMA_SHA256
        );
    }

    #[test]
    fn compiler_identity_names_the_recipe_and_its_compiler() {
        let identity = compiler_identity();
        assert_eq!(
            identity,
            format!(
                "intersect.kerml.bindings.v6+{}+{}",
                recipe_digest(),
                intersect_model::SOURCE_IDENTITY
            )
        );
        // Not keyed on the rest of this crate.
        assert!(!identity.contains(crate::SOURCE_IDENTITY));
    }

    #[test]
    fn recipe_digest_is_the_lf_normalized_module_text() {
        let digest = recipe_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(digest, recipe_digest(), "deterministic");
        let normalized = RECIPE_SOURCE.replace("\r\n", "\n");
        assert_eq!(
            digest,
            format!("{:x}", Sha256::digest(normalized.as_bytes()))
        );
        assert!(RECIPE_SOURCE.contains("pub fn recipe_digest()"));
    }

    #[test]
    fn jobs_split_by_source_and_kind() -> Result<(), String> {
        let bundle = bundle();
        let jobs = binding_jobs(&bundle)?;
        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].source.uri, RESOLUTION_URI);
        assert_eq!(jobs[1].source.uri, CONSTRUCTION_URI);
        assert_eq!(jobs[2].source.uri, VALIDATION_URI);
        assert!(!jobs[2].declarations.is_empty());
        assert!(
            jobs[2]
                .declarations
                .iter()
                .all(|(_, kind)| *kind == BindingKind::Validate)
        );
        assert!(!jobs[0].declarations.is_empty());
        assert!(
            jobs[0]
                .declarations
                .iter()
                .all(|(_, kind)| *kind == BindingKind::Scope)
        );
        assert!(
            jobs[1]
                .declarations
                .iter()
                .all(|(_, kind)| !matches!(kind, BindingKind::Scope | BindingKind::Validate))
        );
        assert_eq!(binding_jobs(&bundle)?, jobs, "deterministic");
        // No results for non-empty jobs is refused.
        assert!(check_job_results(&bundle, &jobs, &[]).is_err());
        assert!(check_recipe_results(&[]).is_err());
        // Every job declaration resolves to a span in its own source.
        let selectors = job_selectors("KerML", &bundle, &jobs)?;
        let owners: Vec<_> = jobs
            .iter()
            .flat_map(|job| job.declarations.iter().map(move |_| &job.source))
            .collect();
        assert_eq!(selectors.len(), owners.len());
        for ((span, _), source) in selectors.iter().zip(owners) {
            assert_eq!(span.source, *source);
        }
        // A job naming a declaration beyond its source is refused.
        let mut beyond = jobs.clone();
        beyond[0]
            .declarations
            .push((usize::MAX, BindingKind::Scope));
        assert!(job_selectors("KerML", &bundle, &beyond).is_err());
        Ok(())
    }

    #[test]
    fn foreign_bundle_has_no_jobs() {
        let mut bundle = bundle();
        bundle.sources.swap(1, 2);
        assert!(binding_jobs(&bundle).is_err());
    }

    #[test]
    fn load_refuses_text_that_is_not_saved_bindings() {
        assert!(load("{}").is_err());
        assert!(load("not json").is_err());
    }
}
