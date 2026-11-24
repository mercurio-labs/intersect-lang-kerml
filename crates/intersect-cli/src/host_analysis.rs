//! Handwritten CLI selection and reporting for SysML analysis profiles.
use super::{UserFile, evaluation::source_position};
use intersect_kerml_eval::{
    EvalMode, Evaluator, Model,
    analysis_execution::{AnalysisError, AnalysisInput, ENGINE_ID},
};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct Arguments {
    manifest: Option<PathBuf>,
    profile: Option<String>,
    case: Option<String>,
    inputs: Option<PathBuf>,
}

impl Arguments {
    pub(super) fn set(&mut self, flag: &str, value: String) -> Result<(), String> {
        if value.trim().is_empty() || value.starts_with("--") {
            return Err(format!("{flag} needs a value"));
        }
        let previous = match flag {
            "--execution-profile" => self.manifest.replace(value.into()).is_some(),
            "--profile" => self.profile.replace(value).is_some(),
            "--case" => self.case.replace(value).is_some(),
            "--inputs" => self.inputs.replace(value.into()).is_some(),
            _ => return Err(format!("unknown analysis option '{flag}'")),
        };
        if previous {
            return Err(format!("{flag} may appear only once"));
        }
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.manifest.is_none() || self.case.is_none() {
            return Err(
                "analyze requires --execution-profile LANGUAGE.toml and --case QNAME".into(),
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Manifest {
    execution: ExecutionManifest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecutionManifest {
    default: Option<String>,
    profiles: BTreeMap<String, Profile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    engine: String,
}

fn select_profile(source: &str, requested: Option<&str>) -> Result<String, String> {
    let Manifest { execution } = toml::from_str(source).map_err(|error| error.to_string())?;
    if execution.profiles.is_empty() {
        return Err("the [execution] table must declare a profile".into());
    }
    for (name, profile) in &execution.profiles {
        if !identifier(name) {
            return Err(format!("invalid execution profile name {name:?}"));
        }
        if !profile.engine.split_once('@').is_some_and(|(id, version)| {
            identifier(id)
                && !version.starts_with('0')
                && version.bytes().all(|byte| byte.is_ascii_digit())
                && version.parse::<u32>().is_ok()
        }) {
            return Err(format!("profile {name:?} needs a versioned engine ID"));
        }
    }
    if let Some(default) = &execution.default
        && !execution.profiles.contains_key(default)
    {
        return Err(format!("unknown default execution profile {default:?}"));
    }
    let name = requested
        .or(execution.default.as_deref())
        .ok_or("choose an execution profile with --profile or declare a default")?;
    let profile = execution
        .profiles
        .get(name)
        .ok_or_else(|| format!("unknown execution profile {name:?}"))?;
    if profile.engine != ENGINE_ID {
        return Err(format!(
            "execution engine {} is unavailable",
            profile.engine
        ));
    }
    Ok(name.to_owned())
}

fn identifier(value: &str) -> bool {
    value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

pub(super) struct Selected {
    name: String,
    case: String,
    inputs: Vec<AnalysisInput>,
    manifest_sha256: String,
    inputs_sha256: Option<String>,
}

pub(super) fn load(arguments: &Arguments) -> Result<Selected, String> {
    let path = arguments
        .manifest
        .as_ref()
        .ok_or("missing execution manifest")?;
    let source = read_config(path)?;
    let name = select_profile(&source, arguments.profile.as_deref())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let input_source = arguments.inputs.as_deref().map(read_config).transpose()?;
    let inputs = if let (Some(path), Some(source)) = (&arguments.inputs, &input_source) {
        serde_json::from_str(source).map_err(|error| format!("{}: {error}", path.display()))?
    } else {
        Vec::new()
    };
    Ok(Selected {
        name,
        case: arguments.case.clone().ok_or("missing analysis case")?,
        inputs,
        manifest_sha256: digest(source.as_bytes()),
        inputs_sha256: input_source
            .as_ref()
            .map(|source| digest(source.as_bytes())),
    })
}

fn read_config(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Selected {
    pub(super) fn run(
        &self,
        model: &Model,
        user: &[UserFile],
        pack: &[u8],
        library_key: &str,
        json_output: bool,
    ) -> Result<bool, String> {
        let evaluator =
            Evaluator::new(model, EvalMode::Extended).map_err(|error| error.to_string())?;
        let solution = match evaluator.execute_scalar_analysis(&self.case, &self.inputs) {
            Ok(solution) => solution,
            Err(error) => {
                self.report_error(&error, user, json_output);
                return Ok(false);
            }
        };
        if json_output {
            let sources: Vec<_> = user
                .iter()
                .map(|file| {
                    json!({
                        "uri": file.source.uri,
                        "sha256": digest(file.source.text.as_bytes()),
                    })
                })
                .collect();
            println!(
                "{}",
                json!({
                    "profile": self.name,
                    "engine": ENGINE_ID,
                    "solution": solution,
                    "provenance": {
                        "manifest_sha256": self.manifest_sha256,
                        "pack_sha256": digest(pack),
                        "library_key": library_key,
                        "inputs_sha256": self.inputs_sha256,
                        "sources": sources,
                    },
                })
            );
        } else {
            println!(
                "{} [{}] {} = {}",
                solution.case, self.name, solution.result_name, solution.result.quantity
            );
        }
        Ok(true)
    }

    fn report_error(&self, error: &AnalysisError, user: &[UserFile], json_output: bool) {
        let location = error.source.as_ref().and_then(|source| {
            let file = user.iter().find(|file| file.source.uri == source.uri)?;
            let (line, column) = source_position(file, source.start);
            Some((&file.path, line, column))
        });
        if json_output {
            let mut diagnostic = json!({
                "phase": "analysis",
                "severity": "error",
                "profile": self.name,
                "case": self.case,
                "code": error.code,
                "message": error.message,
                "source": error.source,
            });
            if let Some((path, line, column)) = location {
                diagnostic["file"] = json!(path);
                diagnostic["line"] = json!(line);
                diagnostic["column"] = json!(column);
            }
            println!("{diagnostic}");
        } else {
            let prefix = location
                .map(|(path, line, column)| format!("{}:{line}:{column}: ", path.display()))
                .unwrap_or_default();
            println!("{prefix}error[analysis]: {}", error.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"
[package]
package_id = "example.sysml"

[execution]
default = "scalar"

[execution.profiles.scalar]
engine = "intersect.scalar-affine@1"

[execution.profiles.future]
engine = "intersect.scalar-affine@2"
"#;

    #[test]
    fn uses_default_and_checks_the_selected_engine_version() -> Result<(), String> {
        assert_eq!(select_profile(MANIFEST, None)?, "scalar");
        assert_eq!(select_profile(MANIFEST, Some("scalar"))?, "scalar");
        let missing = select_profile(MANIFEST, Some("other"))
            .err()
            .ok_or("expected failure")?;
        assert!(missing.contains("unknown execution profile"));
        let future = select_profile(MANIFEST, Some("future"))
            .err()
            .ok_or("expected failure")?;
        assert!(future.contains("intersect.scalar-affine@2 is unavailable"));
        Ok(())
    }

    #[test]
    fn refuses_malformed_and_unselected_profiles() {
        let no_default = MANIFEST.replace("default = \"scalar\"", "");
        assert!(select_profile(&no_default, None).is_err());
        assert!(select_profile(&no_default, Some("scalar")).is_ok());
        for malformed in [
            MANIFEST.replace("default = \"scalar\"", "default = \"other\""),
            MANIFEST.replace("intersect.scalar-affine@1", "intersect.scalar-affine"),
            MANIFEST.replace("intersect.scalar-affine@1", "intersect.scalar-affine@01"),
            MANIFEST.replace(
                "[execution.profiles.scalar]",
                "[execution.profiles.\"a b\"]",
            ),
            MANIFEST.replace("[execution.profiles.scalar]", "[execution.profiles.future]"),
        ] {
            assert!(select_profile(&malformed, None).is_err(), "{malformed}");
        }
    }
}
