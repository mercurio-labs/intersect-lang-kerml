//! Handwritten integration suite for the archive host bridge.
#[cfg(all(feature = "components", not(target_arch = "wasm32")))]
mod hosted;
mod kpar;
