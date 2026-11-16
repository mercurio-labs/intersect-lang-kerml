use intersect_kerml_ide::Host;
use serde_json::{Value, json};

fn request(host: &mut Host, method: &str, params: Value) -> Result<Value, String> {
    host.dispatch(json!({"method":method,"params":params}))
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn contextual_completion_uses_current_buffers_grammar_and_authored_scopes() -> Result<(), String> {
    let mut host = Host::default();
    host.load_pack_for_analysis(
        std::fs::read(std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?,
    )?;
    let uri = "file:///use.sysml";
    let initial = "package Use {}";
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":1,"sources":[
            {"uri":"file:///types.sysml","text":"package Types { part def Vehicle; part def Visible; part def <SV> ShortVehicle; part def 'fuel tank'; part def 'part'; private part def Veiled; alias Van for Vehicle; package Garage { part def Vessel; } }"},
            {"uri":"file:///other.sysml","text":"package Other { part def Vehicle; part def Invisible; }"},
            {"uri":"file:///inherit.kerml","text":"package Inheritance { class Base { feature inherited; } class Derived specializes Base { feature copy subsets inherited; } }"},
            {"uri":uri,"text":initial}
        ]}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    request(
        &mut host,
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"sysml","version":1,"text":initial}}),
    )?;
    let cases = [
        (
            "package Use { part def Base { part inherited; } part def Derived :> Base { part copy :> inh|; } }",
            vec!["inherited"],
            vec!["Vehicle"],
        ),
        (
            "package Use { part def Vehicle; part def Vehicle; part x : Veh|; }",
            vec![],
            vec!["Vehicle"],
        ),
        (
            "package Use { private import Types::*; part x : SV|; }",
            vec!["SV"],
            vec!["Vehicle"],
        ),
        (
            "// 🌕\r\npackage Use { private import Types::*; part x : Veh|icle; }",
            vec!["Vehicle"],
            vec!["Veiled"],
        ),
        (
            "package Use { private import Types::*; part x : fuel|; }",
            vec!["fuel tank"],
            vec!["Vehicle"],
        ),
        (
            "package Use { private import Types::*; part x : par|; }",
            vec!["part"],
            vec!["Vehicle"],
        ),
        (
            "package Use { private import Types::*; part x : V| }",
            vec!["Vehicle", "Visible", "Van"],
            vec!["Veiled", "Vessel", "Invisible", "part"],
        ),
        (
            "package Use { part x : Types::Garage::Ve|; }",
            vec!["Vessel"],
            vec!["Vehicle", "Visible", "Veiled"],
        ),
        (
            "package Use { private import Types::*; part x : Types|; }",
            vec![],
            vec!["Types"],
        ),
        (
            "package Use { private import Types::*; // V|",
            vec![],
            vec!["Vehicle", "Visible"],
        ),
        (
            "package Use { part x : Late|; part def Later; }",
            vec!["Later"],
            vec!["Vehicle"],
        ),
        (
            "package Use { private import Types::*; part x : V|",
            vec!["Vehicle", "Visible", "Van"],
            vec!["Veiled", "Vessel"],
        ),
    ];
    for (i, (marked, included, excluded)) in cases.iter().enumerate() {
        let offset = marked.find('|').ok_or("Missing cursor")?;
        let text = marked.replace('|', "");
        let version = i as i64 + 2;
        let before = &text[..offset];
        let line = before.bytes().filter(|b| *b == b'\n').count();
        let character = before
            .rsplit('\n')
            .next()
            .ok_or("Missing line")?
            .encode_utf16()
            .count();
        request(
            &mut host,
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}),
        )?;
        let result = request(
            &mut host,
            "textDocument/completion",
            json!({"textDocument":{"uri":uri,"version":version},"position":{"line":line,"character":character}}),
        )?;
        let items = result["items"].as_array().ok_or("Missing items")?;
        let labels: Vec<_> = items.iter().filter_map(|i| i["label"].as_str()).collect();
        for name in included {
            assert!(labels.contains(name), "{marked}: missing {name}: {result}");
        }
        for name in excluded {
            assert!(
                !labels.contains(name),
                "{marked}: unexpected {name}: {result}"
            );
        }
        for item in items.iter().filter(|i| i["label"] == "Vehicle") {
            assert_eq!(
                item["textEdit"]["range"]["end"]["character"],
                character
                    + text[offset..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .map(char::len_utf16)
                        .sum::<usize>()
            );
            assert_eq!(item["textEdit"]["newText"], "Vehicle");
            assert_eq!(item["kind"], 7);
        }
        for item in items
            .iter()
            .filter(|i| i["label"] == "fuel tank" || i["label"] == "part")
        {
            assert_eq!(
                item["textEdit"]["newText"],
                format!("'{}'", item["label"].as_str().ok_or("label")?)
            );
        }
    }
    // Completion probes do not change the accepted analysis or its revision.
    assert!(
        request(
            &mut host,
            "textDocument/completion",
            json!({"textDocument":{"uri":uri,"version":1},"position":{"line":0,"character":0}})
        )
        .is_err()
    );
    Ok(())
}
