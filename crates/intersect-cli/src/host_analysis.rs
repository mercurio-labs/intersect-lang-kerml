//! Handwritten CLI selection and reporting for SysML analysis profiles.
use super::{UserFile, evaluation::source_position};
use intersect_kerml_eval::{
    EvalMode, Evaluator, Model,
    analysis_execution::{AnalysisError, AnalysisInput},
    continuous_analysis::{ContinuousBinding, ContinuousOutcome},
    stateful_analysis::{StatefulBinding, StatefulEvent, StatefulOutcome},
};
use intersect_run_catalog::{Capability, Engine, is_versioned_id};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const INSTALLED_ENGINES: &[Engine] = &[
    Engine::ScalarAffine,
    Engine::StateAffine,
    Engine::CoupledOde,
];

#[derive(Default)]
pub(super) struct Arguments {
    manifest: Option<PathBuf>,
    profile: Option<String>,
    case: Option<String>,
    inputs: Option<PathBuf>,
    events: Option<PathBuf>,
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
            "--events" => self.events.replace(value.into()).is_some(),
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
    state_machine: Option<StatefulBinding>,
    continuous: Option<ContinuousBinding>,
}

enum Execution {
    Scalar,
    Stateful(StatefulBinding),
    CoupledOde(ContinuousBinding),
}

impl Execution {
    fn engine(&self) -> Engine {
        match self {
            Self::Scalar => Engine::ScalarAffine,
            Self::Stateful(_) => Engine::StateAffine,
            Self::CoupledOde(_) => Engine::CoupledOde,
        }
    }
}

fn select_profile(source: &str, requested: Option<&str>) -> Result<(String, Execution), String> {
    let Manifest { mut execution } = toml::from_str(source).map_err(|error| error.to_string())?;
    if execution.profiles.is_empty() {
        return Err("the [execution] table must declare a profile".into());
    }
    for (name, profile) in &execution.profiles {
        if !identifier(name) {
            return Err(format!("invalid execution profile name {name:?}"));
        }
        if !is_versioned_id(&profile.engine) {
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
        .ok_or("choose an execution profile with --profile or declare a default")?
        .to_owned();
    let profile = execution
        .profiles
        .remove(&name)
        .ok_or_else(|| format!("unknown execution profile {name:?}"))?;
    let execution = match (
        Engine::select(&profile.engine, INSTALLED_ENGINES),
        profile.state_machine,
        profile.continuous,
    ) {
        (Some(Engine::ScalarAffine), None, None) => Execution::Scalar,
        (Some(Engine::StateAffine), Some(binding), None) => Execution::Stateful(binding),
        (Some(Engine::CoupledOde), None, Some(binding)) => Execution::CoupledOde(binding),
        (Some(Engine::ScalarAffine), _, _) => {
            return Err(
                "the scalar engine does not take a state_machine or continuous binding".into(),
            );
        }
        (Some(Engine::StateAffine), _, _) => {
            return Err("the stateful engine requires exactly a state_machine binding".into());
        }
        (Some(Engine::CoupledOde), _, _) => {
            return Err("the coupled ODE engine requires exactly a continuous binding".into());
        }
        _ => {
            return Err(format!(
                "execution engine {} is unavailable",
                profile.engine
            ));
        }
    };
    Ok((name, execution))
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
    execution: Execution,
    case: String,
    inputs: Vec<AnalysisInput>,
    events: Vec<StatefulEvent>,
    manifest_sha256: String,
    inputs_sha256: Option<String>,
    events_sha256: Option<String>,
}

pub(super) fn load(arguments: &Arguments) -> Result<Selected, String> {
    let path = arguments
        .manifest
        .as_ref()
        .ok_or("missing execution manifest")?;
    let source = read_config(path)?;
    let (name, execution) = select_profile(&source, arguments.profile.as_deref())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !execution.engine().supports(Capability::ScenarioEvents) && arguments.events.is_some() {
        return Err("--events requires a stateful execution profile".into());
    }
    let input_source = arguments.inputs.as_deref().map(read_config).transpose()?;
    let inputs = if let (Some(path), Some(source)) = (&arguments.inputs, &input_source) {
        serde_json::from_str(source).map_err(|error| format!("{}: {error}", path.display()))?
    } else {
        Vec::new()
    };
    let event_source = arguments.events.as_deref().map(read_config).transpose()?;
    let events = if let (Some(path), Some(source)) = (&arguments.events, &event_source) {
        serde_json::from_str(source).map_err(|error| format!("{}: {error}", path.display()))?
    } else {
        Vec::new()
    };
    Ok(Selected {
        name,
        execution,
        case: arguments.case.clone().ok_or("missing analysis case")?,
        inputs,
        events,
        manifest_sha256: digest(source.as_bytes()),
        inputs_sha256: input_source
            .as_ref()
            .map(|source| digest(source.as_bytes())),
        events_sha256: event_source
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
        let result = match &self.execution {
            Execution::Scalar => evaluator
                .execute_scalar_analysis(&self.case, &self.inputs)
                .map(|solution| {
                    if json_output {
                        self.print_json("solution", json!(solution), user, pack, library_key);
                    } else {
                        println!(
                            "{} [{}] {} = {}",
                            solution.case,
                            self.name,
                            solution.result_name,
                            solution.result.quantity
                        );
                    }
                }),
            Execution::Stateful(binding) => evaluator
                .execute_stateful_analysis(&self.case, &self.inputs, binding, &self.events)
                .map(|trace| {
                    if json_output {
                        self.print_json("trace", json!(trace), user, pack, library_key);
                    } else {
                        for step in trace.steps {
                            let outcome = match &step.outcome {
                                StatefulOutcome::Start => "start",
                                StatefulOutcome::Transition { .. } => "transition",
                                StatefulOutcome::Rejected { .. } => "rejected",
                            };
                            println!(
                                "{} {} {} {} {} = {}",
                                step.at,
                                step.event.as_deref().unwrap_or("-"),
                                outcome,
                                step.state_name,
                                step.solution.result_name,
                                step.solution.result.quantity
                            );
                        }
                    }
                }),
            Execution::CoupledOde(binding) => evaluator
                .execute_continuous_analysis(&self.case, &self.inputs, binding)
                .map(|trace| {
                    if json_output {
                        self.print_json("trace", json!(trace), user, pack, library_key);
                    } else {
                        for step in trace.steps {
                            match step.outcome {
                                ContinuousOutcome::Sample => {
                                    println!(
                                        "{} sample {} {}",
                                        step.at, step.state_name, step.value
                                    );
                                }
                                ContinuousOutcome::Crossing { event, .. } => {
                                    println!(
                                        "{} crossing {} {} {}",
                                        step.at, event, step.state_name, step.value
                                    );
                                }
                            }
                        }
                    }
                }),
        };
        if let Err(error) = result {
            self.report_error(&error, user, json_output);
            return Ok(false);
        }
        Ok(true)
    }

    fn print_json(
        &self,
        field: &str,
        result: Value,
        user: &[UserFile],
        pack: &[u8],
        library_key: &str,
    ) {
        let sources: Vec<_> = user
            .iter()
            .map(|file| {
                json!({
                    "uri": file.source.uri,
                    "sha256": digest(file.source.text.as_bytes()),
                })
            })
            .collect();
        let engine = self.execution.engine();
        let mut report = json!({
            "profile": self.name,
            "engine": engine.id(),
            (field): result,
            "provenance": {
                "manifest_sha256": self.manifest_sha256,
                "pack_sha256": digest(pack),
                "library_key": library_key,
                "inputs_sha256": self.inputs_sha256,
                "sources": sources,
            },
        });
        if engine.supports(Capability::ScenarioEvents) {
            report["provenance"]["events_sha256"] = json!(self.events_sha256);
        }
        println!("{report}");
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
        assert!(
            matches!(select_profile(MANIFEST, None)?, (name, Execution::Scalar) if name == "scalar")
        );
        assert_eq!(select_profile(MANIFEST, Some("scalar"))?.0, "scalar");
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
    fn refuses_a_catalog_engine_without_a_sysml_adapter() -> Result<(), String> {
        let ode = MANIFEST.replace(Engine::ScalarAffine.id(), Engine::Ode.id());
        let failure = select_profile(&ode, None).err().ok_or("expected failure")?;
        assert_eq!(
            failure,
            format!("execution engine {} is unavailable", Engine::Ode.id())
        );
        Ok(())
    }

    #[test]
    fn stateful_profile_requires_the_wiring_table() -> Result<(), String> {
        let composed = MANIFEST.replace(Engine::ScalarAffine.id(), Engine::StateAffine.id());
        assert!(select_profile(&composed, None).is_err());
        let wired = format!(
            "{composed}\n[execution.profiles.scalar.state_machine]\nmachine = \"C::Tank::fillMode\"\nstate_output = \"durationSeconds\"\nanalysis_input = \"fillTime\"\nunit = \"s\"\n"
        );
        assert!(
            matches!(select_profile(&wired, None)?, (_, Execution::Stateful(binding)) if binding.analysis_input == "fillTime")
        );
        assert!(
            select_profile(
                &wired.replace(Engine::StateAffine.id(), Engine::ScalarAffine.id()),
                None
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn coupled_profile_requires_only_its_continuous_binding() -> Result<(), String> {
        let missing = MANIFEST.replace(Engine::ScalarAffine.id(), Engine::CoupledOde.id());
        assert!(select_profile(&missing, None).is_err());
        let example = include_str!("../../../docs/examples/sysml-execution-profile/language.toml");
        assert!(matches!(
            select_profile(example, Some("thermostat"))?,
            (_, Execution::CoupledOde(binding))
                if binding.state_input == "temperatureC" && binding.crossings.len() == 2
        ));
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
