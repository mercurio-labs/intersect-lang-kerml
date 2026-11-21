//! Host-only identity of this package's owned inputs. Dependencies expose their
//! installed identities through the compiler components in language_pack.rs.

fn main() -> Result<(), String> {
    intersect_build_support::emit_scoped("INTERSECT_KERML_SOURCE_IDENTITY")
}
