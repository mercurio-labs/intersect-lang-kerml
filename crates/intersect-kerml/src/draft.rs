//! Handwritten home of the pinned original KerML and SysML owner preparation
//! (not generated): `native_pilot` (the prepared sessions), `language_pack`
//! (saved sessions and bindings), and the preparation recipes they share.
//!
//! The K01 native draft pipeline that this module held (physical drafts,
//! cohorts, retained stores and the handwritten Rust KerML policies) was
//! removed in step 3 of docs/old-path-inventory.md. The frozen-engine text
//! load is `crate::text_capability` and `crate::frozen_load`; the names below
//! re-export them under their former paths.
pub(crate) use crate::text_capability::{
    EMPTY_BEHAVIOR_SHA, EXPRESSION_SHA, EXPRESSIONS, KERML, KERML_SHA, SCHEMA,
};

/// Former names of `crate::text_capability` items.
pub use crate::text_capability::{
    KerMLNonuniqueCapability, KerMLStringRules,
    kerml_capability as native_v80_kerml_project_capability, kerml_string_rules,
    nonunique_draft_capability, sysml_capability as sysml_frozen_inner_capability,
    sysml_setting_providers,
};

#[path = "draft_entry.rs"]
mod entry;
pub(crate) use entry::MinimalDraftInputs;

#[path = "sysml_native_inputs.rs"]
mod sysml_native_inputs;

#[path = "language_pack.rs"]
pub mod language_pack;
#[path = "native_pilot.rs"]
pub mod native_pilot;
