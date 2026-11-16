//! Handwritten verification host reusing the existing pack refusal controls.
#![allow(dead_code)]
#[path = "../tests/language_pack_equivalence.rs"]
mod existing;
fn main() -> Result<(), String> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected host-built pack path")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let pack = intersect_model::definitions::pack::LanguagePack::read(bytes)
        .map_err(|error| error.to_string())?;
    let bytes = intersect_kerml::language_pack::without_library_part(&pack)?;
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || existing::negative_controls(&bytes))
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "pack refusal replay panicked".to_owned())?
}
