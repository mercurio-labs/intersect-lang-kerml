//! Handwritten host (not generated): argument parsing, file I/O, the pack and
//! library cache, diagnostics output and export files of the `intersect`
//! command.
use intersect_kerml::{
    frozen_load::{LoadDiagnostic, LoadPhase, prediction_memo_mode_from_env},
    language_pack::{
        self, KERML_SOURCE, LIBRARY_SECTION, LibraryOptions, LibraryOrigin, SYSML_SOURCE,
        pack_library_payload, prepare_library, with_library_part,
    },
    native_pilot::NativePilotSession,
    sysml_host::{self, ExportFormat, SysmlBindings, phase_name, with_sysml_host},
};
use intersect_kerml_eval::{EvalMode, SourceMap, eval_source};
use intersect_model::{
    core::frozen::{Model, SnapshotTrust},
    definitions::{
        conversion::{InvocationContext, InvocationObserver, ProviderId},
        library_part::LibrarySource,
        pack::LanguagePack,
    },
};
use intersect_text::Source;
use intersect_text::grammar::RuntimeAdmissionLoadMode;

#[path = "host_analysis.rs"]
mod analysis;
#[path = "host_kpar.rs"]
mod archives;
#[path = "host_eval.rs"]
mod evaluation;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

const USAGE: &str = "usage:
  intersect check  --pack PACK [OPTIONS] FILE...
  intersect export --pack PACK --format json|xmi --out DIR [OPTIONS] FILE...
  intersect export --pack PACK --format kpar --out DIR --project-name NAME --project-version VERSION [OPTIONS] FILE...
  intersect info   --pack PACK
  intersect eval --pack PACK [--target QNAME] [--mode model-level|extended] [--gated] [--json] [--trace] [--explain] (--expr TEXT | --all) [FILE...]
  intersect analyze --pack PACK --execution-profile LANGUAGE.toml [--profile NAME] --case QNAME [--inputs INPUTS.json] [--events EVENTS.json] [OPTIONS] FILE...
FILE is a .kerml or .sysml file or a project archive (.kpar).
options:
  --library DIR|KPAR  standard library sources (when the pack has no usable snapshot): a
                      folder, or the library archives (repeat --library for each)
  --project-path DIR  folder of .kpar files that archive inputs' usage resolves against
                      (repeatable)
  --read-only         never write the library snapshot into the pack
  --no-library-cache  parse the library even when the pack holds a snapshot
  --json              diagnostics and analysis results as JSON lines on stdout
  --timing            phase times on stderr";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Check,
    Export,
    Info,
    Eval,
    Analyze,
}

struct Options {
    command: Command,
    pack: PathBuf,
    library: Vec<PathBuf>,
    project_path: Vec<PathBuf>,
    project_name: Option<String>,
    project_version: Option<String>,
    read_only: bool,
    no_cache: bool,
    json: bool,
    timing: bool,
    format: Option<OutputFormat>,
    out: Option<PathBuf>,
    files: Vec<PathBuf>,
    expression: Option<String>,
    all: bool,
    target: Option<String>,
    mode: EvalMode,
    gated: bool,
    trace: bool,
    explain: bool,
    analysis: Option<analysis::Arguments>,
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut args = args.iter();
    let command = match args.next().map(String::as_str) {
        Some("check") => Command::Check,
        Some("export") => Command::Export,
        Some("info") => Command::Info,
        Some("eval") => Command::Eval,
        Some("analyze") => Command::Analyze,
        Some("--help" | "-h" | "help") | None => return Err(USAGE.into()),
        Some(other) => return Err(format!("unknown command '{other}'\n{USAGE}")),
    };
    let mut options = Options {
        command,
        pack: PathBuf::new(),
        library: Vec::new(),
        project_path: Vec::new(),
        project_name: None,
        project_version: None,
        read_only: false,
        no_cache: false,
        json: false,
        timing: false,
        format: None,
        out: None,
        files: Vec::new(),
        expression: None,
        all: false,
        target: None,
        mode: EvalMode::Extended,
        gated: false,
        trace: false,
        explain: false,
        analysis: (command == Command::Analyze).then(analysis::Arguments::default),
    };
    let mut pack = None;
    let mut eval_option = false;
    while let Some(arg) = args.next() {
        eval_option |= matches!(
            arg.as_str(),
            "--expr" | "--all" | "--target" | "--mode" | "--gated" | "--trace" | "--explain"
        );
        let mut value = |name: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--execution-profile" | "--profile" | "--case" | "--inputs" | "--events" => {
                let analysis = options
                    .analysis
                    .as_mut()
                    .ok_or_else(|| format!("{arg} requires the analyze command"))?;
                analysis.set(arg, value(arg)?)?;
            }
            "--expr" => {
                if options.expression.is_some() {
                    return Err("--expr may appear only once".into());
                }
                options.expression = Some(value("--expr")?);
            }
            "--all" => options.all = true,
            "--target" => options.target = Some(value("--target")?),
            "--mode" => {
                options.mode = match value("--mode")?.as_str() {
                    "model-level" => EvalMode::ModelLevel,
                    "extended" => EvalMode::Extended,
                    other => {
                        return Err(format!(
                            "unknown evaluation mode '{other}' (model-level or extended)"
                        ));
                    }
                }
            }
            "--gated" => options.gated = true,
            "--trace" => options.trace = true,
            "--explain" => options.explain = true,
            "--pack" => pack = Some(PathBuf::from(value("--pack")?)),
            "--library" => options.library.push(value("--library")?.into()),
            "--project-path" => options.project_path.push(value("--project-path")?.into()),
            "--project-name" => options.project_name = Some(value("--project-name")?),
            "--project-version" => options.project_version = Some(value("--project-version")?),
            "--out" | "-o" => options.out = Some(value("--out")?.into()),
            "--format" => {
                options.format = Some(match value("--format")?.as_str() {
                    "json" => OutputFormat::Model(ExportFormat::Json),
                    "xmi" => OutputFormat::Model(ExportFormat::Xmi),
                    "kpar" => OutputFormat::Kpar,
                    other => {
                        return Err(format!("unknown format '{other}' (json, xmi or kpar)"));
                    }
                })
            }
            "--read-only" => options.read_only = true,
            "--no-library-cache" => options.no_cache = true,
            "--json" => options.json = true,
            "--timing" => options.timing = true,
            flag if flag.starts_with("--") => {
                return Err(format!("unknown option '{flag}'\n{USAGE}"));
            }
            file => options.files.push(file.into()),
        }
    }
    options.pack = pack.ok_or_else(|| format!("--pack is required\n{USAGE}"))?;
    if command != Command::Eval && eval_option {
        return Err("evaluation options require the eval command".into());
    }
    if command == Command::Eval {
        if options.all == options.expression.is_some() {
            return Err("eval requires exactly one of --expr TEXT or --all".into());
        }
        if options.all && options.files.is_empty() {
            return Err("eval --all requires input files".into());
        }
        if options.format.is_some() || options.out.is_some() {
            return Err("eval does not take --format or --out".into());
        }
        // Validate imports before reading any files or pack.
        eval_source(options.target.as_deref(), "", 0).map_err(|e| e.to_string())?;
    }
    if let Some(analysis) = &options.analysis {
        analysis.validate()?;
        if options.format.is_some() || options.out.is_some() {
            return Err("analyze does not take --format or --out".into());
        }
    }
    if (options.project_name.is_some() || options.project_version.is_some())
        && options.format != Some(OutputFormat::Kpar)
    {
        return Err("--project-name and --project-version need export --format kpar".into());
    }
    let library_archives = options
        .library
        .iter()
        .filter(|path| archives::is_archive(path))
        .count();
    if library_archives != options.library.len() && options.library.len() > 1 {
        return Err("--library takes one folder or one or more .kpar archives".into());
    }
    match command {
        Command::Info if !options.files.is_empty() => {
            return Err("info takes no files".into());
        }
        Command::Check | Command::Export | Command::Analyze if options.files.is_empty() => {
            return Err(format!("no input files\n{USAGE}"));
        }
        Command::Export if options.format.is_none() || options.out.is_none() => {
            return Err(format!("export needs --format and --out\n{USAGE}"));
        }
        Command::Export
            if options.format == Some(OutputFormat::Kpar)
                && (options.project_name.is_none() || options.project_version.is_none()) =>
        {
            return Err("export --format kpar needs --project-name and --project-version".into());
        }
        _ => {}
    }
    Ok(options)
}

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

/// What `export` writes: a model format of `sysml_host::export`, or a
/// project archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Model(ExportFormat),
    Kpar,
}

/// Where a user file comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// A `.kerml`/`.sysml` file given on the command line.
    Loose,
    /// A file of a `.kpar` given on the command line.
    Root,
    /// A file of an archive a `.kpar` input uses (its `usage`).
    Dependency,
}

/// One user file: its path as given (`archive.kpar!/entry` for an archive
/// file) and the logical resource URI it loads under (forward slashes;
/// spaces, which resource URIs may not contain, as `%20`).
struct UserFile {
    path: PathBuf,
    source: Source,
    source_map: Option<SourceMap>,
    origin: Origin,
    /// The entry path inside its archive.
    entry: Option<String>,
}

fn resource_uri(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .replace('%', "%25")
        .replace('#', "%23")
        .replace('?', "%3F")
        .replace(' ', "%20")
}

fn read_user_files(paths: &[PathBuf]) -> Result<Vec<UserFile>, String> {
    let mut uris = BTreeMap::new();
    paths
        .iter()
        .map(|path| {
            let language = path.extension().and_then(|e| e.to_str());
            if !matches!(language, Some("kerml" | "sysml")) {
                return Err(format!("{}: not a .kerml or .sysml file", path.display()));
            }
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let uri = resource_uri(path);
            if let Some(previous) = uris.insert(uri.clone(), path.clone()) {
                return Err(format!(
                    "{} and {} load as the same resource {uri}",
                    previous.display(),
                    path.display()
                ));
            }
            Ok(UserFile {
                path: path.clone(),
                source: Source { uri, text },
                source_map: None,
                origin: Origin::Loose,
                entry: None,
            })
        })
        .collect()
}

/// Every `.kerml` and `.sysml` file under `dir`, with URIs relative to its
/// parent (so `.../sysml.library` gives `sysml.library/...`, the URIs the
/// pack's library part is keyed on).
fn library_sources(dir: &Path) -> Result<Vec<LibrarySource>, String> {
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
    let base = dir.parent().unwrap_or(Path::new(""));
    let mut found = Vec::new();
    walk(dir, &mut found)?;
    let mut sources = Vec::new();
    for path in found {
        let language = match path.extension().and_then(|e| e.to_str()) {
            Some("kerml") => KERML_SOURCE,
            Some("sysml") => SYSML_SOURCE,
            _ => continue,
        };
        let relative = path.strip_prefix(base).unwrap_or(&path);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        sources.push(LibrarySource {
            uri: resource_uri(relative),
            language: language.into(),
            text,
        });
    }
    if sources.is_empty() {
        return Err(format!("{}: no .kerml or .sysml files", dir.display()));
    }
    sources.sort_by(|a, b| a.uri.cmp(&b.uri));
    Ok(sources)
}

/// Write a temporary next to `path`, then rename it into place.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut name = path
        .file_name()
        .ok_or_else(|| format!("{}: not a file path", path.display()))?
        .to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    let temporary = path.with_file_name(name);
    let written = std::fs::write(&temporary, bytes)
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

struct Timer {
    enabled: bool,
    clock: Instant,
}
impl Timer {
    fn lap(&mut self, what: &str) {
        if self.enabled {
            eprintln!("{what}: {:.0} ms", self.clock.elapsed().as_secs_f64() * 1e3);
        }
        self.clock = Instant::now();
    }
}

/// Run one command; `Ok(false)` when the input has diagnostics.
pub fn run(args: &[String]) -> Result<bool, String> {
    let options = options(args)?;
    let analysis = options.analysis.as_ref().map(analysis::load).transpose()?;
    let mut timer = Timer {
        enabled: options.timing,
        clock: Instant::now(),
    };
    let bytes = std::fs::read(&options.pack)
        .map_err(|error| format!("{}: {error}", options.pack.display()))?;
    let pack = LanguagePack::read(bytes)
        .map_err(|error| format!("{}: {error}", options.pack.display()))?;
    timer.lap("read pack");
    if options.command == Command::Info {
        return info(&options, &pack);
    }
    let (inputs, loose): (Vec<PathBuf>, Vec<PathBuf>) = options
        .files
        .iter()
        .cloned()
        .partition(|path| archives::is_archive(path));
    let (mut user, mut archives) = archives::read_archive_inputs(&inputs, &options.project_path)?;
    user.extend(read_user_files(&loose)?);
    let mut uris = BTreeMap::new();
    for file in &user {
        if let Some(previous) = uris.insert(file.source.uri.as_str(), &file.path) {
            return Err(format!(
                "{} and {} load as the same resource {}",
                previous.display(),
                file.path.display(),
                file.source.uri
            ));
        }
    }
    if let Some(expression) = &options.expression {
        let (source, source_map) =
            eval_source(options.target.as_deref(), expression, 0).map_err(|e| e.to_string())?;
        user.push(UserFile {
            path: "<expression>".into(),
            source,
            source_map: Some(source_map),
            origin: Origin::Loose,
            entry: None,
        });
    }
    let library = match options.library.as_slice() {
        [] => None,
        [folder] if !archives::is_archive(folder) => Some(library_sources(folder)?),
        archive_paths => Some(archives::library_archive_sources(
            archive_paths,
            &mut archives.problems,
        )?),
    };
    timer.lap("read files");

    let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)
        .map_err(|error| format!("{}: {error}", options.pack.display()))?;
    let (bundle, scopes) = language_pack::load_pack_sysml_bindings(&pack)
        .map_err(|error| format!("{}: {error}", options.pack.display()))?;
    let identity = intersect_kerml::sdk::bindings_identity(&bundle);
    let preparation = InvocationContext::new(Rc::new(Observer));
    let memo = prediction_memo_mode_from_env()?;
    timer.lap("load languages and bindings");

    let bindings = SysmlBindings {
        bundle: &bundle,
        scopes: &scopes,
        identity: &identity,
    };
    with_sysml_host(&session, &preparation, bindings, memo, |host| {
        let payload = match pack_library_payload(&pack) {
            Some(Ok(payload)) => Some(payload),
            Some(Err(error)) => {
                eprintln!("intersect: the pack's library part is not used: {error}");
                None
            }
            None => None,
        };
        let prepared = prepare_library(
            payload,
            library.as_deref(),
            host.context(),
            LibraryOptions {
                use_snapshot: !options.no_cache,
                trust: SnapshotTrust::HostCache,
            },
            |sources| {
                eprintln!(
                    "intersect: preparing the standard library ({} files); this takes a minute \
                     or two once",
                    sources.len()
                );
                host.parse_library(sources, &|_| {}).map(|(model, _)| model)
            },
        )?;
        timer.lap("prepare library");
        if let LibraryOrigin::Parsed(reason) = &prepared.origin {
            eprintln!("intersect: parsed the standard library ({reason})");
        }
        if let Some(payload) = prepared.payload.clone() {
            if options.read_only {
                eprintln!("intersect: --read-only: the library snapshot is not saved");
            } else {
                let bytes = with_library_part(&pack, payload)?;
                write_atomic(&options.pack, &bytes)?;
                eprintln!(
                    "intersect: saved the library snapshot into {}",
                    options.pack.display()
                );
            }
        }

        let sources: Vec<Source> = user.iter().map(|file| file.source.clone()).collect();
        let project = host.load_project(
            &prepared,
            &sources,
            intersect_kerml::sysml_host::ProjectDependencies::default(),
            &|_| {},
        )?;
        let load = project.load;
        timer.lap("load files");
        let mut problems = std::mem::take(&mut archives.problems);
        if let Some(model) = load.model.as_ref() {
            problems.extend(archives::check_index(model, &archives));
        }
        let clean = report(
            &options,
            &user,
            load.model.as_ref(),
            &load.diagnostics,
            &problems,
        ) && load.resolved == load.references
            && load.model.is_some();
        if options.command == Command::Eval && clean {
            let model = load
                .model
                .as_ref()
                .ok_or("evaluation load produced no model")?;
            evaluation::run(&options, &user, model)?;
            timer.lap("evaluate");
        }
        if let Some(analysis) = &analysis
            && clean
        {
            let model = load
                .model
                .as_ref()
                .ok_or("analysis load produced no model")?;
            let solved = analysis.run(model, &user, pack.bytes(), &prepared.key, options.json)?;
            timer.lap("analyze");
            return Ok(solved);
        }
        if options.command == Command::Export {
            let (Some(model), Some(format), Some(out)) =
                (load.model.as_ref(), options.format, options.out.as_ref())
            else {
                return Err("nothing to export: the files did not load".into());
            };
            match format {
                OutputFormat::Model(format) => export(model, &user, format, out)?,
                OutputFormat::Kpar => {
                    let project = archives::ProjectName {
                        name: options.project_name.as_deref().unwrap_or_default(),
                        version: options.project_version.as_deref().unwrap_or_default(),
                    };
                    let path = archives::export_kpar(model, &user, &archives, project, out)?;
                    eprintln!("wrote {}", path.display());
                }
            }
            timer.lap("export");
        }
        Ok(clean)
    })
}

/// Print the diagnostics; true when none is an error (warnings and
/// information do not fail a check).
fn report(
    options: &Options,
    user: &[UserFile],
    model: Option<&Model>,
    diagnostics: &[LoadDiagnostic],
    problems: &[archives::Problem],
) -> bool {
    for problem in problems {
        if options.json {
            let mut value = serde_json::json!({
                "severity": problem.severity,
                "phase": "archive",
                "message": problem.message,
            });
            if let Some(file) = &problem.file {
                value["file"] = serde_json::json!(file);
            }
            println!("{value}");
        } else {
            let place = problem
                .file
                .as_ref()
                .map(|file| format!("{file}: "))
                .unwrap_or_default();
            println!("{place}{}[archive]: {}", problem.severity, problem.message);
        }
    }
    let archive_errors = problems.iter().filter(|p| p.is_error()).count();
    // The linker reports an unresolved reference again, unlocated, after the
    // located scope problem for the same element: print each element's
    // link problems once.
    let located_links: std::collections::BTreeSet<&str> = diagnostics
        .iter()
        .filter(|d| d.phase == LoadPhase::Link && d.start.is_some())
        .filter_map(|d| d.object.as_deref())
        .collect();
    let diagnostics: Vec<&LoadDiagnostic> = diagnostics
        .iter()
        .filter(|d| {
            d.phase != LoadPhase::Link
                || d.start.is_some()
                || d.object
                    .as_deref()
                    .is_none_or(|object| !located_links.contains(object))
        })
        .collect();
    for diagnostic in diagnostics.iter().copied() {
        let located = locate(user, model, diagnostic);
        let mut message = readable(diagnostic).to_owned();
        if diagnostic.phase == LoadPhase::Link
            && message.contains("no element named")
            && in_expression_wrapper(user, model, diagnostic)
        {
            // Plan decision E9 (default): the --expr wrapper adds no library
            // imports, so library members need their qualified names.
            message.push_str(
                "; library names are not imported into --expr: qualify them (for example ControlFunctions::collect or SequenceFunctions::size)",
            );
        }
        if options.json {
            let mut value = serde_json::json!({
                "severity": diagnostic.severity,
                "phase": phase_name(diagnostic.phase),
                "message": message,
            });
            for (key, field) in [
                ("code", &diagnostic.code),
                ("rule", &diagnostic.rule),
                ("feature", &diagnostic.feature),
            ] {
                if let Some(field) = field {
                    value[key] = serde_json::json!(field);
                }
            }
            if let Some((file, line, column)) = &located {
                value["file"] = serde_json::json!(file);
                value["line"] = serde_json::json!(line);
                value["column"] = serde_json::json!(column);
            }
            if let Some(object) = &diagnostic.object {
                value["element"] = serde_json::json!(object);
            }
            println!("{value}");
        } else {
            let place = located
                .map(|(file, line, column)| format!("{file}:{line}:{column}: "))
                .unwrap_or_default();
            let code = diagnostic
                .code
                .as_deref()
                .map(|code| format!(" ({code})"))
                .unwrap_or_default();
            println!(
                "{place}{}[{}]: {message}{code}",
                diagnostic.severity.as_str(),
                phase_name(diagnostic.phase)
            );
        }
    }
    let errors = diagnostics.iter().filter(|d| d.is_error()).count() + archive_errors;
    if !options.json {
        let files = user.len();
        let others = diagnostics.len() + problems.len() - errors;
        match (errors, others) {
            (0, 0) => eprintln!("{files} file(s) checked: no problems"),
            (errors, 0) => eprintln!("{files} file(s) checked: {errors} problem(s)"),
            (errors, others) => eprintln!(
                "{files} file(s) checked: {errors} problem(s), {others} warning(s) or note(s)"
            ),
        }
    }
    errors == 0
}

/// The message without the element identity a link diagnostic starts with
/// (`res.sysml#12.type: no element named 'X'` becomes `type: no element named
/// 'X'`): the location says where the element is.
fn readable(diagnostic: &LoadDiagnostic) -> &str {
    diagnostic
        .object
        .as_deref()
        .and_then(|object| diagnostic.message.strip_prefix(object))
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(&diagnostic.message)
}

/// Whether a diagnostic belongs to the generated `--expr` wrapper resource.
fn in_expression_wrapper(
    user: &[UserFile],
    model: Option<&Model>,
    diagnostic: &LoadDiagnostic,
) -> bool {
    let resource = match &diagnostic.resource {
        Some(resource) => Some(resource.clone()),
        None => model
            .zip(diagnostic.object.as_deref())
            .and_then(|(model, object)| model.lookup(object))
            .map(|object| object.resource().to_owned()),
    };
    resource.is_some_and(|resource| {
        user.iter()
            .any(|file| file.source_map.is_some() && file.source.uri == resource)
    })
}

/// File, line and column of a diagnostic: its own offsets for a parse
/// diagnostic, else the span of the element it names.
fn locate(
    user: &[UserFile],
    model: Option<&Model>,
    diagnostic: &LoadDiagnostic,
) -> Option<(String, usize, usize)> {
    let (resource, start) = match (&diagnostic.resource, diagnostic.start) {
        (Some(resource), Some(start)) => (resource.clone(), start),
        _ if diagnostic.phase != LoadPhase::Parse => {
            let object = model?.lookup(diagnostic.object.as_deref()?)?;
            (object.resource().to_owned(), object.span().0)
        }
        _ => return None,
    };
    let file = user.iter().find(|file| file.source.uri == resource)?;
    let (line, column) = evaluation::source_position(file, start);
    Some((file.path.display().to_string(), line, column))
}

fn export(
    model: &Model,
    user: &[UserFile],
    format: ExportFormat,
    out: &Path,
) -> Result<(), String> {
    let selected: Vec<String> = user.iter().map(|file| file.source.uri.clone()).collect();
    let resources = sysml_host::export(model, &selected, format)?;
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    for resource in resources {
        // XMI documents are named by their resource URI: keep the file name
        // and replace the extension.
        let name = match format {
            ExportFormat::Json => resource.uri.clone(),
            ExportFormat::Xmi => {
                let uri = resource.uri.replace("%20", " ");
                let file = uri.rsplit('/').next().unwrap_or(&uri);
                format!("{file}.xmi")
            }
        };
        let path = out.join(&name);
        write_atomic(&path, &resource.bytes)?;
        eprintln!("wrote {} ({} bytes)", path.display(), resource.bytes.len());
    }
    Ok(())
}

fn info(options: &Options, pack: &LanguagePack) -> Result<bool, String> {
    let manifest = pack.manifest();
    println!("{}", options.pack.display());
    let expected = language_pack::expected_compiler();
    for (component, identity) in &manifest.compiler {
        let state = match expected.get(component) {
            Some(current) if current == identity => "matches this build",
            Some(_) => "DIFFERS from this build",
            None => "unknown to this build",
        };
        println!("  compiler {component}: {state}");
    }
    for part in &manifest.parts {
        println!("  part {}: key {}", part.name, part.input_digest);
    }
    match pack_library_payload(pack) {
        Some(Ok(payload)) => println!(
            "  {LIBRARY_SECTION}: snapshot key {} ({} bytes)",
            language_pack::payload_key(payload)?,
            payload.len()
        ),
        Some(Err(error)) => println!("  {LIBRARY_SECTION}: refused: {error}"),
        None => println!("  {LIBRARY_SECTION}: none (the first check prepares it)"),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn check_needs_a_pack_and_files() -> Result<(), String> {
        assert!(options(&args("check a.sysml")).is_err());
        assert!(options(&args("check --pack p.pack")).is_err());
        let parsed = options(&args("check --pack p.pack --read-only a.sysml b.kerml"))?;
        assert!(parsed.read_only);
        assert_eq!(parsed.files.len(), 2);
        Ok(())
    }

    #[test]
    fn analyze_requires_a_profile_manifest_case_and_model_files() -> Result<(), String> {
        let selected = options(&args(
            "analyze --pack p.pack --execution-profile run.toml --case P::size --profile scalar --inputs values.json --json a.sysml",
        ))?;
        assert!(selected.command == Command::Analyze);
        assert!(selected.analysis.is_some() && selected.json);
        assert_eq!(selected.files, [PathBuf::from("a.sysml")]);

        for invalid in [
            "analyze --pack p.pack --case P::size a.sysml",
            "analyze --pack p.pack --execution-profile run.toml a.sysml",
            "analyze --pack p.pack --execution-profile run.toml --case P::size",
            "analyze --pack p.pack --execution-profile run.toml --case P::size --case P::other a.sysml",
            "analyze --pack p.pack --execution-profile run.toml --case --json a.sysml",
            "analyze --pack p.pack --execution-profile run.toml --case P::size --expr 1 a.sysml",
            "analyze --pack p.pack --execution-profile run.toml --case P::size --out output a.sysml",
            "check --pack p.pack --execution-profile run.toml a.sysml",
        ] {
            assert!(options(&args(invalid)).is_err(), "{invalid}");
        }
        Ok(())
    }

    #[test]
    fn eval_parses_expression_and_all_options() -> Result<(), String> {
        let expression = vec![
            "eval",
            "--pack",
            "p.pack",
            "--expr",
            "1 + 2",
            "--target",
            "P::'a b'",
            "--mode",
            "extended",
            "--json",
            "--trace",
            "--explain",
            "--gated",
        ];
        let parsed = options(
            &expression
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )?;
        assert!(parsed.command == Command::Eval);
        assert_eq!(parsed.expression.as_deref(), Some("1 + 2"));
        assert_eq!(parsed.target.as_deref(), Some("P::'a b'"));
        assert_eq!(parsed.mode, EvalMode::Extended);
        assert!(parsed.json && parsed.trace && parsed.explain && parsed.gated);
        let parsed = options(&args("eval --pack p.pack --all a.sysml b.kerml"))?;
        assert!(parsed.all);
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.mode, EvalMode::Extended);
        let parsed = options(&args("eval --pack p.pack --all --mode model-level a.sysml"))?;
        assert_eq!(parsed.mode, EvalMode::ModelLevel);
        Ok(())
    }

    #[test]
    fn eval_rejects_missing_conflicting_and_malformed_arguments() {
        for command in [
            "eval --pack p.pack",
            "eval --expr 1",
            "eval --pack p.pack --all",
            "eval --pack p.pack --all --expr 1 a.sysml",
            "eval --pack p.pack --expr",
            "eval --pack p.pack --expr 1 --expr 2",
            "eval --pack p.pack --expr 1 --mode bad",
            "eval --pack p.pack --expr 1 --target a;b",
            "eval --pack p.pack --expr 1 --out out",
            "check --pack p.pack --trace a.sysml",
            "info --pack p.pack --mode model-level",
        ] {
            assert!(options(&args(command)).is_err(), "{command}");
        }
    }

    #[test]
    fn expression_diagnostics_map_first_multibyte_and_eof_positions() -> Result<(), String> {
        let (source, map) = eval_source(Some("P"), "'Ã©'\n+", 0).map_err(|e| e.to_string())?;
        let prefix = map.prefix_bytes;
        let end = prefix + map.expression_len;
        let file = UserFile {
            path: "<expression>".into(),
            source,
            source_map: Some(map),
            origin: Origin::Loose,
            entry: None,
        };
        assert_eq!(evaluation::source_position(&file, prefix), (1, 1));
        assert_eq!(evaluation::source_position(&file, end - 1), (2, 1));
        assert_eq!(evaluation::source_position(&file, end), (2, 2));
        Ok(())
    }

    #[test]
    #[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
    fn eval_expression_against_library_pack() -> Result<(), String> {
        let Some(pack) = std::env::var_os("INTERSECT_EVAL_PACK") else {
            eprintln!("SKIPPED: INTERSECT_EVAL_PACK is unset");
            return Ok(());
        };
        assert!(run(&[
            "eval".into(),
            "--pack".into(),
            pack.to_string_lossy().into_owned(),
            "--read-only".into(),
            "--expr".into(),
            "1 + 2".into(),
            "--json".into()
        ])?);
        Ok(())
    }

    #[test]
    #[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
    fn eval_all_reports_every_site_when_one_fails() -> Result<(), String> {
        let Some(pack) = std::env::var_os("INTERSECT_EVAL_PACK") else {
            eprintln!("SKIPPED: INTERSECT_EVAL_PACK is unset");
            return Ok(());
        };
        let dir = std::env::temp_dir().join(format!("intersect-eval-all-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join("recursive.sysml");
        std::fs::write(&file, "package R { attribute r = r + 1; attribute s = 2; }")
            .map_err(|e| e.to_string())?;
        for json in [false, true] {
            let mut arguments: Vec<String> = vec![
                "eval".into(),
                "--pack".into(),
                pack.to_string_lossy().into_owned(),
                "--read-only".into(),
                "--all".into(),
            ];
            if json {
                arguments.push("--json".into());
            }
            arguments.push(file.to_string_lossy().into_owned());
            // The recursive site fails, the later site is still evaluated,
            // and the command reports failure (exit status 2) at the end.
            let error = run(&arguments)
                .err()
                .ok_or("expected an evaluation error")?;
            assert!(error.contains("1 of 2 site(s)"), "{error}");
        }
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn export_needs_a_format_and_an_output() {
        assert!(options(&args("export --pack p.pack a.sysml")).is_err());
        assert!(options(&args("export --pack p.pack --format yaml --out o a.sysml")).is_err());
        let parsed = options(&args("export --pack p.pack --format xmi --out o a.sysml"));
        assert!(parsed.is_ok_and(|o| o.format == Some(OutputFormat::Model(ExportFormat::Xmi))));
        assert!(options(&args("export --pack p.pack --format kpar --out o a.kpar")).is_err());
        let parsed = options(&args(
            "export --pack p.pack --format kpar --out o --project-name P --project-version 1.0.0 a.kpar",
        ));
        assert!(parsed.is_ok_and(|o| o.format == Some(OutputFormat::Kpar)));
        assert!(options(&args("check --pack p.pack --project-name P a.sysml")).is_err());
    }

    #[test]
    fn library_takes_one_folder_or_archives() {
        let parsed = options(&args(
            "check --pack p.pack --library a.kpar --library b.kpar --project-path deps --project-path more x.kpar",
        ));
        assert!(parsed.is_ok_and(|o| o.library.len() == 2 && o.project_path.len() == 2));
        assert!(
            options(&args(
                "check --pack p.pack --library dir --library b.kpar x.sysml"
            ))
            .is_err()
        );
    }

    #[test]
    fn unknown_commands_and_options_are_refused() {
        assert!(options(&args("lint --pack p.pack a.sysml")).is_err());
        assert!(options(&args("check --pack p.pack --fast a.sysml")).is_err());
        assert!(options(&args("info --pack p.pack a.sysml")).is_err());
        assert!(options(&[]).is_err());
    }

    #[test]
    fn resource_uris_use_forward_slashes_and_no_spaces() {
        assert_eq!(
            resource_uri(Path::new(r"./models\Simple Tests/a.sysml")),
            "models/Simple%20Tests/a.sysml"
        );
        assert_eq!(
            resource_uri(Path::new("models/#1%20/a?.sysml")),
            "models/%231%2520/a%3F.sysml"
        );
    }

    #[test]
    fn link_messages_drop_the_element_identity() {
        let diagnostic = LoadDiagnostic {
            object: Some("a.sysml#12".into()),
            ..LoadDiagnostic::error(
                LoadPhase::Link,
                "a.sysml#12.type: no element named 'X'".into(),
            )
        };
        assert_eq!(readable(&diagnostic), "type: no element named 'X'");
    }

    #[test]
    fn user_files_must_be_kerml_or_sysml() {
        let read = read_user_files(&[PathBuf::from("notes.txt")]);
        assert!(read.is_err_and(|error| error.contains("not a .kerml or .sysml file")));
    }

    #[test]
    fn library_sources_are_relative_to_the_library_folder_parent() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("intersect-cli-test-{}", std::process::id()));
        let library = root.join("sysml.library");
        let nested = library.join("Kernel Libraries");
        std::fs::create_dir_all(&nested).map_err(|e| e.to_string())?;
        std::fs::write(nested.join("Base.kerml"), "package Base;").map_err(|e| e.to_string())?;
        std::fs::write(library.join("A.sysml"), "package A;").map_err(|e| e.to_string())?;
        std::fs::write(library.join("README.md"), "skip").map_err(|e| e.to_string())?;
        let sources = library_sources(&library);
        let _ = std::fs::remove_dir_all(&root);
        let uris: Vec<_> = sources?
            .into_iter()
            .map(|source| (source.uri, source.language))
            .collect();
        assert_eq!(
            uris,
            [
                ("sysml.library/A.sysml".to_owned(), SYSML_SOURCE.to_owned()),
                (
                    "sysml.library/Kernel%20Libraries/Base.kerml".to_owned(),
                    KERML_SOURCE.to_owned()
                ),
            ]
        );
        Ok(())
    }
}
