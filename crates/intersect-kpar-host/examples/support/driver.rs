//! Manual shell and executable milestone controls over the public session API.
use super::usage::{PreparedUsage, UsageStore};
use intersect_kerml::components::SysmlComponentLanguage;
use intersect_kpar_host::hosted::{self, FetchLimits};
use intersect_model::{
    components::MountedModel,
    storage::{ModelRead, ReferenceView},
};
use intersect_text::{Source, components::ComponentSession};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const URI: &str = "memory:/open-project.sysml";
const LIBRARY: &str = "urn:example:motors";
const SAVED: &str = "package Drone {}\r\n";
const EDIT: &str =
    "// unsaved α; preserved byte for byte\r\npackage Drone { part engine : Motors::Motor; }\r\n";
const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/motors.kpar");

struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    gate: Arc<Mutex<Option<Gate>>>,
}
struct Gate {
    started: mpsc::Sender<()>,
    resume: mpsc::Receiver<()>,
}
impl Server {
    fn pause_next_download(&self) -> Result<(mpsc::Receiver<()>, mpsc::Sender<()>), String> {
        let (started, observed) = mpsc::channel();
        let (resume, waiting) = mpsc::channel();
        *self.gate.lock().map_err(|_| "HTTP gate lock poisoned")? = Some(Gate {
            started,
            resume: waiting,
        });
        Ok((observed, resume))
    }
    fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let gate: Arc<Mutex<Option<Gate>>> = Arc::new(Mutex::new(None));
        let pending_gate = gate.clone();
        let worker = std::thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                        let mut request = String::new();
                        let _ = std::io::BufReader::new((&mut stream).take(8192))
                            .read_line(&mut request);
                        let target = request.split_whitespace().nth(1).unwrap_or_default();
                        let path = iri_string::types::IriReferenceStr::new(target)
                            .map(|iri| iri.path_str())
                            .unwrap_or_default();
                        println!("Fixture request: {}", request.trim());
                        if path == "/motors.kpar" {
                            let waiting = pending_gate.lock().ok().and_then(|mut gate| gate.take());
                            if let Some(waiting) = waiting {
                                let _ = waiting.started.send(());
                                let _ = waiting.resume.recv_timeout(Duration::from_secs(20));
                            }
                        }
                        let (status, bytes): (&str, &[u8]) = if path == "/motors.kpar" {
                            ("200 OK", FIXTURE)
                        } else if path == "/bad.kpar" {
                            ("200 OK", b"not a KPAR")
                        } else {
                            ("404 Not Found", b"missing")
                        };
                        let _ = write!(
                            stream,
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            bytes.len()
                        );
                        let _ = stream.write_all(bytes);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            url: format!("http://{address}/motors.kpar"),
            stop,
            worker: Some(worker),
            gate,
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Reopen {
    key: String,
    saved: String,
    unsaved: String,
}

fn check(report: &mut Vec<Value>, name: &str, passed: bool) -> Result<(), String> {
    report.push(json!({"check": name, "passed": passed}));
    println!("{} {name}", if passed { "PASS" } else { "FAIL" });
    if passed {
        Ok(())
    } else {
        Err(format!("Check failed: {name}"))
    }
}

fn named(model: &MountedModel, name: &str) -> Result<String, String> {
    let mut matches = Vec::new();
    for (_, id) in model.id_page(None, usize::MAX)? {
        if model.value(&id, "declaredName")? == Some(json!(name)) {
            matches.push(id);
        }
    }
    if matches.len() == 1 {
        Ok(matches.remove(0))
    } else {
        Err(format!(
            "Expected one element named '{name}', found {}",
            matches.len()
        ))
    }
}
fn require_model<'a>(project: &'a ComponentSession<'_>) -> Result<&'a MountedModel, String> {
    project.model().ok_or_else(|| {
        format!(
            "Project did not validate: {:?}",
            project.analysis().diagnostics
        )
    })
}
fn reference_check(project: &ComponentSession<'_>) -> Result<bool, String> {
    let model = require_model(project)?;
    let motor = named(model, "Motor")?;
    let mut navigated = false;
    for (_, id) in model.id_page(None, usize::MAX)? {
        if model.component_target(&id).is_some() {
            continue;
        }
        model.inspect(&id, &mut |object| {
            for (name, field) in model.schema().features(object.class())? {
                if !field.reference {
                    continue;
                }
                let Some(value) = object.value(&name) else {
                    continue;
                };
                let atoms = value
                    .collection()
                    .map(|items| items.iter().collect::<Vec<_>>())
                    .unwrap_or_else(|| vec![value]);
                for atom in atoms {
                    if let Some(ReferenceView::Resolved(handle)) = atom.reference() {
                        model.inspect_handle(&handle, &mut |target| {
                            navigated |= target.id() == motor;
                            Ok(())
                        })?;
                    }
                }
            }
            Ok(())
        })?;
    }
    Ok(model
        .component_target(&motor)
        .is_some_and(|target| target.component_uri == LIBRARY)
        && navigated
        && project
            .analysis()
            .references
            .iter()
            .any(|reference| reference.target == motor))
}

fn offline(language: &SysmlComponentLanguage<'_>, cache: &Path, file: &Path) -> Result<(), String> {
    let reopen: Reopen = serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut store = UsageStore::new(language, cache)?;
    let prepared = store.restore(&reopen.key)?;
    let mut project = ComponentSession::open(
        language,
        &[Source {
            uri: URI.into(),
            text: reopen.saved.clone(),
        }],
    )?;
    println!(
        "Initial project diagnostics: {:?}",
        project.analysis().diagnostics
    );
    project.edit(URI, reopen.unsaved.clone())?;
    store.adopt(&mut project, &prepared)?;
    if !reference_check(&project)?
        || project.documents()[0].text() != reopen.unsaved
        || project.documents()[0].saved_text() != reopen.saved
        || store.downloads != 0
        || store.preparations != 0
        || store.restores != 1
    {
        return Err("Offline fresh-process restore failed its invariants".into());
    }
    std::fs::write(file.with_extension("result.json"), serde_json::to_vec_pretty(&json!({
        "passed": true, "downloads": store.downloads, "library_preparations": store.preparations,
        "snapshot_restores": store.restores, "references": project.analysis().references,
        "lock": project.dependency_lock(), "unsaved_bytes_preserved": true,
    })).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("PASS fresh-process offline restore without downloading or preparing the library");
    Ok(())
}

fn demonstrate(
    language: &SysmlComponentLanguage<'_>,
    cache: &Path,
    report: &mut Vec<Value>,
) -> Result<(), String> {
    let server = Server::start()?;
    println!("Fixture hosted at {}", server.url);
    let saved_path = cache.join("demo-saved.sysml");
    std::fs::write(&saved_path, SAVED).map_err(|e| e.to_string())?;
    let mut project = ComponentSession::open(
        language,
        &[Source {
            uri: URI.into(),
            text: std::fs::read_to_string(&saved_path).map_err(|e| e.to_string())?,
        }],
    )?;
    if project.model().is_none() {
        eprintln!(
            "Initial project diagnostics: {:?}",
            project.analysis().diagnostics
        );
    }
    check(
        report,
        "saved project initially validates",
        project.model().is_some(),
    )?;
    project.edit(URI, EDIT.into())?;
    check(
        report,
        "unsaved project reports a missing library reference",
        project.model().is_none() && !project.analysis().diagnostics.is_empty(),
    )?;
    let mut store = UsageStore::new(language, cache)?;
    // Acquisition runs separately from the open editor. No buffer snapshot is
    // captured by the HTTP job; admission analyzes the latest revision.
    let staging = cache.join("downloads");
    let url = server.url.clone();
    let edited = format!("// typed while acquisition was scheduled\r\n{EDIT}");
    let (started, resume) = server.pause_next_download()?;
    let download =
        std::thread::spawn(move || hosted::download(&url, &staging, FetchLimits::default()));
    started
        .recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("Download did not reach the HTTP server: {e}"))?;
    project.edit(URI, edited.clone())?;
    resume.send(()).map_err(|e| e.to_string())?;
    let bytes = download.join().map_err(|_| "Download worker panicked")??;
    store.downloads += 1;
    let prepared = store.prepare_bytes(
        LIBRARY,
        &bytes.requested_url,
        &bytes.effective_url,
        &bytes.bytes,
    )?;
    let stale = project.prepare_locked_usages(
        vec![prepared.image.clone()],
        store.lock(std::slice::from_ref(&prepared.image))?,
    )?;
    let latest = format!("// typed after preparation\r\n{edited}");
    project.edit(URI, latest.clone())?;
    check(
        report,
        "stale usage proposal is rejected after a newer edit",
        project.adopt_usages(stale).is_err() && project.usages().is_empty(),
    )?;
    store.adopt(&mut project, &prepared)?;
    check(
        report,
        "hosted KPAR resolves the real SysML typing reference",
        reference_check(&project)?,
    )?;
    check(
        report,
        "latest unsaved bytes and saved baseline are preserved",
        project.documents()[0].text() == latest
            && project.documents()[0].saved_text() == SAVED
            && project.documents()[0].is_dirty(),
    )?;
    check(
        report,
        "adding a usage does not save the project file",
        std::fs::read_to_string(&saved_path).map_err(|e| e.to_string())? == SAVED,
    )?;
    check(
        report,
        "exact dependency lock is installed with the model",
        project
            .dependency_lock()
            .is_some_and(|lock| lock.entries().len() == 1),
    )?;
    let model = require_model(&project)?;
    let motor = named(model, "Motor")?;
    let mut fork = model.fork()?;
    check(
        report,
        "checked API rejects edits to the library definition",
        fork.set(&motor, "declaredName", json!("Forbidden"))
            .is_err()
            && model.value(&motor, "declaredName")? == Some(json!("Motor")),
    )?;
    check(
        report,
        "library objects remain outside writable project storage",
        model.project_object_count()? + prepared.image.object_count()
            == model.id_page(None, usize::MAX)?.len(),
    )?;
    let mut second = ComponentSession::open(
        language,
        &[Source {
            uri: "memory:/second.sysml".into(),
            text: "package Second {}".into(),
        }],
    )?;
    store.adopt(&mut second, &prepared)?;
    check(
        report,
        "two open projects share one immutable library arena",
        model.shares_component_with(require_model(&second)?, LIBRARY),
    )?;
    let mut incomplete = ComponentSession::open(
        language,
        &[Source {
            uri: URI.into(),
            text: SAVED.into(),
        }],
    )?;
    incomplete.edit(URI, "package Drone {".into())?;
    store.adopt(&mut incomplete, &prepared)?;
    check(
        report,
        "adding a usage also preserves incomplete editor text and its diagnostics",
        incomplete.documents()[0].text() == "package Drone {"
            && incomplete.documents()[0].is_dirty()
            && incomplete.model().is_none()
            && !incomplete.analysis().diagnostics.is_empty()
            && incomplete.usages().len() == 1,
    )?;
    incomplete.edit(URI, EDIT.into())?;
    check(
        report,
        "finishing an incomplete edit uses the already mounted library",
        reference_check(&incomplete)?,
    )?;
    let before_preparations = store.preparations;
    let pooled = store.prepare_bytes(
        LIBRARY,
        &bytes.requested_url,
        &bytes.effective_url,
        &bytes.bytes,
    )?;
    check(
        report,
        "repeated acquisition shares the cached component allocation",
        pooled.cache_hit
            && Arc::ptr_eq(&pooled.image, &prepared.image)
            && store.preparations == before_preparations,
    )?;
    let revision = project.revision();
    let before_lock = serde_json::to_value(project.dependency_lock()).map_err(|e| e.to_string())?;
    check(
        report,
        "HTTP and malformed-archive failures leave the open project intact",
        store
            .add_hosted(
                &server.url.replace("motors.kpar", "missing.kpar"),
                "urn:missing",
            )
            .is_err()
            && store
                .add_hosted(&server.url.replace("motors.kpar", "bad.kpar"), "urn:bad")
                .is_err()
            && project.revision() == revision
            && project.documents()[0].text() == latest
            && serde_json::to_value(project.dependency_lock()).map_err(|e| e.to_string())?
                == before_lock,
    )?;
    project.edit(URI, format!("// another project-only edit\r\n{latest}"))?;
    check(
        report,
        "editing the project does not prepare its library again",
        reference_check(&project)? && store.preparations == before_preparations,
    )?;
    let image_path = cache
        .join("components")
        .join(&prepared.key)
        .join("image.bin");
    let original = std::fs::read(&image_path).map_err(|e| e.to_string())?;
    let mut corrupt = original.clone();
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 1;
    std::fs::write(&image_path, corrupt).map_err(|e| e.to_string())?;
    let refused = UsageStore::new(language, cache)?
        .restore(&prepared.key)
        .is_err();
    std::fs::write(&image_path, original).map_err(|e| e.to_string())?;
    check(
        report,
        "a corrupt cached snapshot is refused without changing the session",
        refused && reference_check(&project)?,
    )?;
    let reopen = cache.join("reopen.json");
    std::fs::write(
        &reopen,
        serde_json::to_vec_pretty(&Reopen {
            key: prepared.key.clone(),
            saved: SAVED.into(),
            unsaved: project.documents()[0].text().into(),
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    drop(server);
    let status = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(cache)
        .arg("--offline-check")
        .arg(&reopen)
        .status()
        .map_err(|e| e.to_string())?;
    check(
        report,
        "a fresh process reopens the resolved library with its HTTP server stopped",
        status.success(),
    )?;
    report.push(json!({"artifact": prepared.key, "archive_sha256": prepared.archive_sha256,
        "cache_hit": prepared.cache_hit, "library_preparations": store.preparations, "snapshot_restores": store.restores,
        "project_objects": require_model(&project)?.project_object_count()?, "library_objects": prepared.image.object_count(),
        "references": project.analysis().references, "lock": project.dependency_lock()}));
    Ok(())
}

fn show(project: &ComponentSession<'_>) {
    for document in project.documents() {
        println!(
            "{} dirty={}\n{}",
            document.uri(),
            document.is_dirty(),
            document.text()
        );
    }
    println!(
        "revision={} usages={} checked_model={} diagnostics={:?}",
        project.revision(),
        project.usages().len(),
        project.model().is_some(),
        project.analysis().diagnostics
    );
}

fn shell(
    language: &SysmlComponentLanguage<'_>,
    cache: &Path,
    file: Option<&String>,
) -> Result<(), String> {
    let server = Server::start()?;
    let saved = match file {
        Some(path) => std::fs::read_to_string(path).map_err(|e| e.to_string())?,
        None => SAVED.into(),
    };
    let mut project = ComponentSession::open(
        language,
        &[Source {
            uri: URI.into(),
            text: saved,
        }],
    )?;
    let mut store = UsageStore::new(language, cache)?;
    println!(
        "Text session ready. Fixture URL: {}\nCommands: show | edit TEXT | edit-file PATH | add fixture | add URL [URI] | restore KEY | usages | refs | inspect NAME | attempt-write NAME | quit\nUse: edit package Drone {{ part engine : Motors::Motor; }}\nThen: add fixture\nFiles are never saved by this harness.",
        server.url
    );
    show(&project);
    for line in std::io::stdin().lock().lines() {
        print!("> ");
        let _ = std::io::stdout().flush();
        let line = line.map_err(|e| e.to_string())?;
        let (command, argument) = line.split_once(' ').unwrap_or((&line, ""));
        let result = (|| -> Result<(), String> {
            match command {
                "quit" | "exit" => return Ok(()),
                "show" => show(&project),
                "edit" => {
                    project.edit(URI, argument.into())?;
                    show(&project);
                }
                "edit-file" => {
                    project.edit(
                        URI,
                        std::fs::read_to_string(argument).map_err(|e| e.to_string())?,
                    )?;
                    show(&project);
                }
                "add" => {
                    let usage: PreparedUsage = if argument == "fixture" {
                        store.add_hosted(&server.url, LIBRARY)?
                    } else {
                        let (url, uri) = argument.split_once(' ').unwrap_or((argument, argument));
                        store.add_hosted(url, uri)?
                    };
                    store.adopt(&mut project, &usage)?;
                    println!(
                        "Added read-only usage. cache_key={} cache_hit={}",
                        usage.key, usage.cache_hit
                    );
                    show(&project);
                }
                "restore" => {
                    let usage = store.restore(argument)?;
                    store.adopt(&mut project, &usage)?;
                    show(&project);
                }
                "usages" => println!(
                    "{}",
                    serde_json::to_string_pretty(&project.dependency_lock())
                        .map_err(|e| e.to_string())?
                ),
                "refs" => println!("{:?}", project.analysis().references),
                "inspect" | "attempt-write" => {
                    let model = require_model(&project)?;
                    let id = named(model, argument)?;
                    if command == "inspect" {
                        println!("{:?}", model.get(&id)?);
                    } else {
                        println!(
                            "{:?}",
                            model.fork()?.set(&id, "declaredName", json!("Forbidden"))
                        );
                    }
                }
                _ => println!("Unknown command"),
            }
            Ok(())
        })();
        if command == "quit" || command == "exit" {
            break;
        }
        if let Err(error) = result {
            eprintln!("{error}");
        }
    }
    Ok(())
}

pub fn run(
    language: &SysmlComponentLanguage<'_>,
    cache: &Path,
    args: &[String],
) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("--offline-check") => offline(language, cache, &PathBuf::from(args.get(1).ok_or("Missing reopen file")?)),
        Some("--shell") => shell(language, cache, args.get(1)),
        None | Some("--demo") => {
            let mut report = Vec::new();
            let result = demonstrate(language, cache, &mut report);
            std::fs::write(cache.join("report.json"), serde_json::to_vec_pretty(&json!({"passed": result.is_ok(), "error": result.as_ref().err(), "checks": report})).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            result
        }
        _ => Err("Usage: hosted_kpar_usage CACHE_DIRECTORY [--demo | --shell [PROJECT.sysml] | --offline-check FILE]".into()),
    }
}
