use super::*;
use crate::project::{Meta, Project, Usage};

fn archive(uri: &str, version: &str, uses: &[(&str, Option<&str>)]) -> ProjectArchive {
    ProjectArchive {
        uri: uri.into(),
        project: Project {
            name: file_name(uri),
            version: version.into(),
            usage: uses
                .iter()
                .map(|(resource, constraint)| Usage {
                    resource: (*resource).into(),
                    version_constraint: constraint.map(str::to_owned),
                    ..Usage::default()
                })
                .collect(),
            ..Project::default()
        },
        root: String::new(),
        meta: Meta::default(),
        sources: Vec::new(),
        notes: Vec::new(),
        warnings: Vec::new(),
    }
}

const SEMANTIC: &str = "https://www.omg.org/spec/KerML/20250201/Semantic-Library.kpar";

#[test]
fn standard_library_iris_are_recognized() {
    assert!(is_standard_library(SEMANTIC));
    assert!(is_standard_library(
        "https://www.omg.org/spec/SysML/20250201/Quantities-and-Units-Domain-Library.kpar"
    ));
    assert!(!is_standard_library(
        "https://www.omg.org/spec/SysML/2025/Systems-Library.kpar"
    ));
    // The name OMG's Analysis and Geometry archives use.
    assert!(is_standard_library(
        "https://www.omg.org/spec/SysML/20250201/Quantities-and-Units-Library.kpar"
    ));
    assert!(!is_standard_library(
        "https://www.omg.org/spec/SysML/20250201/Vehicles.kpar"
    ));
    assert!(!is_standard_library(
        "https://example.com/Semantic-Library.kpar"
    ));
}

#[test]
fn a_chain_resolves_dependencies_first() -> Result<(), Vec<KparError>> {
    let candidates = [
        archive(
            "app.kpar",
            "1.0.0",
            &[("https://example.com/p/vehicles.kpar", Some("^2.1"))],
        ),
        archive(
            "deps/vehicles.kpar",
            "2.3.0",
            &[("urn:x:units%20lib.kpar", None), (SEMANTIC, Some("1.0.0"))],
        ),
        archive("deps/units%20lib.kpar", "0.1.0", &[]),
        archive("deps/unused.kpar", "0.1.0", &[]),
    ];
    let resolution = resolve(&candidates, &[0])?;
    assert_eq!(resolution.order, [2, 1, 0]);
    assert_eq!(resolution.standard_library.len(), 1);
    assert_eq!(resolution.standard_library[0].by, "deps/vehicles.kpar");
    Ok(())
}

#[test]
fn shared_dependencies_load_once() -> Result<(), Vec<KparError>> {
    let candidates = [
        archive("a.kpar", "1.0.0", &[("urn:x:c.kpar", None)]),
        archive(
            "b.kpar",
            "1.0.0",
            &[("urn:x:c.kpar", None), ("urn:x:a.kpar", None)],
        ),
        archive("c.kpar", "1.0.0", &[]),
    ];
    assert_eq!(resolve(&candidates, &[0, 1])?.order, [2, 0, 1]);
    Ok(())
}

fn errors(candidates: &[ProjectArchive]) -> Vec<String> {
    resolve(candidates, &[0])
        .err()
        .unwrap_or_default()
        .iter()
        .map(|e| {
            assert_eq!(e.kind, ErrorKind::Dependency);
            e.to_string()
        })
        .collect()
}

#[test]
fn every_problem_is_reported() {
    let found = errors(&[
        archive(
            "app.kpar",
            "1.0.0",
            &[
                ("urn:x:missing.kpar", None),
                ("urn:x:old.kpar", Some(">=2")),
                ("urn:x:twice.kpar", None),
                ("urn:x:odd.kpar", Some("1.0")),
                ("urn:x:old.kpar", Some("not a constraint")),
            ],
        ),
        archive("old.kpar", "1.5.0", &[]),
        archive("x/twice.kpar", "1.0.0", &[]),
        archive("y/twice.kpar", "1.0.0", &[]),
        archive("odd.kpar", "one", &[]),
    ]);
    assert_eq!(found.len(), 5, "{found:#?}");
    for expected in [
        "not resolved",
        "does not satisfy '>=2'",
        "ambiguous",
        "not a Semantic Version",
        "not a Semantic Versioning requirement",
    ] {
        assert!(
            found.iter().any(|e| e.contains(expected)),
            "{expected}: {found:#?}"
        );
    }
}

#[test]
fn mutual_usage_loads_each_archive_once() -> Result<(), Vec<KparError>> {
    let candidates = [
        archive("a.kpar", "1.0.0", &[("urn:x:b.kpar", None)]),
        archive("b.kpar", "1.0.0", &[("urn:x:a.kpar", None)]),
    ];
    assert_eq!(resolve(&candidates, &[0])?.order, [1, 0]);
    Ok(())
}

#[test]
fn file_names_ignore_query_fragment_and_encoding() {
    assert_eq!(
        file_name("https://x.org/a/My%20Lib.kpar?v=1#top"),
        "My Lib.kpar"
    );
    assert_eq!(file_name("plain.kpar"), "plain.kpar");
}

#[test]
fn library_resources_map_to_their_omg_archive() {
    for (uri, expected) in [
        (
            "sysml.library/Kernel%20Libraries/Kernel%20Function%20Library/BaseFunctions.kerml",
            Some("https://www.omg.org/spec/KerML/20250201/Function-Library.kpar"),
        ),
        (
            "sysml.library/Systems%20Library/Parts.sysml",
            Some("https://www.omg.org/spec/SysML/20250201/Systems-Library.kpar"),
        ),
        (
            "sysml.library/Domain%20Libraries/Quantities%20and%20Units/ISQ.sysml",
            Some(
                "https://www.omg.org/spec/SysML/20250201/Quantities-and-Units-Domain-Library.kpar",
            ),
        ),
        (
            "lib/Semantic-Library.kpar!/Base.kerml",
            Some("https://www.omg.org/spec/KerML/20250201/Semantic-Library.kpar"),
        ),
        ("lib/Other.kpar!/Base.kerml", None),
        ("models/Vehicles.sysml", None),
    ] {
        assert_eq!(standard_library_iri(uri).as_deref(), expected, "{uri}");
    }
}
