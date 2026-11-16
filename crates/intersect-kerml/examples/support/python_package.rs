//! Handwritten build host for generated Python artifacts and local wheels.
use intersect_kerml::sdk;
use intersect_model::definitions::pack::LanguagePack;
use std::path::{Path, PathBuf};

/// Build in a private sibling directory. The generated package contains its own
/// pack, so the manifest and runtime resource are published by one rename.
pub fn stage(
    root: &Path,
    output: &Path,
    bytes: &[u8],
    wheel: bool,
) -> Result<(PathBuf, PathBuf), String> {
    let output = std::path::absolute(output).map_err(|e| e.to_string())?;
    let parent = output.parent().ok_or("Python output needs a parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = output
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("Invalid Python output name")?;
    let staging = parent.join(format!(".{name}.{}.staging", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| format!("Cannot stage Python output: {e}"))?;
    let package = staging.join("intersect_kerml");
    std::fs::create_dir_all(package.join("resources")).map_err(|e| e.to_string())?;
    let pack = LanguagePack::read(bytes.to_vec()).map_err(|e| e.to_string())?;
    let mut generated = sdk::python_artifacts(&pack)?;
    generated.manifest["distribution_trust"] = serde_json::json!("local-matching-toolchain");
    let exported_names = generated.manifest["classes"]
        .as_object()
        .into_iter()
        .flat_map(|v| v.values())
        .chain(
            generated.manifest["enums"]
                .as_object()
                .into_iter()
                .flat_map(|v| v.values()),
        )
        .filter_map(|v| v["python"].as_str())
        .collect::<Vec<_>>();
    let exports = exported_names.join(", ");
    let public_names = serde_json::to_string(&exported_names).map_err(|e| e.to_string())?;
    let aliases = format!(
        "# GENERATED: combined KerML/SysML metamodel.\nfrom ._generated import {exports}\n__all__ = {public_names}\n"
    );
    generated.files.insert("kerml.py".into(), aliases.clone());
    generated.files.insert("sysml.py".into(), aliases);
    for (name, source) in generated.files {
        std::fs::write(package.join(name), source).map_err(|e| e.to_string())?;
    }
    for name in ["__init__.py", "_api.py", "_native.pyi"] {
        std::fs::copy(
            root.join("python/intersect_kerml").join(name),
            package.join(name),
        )
        .map_err(|e| e.to_string())?;
    }
    std::fs::write(
        package.join("bindings.json"),
        serde_json::to_vec_pretty(&generated.manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(package.join("resources/sysml.pack"), bytes).map_err(|e| e.to_string())?;
    for (input, output) in [
        ("LICENSE", "LICENSE-INTERSECT"),
        ("fixtures/native-pilot/v1/UPSTREAM-LICENSE", "LICENSE-PILOT"),
    ] {
        std::fs::copy(root.join(input), package.join("resources").join(output))
            .map_err(|e| e.to_string())?;
    }
    let native = root
        .join("crates/intersect-kerml-python/Cargo.toml")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let template =
        std::fs::read_to_string(root.join("python/pyproject.toml")).map_err(|e| e.to_string())?;
    let native = native
        .to_str()
        .ok_or("Native manifest path is not Unicode")?;
    let native = if let Some(path) = native.strip_prefix("\\\\?\\UNC\\") {
        format!("//{}", path.replace('\\', "/"))
    } else {
        native
            .strip_prefix("\\\\?\\")
            .unwrap_or(native)
            .replace('\\', "/")
    };
    let quoted = serde_json::to_string(&native).map_err(|e| e.to_string())?;
    let configuration =
        template.replace("\"../crates/intersect-kerml-python/Cargo.toml\"", &quoted);
    std::fs::write(staging.join("pyproject.toml"), configuration).map_err(|e| e.to_string())?;
    if wheel {
        let mut command = std::process::Command::new("maturin");
        command
            .current_dir(&staging)
            .args(["build", "--release", "--locked", "--out", "wheels"]);
        // Identical engine features/profile and shared locked dependencies let
        // the pack tool and extension reuse the expensive native compilation.
        command.arg("--target-dir").arg(
            root.join("target")
                .canonicalize()
                .map_err(|e| e.to_string())?,
        );
        // Development builds can select a local upstream core without baking a
        // workspace path into published dependency manifests.
        if let Ok(configuration) = std::env::var("INTERSECT_CARGO_CONFIG") {
            command.args(["--config", &configuration]);
        }
        let status = command
            .status()
            .map_err(|e| format!("Cannot run maturin: {e}"))?;
        if !status.success() {
            return Err(format!(
                "Wheel build failed; staged artifacts: {}",
                staging.display()
            ));
        }
    }
    Ok((staging, output))
}
