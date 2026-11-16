//! Handwritten measurement host (not generated): load every Pilot example
//! model on the prepared standard library.
//!
//! Usage: `pilot_examples ROOT EXAMPLES [--snapshot FILE] [--report FILE]
//! [--only TEXT]... [--locate] [--diagnostics FILE] [--pack PACK] [--verify-package-load] [--file NAME]... [--user FILE]...`.
//! `ROOT` is the repository root (definitions and library from
//! `fixtures/native-pilot/v1`); `EXAMPLES` is a checkout of the Pilot
//! repository at the pinned commit holding `kerml/src/examples` and
//! `sysml/src/examples` (see tools/pilot-oracle/README.md). Each folder of
//! example files loads as one unit (its files may import each other) on top
//! of the library, in a fresh store. `--snapshot FILE` keeps the prepared
//! library between runs (written after a parse, key-checked when read).
//! Prints one line per folder and writes a JSON report (default
//! `target/pilot-examples.json`). `--only TEXT` (repeatable) loads only the
//! folders whose path contains one of the texts; `--locate` also prints each
//! problem with the file, line, column and line text of the element it names
//! (found by parsing the folder again into an empty store).
//!
//! When `EXAMPLES` holds a `units` folder it is a validation corpus
//! (`tools/pilot-oracle/validation_corpus.py`): each `units/NNN` folder loads
//! as one unit instead. `--diagnostics FILE` writes every diagnostic in the
//! `intersect-diagnostics/1` format that `tools/pilot-oracle/
//! compare_diagnostics.py intersect` reads (resources by `EXAMPLES`-relative
//! path). A folder is clean when no diagnostic is an error (validation-rule
//! warnings and information do not count).
use intersect_kerml::{
    frozen_load::LoadPhase,
    language_pack::{
        LibraryOptions, LibraryOrigin, PreparedLibrary, pack_library_payload, prepare_library,
    },
    native_pilot::NativePilotSession,
    sysml_host::{ProjectLoad, phase_name},
};
use intersect_model::{
    core::frozen::{FrozenStore, SnapshotTrust},
    core::storage::{self, ModelRead, ModelStore},
    definitions::{
        conversion::{InvocationContext, InvocationObserver, ProviderId},
        pack::{FileDigest, LanguagePack},
    },
};
use intersect_text::Source;
use intersect_text::grammar::RuntimeAdmissionLoadMode;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

#[path = "support/sysml_library.rs"]
mod sysml_library;
use sysml_library::{library_sources, ms, read_source, write_atomic};

struct Observer;
impl InvocationObserver for Observer {
    fn before_invoke(&self, _: &ProviderId) -> Result<(), String> {
        Ok(())
    }
}

const DEFINITIONS: [&str; 4] = [
    "org.omg.sysml/model/SysML.ecore",
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
    "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext",
    "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext",
];

/// Every example file under `examples`, grouped by folder (relative paths
/// with forward slashes), folders and files in name order.
fn example_folders(examples: &Path) -> Result<BTreeMap<String, Vec<Source>>, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("kerml" | "sysml")
            ) {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut folders: BTreeMap<String, Vec<Source>> = BTreeMap::new();
    let corpus = examples.join("units");
    let roots = if corpus.is_dir() {
        vec![corpus]
    } else {
        ["kerml", "sysml"]
            .map(|language| examples.join(language).join("src/examples"))
            .to_vec()
    };
    for root in roots {
        let mut files = Vec::new();
        walk(&root, &mut files)?;
        for path in files {
            let relative = path
                .strip_prefix(examples)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let folder = relative
                .rsplit_once('/')
                .map_or(String::new(), |(folder, _)| folder.to_owned());
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            folders.entry(folder).or_default().push(Source {
                uri: relative.replace(' ', "%20"),
                text,
            });
        }
    }
    for sources in folders.values_mut() {
        // KerML files first, as the library loads.
        sources.sort_by(|a, b| {
            (!a.uri.ends_with(".kerml"), &a.uri).cmp(&(!b.uri.ends_with(".kerml"), &b.uri))
        });
    }
    Ok(folders)
}

fn run() -> Result<(), String> {
    run_with_args(std::env::args().skip(1))
}

pub(crate) fn run_with_args(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter();
    let usage = "usage: pilot_examples ROOT EXAMPLES [--snapshot FILE] [--report FILE] \
                 [--only TEXT]... [--locate] [--diagnostics FILE] [--pack PACK] [--verify-package-load] [--file NAME]... [--user FILE]...";
    let root = PathBuf::from(args.next().ok_or(usage)?);
    let examples = PathBuf::from(args.next().ok_or(usage)?);
    let mut snapshot = None;
    let mut pack_path: Option<PathBuf> = None;
    let mut verify_package_load = false;
    let mut file_filters = Vec::new();
    let mut user_paths: Vec<PathBuf> = Vec::new();
    let mut report_path = PathBuf::from("target/pilot-examples.json");
    let mut only = Vec::new();
    let mut locate = false;
    let mut diagnostics_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--snapshot" => snapshot = Some(PathBuf::from(args.next().ok_or(usage)?)),
            "--pack" => pack_path = Some(args.next().ok_or(usage)?.into()),
            "--verify-package-load" => verify_package_load = true,
            "--file" => file_filters.push(args.next().ok_or(usage)?),
            "--user" => user_paths.push(args.next().ok_or(usage)?.into()),
            "--report" => report_path = args.next().ok_or(usage)?.into(),
            "--only" => only.push(args.next().ok_or(usage)?),
            "--locate" => locate = true,
            "--diagnostics" => diagnostics_path = Some(PathBuf::from(args.next().ok_or(usage)?)),
            other => return Err(format!("unknown argument {other}\n{usage}")),
        }
    }
    let mut folders = example_folders(&examples)?;
    if !only.is_empty() {
        folders.retain(|folder, _| only.iter().any(|text| folder.contains(text.as_str())));
    }
    if !file_filters.is_empty() {
        for sources in folders.values_mut() {
            sources.retain(|source| {
                file_filters
                    .iter()
                    .any(|filter| source.uri.rsplit('/').next() == Some(filter.as_str()))
            });
        }
        folders.retain(|_, sources| !sources.is_empty());
    }
    for path in &user_paths {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid user fixture filename")?;
        let uri = format!("user-fixed/{name}").replace(' ', "%20");
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if folders
            .insert(format!("user-fixed/{name}"), vec![Source { uri, text }])
            .is_some()
        {
            return Err(format!("Repeated user fixture: {name}"));
        }
    }
    if folders.is_empty() {
        return Err("No example files selected".into());
    }
    let inputs: BTreeMap<_, _> = folders
        .values()
        .flatten()
        .map(|source| {
            (
                source.uri.clone(),
                FileDigest::of(source.text.as_bytes()).sha256,
            )
        })
        .collect();
    let files: usize = folders.values().map(Vec::len).sum();
    eprintln!("{files} example files in {} folders", folders.len());

    let definitions = DEFINITIONS
        .iter()
        .map(|p| read_source(&root, p))
        .collect::<Result<Vec<_>, _>>()?;
    let library = library_sources(&root)?;
    let preparation = InvocationContext::new(Rc::new(Observer));
    let clock = Instant::now();
    let pack = pack_path
        .as_ref()
        .map(|path| {
            std::fs::read(path)
                .map_err(|e| format!("{}: {e}", path.display()))
                .and_then(|bytes| LanguagePack::read(bytes).map_err(|e| e.to_string()))
        })
        .transpose()?;
    let session = match &pack {
        Some(pack) => NativePilotSession::from_pack(pack, RuntimeAdmissionLoadMode::HostCache)?,
        None => NativePilotSession::prepare(&definitions, &preparation)?,
    };
    eprintln!("prepared definitions in {:.1} s", ms(clock) / 1e3);

    let body = |host: &sysml_library::LibraryHost<'_>| {
        let clock = Instant::now();
        let saved = snapshot
            .as_ref()
            .filter(|path| path.exists())
            .map(|path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
            .transpose()?;
        let pack_payload = pack.as_ref().and_then(pack_library_payload).transpose()?;
        let prepared = prepare_library(
            saved.as_deref().or(pack_payload),
            Some(&library),
            host.context(),
            LibraryOptions {
                use_snapshot: true,
                trust: SnapshotTrust::HostCache,
            },
            |sources| host.parse_library(sources).map(|(model, _, _)| model),
        )?;
        match &prepared.origin {
            LibraryOrigin::Snapshot => eprintln!("library snapshot loaded"),
            LibraryOrigin::Parsed(reason) => eprintln!("library parsed ({reason})"),
        }
        eprintln!("library ready in {:.1} s", ms(clock) / 1e3);
        if let (Some(path), Some(payload)) = (&snapshot, &prepared.payload) {
            write_atomic(path, payload)?;
        }

        let mut rows = Vec::new();
        let mut verified = 0usize;
        let mut equivalence_failed = 0usize;
        let mut totals = BTreeMap::<&str, usize>::new();
        let mut by_resource = BTreeMap::<String, Vec<serde_json::Value>>::new();
        for (folder, sources) in &folders {
            let clock = Instant::now();
            let outcome = host.load_project(&prepared, sources).and_then(|project| {
                if verify_package_load {
                    if let Err(error) =
                        verify_package_equivalence(host, &prepared, sources, &project)
                    {
                        equivalence_failed += 1;
                        return Err(error);
                    }
                    verified += 1;
                }
                Ok((project.load, sysml_library::LoadTimes::default()))
            });
            let elapsed = ms(clock);
            let row = match outcome {
                Ok((load, _)) => {
                    let mut phases = BTreeMap::<&str, usize>::new();
                    for diagnostic in &load.diagnostics {
                        *phases.entry(phase_name(diagnostic.phase)).or_default() += 1;
                    }
                    let clean = load.is_clean();
                    for source in sources {
                        by_resource
                            .entry(source.uri.replace("%20", " "))
                            .or_default();
                    }
                    for diagnostic in &load.diagnostics {
                        let Some(resource) = &diagnostic.resource else {
                            continue;
                        };
                        let class = diagnostic
                            .object
                            .as_deref()
                            .and_then(|id| load.model.as_ref()?.lookup(id))
                            .map(|object| {
                                let key = object.class_key();
                                key.rsplit(['.', '/', ':']).next().unwrap_or(key).to_owned()
                            });
                        by_resource
                            .entry(resource.replace("%20", " "))
                            .or_default()
                            .push(json!({
                                "severity": diagnostic.severity,
                                "code": diagnostic.code,
                                "rule": diagnostic.rule,
                                "feature": diagnostic.feature,
                                "message": diagnostic.message,
                                "phase": phase_name(diagnostic.phase),
                                "element": {"id": diagnostic.object, "class": class},
                                "start": diagnostic.start,
                                "end": diagnostic.end,
                            }));
                    }
                    *totals
                        .entry(if clean { "clean" } else { "problems" })
                        .or_default() += 1;
                    if locate && !load.diagnostics.is_empty() {
                        print_located(host, sources, &load.diagnostics);
                    }
                    let parse_failures = load
                        .diagnostics
                        .iter()
                        .filter(|d| d.phase == LoadPhase::Parse)
                        .count();
                    println!(
                        "{:>8.0} ms  {:>3} files  {:>5} refs {:>5} resolved  {:>4} problems {:?}  {folder}",
                        elapsed,
                        sources.len(),
                        load.references,
                        load.resolved,
                        load.diagnostics.len(),
                        phases,
                    );
                    json!({
                        "folder": folder,
                        "files": sources.iter().map(|s| s.uri.clone()).collect::<Vec<_>>(),
                        "ms": elapsed,
                        "clean": clean,
                        "loaded": load.model.is_some(),
                        "objects": load.model.as_ref().map(|m| m.len() - prepared.model.len()),
                        "references": load.references,
                        "resolved": load.resolved,
                        "parse_problems": parse_failures,
                        "problems_by_phase": phases,
                        "problems": load.diagnostics,
                    })
                }
                Err(error) => {
                    *totals.entry("failed").or_default() += 1;
                    println!("{elapsed:>8.0} ms  FAILED  {folder}: {error}");
                    json!({ "folder": folder, "ms": elapsed, "error": error })
                }
            };
            rows.push(row);
        }
        eprintln!("folders: {totals:?}");
        let report = json!({
            "files": files,
            "folders": rows,
            "totals": totals,
            "inputs": inputs,
            "library_identity": &prepared.identity,
            "library_origin": format!("{:?}", prepared.origin),
            "pack": pack_path,
            "package_equivalence": {"enabled": verify_package_load, "verified_folders": verified, "failed": equivalence_failed,
                "normalization": "resource.package only", "baseline": "pre-slice FrozenStore::from_model + host.load"},
        });
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        write_atomic(&report_path, text.as_bytes())?;
        eprintln!("wrote {}", report_path.display());
        if let Some(path) = &diagnostics_path {
            let diagnostics = json!({
                "format": "intersect-diagnostics/1",
                "producer": "intersect-kerml pilot_examples",
                "resources": by_resource,
            });
            let text = serde_json::to_string_pretty(&diagnostics).map_err(|e| e.to_string())?;
            write_atomic(path, text.as_bytes())?;
            eprintln!("wrote {}", path.display());
        }
        if verify_package_load && (equivalence_failed != 0 || verified != folders.len()) {
            return Err(format!(
                "Package equivalence incomplete: {verified}/{} folders verified, {equivalence_failed} mismatches",
                folders.len()
            ));
        }
        Ok(())
    };
    match &pack {
        Some(pack) => {
            sysml_library::with_library_host_from_pack(&session, pack, &preparation, body)
        }
        None => sysml_library::with_library_host(&session, &preparation, body),
    }
}

/// Independent pre-slice host-body baseline. Only the intentionally added
/// package metadata is normalized; all object data, other resource metadata,
/// reference counts and located diagnostics must agree exactly.
fn verify_package_equivalence(
    host: &sysml_library::LibraryHost<'_>,
    library: &PreparedLibrary,
    sources: &[Source],
    project: &ProjectLoad,
) -> Result<(), String> {
    let entry = project
        .lock
        .entries()
        .get(intersect_kerml::standard_library::IMPLICIT_STDLIB_URI)
        .ok_or("Missing implicit standard-library lock entry")?;
    if entry.identity != library.identity || !entry.direct {
        return Err("Implicit standard-library lock identity/direct flag differs".into());
    }
    let mut baseline_store = FrozenStore::from_model(library.model.clone());
    storage::atomic(&mut baseline_store, |store| {
        for mut record in store.resource_records()?.into_values() {
            record.package = None;
            store.set_resource_record(record)?;
        }
        Ok(())
    })?;
    let (baseline, _) = host.load(baseline_store, sources)?;
    let candidate = &project.load;
    if (baseline.references, baseline.resolved) != (candidate.references, candidate.resolved)
        || serde_json::to_value(&baseline.diagnostics).map_err(|e| e.to_string())?
            != serde_json::to_value(&candidate.diagnostics).map_err(|e| e.to_string())?
    {
        return Err("Reference counts or diagnostics differ from the pre-slice load".into());
    }
    match (&baseline.model, &candidate.model) {
        (None, None) => Ok(()),
        (Some(baseline), Some(candidate)) => {
            let mut before = baseline.to_model().map_err(|e| e.to_string())?;
            let mut after = candidate.to_model().map_err(|e| e.to_string())?;
            let library_resources = library
                .model
                .resource_records()
                .map_err(|e| e.to_string())?;
            for (uri, record) in &after.resources {
                let expected = library_resources
                    .contains_key(uri)
                    .then_some(&library.identity);
                if record.package.as_ref() != expected {
                    return Err(format!("Unexpected resource package origin: {uri}"));
                }
            }
            for record in before
                .resources
                .values_mut()
                .chain(after.resources.values_mut())
            {
                record.package = None;
            }
            if before != after {
                return Err("Model or resource data differs from the pre-slice load".into());
            }
            Ok(())
        }
        _ => Err("Model availability differs from the pre-slice load".into()),
    }
}

/// Print each diagnostic with the place of the element it names: the
/// folder's sources are parsed again into an empty store (identities are
/// per resource, so they match the load's) and exported for the spans.
fn print_located(
    host: &sysml_library::LibraryHost<'_>,
    sources: &[Source],
    diagnostics: &[intersect_kerml::frozen_load::LoadDiagnostic],
) {
    let mut store = FrozenStore::new(host.metamodel.clone());
    for source in sources {
        let _ = host.parse(source, &mut store);
    }
    let elements = match intersect_model::core::storage::export(&store) {
        Ok(model) => model.elements,
        Err(error) => {
            println!("    (cannot locate: {error})");
            Vec::new()
        }
    };
    let located: BTreeMap<&str, (String, usize)> = elements
        .iter()
        .map(|element| {
            (
                element.id.as_str(),
                (element.resource.clone(), element.start),
            )
        })
        .collect();
    // Containment parents and classes, to print where an element sits.
    let class: BTreeMap<&str, &str> = elements
        .iter()
        .map(|element| (element.id.as_str(), element.class.as_str()))
        .collect();
    let mut parent = BTreeMap::<&str, &str>::new();
    for element in &elements {
        for key in ["ownedRelationship", "ownedRelatedElement"] {
            if let Some(children) = element.values.get(key).and_then(|v| v.as_array()) {
                for child in children.iter().filter_map(|c| c.as_str()) {
                    parent.insert(child, element.id.as_str());
                }
            }
        }
    }
    let path = |id: &str| -> String {
        let mut steps = Vec::new();
        let mut current = Some(id);
        while let Some(at) = current {
            if steps.len() >= 8 {
                break;
            }
            let name = class.get(at).copied().unwrap_or("?");
            steps.push(
                name.rsplit(['.', '/', ':'])
                    .next()
                    .unwrap_or(name)
                    .to_owned(),
            );
            current = parent.get(at).copied();
        }
        steps.join(" < ")
    };
    for diagnostic in diagnostics {
        let place = diagnostic
            .object
            .as_ref()
            .and_then(|id| located.get(id.as_str()))
            .map(|(resource, start)| (resource.clone(), *start))
            .or_else(|| diagnostic.resource.clone().zip(diagnostic.start));
        let Some((resource, start)) = place else {
            println!("    ?: {}", diagnostic.message);
            continue;
        };
        let text = sources
            .iter()
            .find(|s| s.uri == resource)
            .map_or("", |s| s.text.as_str());
        let (line, column) = intersect_kerml::sysml_host::line_column(text, start);
        let at = start.min(text.len());
        let from = text
            .get(..at)
            .and_then(|before| before.rfind('\n'))
            .map_or(0, |i| i + 1);
        let to = text
            .get(at..)
            .and_then(|after| after.find('\n'))
            .map_or(text.len(), |i| at + i);
        let line_text = text.get(from..to).unwrap_or("").trim();
        let file = resource.rsplit('/').next().unwrap_or(&resource);
        let within = diagnostic.object.as_deref().map(path).unwrap_or_default();
        println!(
            "    {file}:{line}:{column}: {}\n        | {line_text}\n        in {within}",
            diagnostic.message
        );
        // PILOT_EXAMPLES_DUMP=1: the element, its ancestors and their
        // children, with their stored values (a debugging aid).
        if std::env::var_os("PILOT_EXAMPLES_DUMP").is_some() {
            let by_id: BTreeMap<&str, &intersect_model::core::model::Element> =
                elements.iter().map(|e| (e.id.as_str(), e)).collect();
            let mut current = diagnostic.object.as_deref();
            for _ in 0..5 {
                let Some(at) = current else { break };
                if let Some(element) = by_id.get(at) {
                    let values = serde_json::to_string(&element.values).unwrap_or_default();
                    println!(
                        "          {at} {}: {}",
                        element.class,
                        values.chars().take(400).collect::<String>()
                    );
                }
                current = parent.get(at).copied();
            }
        }
    }
}

fn main() -> Result<(), String> {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "panicked".to_string())?
}
