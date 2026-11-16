//! Handwritten combined KerML+SysML Rcore binding recipe (the
//! `rcore/sysml-bindings` pack part). Not generated.
//!
//! The SysML hosts link and complete both languages with one binding set: the
//! KerML name resolution and construction (`semantics/kerml-*.rcore`, shared
//! with `kerml_bindings`, not copied) layered with the SysML ones
//! (`semantics/sysml-*.rcore`), compiled against the stored SysML structure
//! (`metamodel/SysML.ecore.rcore`). The bundle orders the structure, then the
//! implicit-generalization map, the three KerML behaviors (name resolution,
//! construction, validation) and the three SysML ones; every source depends
//! on every source before it. This module owns
//! WHICH sources, declarations and binding kinds compile; the limits, the
//! compiler call and the job-result check are `kerml_bindings`' (shared, so
//! both recipes compile the same way), and the compiler itself is
//! intersect-model's.
//!
//! The recipe is a list of independent jobs, one per behavior source that
//! declares any `bind scope`, `bind default`, `bind implicit` or
//! `bind validate` (in that kind order within the source). A host may run them on its own threads (each
//! needs a large stack; the examples use 64 MiB), concatenates the results in
//! job order and checks them with `check_job_results`. This module performs
//! no I/O and uses no clocks, threads or environment.
use crate::kerml_bindings::{self, BindingJob};
use intersect_model::definitions::{
    bundle::{DefinitionBundle, DefinitionSource, SourceId, SourceKind},
    rcore_scope::{
        BindingKind, VerifiedScope, load_bindings, load_verified_bindings, save_bindings,
        source_default_bindings, source_implicit_bindings, source_scope_bindings,
        source_validate_bindings,
    },
};
use sha2::{Digest, Sha256};

const IMPLICIT: &str = include_str!("../semantics/implicit-generalizations.rcore");
const KERML_RESOLUTION: &str = include_str!("../semantics/kerml-name-resolution.rcore");
const KERML_CONSTRUCTION: &str = include_str!("../semantics/kerml-construction.rcore");
const KERML_VALIDATION: &str = include_str!("../semantics/kerml-validation.rcore");
const SYSML_RESOLUTION: &str = include_str!("../semantics/sysml-name-resolution.rcore");
const SYSML_CONSTRUCTION: &str = include_str!("../semantics/sysml-construction.rcore");
const SYSML_VALIDATION: &str = include_str!("../semantics/sysml-validation.rcore");

/// URI of the SysML name-resolution behavior; it imports the KerML one.
pub const SYSML_RESOLUTION_URI: &str = "package:/sysml/sysml-name-resolution.rcore";
/// URI of the SysML construction behavior; it imports the KerML one.
pub const SYSML_CONSTRUCTION_URI: &str = "package:/sysml/sysml-construction.rcore";
/// URI of the SysML validation rules.
pub const SYSML_VALIDATION_URI: &str = "package:/sysml/sysml-validation.rcore";

/// The behavior sources in bundle order, by pack input role: (role, URI,
/// text). The first four are `kerml_bindings`' sources.
const BEHAVIORS: [(&str, &str, &str); 7] = [
    (
        "kerml-implicit-generalizations",
        kerml_bindings::IMPLICIT_URI,
        IMPLICIT,
    ),
    (
        "kerml-name-resolution",
        kerml_bindings::RESOLUTION_URI,
        KERML_RESOLUTION,
    ),
    (
        "kerml-construction",
        kerml_bindings::CONSTRUCTION_URI,
        KERML_CONSTRUCTION,
    ),
    (
        "kerml-validation",
        kerml_bindings::VALIDATION_URI,
        KERML_VALIDATION,
    ),
    (
        "sysml-name-resolution",
        SYSML_RESOLUTION_URI,
        SYSML_RESOLUTION,
    ),
    (
        "sysml-construction",
        SYSML_CONSTRUCTION_URI,
        SYSML_CONSTRUCTION,
    ),
    ("sysml-validation", SYSML_VALIDATION_URI, SYSML_VALIDATION),
];

/// Version of this binding recipe (sources, job split, kinds and the format
/// of `compiler_identity`). Version 1 is the recipe the examples'
/// `support/sysml_scopes.rs` compiled before it moved here; version 2 adds
/// the two validation sources and `bind validate`. Bump it when the recipe
/// changes in a way `compiler_identity` does not already capture.
pub const BINDING_RECIPE: &str = "intersect.sysml.bindings.v2";

/// The text of this recipe module, embedded to digest it (`recipe_digest`).
const RECIPE_SOURCE: &str = include_str!("sysml_bindings.rs");

/// SHA-256 (lowercase hex) over this module's own text (CRLF normalized to
/// LF), then `kerml_bindings::recipe_digest()`, whose limits, compiler call
/// and job check this recipe shares.
pub fn recipe_digest() -> String {
    let mut hasher = Sha256::new();
    kerml_bindings::hash_lf_normalized(&mut hasher, RECIPE_SOURCE);
    hasher.update(b"\n+kerml_bindings:");
    hasher.update(kerml_bindings::recipe_digest().as_bytes());
    format!("{:x}", hasher.finalize())
}

/// The identity written into, and required from, saved bindings:
/// `BINDING_RECIPE`, `recipe_digest()` and intersect-model's
/// `SOURCE_IDENTITY`, joined by `+`.
///
/// Covered: this module and `kerml_bindings.rs` (`recipe_digest`), the only
/// intersect-kerml code that decides what compiles, and intersect-model,
/// which compiles, saves and loads the images. The embedded Rcore sources are
/// not in the digest: `load_bindings` checks each image's context hash
/// against `bundle()`, whose source ids hash them, and a pack pins them as
/// inputs of the part. Not covered: the rest of intersect-kerml and
/// intersect-xtext (see `kerml_bindings::compiler_identity`).
pub fn compiler_identity() -> String {
    format!(
        "{BINDING_RECIPE}+{}+{}",
        recipe_digest(),
        intersect_model::SOURCE_IDENTITY
    )
}

/// The definition bundle every binding compiles against: the stored
/// structure, then every behavior source, each depending on the structure and
/// on every behavior source before it.
pub fn bundle() -> DefinitionBundle {
    let mut sources = vec![kerml_bindings::structure_source()];
    for (_, uri, text) in BEHAVIORS {
        let mut source = DefinitionSource::new(uri, SourceKind::RcoreBehavior, text);
        source.dependencies = sources.iter().map(|source| source.id.clone()).collect();
        sources.push(source);
    }
    DefinitionBundle {
        version: 1,
        sources,
        namespaces: vec![],
    }
}

/// The source identities of `bundle()`, by pack input role. The structure
/// and the KerML roles are `kerml_bindings::input_ids`' roles.
pub fn input_ids() -> [(&'static str, SourceId); 8] {
    let [a, b, c, d, e, f, g] =
        BEHAVIORS.map(|(role, uri, text)| (role, SourceId::for_text(uri, text)));
    [
        ("binding-structure", kerml_bindings::structure_source().id),
        a,
        b,
        c,
        d,
        e,
        f,
        g,
    ]
}

/// Every `bind scope`, then `bind default`, then `bind implicit`, then
/// `bind validate` of `source`.
fn source_declarations(source: &DefinitionSource) -> Result<Vec<(usize, BindingKind)>, String> {
    let limits = kerml_bindings::SOURCE_LIMITS;
    let failed = |error| format!("{}: {error:?}", source.id.uri);
    let mut declarations: Vec<_> = source_scope_bindings(source, limits)
        .map_err(failed)?
        .iter()
        .map(|binding| (binding.declaration, BindingKind::Scope))
        .collect();
    declarations.extend(
        source_default_bindings(source, limits)
            .map_err(failed)?
            .iter()
            .map(|binding| (binding.declaration, BindingKind::Default)),
    );
    declarations.extend(
        source_implicit_bindings(source, limits)
            .map_err(failed)?
            .iter()
            .map(|binding| (binding.declaration, BindingKind::Implicit)),
    );
    declarations.extend(
        source_validate_bindings(source, limits)
            .map_err(failed)?
            .iter()
            .map(|binding| (binding.declaration, BindingKind::Validate)),
    );
    Ok(declarations)
}

/// The recipe's jobs over `bundle` (which must be `bundle()`): one per
/// behavior source in bundle order with its scopes, defaults and implicit
/// bindings; a source without any (the implicit-generalization map) has no
/// job. Bindings are concatenated in this job order.
pub fn binding_jobs(bundle: &DefinitionBundle) -> Result<Vec<BindingJob>, String> {
    let behaviors = match bundle.sources.split_first() {
        Some((_, behaviors))
            if behaviors.len() == BEHAVIORS.len()
                && behaviors
                    .iter()
                    .zip(BEHAVIORS)
                    .all(|(source, (_, uri, _))| source.id.uri == uri) =>
        {
            behaviors
        }
        _ => return Err("Unexpected SysML binding bundle".into()),
    };
    let mut jobs = Vec::new();
    for source in behaviors {
        let declarations = source_declarations(source)?;
        if !declarations.is_empty() {
            jobs.push(BindingJob {
                source: source.id.clone(),
                declarations,
            });
        }
    }
    Ok(jobs)
}

/// Compile one job as one batch, with `kerml_bindings`' limits. `observe`
/// receives the compiler's phase markers; any clock belongs to the host. Deep
/// recursion: run it on a thread with a large stack.
pub fn compile_job(
    bundle: &DefinitionBundle,
    job: &BindingJob,
    observe: &impl Fn(&'static str),
) -> Result<Vec<VerifiedScope>, String> {
    kerml_bindings::compile_recipe_job("SysML", bundle, job, observe)
}

/// Require `scopes` to be the jobs' results in job order (see
/// `kerml_bindings::check_job_results`).
pub fn check_job_results(
    bundle: &DefinitionBundle,
    jobs: &[BindingJob],
    scopes: &[VerifiedScope],
) -> Result<(), String> {
    kerml_bindings::check_recipe_job_results("SysML", bundle, jobs, scopes)
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
    fn bundle_layers_the_sysml_behaviors_on_the_kerml_ones() {
        let bundle = bundle();
        let ids: Vec<_> = bundle.sources.iter().map(|source| &source.id).collect();
        let pinned = input_ids();
        assert_eq!(ids, pinned.iter().map(|(_, id)| id).collect::<Vec<_>>());
        assert_eq!(bundle.sources[0].kind, SourceKind::Rcore);
        for (index, source) in bundle.sources.iter().enumerate().skip(1) {
            assert_eq!(source.kind, SourceKind::RcoreBehavior);
            let earlier: Vec<_> = bundle.sources[..index]
                .iter()
                .map(|source| source.id.clone())
                .collect();
            assert_eq!(source.dependencies, earlier);
        }
        // The shared roles name the same sources as the KerML recipe.
        let kerml = kerml_bindings::input_ids();
        for (role, id) in &kerml {
            assert_eq!(
                pinned
                    .iter()
                    .find(|(pinned, _)| pinned == role)
                    .map(|(_, id)| id),
                Some(id),
                "{role}"
            );
        }
        assert_eq!(bundle, super::bundle(), "deterministic");
    }

    #[test]
    fn compiler_identity_names_the_recipe_and_its_compiler() {
        assert_eq!(
            compiler_identity(),
            format!(
                "intersect.sysml.bindings.v2+{}+{}",
                recipe_digest(),
                intersect_model::SOURCE_IDENTITY
            )
        );
        assert_ne!(compiler_identity(), kerml_bindings::compiler_identity());
        assert!(!compiler_identity().contains(crate::SOURCE_IDENTITY));
    }

    #[test]
    fn recipe_digest_covers_this_module_and_the_shared_helpers() {
        let digest = recipe_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let mut expected = RECIPE_SOURCE.replace("\r\n", "\n");
        expected.push_str("\n+kerml_bindings:");
        expected.push_str(&kerml_bindings::recipe_digest());
        assert_eq!(digest, format!("{:x}", Sha256::digest(expected.as_bytes())));
        assert_ne!(digest, kerml_bindings::recipe_digest());
        assert!(RECIPE_SOURCE.contains("pub fn recipe_digest()"));
    }

    #[test]
    fn jobs_are_one_per_binding_source() -> Result<(), String> {
        let bundle = bundle();
        let jobs = binding_jobs(&bundle)?;
        let sources: Vec<_> = jobs.iter().map(|job| job.source.uri.as_str()).collect();
        // The implicit-generalization map declares no binding.
        assert_eq!(
            sources,
            [
                kerml_bindings::RESOLUTION_URI,
                kerml_bindings::CONSTRUCTION_URI,
                kerml_bindings::VALIDATION_URI,
                SYSML_RESOLUTION_URI,
                SYSML_CONSTRUCTION_URI,
                SYSML_VALIDATION_URI
            ]
        );
        for job in &jobs {
            assert!(!job.declarations.is_empty());
            // Scopes, then defaults, then implicit bindings.
            let rank = |kind: &BindingKind| match kind {
                BindingKind::Scope => 0,
                BindingKind::Default => 1,
                BindingKind::Implicit => 2,
                BindingKind::Validate => 3,
            };
            assert!(
                job.declarations
                    .windows(2)
                    .all(|pair| rank(&pair[0].1) <= rank(&pair[1].1))
            );
        }
        // The KerML jobs cover the same declarations as the KerML recipe.
        let kerml_bundle = kerml_bindings::bundle();
        let kerml = kerml_bindings::binding_jobs(&kerml_bundle)?;
        for (combined, alone) in jobs.iter().zip(&kerml) {
            assert_eq!(combined.source, alone.source);
            assert_eq!(combined.declarations, alone.declarations);
        }
        assert_eq!(binding_jobs(&bundle)?, jobs, "deterministic");
        assert!(check_job_results(&bundle, &jobs, &[]).is_err());
        assert!(check_recipe_results(&[]).is_err());
        Ok(())
    }

    #[test]
    fn foreign_bundle_has_no_jobs() {
        let mut bundle = bundle();
        bundle.sources.swap(3, 4);
        assert!(binding_jobs(&bundle).is_err());
        assert!(binding_jobs(&kerml_bindings::bundle()).is_err());
    }

    #[test]
    fn load_refuses_text_that_is_not_saved_bindings() {
        assert!(load("{}").is_err());
        assert!(load("not json").is_err());
    }
}
