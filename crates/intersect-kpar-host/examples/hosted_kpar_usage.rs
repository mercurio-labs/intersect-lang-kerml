//! Native integration harness. Hosts own files, network access and scheduling.
#[path = "support/driver.rs"]
mod driver;
#[path = "support/usage.rs"]
mod usage;
use intersect_kerml::{
    components::with_component_language,
    language_pack::{self, BindingCompile},
    native_pilot::NativePilotSession,
    sysml_host::SysmlBindings,
};
use intersect_model::definitions::{
    conversion::{InvocationContext, InvocationObserver, ProviderId},
    pack::LanguagePack,
    rcore_scope::VerifiedScope,
};
use intersect_text::{Source, grammar::RuntimeAdmissionLoadMode};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

struct Host;
impl InvocationObserver for Host {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

fn definitions() -> Result<Vec<Source>, String> {
    macro_rules! source {
        ($path:literal) => {
            Source {
                uri: $path.into(),
                text: include_str!(concat!("../../../fixtures/native-pilot/v1/sources/", $path))
                    .into(),
            }
        };
    }
    Ok(vec![
        source!("org.omg.sysml/model/SysML.ecore"),
        source!(
            "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext"
        ),
        source!("org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext"),
        source!("org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext"),
    ])
}

fn compile_bindings(requests: &[BindingCompile]) -> Result<Vec<Vec<VerifiedScope>>, String> {
    std::thread::scope(|threads| {
        let mut handles = Vec::new();
        for (index, request) in requests.iter().enumerate() {
            for job in &request.jobs {
                let handle = std::thread::Builder::new()
                    .stack_size(64 * 1024 * 1024)
                    .spawn_scoped(threads, move || {
                        request.compile_job(job, &|marker| {
                            println!("binding {:?}: {marker}", request.recipe)
                        })
                    })
                    .map_err(|e| e.to_string())?;
                handles.push((index, handle));
            }
        }
        let mut results: Vec<Vec<VerifiedScope>> =
            (0..requests.len()).map(|_| Vec::new()).collect();
        for (index, handle) in handles {
            results[index].extend(handle.join().map_err(|_| "Binding compiler panicked")??);
        }
        Ok(results)
    })
}

fn run(cache: &Path, args: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let path = cache.join("programs.pack");
    let previous = std::fs::read(&path)
        .ok()
        .map(LanguagePack::read)
        .transpose()
        .map_err(|e| e.to_string())?;
    let preparation = InvocationContext::new(Rc::new(Host));
    let start = Instant::now();
    let roles = [
        language_pack::ProgramRole::KerML,
        language_pack::ProgramRole::SysML,
    ];
    let pack = match previous {
        Some(pack) if language_pack::check_pack(&pack, &roles).is_ok() => {
            println!("Using the exact compatible local language pack");
            pack
        }
        previous => {
            let bytes = language_pack::build_language_pack(
                &definitions()?,
                previous.as_ref(),
                &preparation,
                compile_bindings,
                |marker| println!("{} ms: {marker}", start.elapsed().as_millis()),
            )?;
            std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
            LanguagePack::read(bytes).map_err(|e| e.to_string())?
        }
    };
    if args.iter().any(|arg| arg == "--pack-only") {
        return Ok(());
    }
    let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)?;
    let (bundle, scopes) = language_pack::load_pack_sysml_bindings(&pack)?;
    let bindings = intersect_kerml::sdk::bindings_identity(&bundle);
    with_component_language(
        &session,
        &preparation,
        SysmlBindings {
            bundle: &bundle,
            scopes: &scopes,
            identity: &bindings,
        },
        |language| driver::run(language, cache, args),
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cache = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/component-demo/cache"));
    let result = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run(&cache, args.get(1..).unwrap_or_default()))
        .map_err(|e| e.to_string())
        .and_then(|thread| thread.join().map_err(|_| "Harness panicked".to_owned())?);
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
