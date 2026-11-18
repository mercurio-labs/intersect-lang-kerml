//! Host-side phase probe for mounting an already prepared whole standard library.
use intersect_kerml::{
    components::with_shared_component_language,
    language_pack,
    native_pilot::NativePilotSession,
    sysml_host::{SysmlBindings, with_sysml_host},
};
use intersect_model::{
    components::{ComponentImage, MountedModel, SharedSchema},
    definitions::{conversion::InvocationContext, pack::LanguagePack},
    frozen::SnapshotTrust,
};
use intersect_text::{
    Source,
    components::ComponentLanguage,
    grammar::{RuntimeAdmissionLoadMode, RuntimePredictionMemoMode},
};
use std::{sync::Arc, time::Instant};

fn phase<T>(name: &str, run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    println!("begin {name}");
    let clock = Instant::now();
    let value = run()?;
    println!("end {name}: {:.2}s", clock.elapsed().as_secs_f64());
    Ok(value)
}
fn run() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("Pass a prepared library pack")?;
    let pack = phase("pack read", || {
        LanguagePack::read(std::fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    })?;
    let session = phase("session", || {
        NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)
    })?;
    let (bundle, scopes) = phase("bindings", || {
        language_pack::load_pack_sysml_bindings(&pack)
    })?;
    let identity = intersect_kerml::sdk::bindings_identity(&bundle);
    let preparation = InvocationContext::default();
    let library = phase("library snapshot", || {
        with_sysml_host(
            &session,
            &preparation,
            SysmlBindings {
                bundle: &bundle,
                scopes: &scopes,
                identity: &identity,
            },
            RuntimePredictionMemoMode::On,
            |host| {
                language_pack::prepare_library(
                    Some(language_pack::pack_library_payload(&pack).ok_or("Missing library")??),
                    None,
                    host.context(),
                    language_pack::LibraryOptions {
                        trust: SnapshotTrust::VerifiedHostCache,
                        ..Default::default()
                    },
                    |_| Err("Stale library".into()),
                )
                .map(|p| p.model)
            },
        )
    })?;
    let context = phase("schema", || {
        SharedSchema::from_metamodel(session.frozen_metamodel()?)
    })?;
    let records = library.resource_records().map_err(|e| e.to_string())?;
    let closed = phase("metadata-only repackage", || {
        use intersect_model::{
            frozen::{ById, FrozenStore},
            storage::{self, ModelStore},
        };
        let mut store = FrozenStore::from_model(library.clone());
        storage::atomic(&mut store, |store| {
            for mut record in records.into_values() {
                record.package = None;
                store.set_resource_record(record)?;
            }
            Ok(())
        })?;
        store
            .into_builder()
            .map_err(|e| e.to_string())?
            .link(&ById)
            .map_err(|e| format!("{e:?}"))?
            .validate()
            .map_err(|e| format!("{e:?}"))
    })?;
    let image = phase("prepare image", || {
        ComponentImage::prepare(context.clone(), "urn:intersect:standard-library", closed)
    })?;
    let view = phase("construct mount", || {
        MountedModel::new(context.clone(), vec![image.clone()])
    })?;
    drop(view);
    with_shared_component_language(
        &session,
        &preparation,
        SysmlBindings {
            bundle: &bundle,
            scopes: &scopes,
            identity: &identity,
        },
        context,
        |language| {
            let sources = [Source {
                uri: "file:///project.sysml".into(),
                text: "package Test { attribute torque : ScalarValues::Real; }".into(),
            }];
            for iteration in 0..2 {
                let result = phase(&format!("project analysis {iteration}"), || {
                    language.analyze(&sources, &[Arc::clone(&image)])
                })?;
                if !result.diagnostics.is_empty() {
                    return Err(format!("{:?}", result.diagnostics));
                }
            }
            Ok(())
        },
    )
}
fn main() {
    let result = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .map_err(|e| e.to_string())
        .and_then(|t| t.join().map_err(|_| "Probe panicked".to_owned())?);
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
