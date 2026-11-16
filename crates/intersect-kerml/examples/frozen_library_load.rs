//! Handwritten measurement host (not generated): load the whole SysML v2
//! standard library (every `.kerml` and `.sysml` file under
//! `fixtures/native-pilot/v1/sources/sysml.library`) through the frozen
//! engine in one process, for the speed and memory comparison with the Pilot
//! (`tools/perf_vs_pilot.py`, profile `intersect-library`), or get it as a
//! host would, from the prepared-library snapshot of decision D10.
//!
//! 1. Setup: prepare the KerML and SysML definitions (`NativePilotSession`,
//!    or `from_pack` with `--pack`), freeze the metamodel, load the combined
//!    KerML+SysML Rcore bindings from the saved binding cache as a language
//!    pack would (`support/sysml_scopes.rs`, `compile_cached`: compiled and
//!    saved only when no matching cache exists), and bind both languages
//!    (`support/sysml_library.rs`).
//! 2. Without a snapshot: parse the KerML files with the KerML language, then
//!    the SysML files with the SysML language, in sorted order, all into one
//!    `FrozenStore` of the one metamodel, single-threaded; link the stored
//!    cross references with the bindings, complete implicit elements, then
//!    defaults, and validate (`frozen_load::load_frozen`).
//! 3. With a snapshot (`--pack PACK` holding a `library/sysml` part, or
//!    `--library-snapshot PATH`): `language_pack::prepare_library` loads it
//!    when its key equals the key recomputed here, otherwise parses (2) and,
//!    when parsed, saves the new payload to PATH or into PACK.
//!
//! Options (debug only, decision D10):
//! - `--pack PACK`: load the programs and the library snapshot from PACK.
//! - `--library-snapshot PATH`: use the library part payload in PATH instead
//!   of the pack's (still key-checked); written there after a parse.
//! - `--no-library-cache`: ignore any snapshot and parse.
//! - `--read-only`: never write a pack or snapshot file.
//! - `--revalidate`: rerun structural validation on a loaded snapshot.
//! - `--repeat N`: after the library is ready, encode and load its snapshot
//!   N more times and report each time and the median.
//! - `--user FILE` (repeatable, relative to the corpus): parse, link and
//!   complete these files on top of the prepared library and report them.
//!
//! Prints one JSON object on stdout with `timing` (milliseconds), the
//! counts, where the library came from and the snapshot size. Exits with an
//! error after printing when a file does not parse cleanly, a reference does
//! not resolve uniquely, or the model is not valid.
//!
//! Usage: `frozen_library_load [ROOT] [OPTIONS]` (ROOT defaults to `.`). The
//! binding cache is `SYSML_BINDING_CACHE`, or `target/sysml-bindings.json`
//! under the current directory. With `FROZEN_LOAD_MODEL_OUT=PATH`, the
//! validated library model is also written to `PATH` as JSON. With
//! `FROZEN_LOAD_USER_XMI=DIR`, the `--user` resources are exported as XMI
//! into `DIR` (one file per resource, at its URI), decoded again on the
//! library and compared (`user.xmi` in the report); a difference is an error.
use intersect_kerml::{
    language_pack::{
        LibraryOptions, LibraryOrigin, pack_library_payload, prepare_library, with_library_part,
    },
    native_pilot::NativePilotSession,
};
use intersect_model::core::frozen::FrozenStore;
use intersect_model::core::frozen::Model;
use intersect_model::core::frozen::SnapshotTrust;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_model::definitions::conversion::InvocationObserver;
use intersect_model::definitions::conversion::ProviderId;
use intersect_model::definitions::library_part::decode_library_part;
use intersect_model::definitions::library_part::key_bytes;
use intersect_model::definitions::pack::LanguagePack;
use intersect_text::grammar::RuntimeAdmissionLoadMode;
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

#[path = "support/sysml_library.rs"]
mod sysml_library;
use sysml_library::{LoadTimes, ms, read_source, write_atomic};

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

#[derive(Default)]
struct Options {
    root: PathBuf,
    pack: Option<PathBuf>,
    snapshot: Option<PathBuf>,
    no_cache: bool,
    read_only: bool,
    revalidate: bool,
    repeat: usize,
    user: Vec<String>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        root: ".".into(),
        ..Options::default()
    };
    let mut args = std::env::args().skip(1);
    let mut root = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--pack" => options.pack = Some(value("--pack")?.into()),
            "--library-snapshot" => options.snapshot = Some(value("--library-snapshot")?.into()),
            "--no-library-cache" => options.no_cache = true,
            "--read-only" => options.read_only = true,
            "--revalidate" => options.revalidate = true,
            "--repeat" => {
                options.repeat = value("--repeat")?
                    .parse()
                    .map_err(|_| "--repeat needs a count")?
            }
            "--user" => options.user.push(value("--user")?),
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other if root.is_none() => root = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    if let Some(root) = root {
        options.root = root;
    }
    Ok(options)
}

fn main() -> Result<(), String> {
    // Deep library expressions recurse deeply; run on a thread with a stack
    // the size the build environment gives every other thread.
    let worker = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .map_err(|e| e.to_string())?;
    worker
        .join()
        .map_err(|_| "library load panicked".to_owned())?
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(f64::NAN)
}

/// The phase times, flat, as `tools/perf_vs_pilot.py` reads them.
fn load_times(times: &LoadTimes) -> serde_json::Value {
    json!({
        "parse_kerml_ms": times.parse_kerml_ms,
        "parse_sysml_ms": times.parse_sysml_ms,
        "link_ms": times.link_ms,
        "implicit_ms": times.implicit_ms,
        "defaults_ms": times.defaults_ms,
        "validate_ms": times.validate_ms,
    })
}

fn run() -> Result<(), String> {
    let total = Instant::now();
    let options = options()?;
    let root = &options.root;

    let clock = Instant::now();
    let definitions = DEFINITIONS
        .iter()
        .map(|p| read_source(root, p))
        .collect::<Result<Vec<_>, _>>()?;
    let sources = sysml_library::library_sources(root)?;
    let user = options
        .user
        .iter()
        .map(|p| read_source(root, p))
        .collect::<Result<Vec<_>, _>>()?;
    let pack_bytes = options
        .pack
        .as_ref()
        .map(|path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display())))
        .transpose()?;
    let snapshot_file = match &options.snapshot {
        Some(path) if path.exists() => {
            Some(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
        }
        _ => None,
    };
    let read_ms = ms(clock);

    let clock = Instant::now();
    let pack = pack_bytes
        .map(LanguagePack::read)
        .transpose()
        .map_err(|e| e.to_string())?;
    let pack_read_ms = ms(clock);
    let clock = Instant::now();
    let preparation = InvocationContext::new(Rc::new(Observer));
    let session = match &pack {
        Some(pack) => NativePilotSession::from_pack(pack, RuntimeAdmissionLoadMode::HostCache)?,
        None => NativePilotSession::prepare(&definitions, &preparation)?,
    };
    let prepare_ms = ms(clock);

    sysml_library::with_library_host(&session, &preparation, |host| {
        let payload: Option<&[u8]> = match (&snapshot_file, &pack) {
            (Some(bytes), _) => Some(bytes),
            (None, Some(pack)) => match pack_library_payload(pack) {
                Some(Ok(bytes)) => Some(bytes),
                Some(Err(error)) => {
                    eprintln!("library part refused: {error}");
                    None
                }
                None => None,
            },
            (None, None) => None,
        };
        let parse_times = RefCell::new(None::<LoadTimes>);
        let references = RefCell::new((0usize, 0usize));
        let clock = Instant::now();
        let prepared = prepare_library(
            payload,
            Some(&sources),
            host.context(),
            LibraryOptions {
                use_snapshot: !options.no_cache,
                trust: if options.revalidate {
                    SnapshotTrust::Revalidate
                } else {
                    SnapshotTrust::HostCache
                },
            },
            |sources| {
                let (model, load, times) = host.parse_library(sources)?;
                *parse_times.borrow_mut() = Some(times);
                *references.borrow_mut() = (load.references, load.resolved);
                Ok(model)
            },
        )?;
        let prepare_library_ms = ms(clock);
        let parse_times = parse_times.into_inner();
        let (references, resolved) = references.into_inner();

        // Save what a parse produced, when this host may write.
        let mut write_ms = None;
        let mut saved = Vec::new();
        if let (Some(payload), false) = (&prepared.payload, options.read_only) {
            let clock = Instant::now();
            if let Some(path) = &options.snapshot {
                write_atomic(path, payload)?;
                saved.push(path.display().to_string());
            } else if let (Some(pack), Some(path)) = (&pack, &options.pack) {
                let bytes = with_library_part(pack, payload.clone())?;
                write_atomic(path, &bytes)?;
                saved.push(path.display().to_string());
            }
            write_ms = Some(ms(clock));
        }

        // Snapshot encode and load times of this library, N more times.
        let key = key_bytes(&prepared.key).map_err(|e| e.to_string())?;
        let mut encode_ms = Vec::new();
        let mut load_ms = Vec::new();
        let mut snapshot = Vec::new();
        for _ in 0..options.repeat {
            let clock = Instant::now();
            let bytes = prepared
                .model
                .to_snapshot(&key)
                .map_err(|e| e.to_string())?;
            encode_ms.push(ms(clock));
            let clock = Instant::now();
            let loaded = Model::from_snapshot(
                host.metamodel.clone(),
                &bytes,
                &key,
                SnapshotTrust::HostCache,
            )
            .map_err(|e| e.to_string())?;
            load_ms.push(ms(clock));
            if loaded.len() != prepared.model.len() {
                return Err("snapshot reload lost objects".into());
            }
            if !snapshot.is_empty() && snapshot != bytes {
                return Err("two encodings of the library differ".into());
            }
            snapshot = bytes;
        }
        let payload_bytes = match (&prepared.payload, payload) {
            (Some(new), _) => Some(new.len()),
            (None, Some(old)) => Some(old.len()),
            (None, None) => None,
        };
        let snapshot_bytes = match (&prepared.payload, payload) {
            (Some(new), _) => decode_library_part(new).ok().map(|p| p.snapshot.len()),
            (None, Some(old)) => decode_library_part(old).ok().map(|p| p.snapshot.len()),
            (None, None) => None,
        };

        // Optional equivalence evidence: the whole validated library model,
        // materialized, as JSON, so two builds can be compared byte for byte.
        if let Ok(path) = std::env::var("FROZEN_LOAD_MODEL_OUT") {
            let materialized = prepared.model.to_model().map_err(|e| e.to_string())?;
            let bytes = serde_json::to_vec(&materialized).map_err(|e| e.to_string())?;
            std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))?;
        }

        // User files on top of the prepared library.
        let mut user_report = serde_json::Value::Null;
        if !user.is_empty() {
            let clock = Instant::now();
            let store = FrozenStore::from_model(prepared.model.clone());
            let (load, times) = host.load(store, &user)?;
            user_report = json!({
                "files": user.iter().map(|s| s.uri.clone()).collect::<Vec<_>>(),
                "total_ms": ms(clock),
                "timing": load_times(&times),
                "objects": load.model.as_ref().map(Model::len),
                "references": load.references,
                "resolved": load.resolved,
                "problems": load.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>(),
            });
            if load.model.is_none() || load.resolved != load.references {
                println!("{user_report}");
                return Err("user files did not load cleanly on the prepared library".into());
            }
            // Optional XMI round trip of the user resources.
            if let (Ok(dir), Some(model), Some(report)) = (
                std::env::var("FROZEN_LOAD_USER_XMI"),
                load.model.as_ref(),
                user_report.as_object_mut(),
            ) {
                let uris: Vec<String> = user.iter().map(|s| s.uri.clone()).collect();
                let xmi = xmi_round_trip(host, &prepared.model, model, &uris, &dir)?;
                let failed = xmi["mismatches"].as_u64() != Some(0);
                report.insert("xmi".into(), xmi);
                if failed {
                    println!("{user_report}");
                    return Err("the XMI round trip of the user files differs".into());
                }
            }
        }

        let setup = host.setup;
        let parsed = parse_times.unwrap_or_default();
        let mut timing = json!({
                "read_ms": read_ms,
                "pack_read_ms": pack_read_ms,
                "prepare_ms": prepare_ms,
                "metamodel_ms": setup.metamodel_ms,
                "bindings_ms": setup.bindings_ms,
                "bind_languages_ms": setup.bind_languages_ms,
                "setup_ms": prepare_ms + setup.metamodel_ms + setup.bindings_ms + setup.bind_languages_ms,
                "prepare_library_ms": prepare_library_ms,
                "write_ms": write_ms,
                "total_ms": ms(total),
        });
        // `timing` holds only measured numbers (tools/perf_vs_pilot.py
        // rejects anything else); absent phases are left out.
        if let Some(timing) = timing.as_object_mut() {
            if matches!(prepared.origin, LibraryOrigin::Snapshot) {
                timing.insert("snapshot_ms".into(), json!(prepare_library_ms));
            }
            if !encode_ms.is_empty() {
                timing.insert(
                    "snapshot_encode_median_ms".into(),
                    json!(median(&mut encode_ms.clone())),
                );
            }
            if !load_ms.is_empty() {
                timing.insert(
                    "snapshot_load_median_ms".into(),
                    json!(median(&mut load_ms.clone())),
                );
            }
        }
        if let (Some(timing), serde_json::Value::Object(phases)) =
            (timing.as_object_mut(), load_times(&parsed))
        {
            timing.extend(phases);
        }
        let report = json!({
            "timing": timing,
            "snapshot_runs": json!({"encode_ms": encode_ms, "load_ms": load_ms}),
            "origin": match &prepared.origin {
                LibraryOrigin::Snapshot => "snapshot".to_owned(),
                LibraryOrigin::Parsed(reason) => format!("parsed: {reason}"),
            },
            "key": prepared.key,
            "saved": saved,
            "payload_bytes": payload_bytes,
            "snapshot_bytes": snapshot_bytes,
            "bindings_cached": host.bindings_cached,
            "library_files": sources.len(),
            "objects": prepared.model.len(),
            "references": references,
            "resolved": resolved,
            "valid": true,
            "user": user_report,
        });
        println!("{report}");
        Ok(())
    })
}

/// `FROZEN_LOAD_USER_XMI=DIR`: export the user resources of `model` as XMI
/// (`sysml_host::export`), write each to `DIR/<uri>`, decode them again on
/// top of `library` (`XmiFormat::decode_frozen`) and compare the decoded
/// model with `model`: the same objects, classes, resources, spans and
/// implicit marks, every stored feature value (an absent value reads as the
/// feature's default; null and an empty list are both empty) and the
/// resource roots. A redefined attribute and its stored redefining one count
/// as one value (`moved_to_redefining` lists values that changed slot).
/// Returns sizes, times, the number of references from the user resources
/// into the library and the mismatches.
fn xmi_round_trip(
    host: &sysml_library::LibraryHost<'_>,
    library: &Model,
    model: &Model,
    uris: &[String],
    dir: &str,
) -> Result<serde_json::Value, String> {
    use intersect_kerml::sysml_host::{ExportFormat, export};
    use intersect_model::core::model::{Element, Model as Materialized};
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    let clock = Instant::now();
    let resources = export(model, uris, ExportFormat::Xmi)?;
    let encode_ms = ms(clock);
    let mut files = Vec::new();
    let mut external_hrefs = 0usize;
    for resource in &resources {
        let path = std::path::Path::new(dir).join(&resource.uri);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &resource.bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let text = String::from_utf8_lossy(&resource.bytes);
        let external = text.matches(" href=\"").count() - text.matches(" href=\"#").count();
        external_hrefs += external;
        files.push(json!({
            "uri": resource.uri,
            "bytes": resource.bytes.len(),
            "external_hrefs": external,
        }));
    }
    let clock = Instant::now();
    let decoded = intersect_model::xmi::XmiFormat.decode_frozen(
        &host.metamodel,
        &resources,
        Some(library),
    )?;
    let decode_ms = ms(clock);

    let schema = host.metamodel.schema();
    // The frozen-engine parts of both times: materializing the whole model
    // (export and decode each materialize) and importing it back (decode).
    let clock = Instant::now();
    let before = model.to_model().map_err(|e| e.to_string())?;
    let materialize_ms = ms(clock);
    let clock = Instant::now();
    Model::from_model(host.metamodel.clone(), &before).map_err(|e| e.to_string())?;
    let frozen_import_ms = ms(clock);
    let after = decoded.to_model().map_err(|e| e.to_string())?;
    let selected: BTreeSet<&str> = uris.iter().map(String::as_str).collect();
    let by_id = |m: &Materialized| -> BTreeMap<String, usize> {
        m.elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.clone(), i))
            .collect()
    };
    let (ids_before, ids_after) = (by_id(&before), by_id(&after));
    let mut mismatches = Vec::new();
    if ids_before.len() != ids_after.len() {
        mismatches.push(format!(
            "object count {} before, {} after",
            ids_before.len(),
            ids_after.len()
        ));
    }
    let mm = &host.metamodel;
    let mut moved = Vec::new();
    let mut user_objects = 0usize;
    let mut library_refs = 0usize;
    for element in &before.elements {
        let Some(&index) = ids_after.get(&element.id) else {
            mismatches.push(format!("{} missing after", element.id));
            continue;
        };
        let other = &after.elements[index];
        if !selected.contains(element.resource.as_str()) {
            continue;
        }
        user_objects += 1;
        let header = |e: &Element| {
            (
                e.class.clone(),
                e.resource.clone(),
                e.start,
                e.end,
                e.implicit,
            )
        };
        if header(element) != header(other) {
            mismatches.push(format!("{}: class, resource, span or implicit", element.id));
        }
        let features = schema.features(&element.class)?;
        // A redefined attribute and its stored redefining attribute are one
        // value (frozen reads of either take the redefining slot; XMI writes
        // it once, under the redefining name): compare the first of the pair
        // that holds a value, and count values that moved between the two.
        let class = mm.class(&element.class).ok_or("unknown class")?;
        let partners = |name: &str| -> Vec<String> {
            let Some(feature) = mm.feature(class, name) else {
                return Vec::new();
            };
            let mut names = vec![name.to_owned()];
            if let Some(redefining) = mm.redefined_by(class, feature) {
                names.insert(0, mm.feature_name(redefining).to_owned());
            }
            for other in features.keys() {
                if mm
                    .feature(class, other)
                    .and_then(|o| mm.redefined_by(class, o))
                    == Some(feature)
                {
                    names.push(other.clone());
                }
            }
            names.retain(|n| features.get(n).is_some_and(|f| !f.derived && !f.reference));
            names
        };
        for (name, feature) in &features {
            if feature.derived {
                continue;
            }
            let raw = |e: &Element, name: &str| {
                let default = features.get(name).map_or(&Value::Null, |f| &f.default);
                let v = e.values.get(name).unwrap_or(default);
                if v.is_null() || v.as_array().is_some_and(Vec::is_empty) {
                    Value::Null
                } else {
                    v.clone()
                }
            };
            let group = if feature.reference {
                Vec::new()
            } else {
                partners(name)
            };
            let value = |e: &Element| {
                group
                    .iter()
                    .map(|n| raw(e, n))
                    .find(|v| !v.is_null())
                    .unwrap_or_else(|| raw(e, name))
            };
            let (a, b) = (value(element), value(other));
            if a != b {
                mismatches.push(format!("{}.{name}: {a} before, {b} after", element.id));
            } else if raw(element, name) != raw(other, name) {
                moved.push(format!(
                    "{}.{name}: {} before, {} after",
                    element.id,
                    raw(element, name),
                    raw(other, name)
                ));
            }
            if feature.reference && !feature.containment {
                let targets: Vec<&str> = match &a {
                    Value::String(id) => vec![id.as_str()],
                    Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                    _ => Vec::new(),
                };
                library_refs += targets
                    .iter()
                    .filter(|id| {
                        ids_before.get(**id).is_some_and(|i| {
                            !selected.contains(before.elements[*i].resource.as_str())
                        })
                    })
                    .count();
            }
        }
    }
    for uri in uris {
        let roots = |m: &Materialized| m.resources.get(uri).map(|r| r.roots.as_ref().clone());
        if roots(&before) != roots(&after) {
            mismatches.push(format!("{uri}: resource roots"));
        }
    }
    Ok(json!({
        "files": files,
        "encode_ms": encode_ms,
        "decode_ms": decode_ms,
        "materialize_ms": materialize_ms,
        "frozen_import_ms": frozen_import_ms,
        "user_objects": user_objects,
        "objects_after": decoded.len(),
        "library_refs": library_refs,
        "external_hrefs": external_hrefs,
        "mismatches": mismatches.len(),
        "first_mismatches": mismatches.iter().take(20).collect::<Vec<_>>(),
        "moved_to_redefining": moved,
    }))
}
