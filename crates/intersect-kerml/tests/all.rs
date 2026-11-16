//! Handwritten test-suite root (not generated): merges the intersect-kerml integration tests into one
//! test binary so the suite builds and links once. Each module is a former test
//! binary of the same name; its tests are named `<module>::<test>`.
//! Modules are `pub` so items a former binary exported stay live, and shared
//! helper files are loaded once per former binary, as before the merge.
#![allow(clippy::duplicate_mod)]

#[path = "fragment_call_summaries.rs"]
pub mod fragment_call_summaries;

#[path = "language_pack_equivalence.rs"]
pub mod language_pack_equivalence;

#[path = "old_path_guard.rs"]
pub mod old_path_guard;

#[path = "rcore_local_scope.rs"]
pub mod rcore_local_scope;

#[path = "source_identity.rs"]
pub mod source_identity;

#[path = "sysml_frozen_sibling_bodies.rs"]
pub mod sysml_frozen_sibling_bodies;

#[path = "sysml_original551.rs"]
pub mod sysml_original551;

#[path = "sysml_original551_input_checked.rs"]
pub mod sysml_original551_input_checked;

#[path = "sysml_structure.rs"]
pub mod sysml_structure;

#[path = "frozen_rule_probes.rs"]
pub mod frozen_rule_probes;

#[path = "frozen_load.rs"]
pub mod frozen_load;

#[path = "operand.rs"]
pub mod operand;
#[path = "sysml_prediction_memo.rs"]
pub mod sysml_prediction_memo;

mod project_package_equivalence;
