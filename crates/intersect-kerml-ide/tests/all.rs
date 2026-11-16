mod completion;
mod evaluation;
mod navigation;
mod visualization;

/// The section ranges an embedder hashes come only from bytes that pass the
/// pack's own header and manifest checks.
#[test]
fn pack_sections_refuse_bytes_that_are_not_a_pack() {
    for bytes in [&b""[..], &b"IXLPACK"[..], &[0u8; 64][..]] {
        let error = intersect_kerml_ide::Host::pack_sections(bytes)
            .expect_err("bytes that are not a pack have no sections");
        assert!(error.starts_with("Language pack: "), "{error}");
    }
}
