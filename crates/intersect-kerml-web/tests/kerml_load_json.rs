//! The host dispatcher's frozen-engine KerML load transport
//! (`draft/kerml/analyze`, `draft/kerml/project/analyze`).
#[allow(dead_code)]
#[path = "../../../fixtures/kerml-draft-controls.rs"]
mod original;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const URI: &str = "memory://intersect/native-minimal-package/package.kerml";

fn request(text: &str) -> Result<Value, String> {
    Ok(
        json!({"method":"draft/kerml/analyze", "original":original::original_inputs()?,
        "source":{"uri":URI,"text":text}}),
    )
}
fn project(sources: Value) -> Result<Value, String> {
    Ok(json!({"method":"draft/kerml/project/analyze", "version":1,
        "mode":"frozen-kerml-load.v1", "original":original::original_inputs()?,
        "sources":sources}))
}
fn dispatch(value: &Value) -> Result<Value, String> {
    intersect_kerml_web::dispatch(&value.to_string())
}
/// (identity, class) of every element of an exported model.
fn rows(elements: &Value) -> BTreeSet<(String, String)> {
    elements
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            (
                e["id"].as_str().unwrap_or("").to_owned(),
                e["class"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn package_construction_has_the_package_elements() -> Result<(), String> {
    let result = dispatch(&request("package P {}")?)?;
    assert_eq!(result["kind"], "frozen_kerml_load");
    assert_eq!(result["linked"], false);
    assert_eq!(result["language_validated"], false);
    assert_eq!(result["publication_permitted"], false);
    assert_eq!(result["diagnostics"], json!([]));
    let elements = &result["model"]["elements"];
    let frozen = rows(elements);
    assert_eq!(
        frozen
            .iter()
            .map(|(_, c)| c.as_str())
            .collect::<BTreeSet<_>>(),
        ["Namespace", "OwningMembership", "Package"].into()
    );
    let package = elements
        .as_array()
        .and_then(|all| all.iter().find(|e| e["class"] == "Package"))
        .ok_or("no Package")?;
    assert_eq!(package["values"]["declaredName"], "P");
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn project_reports_parse_diagnostics_per_source() -> Result<(), String> {
    let result = dispatch(&project(json!([
        {"uri":"memory://a.kerml","text":"package A { classifier C; }"},
        {"uri":"memory://b.kerml","text":"package B { classifier C }"},
    ]))?)?;
    assert_eq!(result["kind"], "frozen_kerml_load");
    assert_eq!(result["mode"], "frozen-kerml-load.v1");
    assert_eq!(
        result["sources"],
        json!(["memory://a.kerml", "memory://b.kerml"])
    );
    let diagnostics = result["diagnostics"].as_array().ok_or("diagnostics")?;
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics
            .iter()
            .all(|d| d["phase"] == "parse" && d["resource"] == "memory://b.kerml")
    );
    assert!(
        result["model"]["elements"]
            .as_array()
            .is_some_and(|all| all.iter().any(|e| e["values"]["declaredName"] == "A"))
    );
    Ok(())
}

#[test]
fn envelope_and_authority_rejections() -> Result<(), String> {
    let input = request("package P {}")?;
    for (field, value) in [
        ("work", json!(4_000_000_001u64)),
        ("package", json!({})),
        ("name_object", json!(format!("{URI}#2"))),
    ] {
        let mut bad = input.clone();
        bad[field] = value;
        assert!(dispatch(&bad).is_err(), "accepted {field}");
    }
    let mut bad = input.clone();
    bad["source"]["text"] = json!("x".repeat(256001));
    assert!(
        dispatch(&bad)
            .err()
            .is_some_and(|error| error.contains("model source byte"))
    );
    let mut bad = input.clone();
    bad["original"]["bundle"]["sources"][0]["text"] = json!("x".repeat(1000001));
    assert!(
        dispatch(&bad)
            .err()
            .is_some_and(|error| error.contains("original source byte"))
    );
    let mut bad = input;
    bad["padding"] = json!("x".repeat(4000001));
    assert!(dispatch(&bad).is_err());
    let sources = json!([{"uri":"memory://a.kerml","text":"package A {}"}]);
    let mut bad = project(sources.clone())?;
    bad["mode"] = json!("basic-kerml-project.v1");
    assert!(dispatch(&bad).is_err());
    let mut bad = project(sources.clone())?;
    bad["sources"] = json!([]);
    assert!(dispatch(&bad).is_err());
    let mut bad = project(json!([
        {"uri":"memory://a.kerml","text":"package A {}"},
        {"uri":"memory://a.kerml","text":"package B {}"},
    ]))?;
    assert!(dispatch(&bad).is_err());
    bad = project(sources)?;
    bad["bindings"] = json!("{}");
    assert!(dispatch(&bad).is_err());
    Ok(())
}
