//! Handwritten host wrapper over `intersect_kerml::sysml_bindings`, the
//! library recipe that compiles the KerML name resolution and construction
//! (`semantics/kerml-*.rcore`) layered with the SysML ones
//! (`semantics/sysml-*.rcore`) against the stored SysML structure, for the
//! frozen engine's Rcore scopes. Mirrors `kerml_scopes.rs`. This file owns
//! only host concerns: the scoped threads with 64 MiB stacks that run the
//! recipe jobs in parallel, the phase timing printout and the binding cache
//! file for hosts without a language pack (a pack carries these bindings as
//! its `rcore/sysml-bindings` part, see
//! `language_pack::load_pack_sysml_bindings`). The recipe, its sources and the
//! saved-bindings compiler identity are library code.
use intersect_kerml::{
    kerml_bindings,
    language_pack::{BindingCompile, BindingRecipe},
    sysml_bindings,
};
use intersect_model::{
    core::frozen::Metamodel,
    definitions::{bundle::DefinitionBundle, rcore_scope::VerifiedScope},
};

const STACK_BYTES: usize = 64 * 1024 * 1024;

/// Run every job of every request at once, one scoped thread each (64 MiB
/// stacks), and return each request's bindings concatenated in job order.
/// Prints each compile phase marker with the elapsed host time.
#[allow(dead_code)]
pub fn compile_requests(requests: &[BindingCompile]) -> Result<Vec<Vec<VerifiedScope>>, String> {
    let started = std::time::Instant::now();
    let compiled = std::thread::scope(|threads| {
        let handles = requests
            .iter()
            .enumerate()
            .flat_map(|(index, request)| request.jobs.iter().map(move |job| (index, request, job)))
            .map(|(index, request, job)| {
                let started = &started;
                std::thread::Builder::new()
                    .stack_size(STACK_BYTES)
                    .spawn_scoped(threads, move || {
                        request.compile_job(job, &|marker| {
                            println!(
                                "{:?} bindings {} {marker} {} ms",
                                request.recipe,
                                job.source.uri,
                                started.elapsed().as_millis()
                            )
                        })
                    })
                    .map(|handle| (index, handle))
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut results: Vec<Vec<VerifiedScope>> =
            (0..requests.len()).map(|_| Vec::new()).collect();
        for (index, handle) in handles {
            let scopes = handle
                .join()
                .map_err(|_| "binding compiler panicked".to_owned())??;
            results[index].extend(scopes);
        }
        Ok::<_, String>(results)
    })?;
    Ok(compiled)
}

/// The definition bundle and one verified binding per `bind scope`,
/// `bind default` and `bind implicit` of the combined recipe, compiled on
/// host threads. `mm` must be the metamodel of the schema whose declarations
/// name the stored structure.
fn compile(mm: &Metamodel) -> Result<(DefinitionBundle, Vec<VerifiedScope>), String> {
    kerml_bindings::structure(mm.schema())?;
    let request = BindingCompile::new(BindingRecipe::SysML)?;
    let scopes = compile_requests(std::slice::from_ref(&request))?
        .pop()
        .ok_or("no SysML binding results")?;
    sysml_bindings::check_job_results(&request.bundle, &request.jobs, &scopes)?;
    Ok((request.bundle, scopes))
}

/// `SYSML_BINDING_CACHE`, or `target/sysml-bindings.json`.
fn cache_path() -> std::path::PathBuf {
    std::env::var_os("SYSML_BINDING_CACHE")
        .map(Into::into)
        .unwrap_or_else(|| "target/sysml-bindings.json".into())
}

/// Identity of the combined bindings for a prepared-library key (decision
/// D10): the binding compiler identity and the logical URI and SHA-256 of
/// every source of the binding bundle.
#[allow(dead_code)]
pub fn identity(mm: &Metamodel) -> Result<String, String> {
    kerml_bindings::structure(mm.schema())?;
    Ok(intersect_kerml::sdk::bindings_identity(
        &sysml_bindings::bundle(),
    ))
}

/// `compile`, reusing bindings this host saved earlier under the same
/// `sysml_bindings::compiler_identity()` for the same bundle (`load` checks
/// each image against the bundle rebuilt from the embedded Rcore sources),
/// when they are the recipe jobs' results. For hosts without a language
/// pack; on any mismatch the bindings are compiled again and the cache is
/// rewritten. The flag says whether the cache was used.
pub fn compile_cached(
    mm: &Metamodel,
) -> Result<(DefinitionBundle, Vec<VerifiedScope>, bool), String> {
    kerml_bindings::structure(mm.schema())?;
    let path = cache_path();
    if let Ok(text) = std::fs::read_to_string(&path) {
        match sysml_bindings::load(&text)
            .and_then(|scopes| sysml_bindings::check_recipe_results(&scopes).map(|()| scopes))
        {
            Ok(scopes) => return Ok((sysml_bindings::bundle(), scopes, true)),
            Err(reason) => println!("sysml binding cache not used: {reason}"),
        }
    }
    let (bundle, scopes) = compile(mm)?;
    let text = sysml_bindings::save(&scopes)?;
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
