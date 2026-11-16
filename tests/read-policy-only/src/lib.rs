//! External read-only capability consumer; not a complete language runtime.
#[path = "../../../fixtures/semantic-read-policy/controls.rs"]
mod controls;
#[path = "../../../fixtures/semantic-read-policy/pilot_membership.rs"]
mod pilot_membership;
fn verify() -> Result<(), String> {
    controls::delegated_value_length_projection_navigation_and_count_share_policy()?;
    controls::unsupported_fields_dependencies_and_complete_projections_reject_before_store()?;
    controls::unsafe_equality_anchor_is_disabled_for_computed_subtype_and_unsupported_fails_early(
    )?;
    controls::source_actual_identity_budget_failure_and_recovery_keep_raw_values()?;
    controls::index_on_and_off_preserve_whole_semantic_results_and_callback_work()?;
    controls::zero_budget_precedes_callbacks_and_exact_one_short_work_recovers()?;
    pilot_membership::exact_owning_membership_getters_match_independent_pilot_observations()
}
#[unsafe(no_mangle)]
pub extern "C" fn verify_model_boundary() -> u32 {
    u32::from(verify().is_err())
}
#[test]
fn selected_read_policy_is_available_to_an_external_consumer() -> Result<(), String> {
    verify()
}
