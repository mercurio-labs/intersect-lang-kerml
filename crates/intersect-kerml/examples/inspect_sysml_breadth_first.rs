//! One explicit breadth-first compile of the unchanged full pinned SysML sources.
//! Admission, runtime workflows and semantic/library support remain separate.
#[path = "inspect_pinned_grammar_admission.rs"]
mod original;

use intersect_text::grammar::Profile;
use intersect_text::grammar::RecursiveFrontierOrder;
use serde_json::{Value, json};
use std::{error::Error, path::Path, process::ExitCode};

const USAGE: &str =
    "Usage: inspect_sysml_breadth_first PILOT_ROOT --root-reviewed-sysml-breadth-first";

fn main() -> ExitCode {
    let _legacy_entry_point: fn() -> ExitCode = original::main;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let mut report = original::initial_report();
    report["profile"] = json!(Profile::NativeV80.as_str());
    report["language"] = json!("sysml");
    report["frontend"] = json!("intersect_text::ordinary::compile_with_context_frontier_selected");
    report["actual_compiler_result"] = Value::Null;
    report["frontier_selection"] = json!({
        "version":1,
        "order":"BreadthFirst",
        "request_count":1,
        "source_prefix_capture":false,
        "new_profile":false,
        "recompile_requires_explicit_reselection":true,
        "project_save_reload_qualified":false,
        "scope":"explicit compiler scheduling only; same real classifier/witness/depth/retention workers and original pools; outcome and utility are unproven",
    });
    let result = (|| -> Result<(), Box<dyn Error>> {
        if args.len() != 2 || args[1] != "--root-reviewed-sysml-breadth-first" {
            return Err(USAGE.into());
        }
        original::inspect_with_optional_frontier(
            Path::new(&args[0]),
            "sysml",
            Profile::NativeV80,
            true,
            true,
            &mut report,
            None,
            Some(RecursiveFrontierOrder::BreadthFirst),
        )
    })();
    let exit = match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if report["first_blocker"].is_null() {
                report["first_blocker"] = json!({"message":error.to_string()});
            }
            report["error"] = json!(error.to_string());
            ExitCode::FAILURE
        }
    };
    // This includes the complete actual Result, even if later projection failed.
    // No success/old-failure/witness assertion precedes or follows this output.
    println!("{report}");
    exit
}
