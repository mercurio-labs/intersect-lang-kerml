//! One original full SysML compiler-pool observation; no admission assertion.
#[path = "inspect_pinned_grammar_admission.rs"]
mod original;

use intersect_text::grammar::Profile;
use intersect_text::grammar::RecursiveFrontierOrder;
use intersect_text::grammar::admission_observation::pool_blocks::CompilerPoolDiagnostic;
use intersect_text::grammar::admission_observation::pool_blocks::ProgramSnapshot;
use serde_json::{Value, json};
use std::{error::Error, path::Path, process::ExitCode};

const USAGE: &str =
    "Usage: inspect_sysml_compiler_pool PILOT_ROOT --root-reviewed-sysml-compiler-pool";

fn program(row: ProgramSnapshot) -> Value {
    let work = row.work;
    json!({
        "ordinal":row.ordinal,"stage":format!("{:?}",row.stage),
        "source":row.source.map(|source| json!({
            "uri":source.uri.as_str().map(str::to_owned).ok(),
            "sha256":source.sha256.as_str().map(str::to_owned).ok(),
            "uri_capture_error":source.uri.as_str().err().map(|error|error.to_string()),
            "sha256_capture_error":source.sha256.as_str().err().map(|error|error.to_string()),
        })),
        "order":format!("{:?}",row.order),
        "decision":row.decision.and_then(|text|text.as_str().ok().map(str::to_owned)),
        "decision_capture_error":row.decision.and_then(|text|text.as_str().err().map(|error|error.to_string())),
        "initial":work.initial,"remaining":work.remaining,
        "units":work.counts.units,
        "actual_denied_leaf_attempts":work.counts.actual_denied_leaf_attempts,
        "calls":work.counts.calls,"error_exits":work.counts.error_exits,
        "first_actual_denial":work.first_actual_denial.map(|block|format!("{block:?}")),
        "first_error_exit":work.first_error_exit.map(|block|format!("{block:?}")),
        "completed_result":work.completed_result,"ended":work.ended,
        "balanced":work.balanced,"overflowed":work.overflowed,
        "inconsistent":work.inconsistent,"peak_depth":work.peak_depth,
        "queue_share_inclusive":work.queue_share_inclusive,
    })
}

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
    report["frontend"] = json!("intersect_text::ordinary::compile_with_context_pool_observed");
    report["actual_compiler_result"] = Value::Null;
    report["frontier_selection"] = json!({
        "version":1,"order":"BreadthFirstTransparentPorts","request_count":1,
        "source_prefix_capture":false,"partial_token_probe":false,"new_profile":false,
        "recompile_requires_explicit_reselection":true,"project_save_reload_qualified":false,
        "scope":"existing Native80 explicit transparent-port worker, original pools and full source context; diagnostic adds no semantic allowance or classification authority",
    });
    let pool = CompilerPoolDiagnostic::new();
    let result = (|| -> Result<(), Box<dyn Error>> {
        if args.len() != 2 || args[1] != "--root-reviewed-sysml-compiler-pool" {
            return Err(USAGE.into());
        }
        original::inspect_with_optional_frontier_pool(
            Path::new(&args[0]),
            "sysml",
            Profile::NativeV80,
            true,
            true,
            &mut report,
            None,
            Some(RecursiveFrontierOrder::BreadthFirstTransparentPorts),
            Some(&pool),
        )
    })();
    let snapshot = pool.snapshot();
    report["compiler_pool_diagnostic"] = json!({
        "version":1,"programs":snapshot.programs,"active":snapshot.active,
        "capture_unavailable":snapshot.capture_unavailable,
        "inconsistent":snapshot.inconsistent,"overflowed":snapshot.overflowed,
        "last":snapshot.last.map(program),"first_failed":snapshot.first_failed.map(program),
        "block_order":["PoolRemainder","GraphSourceBinding","CallFreeRuleRetention",
            "NullableSourceQualification","NullableGraphProof","RepeatCertificate",
            "ClassifierRemainder","FrontierInitial","FrontierPreparation",
            "QueueShareRemainder","PortKernel","PortDescriptor","PortSchedule",
            "Witness","CallFreeReplay","FrontierPublish"],
        "queue_kind_order":["PortQueue","SeededQueue","Other"],
        "original_pool_per_program":10000,
        "queue_kind_totals_overlap_partition":true,
        "scope":"bounded individual original program pool. Inclusive child/leaf subtraction; residual blocks remain unclassified. Actual-denied counters cover only tagged leaf Results, never every error. Earlier FIRST/FOLLOW/state pools, runtime, allocator and CPU costs are outside this observation.",
    });
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
    // Complete actual Result was retained before any fallible legacy projection.
    // Print the whole outcome and both observers before host validation outside.
    println!("{report}");
    exit
}
