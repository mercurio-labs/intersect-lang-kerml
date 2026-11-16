//! Handwritten host support (not generated): bind the two Pilot program
//! owners for parsing, shared by the `sysml_pack` example and the
//! `language_pack_equivalence` and `frozen_rule_probes` tests so all bind the
//! same languages. The binding itself is library code in
//! `intersect_kerml::frozen_load`; this file keeps the names those hosts use.
#[allow(unused_imports)]
pub use intersect_kerml::frozen_load::{
    BoundLanguage, KerMLCapability, SysMLCapability, limits, with_kerml, with_sysml,
};
