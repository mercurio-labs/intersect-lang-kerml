//! Handwritten browser host for the KerML language package.
//!
//! Intersect core's `intersect-web` is the language-neutral JSON host; this
//! crate adds the frozen-engine KerML load methods (`draft/kerml/analyze`,
//! `draft/kerml/project/analyze`) and delegates every other method to it
//! unchanged. Its wasm32 build exports the same ABI as `intersect-web`
//! (`input_buffer`, `execute`, `output_buffer`), so the core Toolkit loads it
//! in place of `intersect_web.wasm` (`INTERSECT_WEB_WASM`, see the core
//! repository's `toolkit/scripts/build-wasm.mjs`).
use intersect_web::function_jobs::FunctionJobHost;
use serde_json::Value;

mod kerml_load_json;

/// Requests larger than this are rejected before a JSON tree is built, as in
/// `intersect-web`.
const REQUEST_BYTES: usize = 4_000_000;

/// The KerML method `input` names, when it is one this crate handles.
fn kerml_method(input: &str) -> Result<Option<&'static str>, String> {
    #[derive(serde::Deserialize)]
    struct MethodProbe {
        method: String,
    }
    let probe: MethodProbe = serde_json::from_str(input).map_err(|e| e.to_string())?;
    Ok(match probe.method.as_str() {
        kerml_load_json::ANALYZE => Some(kerml_load_json::ANALYZE),
        kerml_load_json::PROJECT => Some(kerml_load_json::PROJECT),
        _ => None,
    })
}

/// Run one KerML request, or return `None` for a method this crate does not own.
fn kerml_dispatch(input: &str) -> Option<Result<Value, String>> {
    let method = match kerml_method(input) {
        Ok(Some(method)) => method,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    if input.len() > REQUEST_BYTES {
        return Some(Err(if method == kerml_load_json::PROJECT {
            "Draft project request byte budget exceeded".into()
        } else {
            "Draft request byte budget exceeded".into()
        }));
    }
    let request: Value = match serde_json::from_str(input) {
        Ok(request) => request,
        Err(error) => return Some(Err(error.to_string())),
    };
    Some(if method == kerml_load_json::PROJECT {
        kerml_load_json::analyze_project(&request)
    } else {
        kerml_load_json::analyze(&request)
    })
}

/// Stateless dispatch: the KerML methods, then `intersect_web::dispatch`.
pub fn dispatch(input: &str) -> Result<Value, String> {
    kerml_dispatch(input).unwrap_or_else(|| intersect_web::dispatch(input))
}

/// Stateful host: the KerML methods, then core's [`FunctionJobHost`].
#[derive(Default)]
pub struct Host {
    core: FunctionJobHost,
}

impl Host {
    pub fn dispatch(&mut self, input: &str) -> Result<Value, String> {
        kerml_dispatch(input).unwrap_or_else(|| self.core.dispatch(input))
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::{Host, REQUEST_BYTES};
    use serde_json::json;
    use std::cell::RefCell;
    thread_local! {
        static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static HOST: RefCell<Host> = RefCell::new(Host::default());
    }
    /// Allocate a bounded input region owned by this instance until the next call.
    #[unsafe(no_mangle)]
    pub extern "C" fn input_buffer(length: usize) -> *mut u8 {
        if length > REQUEST_BYTES {
            return std::ptr::null_mut();
        }
        INPUT.with(|input| {
            let mut input = input.borrow_mut();
            input.resize(length, 0);
            input.as_mut_ptr()
        })
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn execute() -> usize {
        let response = INPUT.with(|input| {
            let input = input.borrow();
            std::str::from_utf8(&input)
                .map_err(|e| e.to_string())
                .and_then(|request| HOST.with(|host| host.borrow_mut().dispatch(request)))
        });
        let value = response.unwrap_or_else(|message| json!({"error":message}));
        OUTPUT.with(|output| {
            let mut output = output.borrow_mut();
            *output = value.to_string().into_bytes();
            output.len()
        })
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn output_buffer() -> *const u8 {
        OUTPUT.with(|output| output.borrow().as_ptr())
    }
}
