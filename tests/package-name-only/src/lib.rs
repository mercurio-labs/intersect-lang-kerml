//! External getter consumer. No model validity or full language authority.
#[path = "../../../fixtures/package-name-reads/controls.rs"]
mod controls;
fn verify() -> Result<(), String> {
    controls::independent_pilot_package_name_and_null_ignore_conflicting_raw_name()?;
    controls::exact_source_member_actual_receiver_and_subtype_guards_remain_closed()?;
    controls::zero_exact_one_short_recovery_and_index_modes_share_the_boundary()
}
#[unsafe(no_mangle)]
pub extern "C" fn verify_model_boundary() -> u32 {
    u32::from(verify().is_err())
}
#[test]
fn exact_package_name_external_consumer() -> Result<(), String> {
    verify()
}
