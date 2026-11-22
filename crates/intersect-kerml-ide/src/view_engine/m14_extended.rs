//! Qualification of repeated nested behaviors and actual concurrent state groups.
use super::*;

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_extended_behavior_scenes() -> Result<(), String> {
    let mut host = Host::default();
    let pack = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(pack).map_err(|e| e.to_string())?)?;
    let text = include_str!("../../tests/fixtures/m14-extended.sysml");
    let analysis = host.dispatch(json!({"method":"workspace/analyze","params":{
        "revision":1,"projectNodesOnly":true,"sources":[{"uri":"file:///m14/extended.sysml","text":text}]
    }}))?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let catalog = host.dispatch(json!({"method":"views/catalog","params":{"revision":1}}))?;
    let mut scenes = BTreeMap::new();
    for view in catalog["views"].as_array().ok_or("Missing catalog")? {
        let result = host.dispatch(
            json!({"method":"views/scene","params":{"revision":1,"viewKey":view["key"]}}),
        );
        scenes.insert(
            view["name"].as_str().ok_or("Unnamed view")?.to_owned(),
            result,
        );
    }
    let output = std::env::var("M14_SCENE_RECEIPT").map_err(|e| e.to_string())?;
    let output = std::path::Path::new(&output).with_file_name("native-extended.json");
    std::fs::write(
        output,
        serde_json::to_vec_pretty(
            &json!({"source":text,"analysis":analysis,"catalog":catalog,"scenes":scenes}),
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(scenes.len(), 2);
    for (name, result) in scenes {
        let response = result.map_err(|e| format!("{name}: {e}"))?;
        assert_eq!(
            response["scene"]["problems"],
            json!([]),
            "{name}: {response}"
        );
        let scene: intersect_views::GraphScene =
            serde_json::from_value(response["scene"].clone()).map_err(|e| e.to_string())?;
        let model = host
            .views
            .as_ref()
            .and_then(|h| h.accepted_model())
            .ok_or("No accepted model")?;
        let mut endpoints = BTreeMap::new();
        let mut bindings = Vec::new();
        fn visit<'a>(
            nodes: &'a [intersect_views::Node],
            parent: &str,
            endpoints: &mut BTreeMap<String, String>,
            bindings: &mut Vec<&'a intersect_views::Binding>,
        ) -> Result<(), String> {
            for node in nodes {
                let path = format!("{parent}.{}", node.title.text());
                endpoints.insert(node.id.clone(), path.clone());
                bindings.push(&node.binding);
                for port in &node.ports {
                    endpoints.insert(port.id.clone(), format!("{path}.{}", port.label.text()));
                    bindings.push(&port.binding);
                }
                visit(&node.children, &path, endpoints, bindings)?;
            }
            Ok(())
        }
        visit(&scene.nodes, "", &mut endpoints, &mut bindings)?;
        for edge in &scene.edges {
            bindings.push(&edge.binding);
            for part in &edge.label.parts {
                if let Some(binding) = &part.binding {
                    bindings.push(binding);
                }
            }
        }
        for binding in bindings {
            let object = model
                .objects()
                .find(|o| o.identity() == binding.element_id)
                .ok_or("Binding has no accepted source object")?;
            assert_eq!(binding.uri, object.resource());
            assert_eq!(
                serde_json::to_value(&binding.range).map_err(|e| e.to_string())?,
                serde_json::to_value(host.range(object)).map_err(|e| e.to_string())?
            );
            assert_ne!(binding.range.start, binding.range.end);
            assert_eq!(
                binding.key,
                intersect_view_host::element_key(
                    object,
                    &["declaredName".into(), "declaredShortName".into()],
                    &binding.context
                )?
            );
        }
        let pairs = scene
            .edges
            .iter()
            .map(|e| {
                Ok((
                    endpoints.get(&e.source).ok_or("Unknown source")?.as_str(),
                    endpoints.get(&e.target).ok_or("Unknown target")?.as_str(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if name == "actions" {
            assert_eq!(
                pairs
                    .iter()
                    .filter(|(a, b)| *a == ".pipeline.left" && *b == ".pipeline.right")
                    .count(),
                2
            );
            assert!(pairs.contains(&(".pipeline.right", ".pipeline.left")));
            for parent in ["left", "right"] {
                let a = format!(".pipeline.{parent}.duplicate");
                let b = format!(".pipeline.{parent}.other");
                assert!(pairs.contains(&(a.as_str(), b.as_str())));
                assert!(pairs.contains(&(b.as_str(), a.as_str())));
                assert!(pairs.contains(&(
                    format!("{a}.output").as_str(),
                    format!("{b}.input").as_str()
                )));
            }
            assert_eq!(pairs.len(), 13);
            let left = &scene.nodes[0]
                .children
                .iter()
                .find(|n| n.title.text() == "left")
                .ok_or("No left action")?;
            let right = &scene.nodes[0]
                .children
                .iter()
                .find(|n| n.title.text() == "right")
                .ok_or("No right action")?;
            let a = left
                .children
                .iter()
                .find(|n| n.title.text() == "duplicate")
                .ok_or("No repeated action")?;
            let b = right
                .children
                .iter()
                .find(|n| n.title.text() == "duplicate")
                .ok_or("No second repeated action")?;
            assert_ne!(a.id, b.id);
            assert_ne!(a.binding.context, b.binding.context);
            assert_eq!(a.binding.element_id, b.binding.element_id);
        } else {
            let concurrent = model
                .objects()
                .find(|o| {
                    o.resource() == "file:///m14/extended.sysml"
                        && crate::name(*o) == Some("Concurrent")
                })
                .ok_or("No actual concurrent definition")?;
            let field = model
                .metamodel()
                .class_info(concurrent.class())
                .ok_or("Missing state class")?
                .stored
                .iter()
                .find(|f| model.metamodel().feature_name(**f) == "isParallel")
                .ok_or("No stored concurrency flag")?;
            assert_eq!(
                crate::value_json(
                    model,
                    concurrent
                        .get(*field)
                        .ok_or("No stored concurrency value")?
                ),
                json!(true)
            );
            let root = &scene.nodes[0];
            assert_eq!(root.children.len(), 2);
            for title in ["channelA", "channelB"] {
                assert!(root.children.iter().any(|n| n.title.text() == title));
            }
            assert_eq!(
                pairs
                    .iter()
                    .filter(|(a, b)| *a == ".machine.channelA.duplicate"
                        && *b == ".machine.channelA.other")
                    .count(),
                2
            );
            assert!(pairs.contains(&(".machine.channelA.other", ".machine.channelA.other")));
            assert!(pairs.contains(&(".machine.channelA.other", ".machine.channelA.duplicate")));
            assert!(pairs.contains(&(".machine.channelB.other", ".machine.channelB.duplicate")));
            assert_eq!(pairs.len(), 9);
            let complete = root
                .children
                .iter()
                .find(|n| n.title.text() == "channelA")
                .ok_or("No channel")?
                .children
                .iter()
                .find(|n| n.title.text() == "complete")
                .ok_or("No final state")?;
            assert_eq!(
                serde_json::to_value(&complete.graphic).map_err(|e| e.to_string())?["parameters"]["control"],
                "final"
            );
        }
    }
    Ok(())
}
