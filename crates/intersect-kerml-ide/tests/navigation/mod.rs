use intersect_kerml_ide::Host;
use serde_json::{Value, json};

fn request(host: &mut Host, method: &str, params: Value) -> Result<Value, String> {
    host.dispatch(json!({"method":method,"params":params}))
}

#[test]
fn navigation_advertises_capability_and_requires_current_analysis() {
    let mut host = Host::default();
    assert_eq!(
        request(&mut host, "initialize", json!({})).unwrap()["capabilities"]["definitionProvider"],
        true
    );
    assert!(request(&mut host, "textDocument/definition", json!({})).is_err());
    assert!(request(&mut host, "textDocument/definition", json!({"revision":0})).is_err());
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn navigation_uses_resolved_targets_across_files_and_rejects_stale_buffers() -> Result<(), String> {
    let mut host = Host::default();
    host.load_pack_for_analysis(
        std::fs::read(std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    )?;
    let uri = "file:///use.sysml";
    let text = "// 🌕\r\npackage Use { private import Types::*; part vehicle : Vehicle; }";
    let sources = json!([
        {"uri":"file:///types.sysml","text":"package Types { part def Vehicle; }"},
        {"uri":"file:///other.sysml","text":"package Other { part def Vehicle; }"},
        {"uri":uri,"text":text}
    ]);
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":1,"sources":sources}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    request(
        &mut host,
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"sysml","version":1,"text":text}}),
    )?;
    let column = text
        .split('\n')
        .nth(1)
        .ok_or("Missing line")?
        .find("Vehicle")
        .ok_or("Missing reference")?;
    let params = json!({"revision":1,"textDocument":{"uri":uri,"version":1},"position":{"line":1,"character":column}});
    let result = request(&mut host, "textDocument/definition", params.clone())?;
    assert_eq!(result[0]["targetUri"], "file:///types.sysml", "{result}");
    assert_eq!(
        result[0]["originSelectionRange"]["start"]["character"],
        column
    );
    assert_eq!(
        result[0]["originSelectionRange"]["end"]["character"],
        column + 7
    );
    let mut outside = params.clone();
    let mut end = params.clone();
    end["position"]["character"] = json!(column + 7);
    assert_eq!(
        request(&mut host, "textDocument/definition", end)?[0]["targetUri"],
        "file:///types.sysml"
    );
    outside["position"]["character"] = json!(column - 2);
    assert_eq!(
        request(&mut host, "textDocument/definition", outside)?,
        Value::Null
    );
    request(
        &mut host,
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":text.replace("Vehicle", "Missing")}]}),
    )?;
    assert!(request(&mut host, "textDocument/definition", params).is_err());
    let failed = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":2,"sources":[{"uri":uri,"text":"package Use { part vehicle : Missing; }"}]}),
    )?;
    assert_eq!(failed["hasModel"], false);
    assert!(
        request(
            &mut host,
            "textDocument/definition",
            json!({"revision":2,"textDocument":{"uri":uri},"position":{"line":0,"character":30}})
        )
        .is_err()
    );
    let scalar_uri = "file:///scalar.sysml";
    let scalar = "package Test { private import ScalarValues::*; attribute x : Integer; }";
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":3,"sources":[{"uri":scalar_uri,"text":scalar}]}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let result = request(
        &mut host,
        "textDocument/definition",
        json!({"revision":3,"textDocument":{"uri":scalar_uri},"position":{"line":0,"character":scalar.find("Integer").ok_or("Integer")?}}),
    )?;
    assert!(
        result[0]["targetUri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("ScalarValues.kerml")),
        "{result}"
    );
    Ok(())
}
