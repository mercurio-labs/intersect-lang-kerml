//! Actual accepted-model canonical profile intent qualification.
use super::*;
use intersect_model::frozen::Model;
struct IntentProbe {
    catalog: Catalog,
    observed: Rc<RefCell<Value>>,
}
impl ModelViews for IntentProbe {
    fn views(&self, c: &ModelViewContext<'_>) -> Result<Vec<ModelView>, String> {
        let mut intents = BTreeMap::new();
        for object in c.model().objects().filter(|o| {
            o.resource() == "file:///m14/profiles.sysml"
                && is(c.model(), *o, "ViewUsage")
                && name(*o).is_some()
        }) {
            let value = match self.catalog.0.intent(c, object) {
                Ok(intent) => serde_json::to_value(intent).map_err(|e| e.to_string())?,
                Err(error) => json!({"error":error,"binding":c.binding(object)?}),
            };
            intents.insert(name(object).ok_or("Missing view name")?.to_owned(), value);
        }
        *self.observed.borrow_mut() = json!(intents);
        self.catalog.views(c)
    }
    fn validate_scope(&self, c: &ModelViewContext<'_>, v: &View, s: &Scope) -> Result<(), String> {
        self.catalog.validate_scope(c, v, s)
    }
}
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_canonical_behavior_intents() -> Result<(), String> {
    let mut host = Host::default();
    let pack = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(pack).map_err(|e| e.to_string())?)?;
    let registry =
        crate::view_rendering::Registry::prepare(host.loaded.as_ref().ok_or("No language")?)?;
    let observed = Rc::new(RefCell::new(Value::Null));
    host.views = Some(
        host.views
            .take()
            .ok_or("No view host")?
            .with_model_views(Box::new(IntentProbe {
                catalog: Catalog(registry),
                observed: observed.clone(),
            })),
    );
    let text = include_str!("../../tests/fixtures/m14-profiles.sysml");
    let analysis = host.dispatch(json!({"method":"workspace/analyze","params":{
        "revision":1,"projectNodesOnly":true,"sources":[{"uri":"file:///m14/profiles.sysml","text":text}]
    }}))?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let catalog = host.dispatch(json!({"method":"views/catalog","params":{"revision":1}}))?;
    let intents = observed.borrow().clone();
    let output = std::env::var("M14_INTENT_RECEIPT")
        .unwrap_or_else(|_| "target/m14/native-intents.json".into());
    std::fs::write(output,serde_json::to_vec_pretty(&json!({
        "status":"actual canonical language intents; complete behavior scenes and G2 remain incomplete",
        "source":{"uri":"file:///m14/profiles.sysml","text":text},"analysis":analysis,"intents":intents,"catalog":catalog
    })).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    assert_eq!(intents.as_object().ok_or("No intents")?.len(), 12);
    for (name, profile, origin) in [
        ("defaultAction", "sysml.actionFlow", "default"),
        ("defaultState", "sysml.stateTransition", "default"),
        ("explicitAction", "sysml.actionFlow", "explicit"),
        ("explicitState", "sysml.stateTransition", "explicit"),
        ("inheritedAction", "sysml.actionFlow", "inherited"),
        ("lawfulAction", "sysml.actionFlow", "inherited"),
        ("aliasAction", "sysml.actionFlow", "default"),
        ("lookalike", "sysml.interconnection", "explicit"),
    ] {
        assert_eq!(
            intents[name]["registrationId"], profile,
            "{name}: {}",
            intents[name]
        );
        assert_eq!(intents[name]["origin"], origin, "{name}");
        let binding = &intents[name]["originBinding"];
        assert_ne!(binding["range"]["start"], binding["range"]["end"], "{name}");
        if origin == "default" {
            assert_eq!(binding["readOnly"], true);
            assert!(intents[name]["canonicalRendering"].is_null());
        } else {
            assert_eq!(binding["uri"], "file:///m14/profiles.sysml");
            assert_eq!(intents[name]["canonicalRendering"]["readOnly"], true);
        }
    }
    for (name, reason) in [
        ("wrongTree", "incompatible"),
        ("mixed", "Incomparable"),
        ("fakeDefault", "single verified"),
        ("fakeRendering", "single compatible verified"),
    ] {
        assert!(
            intents[name]["error"]
                .as_str()
                .is_some_and(|e| e.contains(reason)),
            "{name}: {}",
            intents[name]
        );
        assert_eq!(
            intents[name]["binding"]["uri"],
            "file:///m14/profiles.sysml"
        );
    }
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_behavior_scenes() -> Result<(), String> {
    let mut host = Host::default();
    let pack = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(pack).map_err(|e| e.to_string())?)?;
    let text = include_str!("../../tests/fixtures/m14-behavior.sysml");
    let analysis = host.dispatch(json!({"method":"workspace/analyze","params":{
        "revision":1,"projectNodesOnly":true,"sources":[{"uri":"file:///m14/behavior.sysml","text":text}]
    }}))?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    let catalog = host.dispatch(json!({"method":"views/catalog","params":{"revision":1}}))?;
    let mut scenes = BTreeMap::new();
    for view in catalog["views"].as_array().ok_or("Missing catalog views")? {
        let name = view["name"].as_str().ok_or("Missing view name")?;
        let result = host.dispatch(
            json!({"method":"views/scene","params":{"revision":1,"viewKey":view["key"]}}),
        );
        scenes.insert(name.to_owned(), result.map_err(|e| e.to_string()));
    }
    let output = std::env::var("M14_SCENE_RECEIPT")
        .unwrap_or_else(|_| "target/m14/native-scenes.json".into());
    std::fs::write(output,serde_json::to_vec_pretty(&json!({"status":"native behavior scene vertical proof; actual WASM and final M14 qualification pending","source":text,"analysis":analysis,"catalog":catalog,"scenes":scenes})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    assert_eq!(scenes.len(), 2, "{catalog}");
    for (name, result) in scenes {
        let scene = result.map_err(|e| format!("{name}: {e}"))?;
        assert_eq!(scene["scene"]["schemaVersion"], 7, "{scene}");
        assert_eq!(scene["scene"]["problems"], json!([]), "{name}: {scene}");
        let graph: intersect_views::GraphScene =
            serde_json::from_value(scene["scene"].clone()).map_err(|e| e.to_string())?;
        let model = host
            .views
            .as_ref()
            .and_then(|h| h.accepted_model())
            .ok_or("No accepted scene model")?;
        assert_behavior_graph(&host, model, &name, &graph)?;
    }
    Ok(())
}

fn assert_behavior_graph(
    host: &Host,
    model: &Model,
    name: &str,
    scene: &intersect_views::GraphScene,
) -> Result<(), String> {
    fn collect<'a>(nodes: &'a [intersect_views::Node], all: &mut Vec<&'a intersect_views::Node>) {
        for node in nodes {
            all.push(node);
            collect(&node.children, all);
        }
    }
    let mut nodes = Vec::new();
    collect(&scene.nodes, &mut nodes);
    let mut endpoints = BTreeMap::new();
    let mut ports = 0;
    for node in &nodes {
        let title = node.title.text();
        endpoints.insert(node.id.clone(), title.clone());
        for port in &node.ports {
            ports += 1;
            endpoints.insert(port.id.clone(), format!("{title}.{}", port.label.text()));
        }
        for binding in std::iter::once(&node.binding).chain(node.ports.iter().map(|p| &p.binding)) {
            let objects = model
                .objects()
                .filter(|object| object.identity() == binding.element_id)
                .collect::<Vec<_>>();
            assert_eq!(
                objects.len(),
                1,
                "Exact selected source identity for {}",
                binding.key
            );
            let object = objects[0];
            assert_eq!(
                intersect_view_host::element_key(
                    object,
                    &["declaredName".into(), "declaredShortName".into()],
                    &binding.context
                )?,
                binding.key
            );
            assert_eq!(binding.uri, object.resource());
            assert_eq!(
                serde_json::to_value(&binding.range).map_err(|e| e.to_string())?,
                serde_json::to_value(host.range(object)).map_err(|e| e.to_string())?
            );
            assert_ne!(binding.range.start, binding.range.end);
        }
    }
    let pairs = scene
        .edges
        .iter()
        .map(|edge| {
            Ok((
                endpoints
                    .get(&edge.source)
                    .ok_or("Unknown source endpoint")?
                    .as_str(),
                endpoints
                    .get(&edge.target)
                    .ok_or("Unknown target endpoint")?
                    .as_str(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if name == "actions" {
        assert_eq!(nodes.len(), 8);
        assert_eq!(ports, 4);
        assert_eq!(
            pairs,
            vec![
                ("start", "split"),
                ("split", "firstStep"),
                ("split", "secondStep"),
                ("firstStep", "settle"),
                ("secondStep", "settle"),
                ("settle", "finish"),
                ("finish", "done"),
                ("firstStep.output", "secondStep.input")
            ]
        );
        for (title, control) in [
            ("start", "initial"),
            ("done", "final"),
            ("split", "fork"),
            ("settle", "join"),
        ] {
            let node = nodes
                .iter()
                .find(|n| n.title.text() == title)
                .ok_or("Missing control")?;
            assert_eq!(
                serde_json::to_value(&node.graphic).map_err(|e| e.to_string())?["parameters"]["control"],
                control
            );
            if title == "start" || title == "done" {
                assert!(node.binding.read_only);
            }
        }
        for title in ["firstStep", "secondStep"] {
            let node = nodes
                .iter()
                .find(|n| n.title.text() == title)
                .ok_or("Missing parameter owner")?;
            let mut context = node.binding.context.clone();
            context.push(node.binding.key.clone());
            assert_eq!(node.ports.len(), 2);
            for port in &node.ports {
                assert_eq!(port.binding.context, context);
            }
        }
    } else {
        assert_eq!(nodes.len(), 5);
        assert_eq!(ports, 0);
        assert_eq!(
            pairs,
            vec![
                ("normal", "degraded"),
                ("degraded", "normal"),
                ("normal", "maintenance"),
                ("maintenance", "maintenance"),
                ("", "normal")
            ]
        );
        let normal = nodes
            .iter()
            .find(|n| n.title.text() == "normal")
            .ok_or("Missing normal state")?;
        let Some(intersect_views::ContentSource::Text { name }) = normal
            .graphic
            .as_ref()
            .and_then(|graphic| graphic.slots.get("actions"))
        else {
            return Err("Missing bound state action slot".into());
        };
        let block = normal.text.get(name).ok_or("Missing state action block")?;
        assert_eq!(block.runs.len(), 3);
        for (run, expected) in block
            .runs
            .iter()
            .zip(["enterNormal", "monitor", "leaveNormal"])
        {
            assert_eq!(run.parts.last().ok_or("Missing source run")?.text, expected);
            assert!(
                run.parts
                    .iter()
                    .all(|p| p.binding.is_some() && p.edit_tool.is_none())
            );
        }
        let heat = &scene.edges[0].label;
        assert_eq!(heat.text(), "accept signal : Signal [true] / react");
        for part in &heat.parts {
            let binding = part.binding.as_ref().ok_or("Unbound transition role")?;
            assert_eq!(binding.uri, "file:///m14/behavior.sysml");
            assert_ne!(binding.range.start, binding.range.end);
            assert!(part.edit_tool.is_none());
        }
    }
    Ok(())
}
