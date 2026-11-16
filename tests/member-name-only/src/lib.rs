//! Exact related getter consumer; no graph/constructor/domain validity authority.
#[path = "../../../fixtures/member-name-reads/controls.rs"]
mod controls;
fn verify() -> Result<(), String> {
    controls::exact_pilot_names_null_branch_and_raw_conflicts()?;
    controls::exact_source_member_and_actual_contracts()?;
    controls::declared_edge_keys_preflight_and_default_hook_reject()?;
    controls::malformed_dangling_subtype_identity_and_revision_failures()?;
    controls::literal_callbacks_zero_exact_short_recovery_and_index_modes()?;
    controls::shared_reference_domain_is_checked_before_target_value()
}
#[unsafe(no_mangle)]
pub extern "C" fn verify_model_boundary() -> u32 {
    u32::from(verify().is_err())
}
#[test]
fn exact_member_name_external_consumer() -> Result<(), String> {
    verify()
}
