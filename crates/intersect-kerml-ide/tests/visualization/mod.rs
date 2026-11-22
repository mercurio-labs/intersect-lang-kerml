use intersect_kerml_ide::{Host, PrepareCommand, Source};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn request(host: &mut Host, method: &str, params: Value) -> Result<Value, String> {
    host.dispatch(json!({"method": method, "params": params}))
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn redefined_features_render_the_replacing_declarations() -> Result<(), String> {
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    let mut host = Host::default();
    host.load_pack_for_analysis(std::fs::read(path).map_err(|e| e.to_string())?)?;
    let text = "package Redefined {\npart def Base { part engine; part sensor; }\npart def Derived :> Base { part engine redefines Base::engine; part measurement redefines Base::sensor; }\npart vehicle : Derived;\nprivate import StandardViewDefinitions::*;\nview structure : InterconnectionView { expose vehicle; }\n}";
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":1,"projectNodesOnly":true,"sources":[{"uri":"file:///redefined.sysml","text":text}]}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let view = host
        .view_catalog(1)?
        .views
        .into_iter()
        .find(|v| v.name == "structure")
        .ok_or("Missing redefinition view")?;
    let response = host.view_scene(1, &view.key, None)?;
    response.validate()?;
    let scene = response.scene.graph()?;
    assert_eq!(scene.nodes.len(), 1);
    let children = &scene.nodes[0].children;
    assert_eq!(
        children.len(),
        2,
        "only the replacing declarations are effective"
    );
    for (child, needle) in children
        .iter()
        .zip(["part engine redefines", "part measurement redefines"])
    {
        let start = offset(text, &child.binding.range.start)?;
        let end = offset(text, &child.binding.range.end)?;
        assert!(
            text[start..end].contains(needle),
            "{}",
            text[start..end].to_owned()
        );
    }
    assert_eq!(
        children.iter().map(|n| n.title.text()).collect::<Vec<_>>(),
        ["engine", "measurement"]
    );
    assert!(!children.iter().any(|n| n.title == "sensor"));
    Ok(())
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
    let started = std::time::Instant::now();
    let pack = host.load_pack_for_analysis(bytes)?;
    eprintln!("M10 load_pack {:.3}s", started.elapsed().as_secs_f64());
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
    let analyzed = std::time::Instant::now();
    let analysis = analyze(&mut host, &sources, 1)?;
    eprintln!("M10 analyze {:.3}s", analyzed.elapsed().as_secs_f64());
    assert_eq!(analysis["clean"], true, "{analysis}");
    let cataloged = std::time::Instant::now();
    let catalog = host.view_catalog(1)?;
    eprintln!("M10 catalog {:.3}s", cataloged.elapsed().as_secs_f64());
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
    let rendered = std::time::Instant::now();
    let general = host.view_scene(1, &overview.key, None)?;
    eprintln!("M10 general {:.3}s", rendered.elapsed().as_secs_f64());
    assert_eq!(general.scene.graph()?.nodes.len(), 5);
    assert_eq!(general.scene.graph()?.edges.len(), 2);
    assert_eq!(general.scene.graph()?.layout.direction, "DOWN");
    let response = host.view_scene(1, &structure.key, None)?;
    response.validate()?;
    assert_eq!(response.scene.graph()?.layout.direction, "RIGHT");
    assert_eq!(response.scene.graph()?.nodes.len(), 1);
    assert_eq!(response.scene.graph()?.nodes[0].children.len(), 2);
    assert_eq!(
        response.scene.graph()?.nodes[0]
            .children
            .iter()
            .map(|n| n.ports.len())
            .sum::<usize>(),
        2
    );
    assert_eq!(response.scene.graph()?.edges.len(), 1);
    assert!(
        response
            .scene
            .graph()?
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
        .palette()
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
    // Project palettes may rename the language's explicitly registered tools;
    // matching a command alone must not grant an unregistered tool identity.
    let mut custom = response.scene.palette().clone();
    custom.id = "project.engineering".into();
    for entry in custom.groups.iter_mut().flat_map(|g| &mut g.entries) {
        match entry.id.as_str() {
            "part" => entry.id = "equipment".into(),
            "port" => entry.id = "interface".into(),
            _ => {}
        }
    }
    custom
        .groups
        .first_mut()
        .ok_or("Missing palette group")?
        .entries
        .push(
            serde_json::from_value(json!({"id":"future","label":"Planned action","icon":"box",
            "gesture":"place","command":"sysml.createActionUsage","parameters":{},"inputs":[]}))
            .map_err(|e| e.to_string())?,
        );
    let custom_scene = host.view_scene(1, &structure.key, Some(&custom))?;
    assert!(
        custom_scene
            .capabilities
            .iter()
            .any(|c| c.tool_id == "equipment" && c.enabled)
    );
    assert!(
        custom_scene
            .capabilities
            .iter()
            .any(|c| c.tool_id == "interface" && c.enabled)
    );
    assert!(
        custom_scene
            .capabilities
            .iter()
            .any(|c| c.tool_id == "future" && !c.enabled && !c.reason.is_empty())
    );
    let alias_edit = host.prepare_view_command(PrepareCommand {
        revision: 1,
        view_key: structure.key.clone(),
        owner_key: owner_key.clone(),
        tool_id: "equipment".into(),
        arguments: BTreeMap::from([("name".into(), "aliasProbe".into())]),
        palette: Some(custom),
        scope: None,
    })?;
    request(
        &mut host,
        "views/abortCommand",
        json!({"token":alias_edit.token}),
    )?;
    assert_eq!(
        host.view_scene(1, &structure.key, None)?.scene.revision(),
        1
    );
    let command = |name: &str| PrepareCommand {
        revision: 1,
        view_key: structure.key.clone(),
        owner_key: owner_key.clone(),
        tool_id: tool.id.clone(),
        arguments: BTreeMap::from([("name".into(), name.into())]),
        palette: None,
        scope: None,
    };
    assert!(
        host.prepare_view_command(command("x; part injected"))
            .is_err()
    );
    let before = sources[0].text.clone();
    let prepared = host.prepare_view_command(command("sensor"))?;
    assert_eq!(
        host.view_scene(1, &structure.key, None)?.scene.revision(),
        1
    );
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
        updated.scene.graph()?.nodes[0]
            .children
            .iter()
            .any(|n| n.title == "sensor")
    );
    assert_eq!(
        updated.scene.graph()?.nodes[0].id,
        response.scene.graph()?.nodes[0].id
    );
    assert!(
        host.commit_view_command(1, &prepared.token, &sources)
            .is_err()
    );
    sources[0].text = before;
    analyze(&mut host, &sources, 3)?;
    assert!(
        !host
            .view_scene(3, &structure.key, None)?
            .scene
            .graph()?
            .nodes[0]
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
    assert_eq!(parallel.scene.graph()?.edges.len(), 2);
    assert_ne!(
        parallel.scene.graph()?.edges[0].id,
        parallel.scene.graph()?.edges[1].id
    );
    assert!(
        request(
            &mut host,
            "workspace/analyze",
            json!({"revision":"invalid"})
        )
        .is_err()
    );
    assert!(host.view_catalog(4).is_err());
    analyze(&mut host, &sources, 5)?;
    assert_eq!(
        host.view_scene(5, &structure.key, None)?
            .scene
            .graph()?
            .edges
            .len(),
        2
    );
    // A known unsupported expose is explicit, never silently filtered to empty.
    let needle = "expose DriveSystem::drive;";
    let start = sources[1]
        .text
        .rfind(needle)
        .ok_or("Missing structure expose")?;
    sources[1].text.replace_range(
        start..start + needle.len(),
        "expose DriveSystem::PowerPort;",
    );
    analyze(&mut host, &sources, 6)?;
    let unsupported = host
        .view_catalog(6)?
        .views
        .into_iter()
        .find(|v| v.name == "structure")
        .ok_or("Missing unsupported view")?;
    assert!(
        unsupported
            .problems
            .iter()
            .any(|p| p.contains("incompatible"))
    );
    assert!(host.view_scene(6, &unsupported.key, None).is_err());
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
        text: "package Equipment { part left { port p; } part right { port p; } }\npackage Diagrams { private import StandardViewDefinitions::*; view general : GeneralView { expose Equipment::left; expose Equipment::right; } }".into(),
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
    let response = host.view_scene(1, &view.key, None)?;
    let scene = response.scene.graph()?;
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
        scope: None,
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
    assert_eq!(
        host.view_scene(1, &view.key, None)?
            .scene
            .graph()?
            .edges
            .len(),
        0
    );
    let patch = &prepared.patches[0];
    let start = offset(&sources[0].text, &patch.range.start)?;
    let end = offset(&sources[0].text, &patch.range.end)?;
    sources[0]
        .text
        .replace_range(start..end, &patch.replacement);
    let accepted = host.commit_view_command(1, &prepared.token, &sources)?;
    assert_eq!(accepted["clean"], true);
    let next_response = host.view_scene(2, &view.key, None)?;
    let next = next_response.scene.graph()?;
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

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn lunar_rover_scenes_fit_the_registered_budget() -> Result<(), String> {
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    let mut host = Host::default();
    host.load_pack_for_analysis(std::fs::read(path).map_err(|e| e.to_string())?)?;
    let sources = [
        (
            "rover.sysml",
            include_str!("../fixtures/lunar-rover/rover.sysml"),
        ),
        (
            "views.sysml",
            include_str!("../fixtures/lunar-rover/views.sysml"),
        ),
        (
            "types.kerml",
            include_str!("../fixtures/lunar-rover/types.kerml"),
        ),
    ]
    .into_iter()
    .map(|(name, text)| Source {
        uri: format!("file:///project/{name}"),
        text: text.into(),
    })
    .collect::<Vec<_>>();
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"sources":sources,"revision":1,"projectNodesOnly":true}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    fn count(nodes: &[intersect_views::Node]) -> usize {
        nodes.iter().map(|n| 1 + count(&n.children)).sum()
    }
    for (name, roots, total) in [("overview", 6, 9), ("structure", 1, 4)] {
        let view = host
            .view_catalog(1)?
            .views
            .into_iter()
            .find(|v| v.name == name)
            .ok_or("Missing rover view")?;
        let started = std::time::Instant::now();
        let response = host.view_scene(1, &view.key, None)?;
        eprintln!("M10 rover {name} {:.3}s", started.elapsed().as_secs_f64());
        response.validate()?;
        let scene = response.scene.graph()?;
        assert_eq!(scene.nodes.len(), roots);
        assert_eq!(count(&scene.nodes), total);
        assert_eq!(
            serde_json::to_value(response).map_err(|e| e.to_string())?,
            serde_json::to_value(host.view_scene(1, &view.key, None)?).map_err(|e| e.to_string())?
        );
    }
    Ok(())
}

mod rendering;
