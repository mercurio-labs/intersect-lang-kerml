use super::*;

/// The `.project.json` of OMG's Kernel Function Library archive
/// (https://www.omg.org/spec/KerML/20250201/Function-Library.kpar).
const FUNCTION_LIBRARY_PROJECT: &str = r#"{
  "name": "Kernel Function Library",
  "description": "Standard function library for the Kernel Modeling Language (KerML)",
  "version": "1.0.0",
  "usage": [
    {
      "resource": "https://www.omg.org/spec/KerML/20250201/Semantic-Library.kpar",
      "versionConstraint": "1.0.0"
    },
    {
      "resource": "https://www.omg.org/spec/KerML/20250201/Data-Type-Library.kpar",
      "versionConstraint": "1.0.0"
    }
  ]
}"#;

const FUNCTION_LIBRARY_META: &str = r#"{
  "index": {
    "BaseFunctions": "BaseFunctions.kerml",
    "VectorFunctions": "VectorFunctions.kerml"
  },
  "created": "2025-03-13T00:00:00Z",
  "metamodel": "https://www.omg.org/spec/KerML/20250201"
}"#;

#[test]
fn library_project_document_parses() -> Result<(), MetadataError> {
    let project = Project::parse(FUNCTION_LIBRARY_PROJECT.as_bytes())?;
    assert_eq!(project.name, "Kernel Function Library");
    assert_eq!(project.version, "1.0.0");
    assert_eq!(project.usage.len(), 2);
    assert_eq!(
        project.usage[0].resource,
        "https://www.omg.org/spec/KerML/20250201/Semantic-Library.kpar"
    );
    assert_eq!(
        project.usage[1].version_constraint.as_deref(),
        Some("1.0.0")
    );
    assert!(project.extra.is_empty());
    Ok(())
}

#[test]
fn library_meta_document_parses() -> Result<(), MetadataError> {
    let meta = Meta::parse(FUNCTION_LIBRARY_META.as_bytes())?;
    assert_eq!(meta.index["BaseFunctions"], "BaseFunctions.kerml");
    assert_eq!(
        meta.metamodel.as_deref(),
        Some("https://www.omg.org/spec/KerML/20250201")
    );
    assert_eq!(meta.includes_derived, None);
    Ok(())
}

#[test]
fn documents_round_trip_including_unknown_members() -> Result<(), MetadataError> {
    let text = r#"{"name":"P","version":"0.1.0","publisher":"Acme","topic":["x"],
        "usage":[{"resource":"urn:example:q","note":7}]}"#;
    let project = Project::parse(text.as_bytes())?;
    assert_eq!(project.extra["publisher"], "Acme");
    assert_eq!(project.usage[0].extra["note"], 7);
    let again = Project::parse(project.to_value().to_string().as_bytes())?;
    assert_eq!(again, project);

    let meta = r#"{"index":{},"created":"2026-11-10T08:00:00.5+01:00","includesDerived":false,
        "checksum":{"a.sysml":{"value":"00","algorithm":"SHA256"}},"generator":"x"}"#;
    let meta = Meta::parse(meta.as_bytes())?;
    assert_eq!(meta.extra["generator"], "x");
    assert_eq!(Meta::parse(meta.to_value().to_string().as_bytes())?, meta);
    Ok(())
}

#[test]
fn bad_documents_are_refused_with_the_reason() {
    for (text, reason) in [
        ("[]", "not a JSON object"),
        ("{", "not JSON"),
        (r#"{"version":"1"}"#, "'name'"),
        (r#"{"name":"P"}"#, "'version'"),
        (r#"{"name":1,"version":"1"}"#, "'name' must be a string"),
        (
            r#"{"name":"P","version":"1","usage":[{}]}"#,
            "no 'resource'",
        ),
        (
            r#"{"name":"P","version":"1","usage":[{"resource":"not an iri"}]}"#,
            "not an IRI",
        ),
        (r#"{"name":"P","version":"1","website":"x y"}"#, "'website'"),
        (r#"{"name":"P","version":"1","topic":[1]}"#, "'topic'"),
    ] {
        match Project::parse(text.as_bytes()) {
            Err(error) => assert!(error.message.contains(reason), "{text}: {error}"),
            Ok(_) => panic!("{text} was accepted"),
        }
    }
    for (text, reason) in [
        (r#"{"created":"2026-01-01T00:00:00Z"}"#, "'index'"),
        (r#"{"index":{}}"#, "'created'"),
        (r#"{"index":{},"created":"yesterday"}"#, "RFC 3339"),
        (
            r#"{"index":{"A":1},"created":"2026-01-01T00:00:00Z"}"#,
            "index entry 'A'",
        ),
        (
            r#"{"index":{},"created":"2026-01-01T00:00:00Z","checksum":{"a":{"value":"0"}}}"#,
            "'algorithm'",
        ),
        (
            r#"{"index":{},"created":"2026-01-01T00:00:00Z","includesImplied":"no"}"#,
            "boolean",
        ),
    ] {
        match Meta::parse(text.as_bytes()) {
            Err(error) => assert!(error.message.contains(reason), "{text}: {error}"),
            Ok(_) => panic!("{text} was accepted"),
        }
    }
}

#[test]
fn date_times_follow_rfc_3339() {
    for good in [
        "2025-03-13T00:00:00Z",
        "2025-03-13t23:59:60.123z",
        "2025-03-13T10:00:00-05:30",
    ] {
        assert!(is_date_time(good), "{good}");
    }
    for bad in [
        "2025-03-13",
        "2025-13-01T00:00:00Z",
        "2025-03-13T24:00:00Z",
        "2025-03-13T00:00:00",
        "2025-03-13T00:00:00.Z",
        "2025-03-13T00:00:00+0100",
        "2025-03-13T00:00:00Zjunk",
    ] {
        assert!(!is_date_time(bad), "{bad}");
    }
}
