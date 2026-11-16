//! Handwritten host (not generated): the `intersect` command.
//!
//! Loads a user's `.kerml` and `.sysml` files against the SysML v2 standard
//! library prepared in a language pack, reports diagnostics, evaluates expressions,
//! and exports the loaded model as JSON or XMI. All file I/O, clocks and threads
//! of the host are here; loading uses `intersect_kerml::sysml_host` and expression
//! evaluation uses `intersect_kerml_eval`.
//!
//! ```text
//! intersect check  --pack PACK [OPTIONS] FILE...
//! intersect export --pack PACK --format json|xmi --out DIR [OPTIONS] FILE...
//! intersect info   --pack PACK
//! intersect eval   --pack PACK (--expr TEXT | --all) [OPTIONS] [FILE...]
//! ```
//!
//! Options: `--library DIR` (the library sources, when the pack has no
//! usable library snapshot and stores no sources), `--read-only` (never
//! write the pack), `--no-library-cache` (parse the library even when the
//! pack holds a snapshot; debug), `--json` (diagnostics as JSON lines),
//! `--timing` (phase times on stderr).
//!
//! Exit status: 0 when every file loads without diagnostics, 1 when there are
//! diagnostics, 2 on a usage or host error.
mod host;

use std::process::ExitCode;

/// The parser and linker recurse deeply on large models; the host runs them
/// on a thread with this stack.
const STACK: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let worker = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || host::run(&args));
    let outcome = match worker {
        Ok(handle) => handle
            .join()
            .unwrap_or_else(|_| Err("internal error: the worker thread panicked".into())),
        Err(error) => Err(format!("cannot start the worker thread: {error}")),
    };
    match outcome {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => {
            eprintln!("intersect: {error}");
            ExitCode::from(2)
        }
    }
}
