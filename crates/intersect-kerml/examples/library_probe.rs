//! Handwritten measurement host: how far the frozen-engine text load is from
//! the full SysML v2 standard library. It measures; it changes nothing.
//!
//! Phase 1 parses every library file alone into a fresh `FrozenStore`, in
//! worker processes (at most half the machine's cores; each worker prepares
//! its definition once). Phase 2 parses every KerML file that succeeded into
//! one `FrozenStore`, links it with the source-authored KerML name resolution,
//! completes the Rcore defaults and validates. When linking fails, the
//! construction is parsed again, the references that did not resolve are
//! cleared, and the remaining phases run on that reduced model so that their
//! gaps can be measured too. `.sysml` files take the same Phase 1 path only if
//! a public SysML draft capability opens; otherwise the failure is reported.
//!
//! SysML phase 2 loads every constructed KerML and SysML file into one store,
//! from the constructions the phase 1 workers saved, and runs the same link,
//! defaults and validation with the KerML and SysML bindings together
//! (`support/sysml_scopes.rs`).
//!
//! Phase 1 workers save each constructed file's export under
//! `target/library-probe-constructions/`, keyed by the file text, the original
//! definitions, `intersect_kerml::SOURCE_IDENTITY` and `CONSTRUCTION_RECIPE`.
//! With `LIBRARY_PROBE_REUSE` set, phase 1 reuses a saved construction instead
//! of parsing the file again (a measurement convenience for Rcore iterations;
//! reported rows say `reused`). Without it every file is parsed.
//!
//! `library_probe --sysml-phase2 [--kerml-only] ROOT [OUTPUT_JSON]` runs SysML
//! phase 2 alone from the saved constructions.
//!
//! Usage: `library_probe [--skip-sysml] ROOT [OUTPUT_JSON]` (default
//! `target/library-probe.json`). A file counts as constructed only when its
//! parse returns without diagnostics; a parse that recovers with diagnostics
//! is reported with its first diagnostic as the error. Worker mode is internal:
//! `library_probe --worker ROOT kerml|sysml FILE...` writes one JSON line per file.
use intersect_kerml::native_pilot::{NativeKerMLSession, NativePilotSession};
use intersect_model::core::frozen::ClassId;
use intersect_model::core::frozen::FeatureId;
use intersect_model::core::frozen::FrozenError;
use intersect_model::core::frozen::FrozenStore;
use intersect_model::core::frozen::LinkView;
use intersect_model::core::frozen::Metamodel;
use intersect_model::core::frozen::ObjId;
use intersect_model::core::frozen::Violation;
use intersect_model::core::query::QueryBudget;
use intersect_model::core::storage;
use intersect_model::core::storage::ModelStore;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::conversion::InvocationObserver;
use intersect_model::definitions::conversion::ProviderId;
use intersect_text::preparation::PreparedLanguageDefinition;
use intersect_text::{
    EvaluationLimits, Source,
    document::AnalysisLimits,
    draft::{DraftCapability, DraftLanguage, OpenDraftCapability},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

#[path = "support/kerml_scopes.rs"]
mod kerml_scopes;
#[path = "support/sysml_scopes.rs"]
mod sysml_scopes;

/// Bump when the worker's parse setup changes what a construction contains.
const CONSTRUCTION_RECIPE: &str = "intersect.library-probe.construction.v1";
const CONSTRUCTIONS: &str = "target/library-probe-constructions";

/// The saved-construction key of one library file.
fn construction_key(root: &Path, file: &str) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for part in [CONSTRUCTION_RECIPE, intersect_kerml::SOURCE_IDENTITY, file] {
        digest.update((part.len() as u64).to_le_bytes());
        digest.update(part.as_bytes());
    }
    for path in DEFINITIONS.iter().copied().chain([file]) {
        let text = read(root, path)?.text;
        digest.update((text.len() as u64).to_le_bytes());
        digest.update(text.as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn construction_path(root: &Path, file: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(CONSTRUCTIONS).join(format!("{}.json", construction_key(root, file)?)))
}

/// A saved construction: the phase 1 row and the exported model.
fn load_construction(root: &Path, file: &str) -> Option<Value> {
    let path = construction_path(root, file).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

const CORPUS: &str = "fixtures/native-pilot/v1/sources";
const LIBRARY: &str = "sysml.library";
const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];

fn ms(clock: Instant) -> f64 {
    clock.elapsed().as_secs_f64() * 1e3
}

fn read(root: &Path, path: &str) -> Result<Source, String> {
    let text = std::fs::read_to_string(root.join(CORPUS).join(path))
        .map_err(|e| format!("{path}: {e}"))?;
    Ok(Source {
        // Logical resource URIs may not contain spaces.
        uri: path.replace(' ', "%20"),
        text,
    })
}

fn limits() -> EvaluationLimits {
    // Nested library expressions go deeper than the default 64 rule levels
    // (one per KerML operator precedence level).
    EvaluationLimits {
        steps_per_resource: Some(4_000_000_000),
        nesting: 256,
        ..EvaluationLimits::native()
    }
}

fn frozen_metamodel(definition: &PreparedLanguageDefinition<'_>) -> Result<Arc<Metamodel>, String> {
    let redefinitions = kerml_scopes::redefinitions(definition.schema())?;
    Ok(Arc::new(
        Metamodel::freeze(definition.schema())
            .and_then(|mm| mm.with_redefinitions(&redefinitions))
            .map_err(|e| e.to_string())?,
    ))
}

/// Library files, relative to the corpus, sorted.
/// The Pilot example corpus of roadmap decision D9, under `CORPUS`.
const EXAMPLES: [&str; 6] = [
    "kerml/src/examples/Simple Tests/Types.kerml",
    "kerml/src/examples/Simple Tests/Expressions.kerml",
    "sysml/src/examples/Simple Tests/PartTest.sysml",
    "sysml/src/examples/Simple Tests/ConnectionTest.sysml",
    "sysml/src/examples/Simple Tests/RequirementTest.sysml",
    "sysml/src/examples/Simple Tests/ConstraintTest.sysml",
];

fn library_files(root: &Path, extension: &str) -> Result<Vec<String>, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
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
    let mut files: Vec<String> = found
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == extension))
        .filter_map(|p| p.strip_prefix(&base).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    files.sort();
    // LIBRARY_PROBE_EXAMPLES adds the Pilot example corpus (roadmap decision
    // D9), loaded with the library and linked against it.
    if std::env::var_os("LIBRARY_PROBE_EXAMPLES").is_some() {
        files.extend(
            EXAMPLES
                .iter()
                .filter(|f| f.ends_with(&format!(".{extension}")))
                .map(|f| (*f).to_owned()),
        );
    }
    Ok(files)
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--worker") {
        let root = PathBuf::from(args.get(2).ok_or("worker: missing ROOT")?);
        let language = args.get(3).ok_or("worker: missing language")?;
        return worker(&root, language, &args[4..]);
    }
    if args.get(1).map(String::as_str) == Some("--sysml-phase2") {
        // SysML phase 2 alone, from the saved constructions of every library
        // file (`--kerml-only`: of the KerML files).
        let kerml_only = args.iter().any(|a| a == "--kerml-only");
        let rest: Vec<&String> = args[2..].iter().filter(|a| *a != "--kerml-only").collect();
        let root = PathBuf::from(rest.first().map_or(".", |r| r.as_str()));
        let output = PathBuf::from(
            rest.get(1)
                .map_or("target/sysml-phase2.json", |r| r.as_str()),
        );
        let mut files = library_files(&root, "kerml")?;
        if !kerml_only {
            files.extend(library_files(&root, "sysml")?);
        }
        let report = sysml_phase2(&root, &files)?;
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        std::fs::write(&output, text).map_err(|e| format!("{}: {e}", output.display()))?;
        println!("wrote {}", output.display());
        return Ok(());
    }
    let skip_sysml = args.iter().any(|a| a == "--skip-sysml");
    let args: Vec<String> = args.into_iter().filter(|a| a != "--skip-sysml").collect();
    let root = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| ".".into()));
    let output = PathBuf::from(
        args.get(2)
            .cloned()
            .unwrap_or_else(|| "target/library-probe.json".into()),
    );
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    let workers = (cores / 2).max(1);
    println!("{cores} cores; at most {workers} worker processes");

    let kerml_files = library_files(&root, "kerml")?;
    let sysml_files = library_files(&root, "sysml")?;
    println!(
        "{} .kerml and {} .sysml library files",
        kerml_files.len(),
        sysml_files.len()
    );

    let clock = Instant::now();
    let kerml_rows = phase1(&root, "kerml", &kerml_files, workers)?;
    let kerml_wall = ms(clock);
    report_phase1("kerml", &kerml_rows, kerml_wall);

    let succeeded: Vec<String> = kerml_rows
        .iter()
        .filter(|r| r["ok"] == true)
        .filter_map(|r| r["file"].as_str().map(str::to_owned))
        .collect();
    let phase2 = phase2(&root, &succeeded)?;

    // SysML: one worker first; it reports whether a capability opens at all.
    let clock = Instant::now();
    let first = if skip_sysml {
        0
    } else {
        1.min(sysml_files.len())
    };
    let probe = phase1(&root, "sysml", &sysml_files[..first], 1)?;
    let sysml_setup = probe
        .iter()
        .find(|r| r.get("setup").is_some())
        .cloned()
        .unwrap_or(Value::Null);
    let sysml_rows = if probe
        .iter()
        .any(|r| r["ok"] == true || r.get("parse_ms").is_some())
    {
        let rows = phase1(&root, "sysml", &sysml_files, workers)?;
        report_phase1("sysml", &rows, ms(clock));
        rows
    } else if skip_sysml {
        println!("sysml: skipped");
        probe
    } else {
        println!("sysml: no file parsed; setup {sysml_setup}");
        probe
    };

    let sysml_phase2 = if skip_sysml {
        Value::Null
    } else {
        let constructed = |rows: &[Value]| -> Vec<String> {
            rows.iter()
                .filter(|r| r["ok"] == true)
                .filter_map(|r| r["file"].as_str().map(str::to_owned))
                .collect()
        };
        let mut files = constructed(&kerml_rows);
        files.extend(constructed(&sysml_rows));
        sysml_phase2(&root, &files)?
    };

    let report = json!({
        "cores": cores,
        "workers": workers,
        "kerml": { "wall_ms": kerml_wall, "files": kerml_rows },
        "phase2": phase2,
        "sysml": { "setup": sysml_setup, "files": sysml_rows },
        "sysml_phase2": sysml_phase2,
    });
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&output, text).map_err(|e| format!("{}: {e}", output.display()))?;
    println!("wrote {}", output.display());
    Ok(())
}

/// Spread `files` over at most `workers` processes, largest files first to the
/// least-loaded worker, and collect their JSON lines.
fn phase1(
    root: &Path,
    language: &str,
    files: &[String],
    workers: usize,
) -> Result<Vec<Value>, String> {
    let mut sized: Vec<(u64, &String)> = files
        .iter()
        .map(|f| {
            let size = std::fs::metadata(root.join(CORPUS).join(f)).map_or(0, |m| m.len());
            (size, f)
        })
        .collect();
    sized.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut rows = Vec::new();
    if std::env::var_os("LIBRARY_PROBE_REUSE").is_some() {
        sized.retain(|(_, file)| match load_construction(root, file) {
            Some(saved) => {
                let mut row = saved["row"].clone();
                row["reused"] = json!(true);
                rows.push(row);
                false
            }
            None => true,
        });
    }
    if sized.is_empty() {
        rows.sort_by(|a, b| a["file"].as_str().cmp(&b["file"].as_str()));
        return Ok(rows);
    }
    let mut shares: Vec<(u64, Vec<&String>)> = vec![(0, Vec::new()); workers.min(sized.len())];
    for (size, file) in sized {
        if let Some(share) = shares.iter_mut().min_by_key(|s| s.0) {
            share.0 += size;
            share.1.push(file);
        }
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut children = Vec::new();
    for (_, share) in &shares {
        let child = Command::new(&exe)
            .arg("--worker")
            .arg(root)
            .arg(language)
            .args(share.iter().map(|s| s.as_str()))
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| e.to_string())?;
        children.push((child, share.clone()));
    }
    for (mut child, share) in children {
        let stdout = child.stdout.take().ok_or("worker without stdout")?;
        let mut reported = std::collections::BTreeSet::new();
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.map_err(|e| e.to_string())?;
            if let Ok(row) = serde_json::from_str::<Value>(&line) {
                if let Some(file) = row["file"].as_str() {
                    reported.insert(file.to_owned());
                }
                rows.push(row);
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        // A worker that dies (stack overflow, abort) leaves its remaining files unreported.
        for file in share.iter().filter(|f| !reported.contains(f.as_str())) {
            rows.push(json!({
                "file": file,
                "ok": false,
                "error": format!("worker exited ({status}) before reporting this file"),
            }));
        }
    }
    rows.sort_by(|a, b| a["file"].as_str().cmp(&b["file"].as_str()));
    Ok(rows)
}

fn report_phase1(language: &str, rows: &[Value], wall: f64) {
    // Setup rows carry no file.
    let rows: Vec<&Value> = rows.iter().filter(|r| r.get("file").is_some()).collect();
    let ok: Vec<&Value> = rows.iter().copied().filter(|r| r["ok"] == true).collect();
    let objects: u64 = ok.iter().filter_map(|r| r["objects"].as_u64()).sum();
    let parse: f64 = ok.iter().filter_map(|r| r["parse_ms"].as_f64()).sum();
    println!(
        "{language} phase 1: {} of {} files constructed, {objects} objects, parse {parse:.0} ms summed ({:.2} ms/object), wall {wall:.0} ms",
        ok.len(),
        rows.len(),
        parse / (objects.max(1) as f64)
    );
    for row in rows.iter().filter(|r| r["ok"] != true) {
        let error: String = row["error"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect();
        println!("  FAIL {}: {error}", row["file"].as_str().unwrap_or("?"));
    }
}

/// One worker process: prepare once, parse each file alone, one JSON line each.
fn worker(root: &Path, language: &str, files: &[String]) -> Result<(), String> {
    let mut sources = Vec::new();
    for path in DEFINITIONS {
        sources.push(read(root, path)?);
    }
    let preparation = InvocationContext::new(Rc::new(Observer));
    let emit = |row: Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{row}");
        let _ = out.flush();
    };
    let clock = Instant::now();
    if language == "kerml" {
        let session = NativeKerMLSession::prepare(&sources[..3], &preparation)?;
        let prepare_ms = ms(clock);
        session.with_definition(|definition, contracts| {
            let kerml = intersect_kerml::text_capability::kerml_capability(definition, contracts)?;
            let kerml = intersect_kerml::text_capability::nonunique_draft_capability(
                definition,
                kerml,
                &mut QueryBudget::new(usize::MAX),
            )?;
            let capability = OpenDraftCapability::new(definition, &kerml)?;
            let names = intersect_kerml::text_capability::kerml_string_rules(
                definition,
                &mut QueryBudget::new(usize::MAX),
            )?;
            let capability = capability.with_string_rules(&names);
            let operands = intersect_kerml::operand_settings(definition, contracts)?;
            let providers: Vec<&dyn intersect_model::setting::SourceSettingValues> = operands
                .iter()
                .map(|p| p as &dyn intersect_model::setting::SourceSettingValues)
                .collect();
            let settings =
                intersect_text::setting::CheckedDraftTextSettings::bind(definition, &providers)?;
            parse_each(
                root,
                definition,
                &capability,
                Some(&settings),
                files,
                prepare_ms,
                &emit,
            )
        })
    } else {
        // The SysML grammar is prepared by the two-owner Pilot session; the
        // public draft capabilities are tried in turn and every failure kept.
        let session = match NativePilotSession::prepare(&sources, &preparation) {
            Ok(session) => session,
            Err(error) => {
                emit(json!({ "setup": { "session": error } }));
                return Ok(());
            }
        };
        let prepare_ms = ms(clock);
        session.with_definitions(|_, _, sysml, contracts| {
            // SysML on the frozen path: the SysML package constructors with
            // SysML's Boolean conversions, its computed settings (operand and
            // isReference) and the KerML Name conversion.
            let mut budget = QueryBudget::new(usize::MAX);
            let inner = intersect_kerml::text_capability::sysml_capability(
                sysml,
                &preparation,
                &mut budget,
            )?;
            let providers =
                intersect_kerml::text_capability::sysml_setting_providers(sysml, contracts)?;
            let provider_refs: Vec<&dyn intersect_model::setting::SourceSettingValues> =
                providers.iter().map(|p| p.as_ref()).collect();
            let settings =
                intersect_text::setting::CheckedDraftTextSettings::bind(sysml, &provider_refs)?;
            let open = OpenDraftCapability::new(sysml, &inner)?;
            let names = intersect_kerml::text_capability::kerml_string_rules(sysml, &mut budget);
            let open = match &names {
                Ok(names) => open.with_string_rules(names),
                Err(_) => open,
            };
            emit(json!({ "setup": {
                "capability": "sysml_frozen_inner_capability",
                "name_conversion": names.as_ref().err().map(|e| e.to_string()),
            } }));
            parse_each(
                root,
                sysml,
                &open,
                Some(&settings),
                files,
                prepare_ms,
                &emit,
            )
        })
    }
}

fn parse_each<C: DraftCapability>(
    root: &Path,
    definition: &PreparedLanguageDefinition<'_>,
    capability: &OpenDraftCapability<'_, C>,
    settings: Option<&intersect_text::setting::CheckedDraftTextSettings<'_>>,
    files: &[String],
    prepare_ms: f64,
    emit: &impl Fn(Value),
) -> Result<(), String> {
    let language = DraftLanguage::bind(definition, capability, AnalysisLimits::workbench())?;
    let mm = frozen_metamodel(definition)?;
    for file in files {
        let source = read(root, file)?;
        let mut store = FrozenStore::new(mm.clone());
        let mut caller = QueryBudget::new(usize::MAX);
        let clock = Instant::now();
        let result = language.parse_runtime_into_with_settings(
            &source,
            limits(),
            settings,
            &mut store,
            &mut caller,
        );
        let parse_ms = ms(clock);
        let lines = source.text.lines().count();
        let row = match result {
            Ok((evaluation, proxies, _)) => {
                let exported = storage::export(&store);
                let objects = exported.as_ref().map(|m| m.elements.len());
                let first = evaluation.diagnostics.first().map(|d| d.message.clone());
                let row = json!({
                    "file": file,
                    "ok": evaluation.diagnostics.is_empty(),
                    "error": first.clone().filter(|_| !evaluation.diagnostics.is_empty()),
                    "lines": lines,
                    "objects": objects.as_ref().ok(),
                    "export_error": objects.err(),
                    "unresolved_references": proxies,
                    "diagnostics": evaluation.diagnostics.len(),
                    "first_diagnostic": first,
                    "first_diagnostic_at": evaluation.diagnostics.first().map(|d| [d.start, d.end]),
                    "parse_ms": parse_ms,
                    "worker_prepare_ms": prepare_ms,
                });
                if let (true, Ok(model)) = (evaluation.diagnostics.is_empty(), &exported) {
                    save_construction(root, file, &row, model)?;
                }
                row
            }
            Err(error) => json!({
                "file": file,
                "ok": false,
                "lines": lines,
                "error": error,
                "parse_ms": parse_ms,
                "worker_prepare_ms": prepare_ms,
            }),
        };
        emit(row);
    }
    Ok(())
}

fn save_construction(
    root: &Path,
    file: &str,
    row: &Value,
    model: &intersect_model::core::model::Model,
) -> Result<(), String> {
    let path = construction_path(root, file)?;
    std::fs::create_dir_all(CONSTRUCTIONS).map_err(|e| e.to_string())?;
    let text =
        serde_json::to_string(&json!({ "row": row, "model": model })).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, text).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, &path).map_err(|e| format!("{}: {e}", path.display()))
}

/// (owner identity, owner class, feature, spelling, Rcore candidates or error)
struct Outcome {
    owner: String,
    class: String,
    feature: String,
    spelling: String,
    found: Result<Vec<String>, String>,
}

/// All succeeding KerML files in one store; link, defaults, validate.
fn phase2(root: &Path, files: &[String]) -> Result<Value, String> {
    let sources = DEFINITIONS[..3]
        .iter()
        .map(|p| read(root, p))
        .collect::<Result<Vec<_>, _>>()?;
    let libraries = files
        .iter()
        .map(|p| read(root, p))
        .collect::<Result<Vec<_>, _>>()?;
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativeKerMLSession::prepare(&sources, &preparation)?;
    session.with_definition(|definition, contracts| {
        let kerml = intersect_kerml::text_capability::kerml_capability(definition, contracts)?;
        let kerml = intersect_kerml::text_capability::nonunique_draft_capability(
            definition,
            kerml,
            &mut QueryBudget::new(usize::MAX),
        )?;
        let capability = OpenDraftCapability::new(definition, &kerml)?;
        let names = intersect_kerml::text_capability::kerml_string_rules(
            definition,
            &mut QueryBudget::new(usize::MAX),
        )?;
        let capability = capability.with_string_rules(&names);
        let language = DraftLanguage::bind(definition, &capability, AnalysisLimits::workbench())?;
        let operands = intersect_kerml::operand_settings(definition, contracts)?;
        let providers: Vec<&dyn intersect_model::setting::SourceSettingValues> = operands
            .iter()
            .map(|p| p as &dyn intersect_model::setting::SourceSettingValues)
            .collect();
        let settings =
            intersect_text::setting::CheckedDraftTextSettings::bind(definition, &providers)?;
        let mm = frozen_metamodel(definition)?;
        let parse_all = || -> Result<(FrozenStore, f64, Vec<Value>), String> {
            let mut store = FrozenStore::new(mm.clone());
            let mut failures = Vec::new();
            let clock = Instant::now();
            for source in &libraries {
                let mut caller = QueryBudget::new(usize::MAX);
                if let Err(error) = language.parse_runtime_into_with_settings(
                    source,
                    limits(),
                    Some(&settings),
                    &mut store,
                    &mut caller,
                ) {
                    failures.push(json!({ "file": source.uri, "error": error }));
                }
            }
            Ok((store, ms(clock), failures))
        };
        let (store, parse_ms, parse_failures) = parse_all()?;
        let objects = storage::export(&store)
            .map(|m| m.elements.len())
            .map_err(|e| e.to_string())?;
        println!(
            "phase 2: {} files, {objects} objects, parse {parse_ms:.0} ms, {} parse failures",
            libraries.len(),
            parse_failures.len()
        );

        let clock = Instant::now();
        let (bundle, scopes, cached) = kerml_scopes::compile_cached(&mm)?;
        let scope_ms = ms(clock);
        println!(
            "{} Rcore scopes {scope_ms:.0} ms",
            if cached { "loaded saved" } else { "compiled" }
        );
        use intersect_model::frozen_scopes::{Execution, RcoreScopes};
        let resolvers = RcoreScopes::new(&mm, &bundle, &scopes)
            .map_err(|e| e.to_string())?
            .with_execution(Execution::Fast);
        report_unsupported_scopes(&mm, &resolvers);
        let report = json!({
            "files": libraries.len(),
            "objects": objects,
            "parse_ms": parse_ms,
            "parse_failures": parse_failures,
            "scope_ms": scope_ms,
            "scopes_cached": cached,
        });
        let dump = std::env::var("LIBRARY_PROBE_DUMP").ok();
        link_phases(
            &mm,
            &resolvers,
            store,
            &|| parse_all().map(|(store, ..)| store),
            "",
            dump,
            report,
        )
    })
}

/// Every constructed KerML and SysML file in one store, from the saved
/// constructions; link with the KerML and SysML bindings, defaults, validate.
fn sysml_phase2(root: &Path, files: &[String]) -> Result<Value, String> {
    let clock = Instant::now();
    let mut models = Vec::new();
    let mut missing = Vec::new();
    for file in files {
        match load_construction(root, file).map(|saved| {
            serde_json::from_value::<intersect_model::core::model::Model>(saved["model"].clone())
        }) {
            Some(Ok(model)) => models.push(model),
            Some(Err(error)) => missing.push(json!({ "file": file, "error": error.to_string() })),
            None => missing.push(json!({ "file": file, "error": "no saved construction" })),
        }
    }
    let load_ms = ms(clock);
    // The KerML definition's schema is the combined SysML.ecore that the
    // SysML grammar constructs too; preparing it is the cheaper of the two.
    let sources = DEFINITIONS[..3]
        .iter()
        .map(|p| read(root, p))
        .collect::<Result<Vec<_>, _>>()?;
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = NativeKerMLSession::prepare(&sources, &preparation)?;
    session.with_definition(|definition, _| {
        let mm = frozen_metamodel(definition)?;
        let make_store = || -> Result<FrozenStore, String> {
            let mut store = FrozenStore::new(mm.clone());
            storage::atomic(&mut store, |store| {
                for model in &models {
                    for record in model.resources.values() {
                        store.set_resource_record(record.clone())?;
                    }
                    for element in &model.elements {
                        store.put(element.clone())?;
                    }
                }
                Ok(())
            })?;
            Ok(store)
        };
        let clock = Instant::now();
        let store = make_store()?;
        let import_ms = ms(clock);
        let objects: usize = models.iter().map(|m| m.elements.len()).sum();
        println!(
            "sysml phase 2: {} files, {objects} objects, load {load_ms:.0} ms, import {import_ms:.0} ms, {} missing",
            models.len(),
            missing.len()
        );
        let clock = Instant::now();
        let (bundle, scopes, cached) = sysml_scopes::compile_cached(&mm)?;
        let scope_ms = ms(clock);
        println!(
            "sysml {} Rcore scopes {scope_ms:.0} ms",
            if cached { "loaded saved" } else { "compiled" }
        );
        use intersect_model::frozen_scopes::{Execution, RcoreScopes};
        let mut limits = RcoreScopes::default_limits();
        if let Some(steps) = std::env::var("SYSML_SCOPE_STEPS").ok().and_then(|s| s.parse().ok()) {
            limits.scalar.max_steps = steps;
        }
        let resolvers = RcoreScopes::new(&mm, &bundle, &scopes)
            .map_err(|e| e.to_string())?
            .with_limits(limits)
            .with_execution(Execution::Fast);
        let report = json!({
            "files": models.len(),
            "objects": objects,
            "missing": missing,
            "load_ms": load_ms,
            "import_ms": import_ms,
            "scope_ms": scope_ms,
            "scopes_cached": cached,
        });
        let dump = std::env::var("LIBRARY_PROBE_SYSML_DUMP").ok();
        link_phases(&mm, &resolvers, store, &make_store, "sysml ", dump, report)
    })
}

/// Link `store` with `resolvers`, then implicit elements, defaults and
/// validation; when linking fails, `make_store` constructs the model again so
/// the references that failed can be cleared and the later phases measured.
fn link_phases(
    mm: &Arc<Metamodel>,
    resolvers: &intersect_model::frozen_scopes::RcoreScopes<'_>,
    store: FrozenStore,
    make_store: &dyn Fn() -> Result<FrozenStore, String>,
    label: &str,
    dump: Option<String>,
    mut report: Value,
) -> Result<Value, String> {
    let outcomes = std::cell::RefCell::new(Vec::<Outcome>::new());
    let resolve = |view: &LinkView<'_>,
                   owner: ObjId,
                   feature: FeatureId,
                   spelling: &str|
     -> Result<Option<ObjId>, FrozenError> {
        let found = resolvers.candidates(view, owner, feature, spelling);
        let names = found.as_ref().map_err(|e| e.to_string()).map(|c| {
            c.iter()
                .filter_map(|o| view.identity(*o))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
        outcomes.borrow_mut().push(Outcome {
            owner: view.identity(owner).unwrap_or("").to_owned(),
            class: view
                .class(owner)
                .map_or("?", |c| mm.class_key(c))
                .to_owned(),
            feature: mm.feature_name(feature).to_owned(),
            spelling: spelling.to_owned(),
            found: names,
        });
        Ok(match found {
            Ok(c) if c.len() == 1 => c.first().copied(),
            _ => None,
        })
    };
    let clock = Instant::now();
    let builder = store.into_builder().map_err(|e| e.to_string())?;
    let linked = builder.link(&resolve);
    let link_ms = ms(clock);
    let outcomes = outcomes.into_inner();
    let link_groups = group_outcomes(&outcomes);
    let resolved = outcomes
        .iter()
        .filter(|o| o.found.as_ref().is_ok_and(|f| f.len() == 1))
        .count();
    println!(
        "{label}link {link_ms:.0} ms: {} references, {resolved} resolved uniquely",
        outcomes.len()
    );
    report["link_ms"] = json!(link_ms);
    let stats = resolvers.stats();
    if stats.deferred > 0 {
        println!(
            "{label}{} of {} scope runs deferred to the VM: {:?}",
            stats.deferred,
            stats.fast + stats.deferred,
            stats.deferred_by_reason
        );
    }
    report["deferred_by_reason"] = json!(stats.deferred_by_reason);
    report["references"] = json!(outcomes.len());
    report["resolved"] = json!(resolved);
    report["link_groups"] = link_groups;
    // LIBRARY_PROBE_OUTCOMES=PATH writes every reference's outcome, so that
    // two runs can be compared reference by reference.
    if let Ok(path) = std::env::var("LIBRARY_PROBE_OUTCOMES") {
        let all: Vec<Value> = outcomes
            .iter()
            .map(|o| {
                let found = match &o.found {
                    Ok(found) => json!(found),
                    Err(error) => json!(error),
                };
                json!({ "owner": o.owner, "feature": o.feature, "spelling": o.spelling, "found": found })
            })
            .collect();
        let text = serde_json::to_string(&all).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?;
    }

    // Features of the references cleared below; a dump treats them as
    // delegated so that it can export the rest of the model.
    let mut cleared_features: std::collections::BTreeSet<String> = Default::default();
    let (linked, reduced) = match linked {
        Ok(linked) => (linked, None),
        Err(diagnostics) => {
            report["link_violations"] = group_violations(&diagnostics.0);
            // Measure the later phases on the construction without the
            // references that failed: parse again, clear them, link again.
            // Proxies survive until `into_builder`, so the values are
            // cleared in the store; a list loses its resolved items too.
            let mut failed: Vec<(String, String)> = outcomes
                .iter()
                .filter(|o| !o.found.as_ref().is_ok_and(|f| f.len() == 1))
                .map(|o| (o.owner.clone(), o.feature.clone()))
                .collect();
            for v in &diagnostics.0 {
                if let Violation::Unresolved {
                    object, feature, ..
                }
                | Violation::ReferenceCycle {
                    object, feature, ..
                } = v
                {
                    failed.push((object.clone(), feature.clone()));
                }
            }
            failed.sort();
            failed.dedup();
            cleared_features.extend(failed.iter().map(|(_, feature)| feature.clone()));
            let mut store = make_store()?;
            let cleared = storage::atomic(&mut store, |store| {
                let mut cleared = 0;
                for (owner, feature) in &failed {
                    if store.set_value(owner, feature, None).is_ok() {
                        cleared += 1;
                    }
                }
                Ok(cleared)
            })?;
            let builder = store.into_builder().map_err(|e| e.to_string())?;
            let relink = |view: &LinkView<'_>,
                          owner: ObjId,
                          feature: FeatureId,
                          spelling: &str|
             -> Result<Option<ObjId>, FrozenError> {
                Ok(match resolvers.candidates(view, owner, feature, spelling) {
                    Ok(c) if c.len() == 1 => c.first().copied(),
                    _ => None,
                })
            };
            match builder.link(&relink) {
                Ok(linked) => (linked, Some(cleared)),
                Err(diagnostics) => {
                    report["relink_violations"] = group_violations(&diagnostics.0);
                    println!("{label}relink after clearing {cleared} references still failed");
                    return Ok(report);
                }
            }
        }
    };
    report["cleared_for_later_phases"] = json!(reduced);
    let clock = Instant::now();
    let linked = match linked.complete_implicit(resolvers) {
        Ok(linked) => linked,
        Err(diagnostics) => {
            report["implicit_violations"] = group_violations(&diagnostics.0);
            println!(
                "{label}implicit elements failed: {} violations",
                diagnostics.0.len()
            );
            return Ok(report);
        }
    };
    let linked = match linked.complete_defaults(resolvers) {
        Ok(linked) => linked,
        Err(diagnostics) => {
            report["defaults_ms"] = json!(ms(clock));
            report["defaults_violations"] = group_violations(&diagnostics.0);
            println!("{label}defaults failed: {} violations", diagnostics.0.len());
            return Ok(report);
        }
    };
    report["defaults_ms"] = json!(ms(clock));
    let clock = Instant::now();
    // LIBRARY_PROBE_DUMP=PATH writes the linked model for the Pilot
    // comparison; the cleared references' features are not bound-checked.
    if let Some(path) = dump {
        let mm = mm.clone();
        // Also the two implicit values no construction rule supplies yet
        // (the comparison reports where they differ from the Pilot).
        let open = ["subsettingFeature", "typeDisjoined"];
        let delegated = |_: ClassId, f: FeatureId| {
            let name = mm.feature_name(f);
            cleared_features.contains(name) || open.contains(&name)
        };
        let model = linked.validate_delegating(Some(&delegated)).map_err(|d| {
            format!(
                "{} violations besides the cleared features: {}",
                d.0.len(),
                group_violations(&d.0)
            )
        })?;
        let exported = model.to_model().map_err(|e| e.to_string())?;
        let json = serde_json::to_vec(&exported).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| format!("{path}: {e}"))?;
        println!("wrote {path}: {} objects", exported.elements.len());
        return Ok(report);
    }
    match linked.validate() {
        Ok(model) => {
            report["validate_ms"] = json!(ms(clock));
            report["valid_objects"] = json!(model.len());
            println!("{label}validate: valid, {} objects", model.len());
        }
        Err(diagnostics) => {
            report["validate_ms"] = json!(ms(clock));
            report["validation_violations"] = group_violations(&diagnostics.0);
            println!("{label}validate: {} violations", diagnostics.0.len());
        }
    }
    Ok(report)
}

/// Link outcomes grouped by owner class.feature and outcome, one example each.
fn group_outcomes(outcomes: &[Outcome]) -> Value {
    let mut groups: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for o in outcomes {
        let outcome = match &o.found {
            Ok(found) if found.len() == 1 => "resolved".to_owned(),
            Ok(found) if found.is_empty() => "unresolved: no candidate".to_owned(),
            Ok(found) => format!("ambiguous: {} candidates", found.len()),
            Err(error) => format!("error: {}", strip_identities(error)),
        };
        let key = (format!("{}.{}", o.class, o.feature), outcome);
        let entry = groups.entry(key).or_insert((0, String::new()));
        entry.0 += 1;
        if entry.1.is_empty() {
            let found = match &o.found {
                Ok(found) => format!("{found:?}"),
                Err(error) => error.clone(),
            };
            entry.1 = format!("{} '{}' -> {found}", o.owner, o.spelling)
                .chars()
                .take(300)
                .collect();
        }
    }
    Value::Array(
        groups
            .into_iter()
            .map(|((feature, outcome), (count, example))| {
                json!({ "feature": feature, "outcome": outcome, "count": count, "example": example })
            })
            .collect(),
    )
}

/// Violations grouped by kind and feature with object identities removed.
fn group_violations(violations: &[Violation]) -> Value {
    let mut groups: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for violation in violations {
        let text = violation.to_string();
        let key = match violation {
            Violation::Multiplicity {
                feature,
                lower,
                upper,
                count,
                ..
            } => format!(
                "{feature}: {count} values outside [{lower},{}]",
                upper.map_or("*".to_owned(), |u| u.to_string())
            ),
            Violation::Unresolved { feature, .. } => format!("{feature}: unresolved reference"),
            _ => strip_identities(&text),
        };
        let entry = groups.entry(key).or_insert((0, String::new()));
        entry.0 += 1;
        if entry.1.is_empty() {
            entry.1 = text.chars().take(300).collect();
        }
    }
    println!(
        "  {} violations in {} groups",
        violations.len(),
        groups.len()
    );
    Value::Array(
        groups
            .into_iter()
            .map(|(kind, (count, example))| json!({ "kind": kind, "count": count, "example": example }))
            .collect(),
    )
}

fn strip_identities(text: &str) -> String {
    text.split_whitespace()
        .map(|w| if w.contains('#') { "<id>" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

/// Prints each KerML scope the direct executor cannot run, with the reason;
/// such a scope runs on the VM, which is much slower and has a storage limit.
fn report_unsupported_scopes(
    mm: &Metamodel,
    resolvers: &intersect_model::frozen_scopes::RcoreScopes<'_>,
) {
    const SCOPED: &[(&str, &str)] = &[
        ("FeatureTyping", "type"),
        ("Subclassification", "superclassifier"),
        ("Subsetting", "subsettedFeature"),
        ("Redefinition", "redefinedFeature"),
        ("FeatureChaining", "chainingFeature"),
        ("MembershipImport", "importedMembership"),
        ("NamespaceImport", "importedNamespace"),
        ("Membership", "memberElement"),
        ("Annotation", "annotatedElement"),
        ("ReferenceSubsetting", "referencedFeature"),
        ("Subsetting", "subsettingFeature"),
    ];
    for (class, name) in SCOPED {
        let found = ["", "SysML::", "sysml::"]
            .iter()
            .find_map(|prefix| mm.class(&format!("{prefix}{class}")))
            .and_then(|c| mm.feature(c, name));
        match found {
            Some(feature) => {
                if let Some(reason) = resolvers.unsupported(feature) {
                    println!("scope {class}.{name} runs on the VM: {reason}");
                }
            }
            None => println!("scope {class}.{name}: class key not found"),
        }
    }
}
