//! Handwritten host wrapper over `intersect_kerml::kerml_bindings`, the
//! library recipe that compiles the source-authored KerML name resolution and
//! construction defaults against the stored SysML structure for the frozen
//! engine's Rcore scopes. This file owns only host concerns: the scoped
//! threads with 64 MiB stacks that run the recipe's jobs in parallel, the
//! binding cache file and the `KERML_BINDING_TIMING` clock. The recipe, its
//! sources and the saved-bindings compiler identity are library code.
use intersect_kerml::kerml_bindings::{self, BindingJob};
use intersect_model::{
    core::frozen::Metamodel,
    definitions::{
        bundle::{DefinitionBundle, DefinitionSource},
        rcore_scope::VerifiedScope,
        resources::DeclarationId,
        schema::Schema,
    },
};

const STACK_BYTES: usize = 64 * 1024 * 1024;

/// The stored SysML structure source (`metamodel/SysML.ecore.rcore`, the
/// migration output of the Pilot SysML.ecore) that `schema`'s declarations name.
pub fn structure(schema: &Schema) -> Result<DefinitionSource, String> {
    kerml_bindings::structure(schema)
}

/// The metamodel's MOF property redefinitions, for `Metamodel::with_redefinitions`.
pub fn redefinitions(schema: &Schema) -> Result<Vec<(DeclarationId, DeclarationId)>, String> {
    kerml_bindings::redefinitions(schema)
}

/// Run the recipe `jobs` over `bundle` on one scoped thread each (64 MiB
/// stacks) and concatenate the bindings in job order. Set
/// `KERML_BINDING_TIMING` to print per-phase host timings.
pub fn compile_jobs(
    bundle: &DefinitionBundle,
    jobs: &[BindingJob],
) -> Result<Vec<VerifiedScope>, String> {
    // KERML_BINDING_TIMING prints each compile phase marker with the elapsed
    // host time; the library only reports the markers.
    let timing = std::env::var_os("KERML_BINDING_TIMING").is_some();
    let start = std::time::Instant::now();
    let compiled = std::thread::scope(|threads| {
        let handles: Vec<_> = jobs
            .iter()
            .map(|job| {
                std::thread::Builder::new()
                    .stack_size(STACK_BYTES)
                    .spawn_scoped(threads, move || {
                        kerml_bindings::compile_job(bundle, job, &|marker| {
                            if timing {
                                println!(
                                    "binding phase {} {marker}: {:?}",
                                    job.source.uri,
                                    start.elapsed()
                                );
                            }
                        })
                    })
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<_, _>>()?;
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| "binding compiler panicked".to_owned())?
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    let scopes: Vec<_> = compiled.into_iter().flatten().collect();
    kerml_bindings::check_job_results(bundle, jobs, &scopes)?;
    Ok(scopes)
}

/// The definition bundle and one verified binding per `bind scope`,
/// `bind default` and `bind implicit`, compiled on host threads. `mm` must be
/// the metamodel of the schema whose declarations name the stored structure.
pub fn compile(mm: &Metamodel) -> Result<(DefinitionBundle, Vec<VerifiedScope>), String> {
    structure(mm.schema())?;
    let bundle = kerml_bindings::bundle();
    let jobs = kerml_bindings::binding_jobs(&bundle)?;
    let scopes = compile_jobs(&bundle, &jobs)?;
    Ok((bundle, scopes))
}

/// Where `compile_cached` keeps verified bindings: `KERML_BINDING_CACHE`, or
/// `target/kerml-bindings.json` under the working directory, shared by every
/// executable since the compiler identity does not depend on the executable.
fn cache_path() -> std::path::PathBuf {
    std::env::var_os("KERML_BINDING_CACHE")
        .map(Into::into)
        .unwrap_or_else(|| "target/kerml-bindings.json".into())
}

/// `compile`, reusing the verified bindings saved earlier by any host with the
/// same compiler identity (`kerml_bindings::compiler_identity`: the recipe
/// version, the digest of `kerml_bindings.rs`, and intersect-model's sources
/// and features; not the rest of this crate or intersect-xtext, so an edit
/// there keeps the cache) for the same bundle (`load` checks each image
/// against the bundle rebuilt from the embedded Rcore sources), when they are
/// the recipe jobs' results
/// (`kerml_bindings::check_recipe_results`). The cache is this host's own
/// output; on any mismatch the bindings are compiled again and the cache is
/// rewritten. The flag says whether the cache was used.
pub fn compile_cached(
    mm: &Metamodel,
) -> Result<(DefinitionBundle, Vec<VerifiedScope>, bool), String> {
    structure(mm.schema())?;
    let path = cache_path();
    let bundle = kerml_bindings::bundle();
    if let Ok(text) = std::fs::read_to_string(&path) {
        match kerml_bindings::load(&text)
            .and_then(|scopes| kerml_bindings::check_recipe_results(&scopes).map(|()| scopes))
        {
            Ok(scopes) => return Ok((bundle, scopes, true)),
            Err(reason) => println!("binding cache not used: {reason}"),
        }
    }
    let (bundle, scopes) = compile(mm)?;
    let text = kerml_bindings::save(&scopes)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Executables share the file: write a private temporary, then rename it
    // into place so a concurrent reader never sees a partial cache.
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((bundle, scopes, false))
}
