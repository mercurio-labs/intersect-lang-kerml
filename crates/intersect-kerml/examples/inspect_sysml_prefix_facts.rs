//! One explicit diagnostic over the unchanged pinned ordinary SysML setup.
//! Source facts never qualify a parser certificate or language workflow.
#[path = "inspect_pinned_grammar_admission.rs"]
mod original;

use intersect_text::grammar::PrefixDiagnosticSource;
use intersect_text::grammar::PrefixDiagnosticText;
use intersect_text::grammar::Profile;
use intersect_text::grammar::SourcePrefixDiagnostic;
use serde_json::{Value, json};
use std::{error::Error, path::Path, process::ExitCode};

const USAGE: &str = "Usage: inspect_sysml_prefix_facts PILOT_ROOT";

fn text<const N: usize>(
    value: Option<PrefixDiagnosticText<N>>,
) -> Result<Option<String>, Box<dyn Error>> {
    value
        .map(|value| value.as_str().map(str::to_owned))
        .transpose()
        .map_err(Into::into)
}
fn source(value: Option<PrefixDiagnosticSource>) -> Result<Option<Value>, Box<dyn Error>> {
    value
        .map(|value| -> Result<Value, Box<dyn Error>> {
            Ok(json!({
                "source": { "uri":value.uri.as_str()?, "sha256":value.sha256.as_str()? },
                "declaration_span":value.declaration, "body_span":value.body,
                "span_unit":"utf8_byte_offset",
            }))
        })
        .transpose()
}
fn symbols(values: [Option<PrefixDiagnosticText<256>>; 16]) -> Result<Vec<String>, Box<dyn Error>> {
    values
        .into_iter()
        .flatten()
        .map(|value| value.as_str().map(str::to_owned).map_err(Into::into))
        .collect()
}
fn observation(prefix: &SourcePrefixDiagnostic<'_>) -> Result<Value, Box<dyn Error>> {
    let requested = json!({"owner":"ActionBodyItem", "follow_occurrence":"rule/ActionBodyItem/a6/s1",
        "classifier_decision":"rule/ActionBodyItem/a6/s1/repeat", "callee":"TargetSuccessionMember"});
    let Some(record) = prefix.record() else {
        return Ok(
            json!({"requested":requested, "state":"NotReached", "facts":null,
            "scope":"selected canonical FOLLOW FIRST query was not reached; no source or eligibility inferred"}),
        );
    };
    Ok(json!({
        "requested":requested,
        "state":format!("{:?}",record.state), "absence":format!("{:?}",record.absence),
        "owner":text(record.owner)?, "path":text(record.path)?, "source":source(record.source)?,
        "nullable":record.nullable, "body_nullable":record.body_nullable,
        "prefix_hidden_policy":record.prefix_hidden_policy,
        "maximum_source_first_call_depth":record.maximum_source_call_depth,
        "first":symbols(record.first)?, "first_symbol_count":record.first_count,
        "callee":text(record.callee)?, "callee_source":source(record.callee_source)?,
        "callee_nullable":record.callee_nullable, "callee_first":symbols(record.callee_first)?,
        "callee_first_symbol_count":record.callee_first_count,
        "original_first_remaining":record.original_first_remaining,
        "error":text(record.error)?, "capture_unavailable":record.capture_unavailable,
        "original_first_ceiling":10000,
        "classifier_incoming_call_depth":null, "combined_graph_call_depth":null,
        "parser_certificate":false, "optimizer_or_profile_activated":false,
        "scope":"bounded completed source FIRST facts from the SAME required FOLLOW query; original worker result moves back to FOLLOW; no graph/return/witness/semantic authority",
    }))
}

fn main() -> ExitCode {
    // Referencing the retained entry point keeps its original argument/error
    // path compiled without invoking it or suppressing dead-code diagnostics.
    let _legacy_entry_point: fn() -> ExitCode = original::main;
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--help" {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let prefix = SourcePrefixDiagnostic::new(
        "ActionBodyItem",
        "rule/ActionBodyItem/a6/s1",
        "TargetSuccessionMember",
    );
    let mut report = original::initial_report();
    report["profile"] = json!(Profile::NativeV80.as_str());
    report["language"] = json!("sysml");
    report["frontend"] = json!("intersect_text::ordinary::compile_with_context_prefix_inspected");
    report["actual_compiler_result"] = Value::Null;
    report["diagnostic_adapter"] = json!({"version":1,"request_count":1,
        "source_setup":"unchanged inspect_pinned_grammar_admission setup",
        "result_parity_claim":false,
        "scope":"capture fees spend the selected original FIRST10000 and may cause an earlier honest rejection"});
    let result = (|| -> Result<(), Box<dyn Error>> {
        if args.len() != 1 {
            return Err(USAGE.into());
        }
        original::inspect_with_prefix(
            Path::new(&args[0]),
            "sysml",
            Profile::NativeV80,
            false,
            false,
            &mut report,
            Some(&prefix),
        )
    })();
    let result = match observation(&prefix) {
        Ok(value) => {
            report["source_prefix_observation"] = value;
            result
        }
        Err(error) => Err(error),
    };
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
    println!("{report}");
    exit
}
