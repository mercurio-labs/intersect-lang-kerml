//! Handwritten host example (language-pack steps 6b, 7 and 8): build, inspect and
//! check the KerML/SysML language pack. Not generated.
//!
//! Usage (ROOT is the repository root holding `fixtures/native-pilot/v1`):
//!
//! - `sysml_pack build ROOT OUT.pack [--previous OLD.pack]` prepares the
//!   Pilot session from the four original definitions, compiles the two
//!   Rcore binding parts, the KerML bindings (`rcore/bindings`) and the
//!   combined KerML+SysML bindings (`rcore/sysml-bindings`), with every job of
//!   both recipes at once (one 64 MiB-stack thread per job, through
//!   `support/sysml_scopes.rs`) and writes the pack atomically (temporary
//!   file, then rename). With `--previous`, the build is incremental per part
//!   (`language_pack::build_language_pack`): each part of the old pack whose
//!   part key still matches and that still loads is reused, every other part
//!   is rebuilt (only a binding part rebuild compiles). It prints which
//!   parts it reused or rebuilt and why (`language_pack::explain_previous`);
//!   a missing previous file is a first build.
//!   With `--library`, the pack also gets the prepared standard library
//!   (the optional `library/sysml` part of decision D10): the previous pack's
//!   part is kept when `language_pack::plan_library` reuses it (same key, and
//!   its snapshot loads), otherwise the library under ROOT is parsed with the
//!   combined KerML+SysML bindings (`support/sysml_library.rs`) and
//!   snapshotted.
//! - `sysml_pack host-pack PACK OUT.pack` writes the pack of a host that runs
//!   only the combined KerML+SysML bindings: PACK without its KerML bindings
//!   part (`language_pack::without_kerml_bindings`), every other part kept
//!   byte for byte. The result is read back and must load its programs and
//!   SysML bindings. Keep PACK: it is what `build --previous` and `check`
//!   take; a host pack given as `--previous` compiles the KerML bindings
//!   again.
//! - `sysml_pack dump-json PACK [OUT.json]` writes (or prints) the manifest
//!   and every section as pretty JSON (a binary section by size and digest).
//! - `sysml_pack check-library ROOT PACK [--user FILE]...` checks the
//!   library part (decision D10 gates): its snapshot loads under the key
//!   recomputed here; corrupt, truncated, wrongly keyed and wrong-metamodel
//!   variants are refused with a message; a fresh parse of the part's
//!   library sources snapshots to a byte-identical payload (determinism and
//!   the `recompile` check) and exports exactly as the loaded snapshot does;
//!   and each `--user` file (relative to the corpus) parses, links and
//!   completes on top of the loaded snapshot exactly as on top of the parsed
//!   library. `check --mode recompile` runs it too when the pack has a
//!   library part.
//! - `sysml_pack check ROOT PACK --mode host-cache|revalidate|recompile
//!   [--workers N]` times `LanguagePack::read` (on bytes already in memory)
//!   and `NativePilotSession::from_pack` against a fresh preparation from
//!   source. With each session it then parses the whole `sysml.library`
//!   (every `.kerml` file with the KerML owner, every `.sysml` file with the
//!   SysML owner) into frozen stores and requires identical evaluations and
//!   storage exports (sorted by id) and no parse failure. In every mode it
//!   also times `language_pack::load_pack_bindings` and
//!   `language_pack::load_pack_sysml_bindings` (which only trust the saved
//!   images) and compiles both binding parts afresh on host threads: for
//!   each part the loaded bindings, the fresh ones and the stored part must
//!   save to identical text, so `check` always re-verifies the bindings by
//!   recompile. It then requires the host-side binding cache of the hosts
//!   without a pack (`support/sysml_scopes.rs`, `compile_cached`, which
//!   compiles and writes it when it is stale) to hold the same SysML bindings
//!   as the pack. `recompile` loads in
//!   host-cache mode for the parse comparison and additionally prepares and
//!   compiles the bindings from source a second, independent time: both
//!   preparations must pack to the same bytes, and those must equal the pack
//!   file.
//!
//! Timings of `check`: before the library pass, one worker on each session
//! binds both languages and parses two fixed probe files twice each, so the
//! cold first parse after loading (a loaded session takes the cold
//! physical-input path) is reported apart from the warm second parse and
//! from the binding. The library pass then runs `N` scoped threads sharing
//! one session (default: available parallelism, at most 8); it reports each
//! worker's binding time, the wall time, and per-file parse times summed
//! across workers, which measure contended time, not per-parse cost.
//!
//! Exit codes: 2 for usage errors, 1 for any failure. Each command runs on a
//! scoped worker thread with a 64 MiB stack, as does each parse worker. All
//! file I/O, threads and clocks stay in this example; the library performs
//! none. The language bindings are shared with the `language_pack_equivalence`
//! test through `support/pilot_languages.rs`.
//!
//! # Trust
//!
//! A pack is trusted by convention only: it must be this host's own output,
//! like `VerifiedExecutable::load_trusted`. Its digests and recomputed part
//! keys detect corruption, stale inputs and a different compiler; they are not
//! a signature. The compiler identity hashes each crate's enabled cargo
//! features, so a pack built under other features (for example a workspace or
//! `--all-features` build) is refused and must be rebuilt by that binary.
//! Packs fetched over a network or loaded in a browser are out of scope until
//! packs are signed.
//!
//! # Oracles
//!
//! `host-cache` trusts the saved compiled drafts and binding images;
//! `revalidate` re-verifies each runtime admission and replays each migration
//! origin (the library has no cheaper binding re-verification than the full
//! recompile this host runs in every mode). For the programs (grammar and
//! migration), only `recompile` (two independent preparations giving
//! byte-identical packs equal to the file) checks the compiler output itself;
//! the bindings compiler output is checked by the binding recompile in every
//! mode. `recompile` is the determinism oracle for
//! milestone gates and CI, but it is not yet wired into `ci.yml` (one run takes
//! several minutes in release).
use intersect_kerml::{
    language_pack::{
        self, BindingCompile, BindingRecipe, PartDecision, RcoreBindingsPart, SysmlBindingsPart,
    },
    native_pilot::NativePilotSession,
    sysml_bindings,
};
use intersect_model::core::frozen::FrozenStore;
use intersect_model::core::frozen::Metamodel;
use intersect_model::core::frozen::SnapshotTrust;
use intersect_model::core::storage;
use intersect_model::definitions::bundle::SourceId;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::conversion::InvocationObserver;
use intersect_model::definitions::conversion::ProviderId;
use intersect_model::definitions::library_part::LibraryManifest;
use intersect_model::definitions::library_part::decode_library_part;
use intersect_model::definitions::library_part::encode_library_part;
use intersect_model::definitions::library_part::key_bytes;
use intersect_model::definitions::pack;
use intersect_model::definitions::pack::FileDigest;
use intersect_model::definitions::pack::LanguagePack;
use intersect_text::grammar::RuntimeAdmissionLoadMode;
use intersect_text::{Source, draft::DraftCapability};
use serde_json::json;
use std::{
    cell::Cell,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

#[path = "support/pilot_languages.rs"]
mod pilot_languages;

#[path = "support/sysml_library.rs"]
mod sysml_library;

#[path = "support/python_package.rs"]
mod python_package;

// `sysml_library` loads its own private copy for the library pass; this one
// runs the binding jobs and the host-cache comparison of `check`.
#[allow(clippy::duplicate_mod)]
#[path = "support/sysml_scopes.rs"]
mod sysml_scopes;
use pilot_languages::BoundLanguage;

const CORPUS: &str = "fixtures/native-pilot/v1/sources";
const LIBRARY: &str = "sysml.library";
const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];
/// The probe files parsed by one worker, twice each, before the library pass.
const PROBES: [&str; 2] = [
    "sysml.library/Kernel Libraries/Kernel Data Type Library/ScalarValues.kerml",
    "sysml.library/Systems Library/Items.sysml",
];
const WORKER_STACK_BYTES: usize = 64 * 1024 * 1024;
const USAGE: &str = "usage:
  sysml_pack build ROOT OUT.pack [--previous OLD.pack] [--library] [--python-out DIR] [--wheel]
  sysml_pack host-pack PACK OUT.pack
  sysml_pack dump-json PACK [OUT.json]
  sysml_pack check ROOT PACK --mode host-cache|revalidate|recompile [--workers N]
  sysml_pack check-library ROOT PACK [--user FILE]...";

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    HostCache,
    Revalidate,
    Recompile,
}

enum Command {
    Build {
        root: PathBuf,
        out: PathBuf,
        previous: Option<PathBuf>,
        library: bool,
        python: Option<PathBuf>,
        wheel: bool,
    },
    CheckLibrary {
        root: PathBuf,
        pack: PathBuf,
        user: Vec<String>,
    },
    HostPack {
        pack: PathBuf,
        out: PathBuf,
    },
    DumpJson {
        pack: PathBuf,
        out: Option<PathBuf>,
    },
    Check {
        root: PathBuf,
        pack: PathBuf,
        mode: Mode,
        workers: usize,
    },
}

fn parse_args(args: &[String]) -> Option<Command> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["build", root, out, rest @ ..] => {
            let (mut previous, mut library) = (None, false);
            let (mut python, mut wheel) = (None, false);
            let mut rest = rest.iter();
            while let Some(word) = rest.next() {
                match *word {
                    "--previous" => previous = Some(PathBuf::from(rest.next()?)),
                    "--library" => library = true,
                    "--python-out" => python = Some(PathBuf::from(rest.next()?)),
                    "--wheel" => wheel = true,
                    _ => return None,
                }
            }
            if (wheel && python.is_none()) || (python.is_some() && !library) {
                return None;
            }
            Some(Command::Build {
                root: root.into(),
                out: out.into(),
                previous,
                library,
                python,
                wheel,
            })
        }
        ["check-library", root, pack, rest @ ..] => {
            let mut user = Vec::new();
            let mut rest = rest.iter();
            while let Some(word) = rest.next() {
                match *word {
                    "--user" => user.push((*rest.next()?).to_owned()),
                    _ => return None,
                }
            }
            Some(Command::CheckLibrary {
                root: root.into(),
                pack: pack.into(),
                user,
            })
        }
        ["host-pack", pack, out] => Some(Command::HostPack {
            pack: pack.into(),
            out: out.into(),
        }),
        ["dump-json", pack] => Some(Command::DumpJson {
            pack: pack.into(),
            out: None,
        }),
        ["dump-json", pack, out] => Some(Command::DumpJson {
            pack: pack.into(),
            out: Some(out.into()),
        }),
        ["check", root, pack, "--mode", mode, rest @ ..] => Some(Command::Check {
            root: root.into(),
            pack: pack.into(),
            mode: match *mode {
                "host-cache" => Mode::HostCache,
                "revalidate" => Mode::Revalidate,
                "recompile" => Mode::Recompile,
                _ => return None,
            },
            workers: match rest {
                [] => default_workers(),
                ["--workers", count] => count.parse().ok().filter(|count| *count > 0)?,
                _ => return None,
            },
        }),
        _ => None,
    }
}

/// Parse workers by default: the available parallelism, at most eight (each
/// worker binds both languages and holds one library file's store).
fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get().min(8))
        .unwrap_or(1)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = parse_args(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("sysml-pack".into())
            .stack_size(WORKER_STACK_BYTES)
            .spawn_scoped(scope, || run(&command))
            .map_err(|error| format!("Cannot start the pack worker: {error}"))?
            .join()
            .map_err(|_| "Pack worker panicked".to_owned())?
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(command: &Command) -> Result<(), String> {
    match command {
        Command::Build {
            root,
            out,
            previous,
            library,
            python,
            wheel,
        } => build(
            root,
            out,
            previous.as_deref(),
            *library,
            python.as_deref(),
            *wheel,
        ),
        Command::CheckLibrary { root, pack, user } => check_library(root, pack, user),
        Command::HostPack { pack, out } => host_pack(pack, out),
        Command::DumpJson { pack, out } => dump(pack, out.as_deref()),
        Command::Check {
            root,
            pack,
            mode,
            workers,
        } => check(root, pack, *mode, *workers),
    }
}

/// Write `pack` without its KerML bindings part to `out`, self-checked.
fn host_pack(pack: &Path, out: &Path) -> Result<(), String> {
    let full = LanguagePack::read(read_pack(pack)?).map_err(|error| error.to_string())?;
    let bytes = language_pack::without_kerml_bindings(&full)?;
    let host = LanguagePack::read(bytes).map_err(|error| error.to_string())?;
    NativePilotSession::from_pack(&host, RuntimeAdmissionLoadMode::HostCache)?;
    language_pack::load_pack_sysml_bindings(&host)?;
    write_atomic(out, host.bytes())?;
    println!(
        "host pack {}: {:.1} MiB (full pack {:.1} MiB)",
        out.display(),
        mib(host.bytes().len()),
        mib(full.bytes().len())
    );
    Ok(())
}

fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Read one corpus file. Logical resource URIs may not contain spaces.
fn read_source(root: &Path, path: &str) -> Result<Source, String> {
    let text = std::fs::read_to_string(root.join(CORPUS).join(path))
        .map_err(|error| format!("{path}: {error}"))?;
    Ok(Source {
        uri: path.replace(' ', "%20"),
        text,
    })
}

fn read_definitions(root: &Path) -> Result<Vec<Source>, String> {
    DEFINITIONS
        .iter()
        .map(|path| read_source(root, path))
        .collect()
}

/// Read at most `MAX_PACK_BYTES + 1` bytes, so an oversized file is refused
/// (by `LanguagePack::read`) without first being read whole into memory.
fn read_pack(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    let file = std::fs::File::open(path).map_err(failed)?;
    let limit = u64::try_from(pack::MAX_PACK_BYTES).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(failed)?;
    Ok(bytes)
}

/// Write a private temporary next to `path`, sync it, then rename it into
/// place, so a reader never sees a partial file. On failure the temporary is
/// removed (best effort).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
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
        // Best effort: the original error is the one reported.
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

fn preparation() -> InvocationContext<'static> {
    InvocationContext::new(Rc::new(Observer))
}

/// Explain the refusal that a feature-unified build most often hits.
fn load_error(error: String) -> String {
    if error.contains("compiler identity") {
        format!(
            "{error} (the identity includes cargo features: a pack built by a binary with \
             other features is refused; rebuild it with this binary)"
        )
    } else {
        error
    }
}

fn print_pack(pack: &LanguagePack) {
    let manifest = pack.manifest();
    println!(
        "pack {} bytes ({:.1} MiB), format {}, {} {} v{}",
        pack.bytes().len(),
        mib(pack.bytes().len()),
        manifest.format,
        manifest.language,
        manifest.recipe.name,
        manifest.recipe.version
    );
    for part in &manifest.parts {
        println!(
            "  part {}: key {} output {}",
            part.name, part.input_digest, part.output_digest
        );
    }
    for section in &manifest.sections {
        println!(
            "  section {} ({}): {} bytes ({:.1} MiB)",
            section.name,
            section.kind,
            section.digest.bytes,
            mib(section.digest.bytes)
        );
    }
    println!(
        "  {} compiler components, {} pinned inputs, {} required hooks",
        manifest.compiler.len(),
        manifest.inputs.len(),
        manifest.requires_hooks.len()
    );
}

fn build(
    root: &Path,
    out: &Path,
    previous: Option<&Path>,
    library: bool,
    python: Option<&Path>,
    wheel: bool,
) -> Result<(), String> {
    // A release gets a new output location, preventing mixed old/new artifacts.
    if python.is_some_and(Path::exists) {
        return Err("Python output already exists; choose a fresh release directory".into());
    }
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create output directory: {e}"))?;
    }
    if python.is_some() {
        println!(
            "Python schema preflight: {} classes",
            intersect_kerml::sdk::check_python_schema()?
        );
    }
    let definitions = read_definitions(root)?;
    let previous = match previous {
        // A missing previous pack is the first build, not a failure.
        Some(path) if !path.exists() => {
            println!("previous {} does not exist; building", path.display());
            None
        }
        Some(path) => match LanguagePack::read(read_pack(path)?) {
            Ok(pack) => Some(pack),
            Err(error) => {
                println!(
                    "previous {} is not a readable pack ({error}); rebuilding",
                    path.display()
                );
                None
            }
        },
        None => None,
    };
    let preparation = preparation();
    let reused = Cell::new(false);
    let clock = Instant::now();
    let observe = |marker: &'static str| {
        if marker == "pack_reused" {
            reused.set(true);
        }
        println!("  {:>10.1} ms  {marker}", ms(clock));
    };
    let bytes = language_pack::build_language_pack(
        &definitions,
        previous.as_ref(),
        &preparation,
        sysml_scopes::compile_requests,
        observe,
    )?;
    let total = ms(clock);
    if reused.get() {
        println!("previous pack reused unchanged ({total:.0} ms)");
    } else {
        println!("pack built and self-checked ({total:.0} ms)");
    }
    if let Some(previous) = previous.as_ref() {
        // The markers above name each part reused or rebuilt; the library
        // explains why with the same checks, and the host adds how the
        // definitions under ROOT differ from the pins and from the pack.
        let clock = Instant::now();
        for (part, decision, refusal) in language_pack::explain_previous(previous)? {
            match (decision, refusal) {
                (PartDecision::Reuse, _) => println!("part {part}: reused"),
                (PartDecision::Rebuild(reason), None) => {
                    println!("part {part}: rebuilt ({reason:?})")
                }
                (PartDecision::Rebuild(reason), Some(error)) => {
                    println!("part {part}: rebuilt ({reason:?}: {})", load_error(error))
                }
            }
        }
        for reason in input_mismatches(&definitions, previous) {
            println!("previous pack: {reason}");
        }
        println!("  (part decisions explained in {:.0} ms)", ms(clock));
    }
    let bytes = if library {
        add_library(root, bytes, previous.as_ref())?
    } else {
        bytes
    };
    let clock = Instant::now();
    if python.is_some() {
        let checkpoint = out.with_extension("build-cache.pack");
        write_atomic(&checkpoint, &bytes)?;
        println!("recoverable pack checkpoint: {}", checkpoint.display());
    }
    let package = python
        .map(|output| python_package::stage(root, output, &bytes, wheel))
        .transpose()?;
    write_atomic(out, &bytes)?;
    if let Some((stage, output)) = package {
        std::fs::rename(&stage, &output).map_err(|e| {
            format!(
                "Cannot publish Python output: {e}; staged artifacts remain at {}",
                stage.display()
            )
        })?;
        println!("Python package: {}", output.display());
    }
    println!("wrote {} ({:.0} ms)", out.display(), ms(clock));
    print_pack(&LanguagePack::read(bytes).map_err(|error| error.to_string())?);
    Ok(())
}

/// How the definitions under ROOT differ from the pins this binary builds
/// against (`language_pack::pinned_inputs`) and from the inputs the previous
/// pack recorded. Empty when every definition matches both.
fn input_mismatches(definitions: &[Source], previous: &LanguagePack) -> Vec<String> {
    let recorded = &previous.manifest().inputs;
    let mut reasons = Vec::new();
    for (role, pinned) in language_pack::pinned_inputs() {
        let Some(source) = definitions.iter().find(|source| source.uri == pinned.uri) else {
            // A role without a definition file (the probe behavior) is
            // compiled in, so only the pack's record of it is checked.
            if recorded.get(&role) != Some(&pinned) {
                reasons.push(format!(
                    "input {role}: the pack records {}, this binary pins {}",
                    describe(recorded.get(&role)),
                    describe(Some(&pinned))
                ));
            }
            continue;
        };
        let actual = SourceId::for_text(&source.uri, &source.text);
        if actual != pinned {
            reasons.push(format!(
                "input {role}: ROOT has {}, this binary pins {}",
                describe(Some(&actual)),
                describe(Some(&pinned))
            ));
        }
        if recorded.get(&role) != Some(&actual) {
            reasons.push(format!(
                "input {role}: ROOT has {}, the pack records {}",
                describe(Some(&actual)),
                describe(recorded.get(&role))
            ));
        }
    }
    reasons
}

fn describe(id: Option<&SourceId>) -> String {
    match id {
        Some(id) => format!("{} sha256 {}", id.uri, id.sha256),
        None => "nothing".into(),
    }
}

fn dump(path: &Path, out: Option<&Path>) -> Result<(), String> {
    let pack = LanguagePack::read(read_pack(path)?).map_err(|error| error.to_string())?;
    let text = pack::dump_json_pretty(&pack).map_err(|error| error.to_string())?;
    match out {
        Some(out) => {
            write_atomic(out, text.as_bytes())?;
            println!(
                "wrote {} ({} bytes, {:.1} MiB)",
                out.display(),
                text.len(),
                mib(text.len())
            );
        }
        None => println!("{text}"),
    }
    Ok(())
}

/// The library files the probes use, relative to the corpus and sorted:
/// every `.kerml` file, then every `.sysml` file.
fn library_files(root: &Path) -> Result<(Vec<String>, Vec<String>), String> {
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
    let select = |extension: &str| {
        let mut files: Vec<String> = found
            .iter()
            .filter(|path| path.extension().is_some_and(|e| e == extension))
            .filter_map(|path| path.strip_prefix(&base).ok())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        files.sort();
        files
    };
    Ok((select("kerml"), select("sysml")))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Language {
    KerML,
    SysML,
}

/// One library file to parse, with its position in the sorted file list.
struct Job {
    index: usize,
    language: Language,
    source: Source,
}

/// One parsed library file: digests of its evaluation (diagnostics,
/// references, consumed input) and of its frozen storage export sorted by id.
struct Row {
    index: usize,
    uri: String,
    language: Language,
    failure: Option<String>,
    diagnostics: usize,
    objects: usize,
    evaluation: String,
    model: String,
    parse_ms: f64,
}

impl Row {
    /// Equal outcome; the timing is not part of it.
    fn same(&self, other: &Self) -> bool {
        self.uri == other.uri
            && self.failure == other.failure
            && self.diagnostics == other.diagnostics
            && self.objects == other.objects
            && self.evaluation == other.evaluation
            && self.model == other.model
    }
}

struct Parsed {
    rows: Vec<Row>,
    /// Each worker's binding time (both languages and metamodels).
    bind_ms: Vec<f64>,
    wall_ms: f64,
}

fn digest<T: serde::Serialize>(value: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    Ok(FileDigest::of(&bytes).sha256)
}

/// Parse one source into its own `FrozenStore` over the definition's frozen
/// metamodel (redefinitions computed from its own converted structure).
fn parse_one<C: DraftCapability>(
    language: &BoundLanguage<'_, '_, C>,
    metamodel: &Arc<Metamodel>,
    job: &Job,
) -> Result<Row, String> {
    let source = &job.source;
    let mut store = FrozenStore::new(metamodel.clone());
    let clock = Instant::now();
    let result = language.parse(source, &mut store);
    let parse_ms = ms(clock);
    Ok(match result {
        Ok((evaluation, references, consumed)) => {
            let mut model = storage::export(&store).map_err(|error| error.to_string())?;
            model.elements.sort_by(|a, b| a.id.cmp(&b.id));
            Row {
                index: job.index,
                uri: source.uri.clone(),
                language: job.language,
                failure: None,
                diagnostics: evaluation.diagnostics.len(),
                objects: model.elements.len(),
                evaluation: digest(&json!({
                    "evaluation": evaluation,
                    "references": references,
                    "consumed": consumed,
                }))?,
                model: digest(&model)?,
                parse_ms,
            }
        }
        Err(failure) => Row {
            index: job.index,
            uri: source.uri.clone(),
            language: job.language,
            evaluation: digest(&failure)?,
            failure: Some(failure),
            diagnostics: 0,
            objects: 0,
            model: String::new(),
            parse_ms,
        },
    })
}

/// Bind both languages of `session` (`support/pilot_languages.rs`) and their
/// frozen metamodels, then run `body` with a parser for any job and the
/// binding time in milliseconds.
fn with_parser<T>(
    session: &NativePilotSession,
    body: impl FnOnce(&dyn Fn(&Job) -> Result<Row, String>, f64) -> Result<T, String>,
) -> Result<T, String> {
    let clock = Instant::now();
    session.with_definitions(|kerml, kerml_contracts, sysml, sysml_contracts| {
        let kerml_metamodel = language_pack::frozen_metamodel(kerml)?;
        let sysml_metamodel = language_pack::frozen_metamodel(sysml)?;
        let preparation = preparation();
        pilot_languages::with_kerml(kerml, kerml_contracts, |kerml_language| {
            pilot_languages::with_sysml(sysml, sysml_contracts, &preparation, |sysml_language| {
                let bind_ms = ms(clock);
                let parse = |job: &Job| match job.language {
                    Language::KerML => parse_one(kerml_language, &kerml_metamodel, job),
                    Language::SysML => parse_one(sysml_language, &sysml_metamodel, job),
                };
                body(&parse, bind_ms)
            })
        })
    })
}

/// One parse worker: bind both languages, then take jobs until none remain.
fn parse_worker(
    session: &NativePilotSession,
    jobs: &[Job],
    next: &AtomicUsize,
) -> Result<(f64, Vec<Row>), String> {
    with_parser(session, |parse, bind_ms| {
        let mut rows = Vec::new();
        while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
            rows.push(parse(job)?);
        }
        Ok((bind_ms, rows))
    })
}

/// The single-worker probe on a session not yet used for parsing: bind both
/// languages, then parse each probe file twice, so the cold first parse
/// shows apart from the warm second one. Every probe parse must succeed.
fn probe(label: &str, session: &NativePilotSession, jobs: &[Job]) -> Result<(), String> {
    let probes = PROBES
        .iter()
        .map(|path| {
            let uri = path.replace(' ', "%20");
            jobs.iter()
                .find(|job| job.source.uri == uri)
                .ok_or_else(|| format!("Probe file {path} is not in the library"))
        })
        .collect::<Result<Vec<&Job>, String>>()?;
    with_parser(session, |parse, bind_ms| {
        println!("  {label} probe: bind both languages {bind_ms:.1} ms (one worker)");
        for job in probes {
            let first = parse(job)?;
            let second = parse(job)?;
            for row in [&first, &second] {
                if let Some(failure) = &row.failure {
                    return Err(format!("Probe parse of {} failed: {failure}", row.uri));
                }
            }
            println!(
                "  {label} probe: {} first parse {:.1} ms, second {:.1} ms",
                job.source.uri, first.parse_ms, second.parse_ms
            );
        }
        Ok(())
    })
}

/// Parse every job with `workers` threads sharing one session (largest files
/// are queued first); rows come back in the sorted file order.
fn parse_library(
    session: &NativePilotSession,
    jobs: &[Job],
    workers: usize,
) -> Result<Parsed, String> {
    let next = AtomicUsize::new(0);
    let clock = Instant::now();
    let results: Vec<Result<(f64, Vec<Row>), String>> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for worker in 0..workers {
            handles.push(
                std::thread::Builder::new()
                    .name(format!("sysml-pack-parse-{worker}"))
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, || parse_worker(session, jobs, &next))
                    .map_err(|error| format!("Cannot start a parse worker: {error}")),
            );
        }
        handles
            .into_iter()
            .map(|handle| {
                handle?
                    .join()
                    .map_err(|_| "Parse worker panicked".to_owned())?
            })
            .collect()
    });
    let wall_ms = ms(clock);
    let mut rows = Vec::new();
    let mut bind_ms = Vec::new();
    for result in results {
        let (bind, worker_rows) = result?;
        bind_ms.push(bind);
        rows.extend(worker_rows);
    }
    if rows.len() != jobs.len() {
        return Err("Parse workers did not cover every library file".into());
    }
    rows.sort_by_key(|row| row.index);
    Ok(Parsed {
        rows,
        bind_ms,
        wall_ms,
    })
}

/// The jobs for every KerML then SysML library file, queued largest first.
fn library_jobs(kerml: Vec<Source>, sysml: Vec<Source>) -> Vec<Job> {
    let mut jobs: Vec<Job> = kerml
        .into_iter()
        .map(|source| (Language::KerML, source))
        .chain(sysml.into_iter().map(|source| (Language::SysML, source)))
        .enumerate()
        .map(|(index, (language, source))| Job {
            index,
            language,
            source,
        })
        .collect();
    jobs.sort_by_key(|job| std::cmp::Reverse(job.source.text.len()));
    jobs
}

fn summarize(label: &str, parsed: &Parsed, workers: usize) {
    for language in [Language::KerML, Language::SysML] {
        let rows: Vec<&Row> = parsed
            .rows
            .iter()
            .filter(|row| row.language == language)
            .collect();
        let failed = rows.iter().filter(|row| row.failure.is_some()).count();
        let clean = rows
            .iter()
            .filter(|row| row.failure.is_none() && row.diagnostics == 0)
            .count();
        let diagnostics: usize = rows.iter().map(|row| row.diagnostics).sum();
        let objects: usize = rows.iter().map(|row| row.objects).sum();
        let parse_ms: f64 = rows.iter().map(|row| row.parse_ms).sum();
        println!(
            "  {label} {language:?}: {} files ({clean} without diagnostics, {failed} failed), \
             {diagnostics} diagnostics, {objects} objects, contended parse time summed over \
             workers {parse_ms:.0} ms",
            rows.len()
        );
    }
    let bind_min = parsed.bind_ms.iter().copied().fold(f64::INFINITY, f64::min);
    let bind_max = parsed.bind_ms.iter().copied().fold(0.0, f64::max);
    println!(
        "  {label}: parse wall {:.0} ms with {workers} workers (worker binding {bind_min:.0} \
         to {bind_max:.0} ms, included in the wall time)",
        parsed.wall_ms
    );
}

/// Every library file must parse: an equal comparison of two failed passes
/// proves nothing.
fn require_parsed(label: &str, parsed: &Parsed) -> Result<(), String> {
    if parsed.rows.is_empty() {
        return Err(format!("The {label} pass parsed no library files"));
    }
    let failed: Vec<&Row> = parsed
        .rows
        .iter()
        .filter(|row| row.failure.is_some())
        .collect();
    for row in failed.iter().take(10) {
        if let Some(failure) = &row.failure {
            println!("  {label} failed: {}: {failure}", row.uri);
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} of {} library files failed to parse with the {label} session",
            failed.len(),
            parsed.rows.len()
        ))
    }
}

fn compare(loaded: &Parsed, fresh: &Parsed) -> Result<(), String> {
    if loaded.rows.len() != fresh.rows.len() {
        return Err("Parse comparison covered different file counts".into());
    }
    let mut differing = Vec::new();
    for (loaded, fresh) in loaded.rows.iter().zip(&fresh.rows) {
        if !loaded.same(fresh) {
            let what = match (
                loaded.evaluation == fresh.evaluation,
                loaded.model == fresh.model,
            ) {
                (false, false) => "evaluation and export",
                (false, true) => "evaluation",
                (true, false) => "export",
                (true, true) => "summary",
            };
            differing.push(format!("{} ({what})", fresh.uri));
        }
    }
    if differing.is_empty() {
        println!(
            "parse comparison: all {} files identical (evaluation and sorted export)",
            fresh.rows.len()
        );
        Ok(())
    } else {
        for file in &differing {
            println!("  differs: {file}");
        }
        Err(format!(
            "{} of {} library files parse differently after loading the pack",
            differing.len(),
            fresh.rows.len()
        ))
    }
}

/// Both binding parts compiled afresh on host threads, every job of both
/// recipes at once, as `build` does.
type FreshBindings = (RcoreBindingsPart, SysmlBindingsPart);

fn compile_bindings() -> Result<FreshBindings, String> {
    let requests = [
        BindingCompile::new(BindingRecipe::KerML)?,
        BindingCompile::new(BindingRecipe::SysML)?,
    ];
    match <[_; 2]>::try_from(sysml_scopes::compile_requests(&requests)?) {
        Ok([kerml, sysml]) => Ok((
            RcoreBindingsPart::new(&kerml)?,
            SysmlBindingsPart::new(&sysml)?,
        )),
        Err(results) => Err(format!("{} binding results for 2 requests", results.len())),
    }
}

/// Load both binding parts of the pack, compile them afresh, and require,
/// per part, the loaded, fresh and stored bindings to save to identical
/// text. Then require the host-side SysML binding cache to hold the pack's
/// SysML bindings. Returns the fresh parts.
fn check_bindings(
    pack: &LanguagePack,
    session: &NativePilotSession,
) -> Result<FreshBindings, String> {
    let clock = Instant::now();
    let loaded = language_pack::load_pack_bindings(pack).map_err(load_error)?;
    let load_ms = ms(clock);
    let clock = Instant::now();
    let (sysml_bundle, loaded_sysml) =
        language_pack::load_pack_sysml_bindings(pack).map_err(load_error)?;
    let sysml_load_ms = ms(clock);
    let stored: RcoreBindingsPart = pack
        .section_json(language_pack::BINDINGS_SECTION)
        .map_err(|error| error.to_string())?;
    let stored_sysml: SysmlBindingsPart = pack
        .section_json(language_pack::SYSML_BINDINGS_SECTION)
        .map_err(|error| error.to_string())?;
    let loaded_sysml_count = loaded_sysml.len();
    let loaded = RcoreBindingsPart::new(&loaded)?;
    let loaded_sysml_part = SysmlBindingsPart::new(&loaded_sysml)?;
    let clock = Instant::now();
    let (fresh, fresh_sysml) = compile_bindings()?;
    let compile_ms = ms(clock);
    println!(
        "load_pack_bindings {load_ms:.1} ms ({:.1} MiB saved text); load_pack_sysml_bindings \
         {sysml_load_ms:.1} ms ({loaded_sysml_count} bindings, {:.1} MiB saved text); fresh \
         compile of both on host threads {compile_ms:.1} ms",
        mib(stored.saved().len()),
        mib(stored_sysml.saved().len())
    );
    if loaded != stored || loaded_sysml_part != stored_sysml {
        return Err("Loaded bindings do not save to the stored bindings text".into());
    }
    if fresh != stored || fresh_sysml != stored_sysml {
        return Err("Freshly compiled bindings differ from the pack's stored bindings".into());
    }
    if sysml_bundle != *stored_sysml.bundle() {
        return Err("load_pack_sysml_bindings returned another bundle than the stored one".into());
    }
    println!(
        "bindings comparison: loaded, fresh and stored bindings save to identical text \
         (both parts)"
    );
    // The hosts without a pack keep the same bindings in their own cache.
    let metamodel =
        session.with_definitions(|_, _, sysml, _| language_pack::frozen_metamodel(sysml))?;
    let clock = Instant::now();
    let (cache_bundle, cached, cache_used) = sysml_scopes::compile_cached(&metamodel)?;
    println!(
        "host binding cache: {} {} bindings in {:.1} ms",
        if cache_used { "loaded" } else { "compiled" },
        cached.len(),
        ms(clock)
    );
    if cache_bundle != sysml_bundle
        || cached.len() != loaded_sysml_count
        || sysml_bindings::save(&cached)? != stored_sysml.saved()
    {
        return Err("The host binding cache differs from the pack's rcore/sysml-bindings".into());
    }
    println!(
        "host binding cache comparison: same bundle, {loaded_sysml_count} bindings and saved \
         text as the pack's rcore/sysml-bindings"
    );
    Ok((fresh, fresh_sysml))
}

fn check(root: &Path, path: &Path, mode: Mode, workers: usize) -> Result<(), String> {
    let definitions = read_definitions(root)?;
    let (kerml_files, sysml_files) = library_files(root)?;
    let read = |files: &[String]| -> Result<Vec<Source>, String> {
        files.iter().map(|file| read_source(root, file)).collect()
    };
    let jobs = library_jobs(read(&kerml_files)?, read(&sysml_files)?);
    println!(
        "check {} --mode {mode:?}: {} .kerml and {} .sysml library files, {workers} parse workers",
        path.display(),
        kerml_files.len(),
        sysml_files.len()
    );

    let bytes = read_pack(path)?;
    let size = bytes.len();
    let clock = Instant::now();
    let pack = LanguagePack::read(bytes).map_err(|error| error.to_string())?;
    let read_ms = ms(clock);
    let load_mode = match mode {
        Mode::Revalidate => RuntimeAdmissionLoadMode::Revalidate,
        Mode::HostCache | Mode::Recompile => RuntimeAdmissionLoadMode::HostCache,
    };
    let clock = Instant::now();
    let loaded = NativePilotSession::from_pack(&pack, load_mode).map_err(load_error)?;
    let load_ms = ms(clock);
    println!(
        "LanguagePack::read (in memory) {read_ms:.1} ms ({:.1} MiB); \
         from_pack({load_mode:?}) {load_ms:.1} ms",
        mib(size)
    );
    let fresh_bindings = check_bindings(&pack, &loaded)?;
    probe("loaded", &loaded, &jobs)?;
    let loaded_parse = parse_library(&loaded, &jobs, workers)?;
    drop(loaded);

    let preparation = preparation();
    let clock = Instant::now();
    let fresh = NativePilotSession::prepare(&definitions, &preparation)?;
    let prepare_ms = ms(clock);
    probe("fresh", &fresh, &jobs)?;
    let fresh_parse = parse_library(&fresh, &jobs, workers)?;
    let restore_ms = read_ms + load_ms;
    println!(
        "fresh prepare {prepare_ms:.1} ms; in-memory read + from_pack {restore_ms:.1} ms; \
         speed-up {:.1}x",
        prepare_ms / restore_ms.max(f64::MIN_POSITIVE)
    );
    summarize("loaded", &loaded_parse, workers);
    summarize("fresh", &fresh_parse, workers);
    compare(&loaded_parse, &fresh_parse)?;
    require_parsed("loaded", &loaded_parse)?;
    require_parsed("fresh", &fresh_parse)?;

    if mode == Mode::Recompile {
        let clock = Instant::now();
        let first = language_pack::pack_session(&fresh, &fresh_bindings.0, &fresh_bindings.1)?;
        let encode_ms = ms(clock);
        drop(fresh);
        drop(fresh_bindings);
        let clock = Instant::now();
        let second_session = NativePilotSession::prepare(&definitions, &preparation)?;
        let second_bindings = compile_bindings()?;
        let second_ms = ms(clock);
        let second =
            language_pack::pack_session(&second_session, &second_bindings.0, &second_bindings.1)?;
        drop(second_session);
        println!(
            "recompile: pack_session {encode_ms:.1} ms; second independent prepare and binding \
             compile {second_ms:.1} ms"
        );
        // `pack_session` does not encode a library part: compare against
        // the pack without it, then rebuild the library itself.
        let file = language_pack::without_library_part(&pack)?;
        if first != second {
            return Err("Two independent preparations packed to different bytes".into());
        }
        if first != file {
            return Err(format!(
                "Recompiled pack ({} bytes) differs from {} ({} bytes without a library part)",
                first.len(),
                path.display(),
                file.len()
            ));
        }
        println!(
            "recompile: both preparations pack to identical bytes, equal to the pack file ({} bytes)",
            first.len()
        );
        if language_pack::pack_library_payload(&pack).is_some() {
            drop(pack);
            check_library(root, path, &[])?;
        }
    }
    println!("check passed");
    Ok(())
}

/// Add the prepared standard library to freshly built pack `bytes`: keep the
/// previous pack's part when `plan_library` reuses it, else parse the
/// library under ROOT and snapshot it.
fn add_library(
    root: &Path,
    bytes: Vec<u8>,
    previous: Option<&LanguagePack>,
) -> Result<Vec<u8>, String> {
    let pack = LanguagePack::read(bytes).map_err(|error| error.to_string())?;
    let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)
        .map_err(load_error)?;
    let sources = sysml_library::library_sources(root)?;
    let preparation = preparation();
    sysml_library::with_library_host_from_pack(&session, &pack, &preparation, |host| {
        let manifest = LibraryManifest::of_sources(&sources).map_err(|e| e.to_string())?;
        let key = language_pack::library_key(&manifest, host.context())?;
        let (decision, reused, refusal) =
            language_pack::plan_library(previous, &key, host.context());
        let name = language_pack::LIBRARY_SECTION;
        match (&decision, &refusal) {
            (PartDecision::Reuse, _) => println!("part {name}: reused"),
            (PartDecision::Rebuild(reason), None) => println!("part {name}: rebuilt ({reason:?})"),
            (PartDecision::Rebuild(reason), Some(error)) => {
                println!("part {name}: rebuilt ({reason:?}: {error})")
            }
        }
        let prepared = language_pack::prepare_library(
            reused,
            Some(&sources),
            host.context(),
            language_pack::LibraryOptions::default(),
            |sources| {
                let clock = Instant::now();
                let (model, load, times) = host.parse_library(sources)?;
                println!(
                    "library parsed, linked, completed and validated in {:.0} ms ({} objects, {} of {} references; parse {:.0} + {:.0} ms, link {:.0} ms)",
                    ms(clock),
                    model.len(),
                    load.resolved,
                    load.references,
                    times.parse_kerml_ms,
                    times.parse_sysml_ms,
                    times.link_ms
                );
                Ok(model)
            },
        )?;
        let payload = match prepared.payload {
            Some(payload) => payload,
            None => reused
                .ok_or("Library preparation returned no reusable payload")?
                .to_vec(),
        };
        println!(
            "library package {}: {} objects, payload {:.1} MiB",
            prepared.identity.content_digest,
            prepared.model.len(),
            mib(payload.len())
        );
        let bytes = language_pack::with_library_part(&pack, payload)?;
        #[cfg(feature = "components")]
        {
            use intersect_model::components::SharedSchema;
            let pack = LanguagePack::read(bytes).map_err(|e| e.to_string())?;
            let context = SharedSchema::from_metamodel(host.context().metamodel.clone())?;
            let reused_component = previous.and_then(|previous| {
                language_pack::pack_standard_component_payload(previous, &prepared.key)
                    .ok()
                    .flatten()
                    .filter(|payload| {
                        language_pack::load_standard_component_cache(
                            context.clone(),
                            payload,
                            &prepared.key,
                            SnapshotTrust::HostCache,
                        )
                        .is_ok()
                    })
            });
            let payload = match reused_component {
                Some(payload) => {
                    println!("standard component: reused build-time admission cache");
                    payload.to_vec()
                }
                None => {
                    println!("standard component: preparing build-time admission cache");
                    language_pack::prepare_standard_component_cache(
                        context.clone(),
                        &prepared.key,
                        language_pack::pack_library_payload(&pack)
                            .ok_or("Missing selected library payload")??,
                    )?
                }
            };
            // Independent checked restore is an offline qualification of the
            // newly encoded cache; browser loading trusts this host-built proof.
            let checked = language_pack::load_standard_component_cache(
                context,
                &payload,
                &prepared.key,
                if reused_component.is_some() {
                    SnapshotTrust::HostCache
                } else {
                    SnapshotTrust::Revalidate
                },
            )?;
            if checked.object_count() != prepared.model.len() {
                return Err(
                    "Prepared component object count differs from the standard library".into(),
                );
            }
            println!(
                "standard component cache checked: {} objects",
                checked.object_count()
            );
            language_pack::with_standard_component_part(&pack, payload)
        }
        #[cfg(not(feature = "components"))]
        {
            Ok(bytes)
        }
    })
}

/// Expect `result` to be refused, with a message; print it.
fn refused<T>(label: &str, result: Result<T, String>) -> Result<(), String> {
    match result {
        Ok(_) => Err(format!("library check: {label} was accepted")),
        Err(message) => {
            println!("  refused {label}: {message}");
            Ok(())
        }
    }
}

/// The decision D10 gates on a pack's library part (see the module docs).
fn check_library(root: &Path, path: &Path, user: &[String]) -> Result<(), String> {
    let bytes = read_pack(path)?;
    let clock = Instant::now();
    let pack = LanguagePack::read(bytes).map_err(|error| error.to_string())?;
    let read_ms = ms(clock);
    let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)
        .map_err(load_error)?;
    let user = user
        .iter()
        .map(|file| sysml_library::read_source(root, file))
        .collect::<Result<Vec<_>, _>>()?;
    let preparation = preparation();
    sysml_library::with_library_host_from_pack(&session, &pack, &preparation, |host| {
        let context = host.context();
        let payload = language_pack::pack_library_payload(&pack)
            .ok_or_else(|| format!("{} has no library part", path.display()))??;
        let part = decode_library_part(payload).map_err(|e| e.to_string())?;
        println!(
            "check-library {}: pack read {read_ms:.1} ms; payload {:.1} MiB ({} sources, \
             snapshot {:.1} MiB)",
            path.display(),
            mib(payload.len()),
            part.manifest.entries.len(),
            mib(part.snapshot.len())
        );
        let on_disk = LibraryManifest::of_sources(&sysml_library::library_sources(root)?)
            .map_err(|e| e.to_string())?;
        if on_disk != part.manifest {
            println!("  note: the library under ROOT differs from the pack's library sources");
        }

        // Load the snapshot (median of 5), and once more revalidated.
        let mut loads = Vec::new();
        let mut loaded = None;
        for _ in 0..5 {
            let clock = Instant::now();
            let (model, _) = language_pack::load_library_snapshot(
                payload,
                context,
                None,
                SnapshotTrust::HostCache,
            )?;
            loads.push(ms(clock));
            loaded = Some(model);
        }
        let loaded = loaded.ok_or("no snapshot load")?;
        loads.sort_by(f64::total_cmp);
        let clock = Instant::now();
        language_pack::load_library_snapshot(payload, context, None, SnapshotTrust::Revalidate)?;
        println!(
            "  snapshot load: median {:.1} ms of 5 ({:?}); revalidated load {:.1} ms; {} objects",
            loads.get(2).copied().unwrap_or(f64::NAN),
            loads
                .iter()
                .map(|t| (t * 10.0).round() / 10.0)
                .collect::<Vec<_>>(),
            ms(clock),
            loaded.len()
        );

        // Fail closed: corrupt, truncated, wrongly keyed, wrong metamodel.
        let load = |bytes: &[u8], context| {
            language_pack::load_library_snapshot(bytes, context, None, SnapshotTrust::HostCache)
                .map(|_| ())
        };
        let snapshot_start = payload.len() - part.snapshot.len();
        let mut flipped = payload.to_vec();
        flipped[snapshot_start + part.snapshot.len() / 2] ^= 0x10;
        refused("a flipped snapshot byte", load(&flipped, context))?;
        let mut flipped = payload.to_vec();
        // The last byte of the last source text, before the snapshot length.
        flipped[snapshot_start - 8 - 1] ^= 0x01;
        refused("a flipped source byte", load(&flipped, context))?;
        refused(
            "a truncated payload",
            load(&payload[..payload.len() - 1], context),
        )?;
        let other = format!("{}+other", host.bindings);
        refused(
            "other bindings (a different key)",
            load(
                payload,
                language_pack::LibraryContext {
                    metamodel: context.metamodel,
                    bindings: &other,
                },
            ),
        )?;
        let unredefined = session.with_definitions(|kerml, _, _, _| {
            Metamodel::freeze(kerml.schema())
                .map(Arc::new)
                .map_err(|e| e.to_string())
        })?;
        refused(
            "another metamodel (no redefinitions)",
            load(
                payload,
                language_pack::LibraryContext {
                    metamodel: &unredefined,
                    bindings: context.bindings,
                },
            ),
        )?;

        // Recompile: parse the part's sources afresh.
        let sources = part.sources();
        let clock = Instant::now();
        let (parsed, load, times) = host.parse_library(&sources)?;
        let identity = language_pack::library_package_identity(&part.manifest, context)?;
        let parsed = parsed
            .with_package_identity(&identity)
            .map_err(|e| e.to_string())?;
        let parse_ms = ms(clock);
        let key = language_pack::library_key(&part.manifest, context)?;
        let clock = Instant::now();
        let snapshot = parsed
            .to_snapshot(&key_bytes(&key).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let encode_ms = ms(clock);
        let fresh = encode_library_part(&sources, &snapshot).map_err(|e| e.to_string())?;
        println!(
            "  fresh parse, link, complete and validate {parse_ms:.0} ms (parse {:.0} + {:.0} ms, \
             link {:.0} ms; {} of {} references); snapshot encode {encode_ms:.1} ms",
            times.parse_kerml_ms,
            times.parse_sysml_ms,
            times.link_ms,
            load.resolved,
            load.references
        );
        if fresh != payload {
            return Err(
                "A fresh parse of the library snapshots to other bytes than the pack's part".into(),
            );
        }
        println!(
            "  determinism: the fresh payload is byte-identical to the pack's ({} bytes)",
            fresh.len()
        );
        let clock = Instant::now();
        let from_snapshot = loaded.to_model().map_err(|e| e.to_string())?;
        let from_parse = parsed.to_model().map_err(|e| e.to_string())?;
        if from_snapshot != from_parse {
            return Err("The loaded snapshot exports differently from the parsed library".into());
        }
        println!(
            "  equivalence: the loaded snapshot and the parsed library export identically \
             ({} elements, {} resources; compared in {:.0} ms)",
            from_parse.elements.len(),
            from_parse.resources.len(),
            ms(clock)
        );
        drop(from_snapshot);
        drop(from_parse);

        for source in &user {
            let files = std::slice::from_ref(source);
            let (on_snapshot, _) = host.load(FrozenStore::from_model(loaded.clone()), files)?;
            let (on_parse, _) = host.load(FrozenStore::from_model(parsed.clone()), files)?;
            let export = |load: &intersect_kerml::frozen_load::FrozenLoad| {
                load.model
                    .as_ref()
                    .ok_or_else(|| {
                        format!(
                            "{}: did not load: {:?}",
                            source.uri,
                            load.diagnostics.first().map(|d| d.message.clone())
                        )
                    })?
                    .to_model()
                    .map_err(|e| e.to_string())
            };
            if !on_snapshot.diagnostics.is_empty() || on_snapshot.resolved != on_snapshot.references
            {
                return Err(format!(
                    "{}: {} of {} references resolved on the snapshot, {} problems, first {:?}",
                    source.uri,
                    on_snapshot.resolved,
                    on_snapshot.references,
                    on_snapshot.diagnostics.len(),
                    on_snapshot.diagnostics.first().map(|d| d.message.clone())
                ));
            }
            if export(&on_snapshot)? != export(&on_parse)?
                || on_snapshot.references != on_parse.references
            {
                return Err(format!(
                    "{}: loads differently on the snapshot than on the parsed library",
                    source.uri
                ));
            }
            println!(
                "  user file {}: {} of {} references resolved; same export on the snapshot and \
                 on the parsed library",
                source.uri, on_snapshot.resolved, on_snapshot.references
            );
        }
        println!("check-library passed");
        Ok(())
    })
}
