//! Handwritten milestone controls of the real language/stdlib host path.
//! Whole-library admission and model/diagnostic equivalence are milestone-only
//! under AGENTS.md. The portable identity/catalog controls remain quick tests.
#[allow(dead_code)]
#[path = "../examples/pilot_examples.rs"]
mod pilot_examples;

use std::path::{Path, PathBuf};

fn input(name: &str) -> Result<PathBuf, String> {
    std::env::var_os(name).map(PathBuf::from).ok_or_else(|| {
        format!("Set {name}; equivalence requires an explicitly prepared pack/corpus")
    })
}

fn run(fixed: bool) -> Result<serde_json::Value, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("Repository root")?;
    let pack = input("INTERSECT_PACKAGE_EQUIVALENCE_PACK")?;
    let corpus = input("INTERSECT_PILOT_EXAMPLES")?;
    let output = input("INTERSECT_PACKAGE_EQUIVALENCE_REPORTS")?;
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let report = output.join(if fixed {
        "fixed-set.json"
    } else {
        "pilot-corpus.json"
    });
    let mut args = vec![
        root.display().to_string(),
        corpus.display().to_string(),
        "--pack".into(),
        pack.display().to_string(),
        "--verify-package-load".into(),
        "--report".into(),
        report.display().to_string(),
    ];
    if fixed {
        for file in [
            "Types.kerml",
            "Expressions.kerml",
            "PartTest.sysml",
            "ConnectionTest.sysml",
            "RequirementTest.sysml",
            "ConstraintTest.sysml",
        ] {
            args.extend(["--file".into(), file.into()]);
        }
        for file in ["valid.sysml", "invalid.sysml"] {
            args.extend([
                "--user".into(),
                root.join("fixtures/project-usages/fixed")
                    .join(file)
                    .display()
                    .to_string(),
            ]);
        }
    }
    let worker = std::thread::Builder::new()
        .name("package-equivalence".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || pilot_examples::run_with_args(args))
        .map_err(|e| e.to_string())?;
    worker
        .join()
        .map_err(|_| "Package equivalence worker panicked".to_string())??;
    serde_json::from_slice(&std::fs::read(report).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn fixed_user_files_and_six_pilot_examples_match_the_pre_slice_load() -> Result<(), String> {
    let report = run(true)?;
    assert_eq!(report["files"], 8);
    assert_eq!(report["package_equivalence"]["failed"], 0);
    assert_eq!(
        report["package_equivalence"]["verified_folders"],
        report["folders"].as_array().ok_or("folders")?.len()
    );
    assert!(
        report["inputs"]
            .as_object()
            .ok_or("input hashes")?
            .contains_key("user-fixed/invalid.sysml")
    );
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn full_pinned_pilot_corpus_matches_the_pre_slice_load() -> Result<(), String> {
    let report = run(false)?;
    assert_eq!(
        report["files"], 157,
        "Use the complete example corpus at the pinned Pilot revision"
    );
    assert_eq!(report["package_equivalence"]["failed"], 0);
    assert_eq!(
        report["package_equivalence"]["verified_folders"],
        report["folders"].as_array().ok_or("folders")?.len()
    );
    assert_eq!(
        report["inputs"].as_object().ok_or("input hashes")?.len(),
        157
    );
    Ok(())
}
