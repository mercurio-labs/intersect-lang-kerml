//! Verification-only WASM entry point for the same native assertions.
#[cfg(target_arch = "wasm32")]
#[path = "../tests/operand.rs"]
mod controls;
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn verify_operands() -> u32 {
    controls::seven_pilot_operand_trees_match_with_exact_source_contracts();
    controls::invalid_operands_and_every_provider_budget_cutoff_restore_graph();
    2
}
fn main() {}
