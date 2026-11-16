//! Handwritten host support (not generated): the SysML v2 standard library
//! for the `frozen_library_load` and `sysml_pack` examples. Reads the
//! library files, binds both languages and the combined KerML+SysML Rcore
//! bindings (`sysml_scopes.rs`), and parses sources into a frozen store with
//! `frozen_load::load_frozen` (the whole library, or user files on top of a
//! prepared library). All file I/O and clocks of the library path are here.
// Each example uses its own subset of this support.
#![allow(dead_code)]
use intersect_kerml::{
    frozen_load::{self, FrozenLoad},
    language_pack::{KERML_SOURCE, LibraryContext, PreparedLibrary, SYSML_SOURCE},
    native_pilot::NativePilotSession,
    sysml_host::{ProjectDependencies, ProjectLoad, SysmlBindings, SysmlHost, with_sysml_host},
};
use intersect_model::{
    core::frozen::{FrozenStore, Metamodel},
    definitions::{conversion::InvocationContext, library_part::LibrarySource},
};
use intersect_text::{Source, StoredEvaluation};
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

#[path = "sysml_scopes.rs"]
mod sysml_scopes;

pub const CORPUS: &str = "fixtures/native-pilot/v1/sources";
pub const LIBRARY: &str = "sysml.library";

pub fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

/// Read one corpus file. Logical resource URIs may not contain spaces.
pub fn read_source(root: &Path, path: &str) -> Result<Source, String> {
    let text = std::fs::read_to_string(root.join(CORPUS).join(path))
        .map_err(|error| format!("{path}: {error}"))?;
    Ok(Source {
        uri: path.replace(' ', "%20"),
        text,
    })
}

/// Every `.kerml` and `.sysml` file of the library, with its language.
pub fn library_sources(root: &Path) -> Result<Vec<LibrarySource>, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries =
            std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let base = root.join(CORPUS);
    let mut found = Vec::new();
    walk(&base.join(LIBRARY), &mut found)?;
    let mut sources = Vec::new();
    for path in found {
        let language = match path.extension().and_then(|e| e.to_str()) {
            Some("kerml") => KERML_SOURCE,
            Some("sysml") => SYSML_SOURCE,
            _ => continue,
        };
        let Ok(relative) = path.strip_prefix(&base) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let source = read_source(root, &relative)?;
        sources.push(LibrarySource {
            uri: source.uri,
            language: language.into(),
            text: source.text,
        });
    }
    sources.sort_by(|a, b| a.uri.cmp(&b.uri));
    Ok(sources)
}

/// Setup times of a [`LibraryHost`], in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct SetupTimes {
    pub metamodel_ms: f64,
    pub bindings_ms: f64,
    pub bind_languages_ms: f64,
}

/// Phase times of one load, in milliseconds.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadTimes {
    pub parse_kerml_ms: f64,
    pub parse_sysml_ms: f64,
    pub link_ms: f64,
    pub implicit_ms: f64,
    pub defaults_ms: f64,
    pub validate_ms: f64,
}

/// Both languages bound, the frozen metamodel and the combined bindings
/// (`intersect_kerml::sysml_host`), with the setup times and phase timing.
pub struct LibraryHost<'h> {
    pub metamodel: Arc<Metamodel>,
    pub bindings: String,
    pub bindings_cached: bool,
    pub setup: SetupTimes,
    host: &'h SysmlHost<'h>,
}

impl LibraryHost<'_> {
    pub fn context(&self) -> LibraryContext<'_> {
        self.host.context()
    }
    /// Parse one source into `store` with its language, without linking.
    pub fn parse(
        &self,
        source: &Source,
        store: &mut FrozenStore,
    ) -> Result<(StoredEvaluation, usize, usize), String> {
        self.host.parse(source, store)
    }
    /// Parse `sources` into `store` (KerML files with the KerML language,
    /// the rest with SysML), link, complete and validate, timing each phase.
    pub fn load(
        &self,
        store: FrozenStore,
        sources: &[Source],
    ) -> Result<(FrozenLoad, LoadTimes), String> {
        let (times, mut parse, observe) = self.timers();
        let load = self.host.load(store, sources, &mut parse, &observe)?;
        Ok((load, times.get()))
    }
    /// Load user files through package resolution and lock admission.
    pub fn load_project(
        &self,
        library: &PreparedLibrary,
        sources: &[Source],
    ) -> Result<ProjectLoad, String> {
        self.host
            .load_project(library, sources, ProjectDependencies::default(), &|_| {})
    }

    /// The whole library in parse order (KerML, then SysML), from an empty
    /// store; an error unless every phase passed and every reference
    /// resolved uniquely.
    pub fn parse_library(
        &self,
        sources: &[LibrarySource],
    ) -> Result<(intersect_model::core::frozen::Model, FrozenLoad, LoadTimes), String> {
        let (times, mut parse, observe) = self.timers();
        let (model, load) = self
            .host
            .parse_library_with(sources, &mut parse, &observe)?;
        Ok((model, load, times.get()))
    }
    /// A timed parse and a phase observer filling the returned times.
    #[allow(clippy::type_complexity)]
    fn timers(
        &self,
    ) -> (
        Rc<Cell<LoadTimes>>,
        impl FnMut(&Source, &mut FrozenStore) -> Result<(StoredEvaluation, usize, usize), String> + '_,
        impl Fn(&'static str) + '_,
    ) {
        let times = Rc::new(Cell::new(LoadTimes::default()));
        let clock = Rc::new(Cell::new(Instant::now()));
        let kerml_end = Rc::new(Cell::new(None::<f64>));
        let parse = {
            let (clock, kerml_end) = (clock.clone(), kerml_end.clone());
            move |source: &Source, store: &mut FrozenStore| {
                if !source.uri.ends_with(".kerml") && kerml_end.get().is_none() {
                    kerml_end.set(Some(ms(clock.get())));
                }
                self.host.parse(source, store)
            }
        };
        let observe = {
            let times = times.clone();
            move |marker: &'static str| {
                let mut t = times.get();
                let elapsed = ms(clock.get());
                match marker {
                    "parse_end" => {
                        let kerml = kerml_end.get().unwrap_or(elapsed);
                        t.parse_kerml_ms = kerml;
                        t.parse_sysml_ms = elapsed - kerml;
                    }
                    "link_end" => t.link_ms = elapsed,
                    "implicit_end" => t.implicit_ms = elapsed,
                    "defaults_end" => t.defaults_ms = elapsed,
                    "validate_end" => t.validate_ms = elapsed,
                    _ => {}
                }
                times.set(t);
                clock.set(Instant::now());
            }
        };
        (times, parse, observe)
    }
}

/// Bind both languages of `session` and the combined bindings (from the
/// host's binding cache, `sysml_scopes.rs`), then run `body` with the host.
pub fn with_library_host<T>(
    session: &NativePilotSession,
    preparation: &InvocationContext<'_>,
    body: impl FnOnce(&LibraryHost<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let metamodel =
        session.with_definitions(|kerml, _, _, _| frozen_load::frozen_metamodel(kerml))?;
    let clock = Instant::now();
    let (bundle, scopes, bindings_cached) = sysml_scopes::compile_cached(&metamodel)?;
    let bindings = sysml_scopes::identity(&metamodel)?;
    let cache_ms = ms(clock);
    let clock = Instant::now();
    let mode = frozen_load::prediction_memo_mode_from_env()?;
    let identity = bindings.clone();
    with_sysml_host(
        session,
        preparation,
        SysmlBindings {
            bundle: &bundle,
            scopes: &scopes,
            identity: &identity,
        },
        mode,
        |host| {
            // Binding the metamodel, the resolvers and both languages.
            let bind_ms = ms(clock);
            body(&LibraryHost {
                metamodel: host.metamodel().clone(),
                bindings,
                bindings_cached,
                setup: SetupTimes {
                    metamodel_ms: 0.0,
                    bindings_ms: cache_ms,
                    bind_languages_ms: bind_ms,
                },
                host,
            })
        },
    )
}

/// Bind a prepared pack without recompiling grammars or bindings. The library
/// binding identity is the same source-based identity used by `with_library_host`
/// and `sysml_pack build --library`, so their prepared snapshots can be reused.
pub fn with_library_host_from_pack<T>(
    session: &NativePilotSession,
    pack: &intersect_model::definitions::pack::LanguagePack,
    preparation: &InvocationContext<'_>,
    body: impl FnOnce(&LibraryHost<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let metamodel =
        session.with_definitions(|kerml, _, _, _| frozen_load::frozen_metamodel(kerml))?;
    let (bundle, scopes) = intersect_kerml::language_pack::load_pack_sysml_bindings(pack)?;
    let bindings = sysml_scopes::identity(&metamodel)?;
    with_sysml_host(
        session,
        preparation,
        SysmlBindings {
            bundle: &bundle,
            scopes: &scopes,
            identity: &bindings,
        },
        frozen_load::prediction_memo_mode_from_env()?,
        |host| {
            body(&LibraryHost {
                metamodel: host.metamodel().clone(),
                bindings: bindings.clone(),
                bindings_cached: true,
                setup: SetupTimes::default(),
                host,
            })
        },
    )
}

/// Write a private temporary next to `path`, then rename it into place.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut name = path
        .file_name()
        .ok_or_else(|| format!("{}: not a file path", path.display()))?
        .to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    let temporary = path.with_file_name(name);
    let written = std::fs::File::create(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        })
        .map_err(|error| format!("{}: {error}", temporary.display()))
        .and_then(|()| {
            std::fs::rename(&temporary, path)
                .map_err(|error| format!("{}: {error}", path.display()))
        });
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}
