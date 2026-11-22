use super::*;
use intersect_views::{Node, RenderingOrigin, RenderingResolution, RenderingStatus};
fn flatten<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        flatten(&n.children, out);
    }
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m13_source_rendering_and_three_projections() -> Result<(), String> {
    let mut host = Host::default();
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(path).map_err(|e| e.to_string())?)?;
    let sources = vec![
        Source {
            uri: "file:///m13/structure.sysml".into(),
            text: include_str!("../fixtures/m13-structure.sysml").into(),
        },
        Source {
            uri: "file:///m13/views.sysml".into(),
            text: include_str!("../fixtures/m13-views.sysml").into(),
        },
    ];
    let analysis = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":1,"projectNodesOnly":true,"sources":sources}),
    )?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let catalog = host.view_catalog(1)?;
    let mut resolutions = BTreeMap::new();
    for view in &catalog.views {
        let response = request(
            &mut host,
            "views/rendering",
            json!({"revision":1,"viewKey":view.key}),
        )?;
        let resolution: RenderingResolution =
            serde_json::from_value(response).map_err(|e| e.to_string())?;
        resolution.validate()?;
        eprintln!(
            "M13 {}: {:?} ({})",
            view.name,
            resolution.status,
            resolution
                .selection
                .as_ref()
                .map(|s| s.registration_id.as_str())
                .unwrap_or("located refusal")
        );
        resolutions.insert(view.name.clone(), resolution);
    }
    for (name, id, origin) in [
        ("compartments", "sysml.compartments", "default"),
        ("tree", "sysml.partsTree", "inherited"),
        ("wiring", "sysml.interconnection", "inherited"),
        ("explicit", "sysml.partsTree", "explicit"),
        ("replaced", "sysml.interconnection", "inherited"),
        ("aliasSelection", "sysml.partsTree", "explicit"),
    ] {
        let resolved = resolutions.get(name).ok_or("Missing resolution")?;
        assert_eq!(resolved.status, RenderingStatus::Resolved, "{name}");
        let selected = resolved.selection.as_ref().ok_or("Missing selection")?;
        assert_eq!(selected.registration_id, id, "{name}");
        assert_eq!(
            serde_json::to_value(&selected.origin).map_err(|e| e.to_string())?,
            origin
        );
        if matches!(selected.origin, RenderingOrigin::Default) {
            assert!(selected.canonical_rendering.is_none());
        } else {
            assert!(
                selected
                    .canonical_rendering
                    .as_ref()
                    .ok_or("No canonical target")?
                    .read_only
            );
        }
    }
    for name in [
        "incompatibleRoots",
        "lookalikeDefault",
        "lookalikeRendering",
    ] {
        let r = resolutions
            .get(name)
            .ok_or("Missing unsupported resolution")?;
        assert_eq!(r.status, RenderingStatus::Unsupported, "{name}");
        assert!(r.selection.is_none());
        assert!(!r.diagnostics.is_empty());
        assert_eq!(r.diagnostics[0].binding.uri, "file:///m13/views.sysml");
        assert!(host.view_scene(1, &r.view_key, None).is_err());
    }
    let scene = |name: &str| -> Result<intersect_views::GraphScene, String> {
        let key = &resolutions.get(name).ok_or("Missing view")?.view_key;
        Ok(host.view_scene(1, key, None)?.scene.graph()?.clone())
    };
    let compartments = scene("compartments")?;
    let tree = scene("tree")?;
    let wiring = scene("wiring")?;
    assert!(
        compartments.problems.is_empty(),
        "{:?}",
        compartments.problems
    );
    assert!(tree.problems.is_empty(), "{:?}", tree.problems);
    assert!(wiring.problems.is_empty(), "{:?}", wiring.problems);
    assert_eq!(compartments.nodes.len(), 5);
    let mut nested = vec![];
    flatten(&wiring.nodes, &mut nested);
    assert_eq!(tree.nodes.len(), nested.len());
    assert_eq!(tree.nodes.len(), 10);
    assert_eq!(tree.edges.len(), 8);
    assert!(tree.nodes.iter().all(|n| n.children.is_empty()));
    assert_eq!(tree.edges.len(), tree.nodes.len() - wiring.nodes.len());
    let signature = |n: &Node| serde_json::to_string(&n.binding).map_err(|e| e.to_string());
    assert_eq!(
        tree.nodes
            .iter()
            .map(signature)
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?,
        nested
            .iter()
            .map(|n| signature(n))
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?
    );
    for edge in &tree.edges {
        let parent = tree
            .nodes
            .iter()
            .find(|n| n.id == edge.source)
            .ok_or("Missing tree parent")?;
        let child = tree
            .nodes
            .iter()
            .find(|n| n.id == edge.target)
            .ok_or("Missing tree child")?;
        let mut context = parent.binding.context.clone();
        context.push(parent.binding.key.clone());
        assert_eq!(child.binding.context, context);
        assert_eq!(
            serde_json::to_value(&edge.binding).map_err(|e| e.to_string())?,
            serde_json::to_value(&child.binding).map_err(|e| e.to_string())?
        );
    }
    assert_eq!(wiring.edges.len(), 1);
    let edge = &wiring.edges[0];
    let ports: Vec<_> = nested.iter().flat_map(|n| &n.ports).collect();
    let a = ports
        .iter()
        .find(|p| p.id == edge.source)
        .ok_or("Missing source port")?;
    let b = ports
        .iter()
        .find(|p| p.id == edge.target)
        .ok_or("Missing target port")?;
    assert_ne!(a.binding.context, b.binding.context);
    assert_eq!(a.binding.key, b.binding.key);
    assert_eq!(a.binding.range, b.binding.range);
    let (source_key, source_context) = a
        .binding
        .context
        .split_last()
        .ok_or("Missing source context")?;
    let (target_key, target_context) = b
        .binding
        .context
        .split_last()
        .ok_or("Missing target context")?;
    let source_node = nested
        .iter()
        .find(|n| n.title.text().starts_with("retained") && n.binding.context == source_context)
        .ok_or("Missing source part occurrence")?;
    let target_node = nested
        .iter()
        .find(|n| n.title.text().starts_with("chosen") && n.binding.context == target_context)
        .ok_or("Missing target part occurrence")?;
    assert_eq!(source_key, &source_node.binding.key);
    assert_eq!(target_key, &target_node.binding.key);
    assert!(compartments.nodes.iter().any(|n| {
        n.compartments
            .iter()
            .any(|c| c.rows.iter().any(|r| r.label.text().contains("chosen")))
    }));
    assert!(ports.iter().any(|p| p.side == "WEST"));
    assert!(ports.iter().any(|p| p.side == "EAST"));
    let io_row = compartments
        .nodes
        .iter()
        .flat_map(|n| &n.compartments)
        .flat_map(|c| &c.rows)
        .find(|r| r.binding.key == a.binding.key)
        .ok_or("Port is not represented by a bound row")?;
    assert_eq!(io_row.binding.range, a.binding.range);
    assert_ne!(io_row.binding.context, a.binding.context);
    for (label, needle) in [
        ("retained", "part retained : Module"),
        ("chosen", "part chosen : Module redefines"),
        ("leaf", "part leaf : Leaf"),
    ] {
        let occurrences: Vec<_> = tree
            .nodes
            .iter()
            .filter(|n| n.title.text().starts_with(label))
            .collect();
        assert_eq!(occurrences.len(), if label == "leaf" { 4 } else { 2 });
        for node in occurrences {
            assert_eq!(node.binding.uri, "file:///m13/structure.sysml");
            let start = offset(&sources[0].text, &node.binding.range.start)?;
            let end = offset(&sources[0].text, &node.binding.range.end)?;
            assert!(sources[0].text[start..end].contains(needle));
        }
    }
    for source in &sources {
        assert_eq!(
            request(&mut host, "intersect/source", json!({"uri":source.uri}))?["text"],
            source.text
        );
    }
    assert!(
        request(
            &mut host,
            "views/rendering",
            json!({"revision":0,"viewKey":resolutions["tree"].view_key})
        )
        .is_err()
    );
    let incompatible = compartments
        .nodes
        .iter()
        .find(|n| n.title.text() == "Derived")
        .ok_or("Missing definition root")?;
    let error=request(&mut host,"views/scene",json!({"revision":1,"viewKey":resolutions["tree"].view_key,"scope":{"schemaVersion":1,"ops":[{"op":"addSeed","element":incompatible.binding.key}]}})).err().ok_or("Incompatible exploration root accepted")?;
    assert!(
        error.contains("file:///m13/structure.sysml") && error.contains("incompatible"),
        "{error}"
    );
    if let Ok(path) = std::env::var("INTERSECT_M13_RECEIPT") {
        let receipt = json!({"schemaVersion":1,"sources":sources,"resolutions":resolutions,"scenes":{"compartments":compartments,"tree":tree,"wiring":wiring}});
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    for (index, name, declaration) in [
        (
            2,
            "filtered",
            "view filtered : GeneralView { filter true; expose M13Structure::vehicle; }",
        ),
        (
            3,
            "recursive",
            "view recursive : GeneralView { expose M13Structure::*::**; }",
        ),
        (
            4,
            "aliasRoot",
            "alias RootAlias for M13Structure::vehicle; view aliasRoot : GeneralView { expose RootAlias; }",
        ),
        (
            5,
            "conflictingDefault",
            "view def Dual :> GeneralView, InterconnectionView; view conflictingDefault : Dual { expose M13Structure::vehicle; }",
        ),
    ] {
        let views = sources[1].text.replace(
            "  alias TreeAlias",
            &format!("  {declaration}\n  alias TreeAlias"),
        );
        let analyzed = request(
            &mut host,
            "workspace/analyze",
            json!({"revision":index,"projectNodesOnly":true,"sources":[sources[0].clone(),Source{uri:sources[1].uri.clone(),text:views}]}),
        )?;
        assert_eq!(analyzed["clean"], true, "{name}: {analyzed}");
        let view = host
            .view_catalog(index)?
            .views
            .into_iter()
            .find(|v| v.name == name)
            .ok_or("Missing refusal fixture view")?;
        let resolution = request(
            &mut host,
            "views/rendering",
            json!({"revision":index,"viewKey":view.key}),
        )?;
        assert_eq!(resolution["status"], "unsupported", "{name}: {resolution}");
        assert_eq!(
            resolution["diagnostics"][0]["binding"]["uri"],
            sources[1].uri
        );
        assert!(host.view_scene(index, &view.key, None).is_err());
    }
    let changed = sources[1].text.replace(
        "view explicit : GeneralView { render asTreeDiagram;",
        "view explicit : GeneralView { render asInterconnectionDiagram;",
    );
    let analyzed = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":6,"projectNodesOnly":true,"sources":[sources[0].clone(),Source{uri:sources[1].uri.clone(),text:changed}]}),
    )?;
    assert_eq!(analyzed["clean"], true);
    let switched = request(
        &mut host,
        "views/rendering",
        json!({"revision":6,"viewKey":resolutions["explicit"].view_key}),
    )?;
    assert_eq!(switched["selection"]["ruleSetId"], "sysml.interconnection");
    assert_eq!(switched["viewKey"], resolutions["explicit"].view_key);
    assert!(
        host.view_scene(1, &resolutions["explicit"].view_key, None)
            .is_err()
    );
    for (revision, name, declaration, expected) in [
        (
            7,
            "competing",
            "view def Competing :> TreeBase { render asInterconnectionDiagram; } view competing : Competing { expose M13Structure::vehicle; }",
            "unsupported",
        ),
        (
            8,
            "wireDefault",
            "view wireDefault : InterconnectionView { expose M13Structure::vehicle; }",
            "resolved",
        ),
        (
            9,
            "relationshipRoot",
            "package RelationshipInput { part a; part b; connection wire connect a to b; } view relationshipRoot : GeneralView { expose RelationshipInput::wire; }",
            "unsupported",
        ),
    ] {
        let views = sources[1].text.replace(
            "  alias TreeAlias",
            &format!("  {declaration}\n  alias TreeAlias"),
        );
        let analyzed = request(
            &mut host,
            "workspace/analyze",
            json!({"revision":revision,"projectNodesOnly":true,"sources":[sources[0].clone(),Source{uri:sources[1].uri.clone(),text:views}]}),
        )?;
        assert_eq!(analyzed["clean"], true, "{name}: {analyzed}");
        let view = host
            .view_catalog(revision)?
            .views
            .into_iter()
            .find(|v| v.name == name)
            .ok_or("Missing variant view")?;
        let resolution = request(
            &mut host,
            "views/rendering",
            json!({"revision":revision,"viewKey":view.key}),
        )?;
        assert_eq!(resolution["status"], expected, "{name}: {resolution}");
        if expected == "resolved" {
            assert_eq!(
                resolution["selection"]["registrationId"],
                "sysml.interconnection"
            );
            assert_eq!(resolution["selection"]["origin"], "default");
        } else {
            assert!(host.view_scene(revision, &view.key, None).is_err());
        }
    }
    let missing = sources[1].text.replace(
        "view explicit : GeneralView { render asTreeDiagram;",
        "view explicit : GeneralView { render missingRendering;",
    );
    let invalid = request(
        &mut host,
        "workspace/analyze",
        json!({"revision":10,"projectNodesOnly":true,"sources":[sources[0].clone(),Source{uri:sources[1].uri.clone(),text:missing}]}),
    )?;
    assert_eq!(invalid["clean"], false);
    assert!(
        invalid["diagnostics"]
            .as_array()
            .is_some_and(|d| !d.is_empty())
    );
    assert!(host.view_catalog(10).is_err());
    Ok(())
}
