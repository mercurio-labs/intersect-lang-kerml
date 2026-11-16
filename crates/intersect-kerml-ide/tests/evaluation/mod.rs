use intersect_kerml_ide::Host;
use serde_json::{Value, json};

fn request(host: &mut Host, method: &str, params: Value) -> Result<Value, String> {
    host.dispatch(json!({"method":method,"params":params}))
}

#[test]
fn evaluation_requires_an_explicit_current_project_revision() {
    let mut host = Host::default();
    for method in ["evaluation/sites", "evaluation/evaluate"] {
        assert!(request(&mut host, method, json!({})).is_err_and(|e| e.contains("stale")));
        assert!(
            request(&mut host, method, json!({"revision":0}))
                .is_err_and(|e| e.contains("No current project"))
        );
    }
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn source_evaluation_uses_context_and_preserves_outcomes() -> Result<(), String> {
    let bytes = std::fs::read(std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut host = Host::default();
    host.load_pack_for_analysis(bytes)?;
    let uri = "file:///evaluation.sysml";
    let text = "// 🌕\r\npackage Evaluation { private import ScalarValues::*; part def Rover { attribute count : Integer = 6; attribute answer : Integer = count * 7; attribute big : Integer = 2147483647 * 4194304 + 4194305; attribute empty : Integer[0..*] = (); attribute unset : Integer; attribute missing : Integer = unset + 1; } }";
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":1,"sources":[{"uri":uri,"text":text}]}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let catalog = request(
        &mut host,
        "evaluation/sites",
        json!({"revision":1,"uri":uri}),
    )?;
    let sites = catalog["sites"].as_array().ok_or("Missing sites")?;
    let evaluate = |host: &mut Host, label: &str| -> Result<Value, String> {
        let site = sites
            .iter()
            .find(|s| {
                s["kind"] == "feature-value"
                    && s["label"].as_str().is_some_and(|s| s.ends_with(label))
            })
            .ok_or_else(|| format!("Missing {label}: {catalog}"))?;
        request(
            host,
            "evaluation/evaluate",
            json!({"revision":1,"expressionId":site["id"]}),
        )
    };
    let answer = evaluate(&mut host, "::answer")?;
    assert_eq!(answer["status"], "values", "{answer}");
    assert_eq!(
        answer["outcome"]["values"][0],
        json!({"kind":"int","value":"42"})
    );
    assert_eq!(answer["site"]["context"]["name"], "Evaluation::Rover");
    assert_eq!(answer["site"]["range"]["start"]["line"], 1);
    assert_eq!(
        evaluate(&mut host, "::big")?["outcome"]["values"][0]["value"],
        "9007199254740993"
    );
    let empty = evaluate(&mut host, "::empty")?;
    assert_eq!(empty["status"], "values", "{empty}");
    assert_eq!(empty["outcome"]["values"], json!([]));
    let missing = evaluate(&mut host, "::missing")?;
    assert_ne!(missing["status"], "values", "{missing}");
    assert!(
        !missing["messages"]
            .as_array()
            .ok_or("Missing messages")?
            .is_empty()
    );
    assert!(
        request(
            &mut host,
            "evaluation/evaluate",
            json!({"revision":0,"expressionId":answer["site"]["id"]})
        )
        .is_err_and(|e| e.contains("stale"))
    );
    let owner = &answer["site"]["ownerId"];
    assert!(
        request(
            &mut host,
            "evaluation/evaluate",
            json!({"revision":1,"expressionId":owner})
        )
        .is_err()
    );
    request(
        &mut host,
        "workspace/analyze",
        json!({"revision":2,"sources":[{"uri":uri,"text":"package Empty {}"}]}),
    )?;
    assert!(
        request(
            &mut host,
            "evaluation/evaluate",
            json!({"revision":2,"expressionId":answer["site"]["id"]})
        )
        .is_err()
    );
    Ok(())
}
