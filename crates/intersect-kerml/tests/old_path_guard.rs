//! Handwritten deprecation guard (not generated) for the KerML K01 native-draft
//! pipeline: the `pub fn` project runners of `draft_project_entry.rs`
//! (`with_basic_kerml_project`, `with_*_k01_project*`, `with_runtime_*`, ...)
//! plus `analyze_minimal_package`, `prepare_minimal_inputs` and the
//! `basic_kerml_draft_capability`/`basic_kerml_project_capability*` builders,
//! all re-exported from `intersect_kerml::draft`. They construct into a
//! legacy store through the physical draft pipeline. The frozen path uses
//! `native_v80_kerml_project_capability`, `nonunique_draft_capability` and
//! `kerml_string_rules` instead, which are not guarded. Inventory, consumers
//! and migration order: docs/old-path-inventory.md. Scan rules:
//! tests/support/old_path_scan.rs (a copy of intersect core's scanner).
//!
//! Recorded 2026-11-05 (non-test files under crates/*/src / crates/*/examples):
//! 9 / 1. src: intersect-kerml 7 (the definitions and their re-exports in
//! draft, draft_entry, draft_project_entry, k01_draft, k01_scope,
//! owned_element_reads, typing_reads), intersect-web 2 (draft_json,
//! draft_project_json). examples: inspect_k01_runtime_audit.
//! 2026-11-06, migration step 2: 7 / 1 (intersect-web's KerML transport moved
//! to `intersect_kerml::frozen_load`).
//!
//! 2026-11-06, migration step 3: 0 / 0. The K01 pipeline, `draft_project_entry.rs`
//! and every caller are deleted; the former names below stay guarded so that
//! none returns.
//!
//! Never raise the number.
#[path = "../../../tests/support/old_path_scan.rs"]
mod scan;

use scan::{Entry, File};

/// The former K01 entry points: the fixed builders and the 34 `pub fn`
/// runners of the deleted `draft_project_entry.rs`.
const K01_ENTRIES: [&str; 39] = [
    "analyze_minimal_package",
    "prepare_minimal_inputs",
    "basic_kerml_draft_capability",
    "basic_kerml_project_capability",
    "basic_kerml_project_capability_with_operand_settings",
    "with_authored_library_member_project",
    "with_basic_kerml_project",
    "with_observed_prepared_authored_library_member_project",
    "with_observed_prepared_basic_kerml_project",
    "with_observed_runtime_authored_classifier_project",
    "with_observed_runtime_authored_classifier_project_arena_returns",
    "with_observed_runtime_authored_classifier_project_neutral_initials",
    "with_observed_runtime_base_scalar_authored_project",
    "with_observed_runtime_basic_kerml_project",
    "with_original_runtime_base_scalar_neutral_project",
    "with_original_runtime_base_scalar_neutral_project_reopened_edits",
    "with_original_runtime_base_scalar_neutral_project_reopened_reads",
    "with_original_runtime_base_scalar_neutral_project_retained_edits",
    "with_original_runtime_base_scalar_neutral_project_with_reads",
    "with_original_runtime_classifier_neutral_project",
    "with_original_runtime_classifier_neutral_project_with_reads",
    "with_original_runtime_required_library_k01_project",
    "with_original_runtime_required_library_k01_project_retained_edits",
    "with_original_runtime_required_library_k01_project_token_provenance",
    "with_original_runtime_required_library_k01_project_validation_slice",
    "with_original_runtime_required_library_k01_reopened_reads",
    "with_prepared_authored_library_member_project",
    "with_prepared_basic_kerml_project",
    "with_runtime_authored_classifier_project",
    "with_runtime_base_scalar_authored_project",
    "with_runtime_base_scalar_authored_project_arena_returns",
    "with_runtime_base_scalar_authored_project_neutral_initials",
    "with_runtime_base_scalar_neutral_project_reopened_edits",
    "with_runtime_base_scalar_neutral_project_reopened_reads",
    "with_runtime_basic_kerml_project",
    "with_runtime_required_library_k01_project",
    "with_runtime_required_library_k01_project_retained_edits",
    "with_runtime_required_library_k01_project_validation_slice",
    "with_runtime_required_library_k01_reopened_reads",
];

fn uses_k01_entry(f: &File) -> bool {
    K01_ENTRIES.iter().any(|n| f.has_word(n))
}

#[test]
fn k01_native_draft_users_do_not_grow() -> Result<(), String> {
    scan::check(&[Entry {
        name: "K01 native-draft entry points",
        src: 0,
        examples: 0,
        uses: uses_k01_entry,
    }])
}
