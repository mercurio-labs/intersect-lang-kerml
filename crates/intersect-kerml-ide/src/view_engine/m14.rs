//! Native stored-shape probe before qualifying the language's behavior navigation.
use super::*;
use std::{cell::RefCell, rc::Rc};

#[path = "m14_extended.rs"]
mod extended;
#[path = "m14_profiles.rs"]
mod profiles;
#[path = "m14_read_profile.rs"]
mod read_profile;

struct NavigationProbe {
    catalog: Catalog,
    observed: Rc<RefCell<Value>>,
}
impl ModelViews for NavigationProbe {
    fn views(&self, c: &ModelViewContext<'_>) -> Result<Vec<ModelView>, String> {
        // Compare existing queries on the same accepted objects before the new
        // selector. These observations diagnose a failed gate, not a replacement
        // graph or a larger execution allowance.
        for object in c.model().objects().filter(|o| {
            o.resource() == "file:///m14/behavior.sysml"
                && matches!(name(*o), Some("pipeline" | "Work"))
        }) {
            for query in [
                "members",
                "m14FeatureCandidates",
                "m14ConcreteCandidates",
                "m14RedefinedTargets",
                "features",
            ] {
                let outcome = c.elements(query, object).map(|items| {
                    items
                        .iter()
                        .map(|item| (item.class_key(), name(*item)))
                        .collect::<Vec<_>>()
                });
                eprintln!(
                    "M14 existing query {} / {query}: {outcome:?}",
                    name(object).unwrap_or("")
                );
            }
        }
        let mut roots = BTreeMap::new();
        let mut missing = Vec::new();
        let describe = |object| -> Result<Value, String> {
            Ok(json!({"name":name(object),"class":object.class_key(),
                "qualified":qualified(object),"binding":c.binding(object)?}))
        };
        for root in c.model().objects().filter(|o| {
            o.resource() == "file:///m14/behavior.sysml"
                && matches!(name(*o), Some("pipeline" | "machine"))
        }) {
            let action = name(root) == Some("pipeline");
            let mut values = BTreeMap::new();
            for entry in if action {
                &[
                    "behaviorActions",
                    "behaviorParameters",
                    "behaviorFlows",
                    "behaviorSuccessions",
                ][..]
            } else {
                &[
                    "behaviorStates",
                    "behaviorTransitions",
                    "behaviorSuccessions",
                ][..]
            } {
                let objects = c.elements(entry, root)?;
                let mut items = Vec::new();
                for object in objects {
                    let mut item = describe(object)?;
                    let queries: &[&str] = match *entry {
                        "behaviorActions" => &["behaviorParameters"],
                        "behaviorFlows" => &["flowSourcePath", "flowTargetPath"],
                        "behaviorSuccessions" => &["successionSourcePath", "successionTargetPath"],
                        "behaviorStates" => &["stateEntry", "stateDo", "stateExit"],
                        "behaviorTransitions" => &[
                            "transitionSourcePath",
                            "transitionTargetPath",
                            "transitionTrigger",
                            "transitionGuard",
                            "transitionEffect",
                        ],
                        _ => &[],
                    };
                    for query in queries {
                        let targets = c.elements(query, object)?;
                        if query.ends_with("Path") && targets.is_empty() {
                            missing.push(json!({"query":query,"object":describe(object)?}));
                        }
                        item[*query] = targets
                            .into_iter()
                            .map(describe)
                            .collect::<Result<Vec<_>, _>>()?
                            .into();
                    }
                    items.push(item);
                }
                values.insert(*entry, items);
            }
            roots.insert(name(root).ok_or("Unnamed fixture root")?, values);
        }
        *self.observed.borrow_mut() = json!({"roots":roots,"missingEndpoints":missing});
        self.catalog.views(c)
    }
    fn validate_scope(
        &self,
        c: &ModelViewContext<'_>,
        v: &View,
        scope: &Scope,
    ) -> Result<(), String> {
        self.catalog.validate_scope(c, v, scope)
    }
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_behavior_navigation_probe() -> Result<(), String> {
    qualify_navigation(
        vec![json!({"uri":"file:///m14/behavior.sysml",
        "text":include_str!("../../tests/fixtures/m14-behavior.sysml")})],
        "M14Behavior::Work",
        "file:///m14/behavior.sysml",
    )
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_behavior_multifile_navigation() -> Result<(), String> {
    qualify_navigation(
        vec![
            json!({"uri":"file:///m14/behavior.sysml",
            "text":include_str!("../../tests/fixtures/m14-behavior-multifile.sysml")}),
            json!({"uri":"file:///m14/types.sysml",
            "text":include_str!("../../tests/fixtures/m14-behavior-types.sysml")}),
        ],
        "M14Types::Work",
        "file:///m14/types.sysml",
    )
}

fn qualify_navigation(sources: Vec<Value>, work: &str, work_uri: &str) -> Result<(), String> {
    let mut host = Host::default();
    let pack = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(pack).map_err(|e| e.to_string())?)?;
    let canonical = crate::view_rendering::Registry::prepare(
        host.loaded.as_ref().ok_or("No prepared language")?,
    )?;
    let observed = Rc::new(RefCell::new(Value::Null));
    host.views = Some(
        host.views
            .take()
            .ok_or("No graphical host")?
            .with_model_views(Box::new(NavigationProbe {
                catalog: Catalog(canonical),
                observed: observed.clone(),
            })),
    );
    let analysis = host.dispatch(json!({"method":"workspace/analyze","params":{
        "revision":1,"projectNodesOnly":true,"sources":sources
    }}))?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    host.dispatch(json!({"method":"views/catalog","params":{"revision":1}}))?;
    let value = observed.borrow().clone();
    let output = std::env::var("M14_NAVIGATION_RECEIPT")
        .unwrap_or_else(|_| "target/m14/native-navigation.json".into());
    let output = std::path::PathBuf::from(output);
    let output = if sources.len() > 1 {
        output.with_file_name("native-navigation-multifile.json")
    } else {
        output
    };
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(output, serde_json::to_vec_pretty(&json!({
        "status":"actual native Rcore observations; G2 is incomplete until all required endpoints/projections qualify",
        "analysis":analysis,"sources":sources,"navigation":value
    })).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let missing = value["missingEndpoints"]
        .as_array()
        .ok_or("No navigation observations")?;
    if !missing.is_empty() {
        return Err(format!(
            "G2 native navigation has {} unresolved control/data endpoint queries; full receipt retained",
            missing.len()
        ));
    }
    assert_fixture_navigation(&value, work, work_uri)?;
    Ok(())
}

fn assert_fixture_navigation(value: &Value, work: &str, work_uri: &str) -> Result<(), String> {
    let roots = &value["roots"];
    let path = |item: &Value, name: &str| -> Result<Vec<String>, String> {
        item[name]
            .as_array()
            .ok_or_else(|| format!("Missing {name}"))?
            .iter()
            .map(|e| {
                e["qualified"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("Missing qualified target".into())
            })
            .collect()
    };
    let successions = roots["pipeline"]["behaviorSuccessions"]
        .as_array()
        .ok_or("No control edges")?;
    let actual = successions
        .iter()
        .map(|s| {
            Ok((
                path(s, "successionSourcePath")?,
                path(s, "successionTargetPath")?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let local = |name: &str| format!("M14Behavior::Pipeline::{name}");
    let expected = [
        ("Actions::Action::start".into(), local("split")),
        (local("split"), local("firstStep")),
        (local("split"), local("secondStep")),
        (local("firstStep"), local("settle")),
        (local("secondStep"), local("settle")),
        (local("settle"), local("finish")),
        (local("finish"), "Actions::Action::done".into()),
    ]
    .into_iter()
    .map(|(s, t)| (vec![s], vec![t]))
    .collect::<Vec<_>>();
    assert_eq!(
        actual, expected,
        "Exact control pairs, including implied initial, fork and join semantics"
    );
    let flow = &roots["pipeline"]["behaviorFlows"][0];
    assert_eq!(
        path(flow, "flowSourcePath")?,
        [local("firstStep"), format!("{work}::output")]
    );
    assert_eq!(
        path(flow, "flowTargetPath")?,
        [local("secondStep"), format!("{work}::input")]
    );
    assert_eq!(
        roots["pipeline"]["behaviorFlows"]
            .as_array()
            .ok_or("No flows")?
            .len(),
        1
    );
    for role in ["flowSourcePath", "flowTargetPath"] {
        assert_eq!(
            flow[role][0]["binding"]["uri"],
            "file:///m14/behavior.sysml"
        );
        assert_eq!(flow[role][1]["binding"]["uri"], work_uri);
        assert_ne!(
            flow[role][1]["binding"]["range"]["start"],
            flow[role][1]["binding"]["range"]["end"]
        );
    }
    let transitions = roots["machine"]["behaviorTransitions"]
        .as_array()
        .ok_or("No transitions")?;
    assert_eq!(transitions.len(), 4);
    for (name, source, target, trigger, guard, effect) in [
        ("heat", "normal", "degraded", 1, 1, 1),
        ("recover", "degraded", "normal", 1, 0, 0),
        ("service", "normal", "maintenance", 0, 0, 0),
        ("again", "maintenance", "maintenance", 0, 0, 0),
    ] {
        let transition = transitions
            .iter()
            .find(|t| t["name"] == name)
            .ok_or_else(|| format!("Missing {name}"))?;
        assert_eq!(
            path(transition, "transitionSourcePath")?,
            [format!("M14Behavior::Machine::{source}")]
        );
        assert_eq!(
            path(transition, "transitionTargetPath")?,
            [format!("M14Behavior::Machine::{target}")]
        );
        for (role, count) in [
            ("transitionTrigger", trigger),
            ("transitionGuard", guard),
            ("transitionEffect", effect),
        ] {
            let members = transition[role]
                .as_array()
                .ok_or("Missing transition content")?;
            assert_eq!(members.len(), count, "{name}/{role}");
            for member in members {
                assert_eq!(member["binding"]["uri"], "file:///m14/behavior.sysml");
                assert_ne!(
                    member["binding"]["range"]["start"], member["binding"]["range"]["end"],
                    "{name}/{role} must bind authored content"
                );
            }
        }
    }
    let states = roots["machine"]["behaviorStates"]
        .as_array()
        .ok_or("No states")?;
    assert_eq!(states.len(), 3);
    for state in states {
        for role in ["stateEntry", "stateDo", "stateExit"] {
            assert_eq!(
                state[role].as_array().ok_or("Missing state content")?.len(),
                usize::from(state["name"] == "normal"),
                "{role}"
            );
        }
    }
    let initial = roots["machine"]["behaviorSuccessions"]
        .as_array()
        .ok_or("Missing state initial succession")?;
    assert_eq!(initial.len(), 1);
    assert_eq!(
        path(&initial[0], "successionSourcePath")?,
        ["M14Behavior::Machine"]
    );
    assert_eq!(
        path(&initial[0], "successionTargetPath")?,
        ["M14Behavior::Machine::normal"]
    );
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m14_behavior_source_shapes() -> Result<(), String> {
    let mut host = Host::default();
    let pack = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    host.load_pack_for_analysis(std::fs::read(pack).map_err(|e| e.to_string())?)?;
    let uri = "file:///m14/behavior.sysml";
    let text = include_str!("../../tests/fixtures/m14-behavior.sysml");
    let analysis_result = host.dispatch(json!({"method":"workspace/analyze","params":{
        "revision":1,"projectNodesOnly":true,"sources":[{"uri":uri,"text":text}]
    }}));
    let admission_error = analysis_result.as_ref().err().cloned();
    // If publication fails, inspect the same language loader's actual result.
    // This does not admit the candidate or turn a failed scene gate into a pass.
    let candidate = if admission_error.is_some() {
        use intersect_view_host::SourceLoader;
        let loaded = host.loaded.as_ref().ok_or("No prepared language")?.clone();
        let result = Loader {
            loaded,
            project: host.project.clone(),
        }
        .load(&[crate::Source {
            uri: uri.into(),
            text: text.into(),
        }])?;
        if result.load.diagnostics.iter().any(|d| d.is_error())
            || result.load.references != result.load.resolved
        {
            return Err(format!(
                "Candidate loading failed: {:?}",
                result.load.diagnostics
            ));
        }
        let Some(intersect_view_host::LoadedModel::Frozen(model)) = result.load.model else {
            return Err("No frozen candidate model".into());
        };
        *model
    } else {
        host.views
            .as_ref()
            .and_then(|v| v.accepted_model())
            .ok_or("No accepted model")?
            .clone()
    };
    let analysis = analysis_result
        .unwrap_or_else(|error| json!({"admissionError":error,"cleanLoaderResult":true}));
    host.sources.insert(uri.into(), text.into());
    let model = &candidate;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut objects = Vec::new();
    let mut keys = BTreeMap::<String, Vec<Value>>::new();
    for object in model.objects().filter(|o| o.resource() == uri) {
        *counts.entry(object.class_key().to_owned()).or_default() += 1;
        let info = model
            .metamodel()
            .class_info(object.class())
            .ok_or("Missing class")?;
        let values: BTreeMap<_, _> = info
            .stored
            .iter()
            .filter_map(|f| {
                object.get(*f).map(|v| {
                    (
                        model.metamodel().feature_name(*f),
                        crate::value_json(model, v),
                    )
                })
            })
            .collect();
        let item = json!({"id":object.identity(),"class":object.class_key(),"name":crate::name(object),"qualified":qualified(object),"range":host.range(object),"values":values});
        keys.entry(key(object)?).or_default().push(item.clone());
        objects.push(item);
    }
    for class in [
        "ForkNode",
        "JoinNode",
        "FlowUsage",
        "TransitionUsage",
        "StateSubactionMembership",
        "TransitionFeatureMembership",
    ] {
        assert!(
            model
                .objects()
                .any(|o| o.resource() == uri && is(model, o, class)),
            "Missing {class}"
        );
    }
    let collisions: BTreeMap<_, _> = keys
        .into_iter()
        .filter(|(_, items)| items.len() > 1)
        .collect();
    if let Ok(path) = std::env::var("M14_SHAPE_RECEIPT") {
        let path = std::path::Path::new(&path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path,serde_json::to_string_pretty(&json!({"status":"stored source shapes only; semantic navigation and projections not yet qualified","admissionError":admission_error,"collisions":collisions,"source":{"uri":uri,"text":text},"counts":counts,"objects":objects,"analysis":analysis})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    }
    eprintln!("M14 stored shape counts: {counts:?}");
    if let Some(error) = admission_error {
        return Err(format!(
            "Native admission failed: {error}; {} colliding project keys; raw candidate receipt retained",
            collisions.len()
        ));
    }
    Ok(())
}
