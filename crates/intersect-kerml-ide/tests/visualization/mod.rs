use intersect_kerml_ide::{Host, PrepareCommand, Source};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn request(host: &mut Host, method: &str, params: Value) -> Result<Value, String> {
    host.dispatch(json!({"method": method, "params": params}))
}

fn offset(text: &str, position: &intersect_views::Position) -> Result<usize, String> {
    let mut start = 0;
    for _ in 0..position.line {
        start += text[start..].find('\n').ok_or("Missing source line")? + 1;
    }
    let mut units = 0;
    for (byte, c) in text[start..].char_indices() {
        if units == position.character {
            return Ok(start + byte);
        }
        units += c.len_utf16();
    }
    if units == position.character {
        Ok(text.len())
    } else {
        Err("Invalid source coordinate".into())
    }
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn compiled_views_and_source_commands_share_one_model() -> Result<(), String> {
    let path = std::env::var("INTERSECT_IDE_TEST_PACK")
        .map_err(|_| "Set INTERSECT_IDE_TEST_PACK to a trusted pack built with the default language features")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut host = Host::default();
    let pack = host.load_pack_for_analysis(bytes)?;
    assert_eq!(pack["nodes"], json!([]));
    let mut sources = vec![
        Source {
            uri: "file:///project/system.sysml".into(),
            text: format!(
                "// 🌕 café {{ }}\r\n{}",
                include_str!("../fixtures/system.sysml")
            ),
        },
        Source {
            uri: "file:///project/views.sysml".into(),
            text: include_str!("../fixtures/views.sysml").into(),
        },
    ];
    let analyze = |host: &mut Host, sources: &[Source], revision| {
        request(
            host,
            "workspace/analyze",
            json!({"sources": sources,"revision": revision,"projectNodesOnly":true}),
        )
    };
    let analysis = analyze(&mut host, &sources, 1)?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let catalog = host.view_catalog(1)?;
    let overview = catalog
        .views
        .iter()
        .find(|v| v.name == "definitions")
        .ok_or("Missing overview")?;
    let structure = catalog
        .views
        .iter()
        .find(|v| v.name == "structure")
        .ok_or("Missing structure")?;
    let general = host.view_scene(1, &overview.key, None)?;
    assert_eq!(general.scene.nodes.len(), 5);
    assert_eq!(general.scene.edges.len(), 2);
    assert_eq!(general.scene.layout.direction, "DOWN");
    let response = host.view_scene(1, &structure.key, None)?;
    response.validate()?;
    assert_eq!(response.scene.layout.direction, "RIGHT");
    assert_eq!(response.scene.nodes.len(), 1);
    assert_eq!(response.scene.nodes[0].children.len(), 2);
    assert_eq!(
        response.scene.nodes[0]
            .children
            .iter()
            .map(|n| n.ports.len())
            .sum::<usize>(),
        2
    );
    assert_eq!(response.scene.edges.len(), 1);
    assert!(
        response
            .scene
            .view
            .details
            .iter()
            .any(|d| d.label.contains("StructurePerspective"))
    );
    let wire = request(
        &mut host,
        "views/scene",
        json!({"revision":1,"viewKey":structure.key}),
    )?;
    assert_eq!(
        wire,
        serde_json::to_value(&response).map_err(|e| e.to_string())?
    );
    let tool = response
        .scene
        .palette
        .groups
        .iter()
        .flat_map(|g| &g.entries)
        .find(|t| t.command == "sysml.createPartUsage")
        .ok_or("Missing create tool")?;
    let owner_key = structure
        .owners
        .first()
        .ok_or("Missing writable owner")?
        .key
        .clone();
    let command = |name: &str| PrepareCommand {
        revision: 1,
        view_key: structure.key.clone(),
        owner_key: owner_key.clone(),
        tool_id: tool.id.clone(),
        arguments: BTreeMap::from([("name".into(), name.into())]),
        palette: None,
    };
    assert!(
        host.prepare_view_command(command("x; part injected"))
            .is_err()
    );
    let before = sources[0].text.clone();
    let prepared = host.prepare_view_command(command("sensor"))?;
    assert_eq!(host.view_scene(1, &structure.key, None)?.scene.revision, 1);
    assert_eq!(
        request(&mut host, "intersect/source", json!({"uri":sources[0].uri}))?["text"],
        before
    );
    assert!(
        host.commit_view_command(1, &prepared.token, &sources)
            .is_err()
    );
    let patch = &prepared.patches[0];
    assert_eq!(patch.uri, sources[0].uri);
    let start = offset(&before, &patch.range.start)?;
    let end = offset(&before, &patch.range.end)?;
    assert_eq!(&before[start..end], patch.original);
    sources[0]
        .text
        .replace_range(start..end, &patch.replacement);
    let accepted = host.commit_view_command(1, &prepared.token, &sources)?;
    assert_eq!(accepted["revision"], 2);
    assert!(
        accepted["nodes"]
            .as_array()
            .ok_or("Missing project nodes")?
            .iter()
            .all(|node| node["uri"]
                .as_str()
                .is_some_and(|uri| uri.starts_with("file:///project/")))
    );
    assert!(host.view_catalog(1).is_err());
    let updated = host.view_scene(2, &structure.key, None)?;
    assert!(
        updated.scene.nodes[0]
            .children
            .iter()
            .any(|n| n.title == "sensor")
    );
    assert_eq!(updated.scene.nodes[0].id, response.scene.nodes[0].id);
    assert!(
        host.commit_view_command(1, &prepared.token, &sources)
            .is_err()
    );
    sources[0].text = before;
    analyze(&mut host, &sources, 3)?;
    assert!(
        !host.view_scene(3, &structure.key, None)?.scene.nodes[0]
            .children
            .iter()
            .any(|n| n.title == "sensor")
    );
    // Distinct relationships between the same occurrence endpoints must not collapse.
    sources[0].text = sources[0].text.replace(
        "connect battery.supply to motor.power;",
        "connect battery.supply to motor.power;\n        connect battery.supply to motor.power;",
    );
    analyze(&mut host, &sources, 4)?;
    let parallel = host.view_scene(4, &structure.key, None)?;
    assert_eq!(parallel.scene.edges.len(), 2);
    assert_ne!(parallel.scene.edges[0].id, parallel.scene.edges[1].id);
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn general_view_creates_connections_from_visible_occurrences() -> Result<(), String> {
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    let mut host = Host::default();
    host.load_pack_for_analysis(std::fs::read(path).map_err(|e| e.to_string())?)?;
    let mut sources = vec![Source {
        uri: "file:///project/general.sysml".into(),
        text: "package Equipment { part left { port p; } part right { port p; } }\npackage Diagrams { private import StandardViewDefinitions::*; view general : GeneralView { expose Equipment::*; } }".into(),
    }];
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"sources":sources,"revision":1,"projectNodesOnly":true}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let view = host
        .view_catalog(1)?
        .views
        .into_iter()
        .find(|v| v.name == "general")
        .ok_or("Missing general view")?;
    let scene = host.view_scene(1, &view.key, None)?.scene;
    assert_eq!(scene.nodes.len(), 2);
    let left = scene
        .nodes
        .iter()
        .find(|n| n.title == "left")
        .ok_or("Missing left")?;
    let right = scene
        .nodes
        .iter()
        .find(|n| n.title == "right")
        .ok_or("Missing right")?;
    let owner_key = view
        .owners
        .iter()
        .find(|o| o.label == "Equipment")
        .ok_or("Missing namespace owner")?
        .key
        .clone();
    let mut command = PrepareCommand {
        revision: 1,
        view_key: view.key.clone(),
        owner_key,
        tool_id: "connection".into(),
        arguments: BTreeMap::from([
            ("name".into(), "wire".into()),
            ("source".into(), left.ports[0].id.clone()),
            ("target".into(), right.ports[0].id.clone()),
        ]),
        palette: None,
    };
    let valid = command.clone();
    command
        .arguments
        .insert("target".into(), "right.p; part injected;".into());
    assert!(host.prepare_view_command(command.clone()).is_err());
    command = valid.clone();
    command
        .arguments
        .insert("target".into(), left.ports[0].id.clone());
    assert!(host.prepare_view_command(command.clone()).is_err());
    command = valid.clone();
    command.owner_key = left.binding.key.clone();
    assert!(host.prepare_view_command(command).is_err());
    let prepared = host.prepare_view_command(valid.clone())?;
    assert!(
        prepared.patches[0]
            .replacement
            .contains("connection wire connect 'left'.'p' to 'right'.'p';")
    );
    assert_eq!(host.view_scene(1, &view.key, None)?.scene.edges.len(), 0);
    let patch = &prepared.patches[0];
    let start = offset(&sources[0].text, &patch.range.start)?;
    let end = offset(&sources[0].text, &patch.range.end)?;
    sources[0]
        .text
        .replace_range(start..end, &patch.replacement);
    let accepted = host.commit_view_command(1, &prepared.token, &sources)?;
    assert_eq!(accepted["clean"], true);
    let next = host.view_scene(2, &view.key, None)?.scene;
    assert!(
        next.view
            .owners
            .iter()
            .all(|owner| owner.label != "Equipment::wire")
    );
    assert_eq!(next.view.owners[0].label, "Equipment");
    assert_eq!(next.edges.len(), 1);
    assert_eq!(next.edges[0].source, left.ports[0].id);
    assert_eq!(next.edges[0].target, right.ports[0].id);
    assert!(
        host.prepare_view_command(valid).is_err(),
        "stale commands must fail"
    );
    Ok(())
}
