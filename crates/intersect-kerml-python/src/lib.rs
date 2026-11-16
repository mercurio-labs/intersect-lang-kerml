//! Handwritten native transport. All language/model behavior lives in the SDK.
use base64::Engine as _;
use intersect_kerml::sdk::{Engine as Session, Snapshot};
use pyo3::{exceptions::PyRuntimeError, prelude::*};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::Read,
    sync::{Mutex, mpsc},
};

type Reply = mpsc::Sender<Result<String, String>>;
struct Request {
    method: String,
    params: Value,
    reply: Reply,
}

#[pyclass(frozen)]
struct Engine {
    sender: Mutex<Option<mpsc::SyncSender<Request>>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str, String> {
    params[name]
        .as_str()
        .ok_or_else(|| format!("Missing string argument: {name}"))
}
fn number(params: &Value, name: &str) -> Result<usize, String> {
    params[name]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("Missing integer argument: {name}"))
}

fn dispatch(
    session: &Session,
    snapshots: &mut BTreeMap<usize, Snapshot>,
    next: &mut usize,
    method: &str,
    params: &Value,
) -> Result<Value, String> {
    match method {
        "manifest" => return Ok(session.manifest.clone()),
        "release" => {
            snapshots.remove(&number(params, "snapshot")?);
            return Ok(Value::Null);
        }
        "analyze" => {
            if snapshots.len() >= 64 {
                return Err("At most 64 retained model snapshots; close unused models".into());
            }
            let sources = serde_json::from_value(params["sources"].clone())
                .map_err(|e| format!("Invalid source map: {e}"))?;
            let mut archives = Vec::new();
            if let Some(inputs) = params["archives"].as_array() {
                for input in inputs {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(text(input, "base64")?)
                        .map_err(|e| format!("Invalid archive encoding: {e}"))?;
                    archives.push((text(input, "uri")?.to_owned(), bytes));
                }
            }
            let mut result = session.analyze(sources, params["descriptor"].as_str(), archives)?;
            let mut wire = serde_json::to_value(&result).map_err(|e| e.to_string())?;
            wire["snapshot"] = match result.model.take() {
                Some(model) => {
                    *next = next.checked_add(1).ok_or("Snapshot counter exhausted")?;
                    snapshots.insert(*next, model);
                    json!(*next)
                }
                None => Value::Null,
            };
            return Ok(wire);
        }
        _ => {}
    }
    let model = snapshots
        .get(&number(params, "snapshot")?)
        .ok_or("Closed or unknown snapshot")?;
    match method {
        "elements" => Ok(json!(model.elements(
            params["class"].as_str(),
            params["include_libraries"].as_bool().unwrap_or(false),
            number(params, "offset")?,
            number(params, "limit")?
        )?)),
        "inspect" => model.inspect(text(params, "id")?),
        "get" => model.get(text(params, "id")?, text(params, "feature")?),
        "properties" => Ok(json!(model.stored_properties(text(params, "id")?)?)),
        "children" => Ok(json!(model.children(text(params, "id")?)?)),
        "is_instance" => Ok(json!(
            model.is_instance(text(params, "id")?, text(params, "class")?)?
        )),
        _ => Err(format!("Unsupported SDK method: {method}")),
    }
}

#[pymethods]
impl Engine {
    #[new]
    fn new(py: Python<'_>, path: String) -> PyResult<Self> {
        py.detach(move || {
            let (sender, receiver) = mpsc::sync_channel::<Request>(16);
            let (ready, result) = mpsc::channel();
            let worker = std::thread::Builder::new()
                .name("intersect-python".into())
                .stack_size(64 * 1024 * 1024)
                .spawn(move || {
                    // Thread-affine session is constructed, used and dropped here.
                    let loaded = (|| {
                        let file = std::fs::File::open(&path)
                            .map_err(|e| format!("Cannot open pack: {e}"))?;
                        let mut bytes = Vec::new();
                        file.take(512 * 1024 * 1024 + 1)
                            .read_to_end(&mut bytes)
                            .map_err(|e| e.to_string())?;
                        if bytes.len() > 512 * 1024 * 1024 {
                            return Err("Pack exceeds 512 MiB".into());
                        }
                        Session::from_trusted_bytes(bytes)
                    })();
                    let session = match loaded {
                        Ok(session) => {
                            if ready.send(Ok(())).is_err() {
                                return;
                            }
                            session
                        }
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return;
                        }
                    };
                    let mut snapshots = BTreeMap::new();
                    let mut next = 0;
                    for request in receiver {
                        if request.method == "close" {
                            let _ = request.reply.send(Ok("null".into()));
                            break;
                        }
                        let answer = dispatch(
                            &session,
                            &mut snapshots,
                            &mut next,
                            &request.method,
                            &request.params,
                        )
                        .and_then(|value| serde_json::to_string(&value).map_err(|e| e.to_string()));
                        let _ = request.reply.send(answer);
                    }
                })
                .map_err(|e| PyRuntimeError::new_err(format!("Cannot start engine: {e}")))?;
            result
                .recv()
                .map_err(|_| PyRuntimeError::new_err("Engine stopped during initialization"))?
                .map_err(PyRuntimeError::new_err)?;
            Ok(Self {
                sender: Mutex::new(Some(sender)),
                worker: Mutex::new(Some(worker)),
            })
        })
    }
    fn request(&self, py: Python<'_>, method: String, payload: String) -> PyResult<String> {
        if payload.len() > 16 * 1024 * 1024 {
            return Err(PyRuntimeError::new_err("Request exceeds 16 MiB"));
        }
        let params =
            serde_json::from_str(&payload).map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let sender = self
            .sender
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Engine lock poisoned"))?
            .as_ref()
            .cloned()
            .ok_or_else(|| PyRuntimeError::new_err("Engine is closed"))?;
        py.detach(move || {
            let (reply, result) = mpsc::channel();
            sender
                .send(Request {
                    method,
                    params,
                    reply,
                })
                .map_err(|_| PyRuntimeError::new_err("Engine stopped"))?;
            result
                .recv()
                .map_err(|_| PyRuntimeError::new_err("Engine stopped during request"))?
                .map_err(PyRuntimeError::new_err)
        })
    }
    fn close(&self, py: Python<'_>) -> PyResult<()> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Engine lock poisoned"))?
            .take();
        if let Some(sender) = sender {
            py.detach(move || {
                let (reply, result) = mpsc::channel();
                sender
                    .send(Request {
                        method: "close".into(),
                        params: Value::Null,
                        reply,
                    })
                    .map_err(|_| PyRuntimeError::new_err("Engine stopped"))?;
                result
                    .recv()
                    .map_err(|_| PyRuntimeError::new_err("Engine stopped during shutdown"))?
                    .map_err(PyRuntimeError::new_err)?;
                Ok::<_, PyErr>(())
            })?;
        }
        let worker = self
            .worker
            .lock()
            .map_err(|_| PyRuntimeError::new_err("Engine lock poisoned"))?
            .take();
        if let Some(worker) = worker {
            py.detach(move || {
                worker
                    .join()
                    .map_err(|_| PyRuntimeError::new_err("Engine worker panicked"))
            })?;
        }
        Ok(())
    }
}

#[pymodule(gil_used = true)]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Engine>()?;
    Ok(())
}
