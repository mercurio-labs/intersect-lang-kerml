//! Handwritten milestone-only text fixtures using the CLI's prepared-pack path.
//!
//! INTERSECT_EVAL_PACK must contain the standard library snapshot, built with
//! `sysml_pack build . PACK --library` from the same sources as the test binary.
//! A plain pack fails with "no library snapshot". These are milestone tests;
//! they never build a missing library or repair an incompatible pack implicitly.
use intersect_kerml::{
    language_pack::{self, LibraryOptions, pack_library_payload, prepare_library},
    native_pilot::NativePilotSession,
    sysml_host::{SysmlBindings, with_sysml_host},
};
use intersect_model::{
    core::frozen::{Model, SnapshotTrust},
    definitions::{conversion::InvocationContext, pack::LanguagePack},
};
use intersect_text::Source;
use intersect_text::grammar::{RuntimeAdmissionLoadMode, RuntimePredictionMemoMode};

/// None means that no pack was configured, never that a configured pack failed.
pub fn load(text: &str) -> Result<Option<Model>, String> {
    load_sources(&[Source {
        uri: "memory:/eval-fixture.sysml".into(),
        text: text.into(),
    }])
}

pub fn load_sources(sources: &[Source]) -> Result<Option<Model>, String> {
    let Some(path) = std::env::var_os("INTERSECT_EVAL_PACK") else {
        eprintln!("SKIPPED milestone text fixture: INTERSECT_EVAL_PACK is unset");
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|e| format!("INTERSECT_EVAL_PACK {path:?}: {e}"))?;
    let pack = LanguagePack::read(bytes).map_err(|e| e.to_string())?;
    let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)
        .map_err(|e| e.to_string())?;
    let (bundle, scopes) = language_pack::load_pack_sysml_bindings(&pack)?;
    let identity = pack
        .manifest()
        .parts
        .iter()
        .find(|part| part.name == language_pack::SYSML_BINDINGS_SECTION)
        .map(|part| format!("{}:{}", part.input_digest, part.output_digest))
        .ok_or("pack has no combined SysML binding identity")?;
    let preparation = InvocationContext::default();
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
            let payload = pack_library_payload(&pack).transpose()?;
            let library = prepare_library(
                payload,
                None,
                host.context(),
                LibraryOptions {
                    use_snapshot: true,
                    trust: SnapshotTrust::HostCache,
                },
                |sources| host.parse_library(sources, &|_| {}).map(|(model, _)| model),
            )?;
            let loaded = host.load_on_library(&library.model, sources, &|_| {})?;
            if !loaded.diagnostics.is_empty() || loaded.resolved != loaded.references {
                return Err(format!(
                    "text load: {} of {} references resolved; diagnostics: {:?}",
                    loaded.resolved, loaded.references, loaded.diagnostics
                ));
            }
            loaded
                .model
                .map(Some)
                .ok_or_else(|| "text load produced no validated model".into())
        },
    )
}
