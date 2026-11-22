//! Diagnostic for real-schema source reads; no whole-library or UI substitution.
use super::*;
use intersect_model::{
    core::{
        frozen::{ById, FrozenStore, ModelBuilder},
        query::QueryBudget,
        storage::{self, ModelStore},
    },
    definitions::{
        bundle::{DefinitionSource, SourceKind},
        ecore::ParseLimits,
        rcore_behavior::{
            ir_v2::{DataWire, EntrySelection, Limits},
            modules::ModuleIndexPreparation,
        },
    },
    rcore::ExecutionLimits,
    source_model_functions::SourceModelFunction,
};
use std::time::Instant;

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn public_source_read_limits_over_sysml_schema() -> Result<(), String> {
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    let pack = LanguagePack::read_checked(std::fs::read(path).map_err(|e| e.to_string())?, |_| {
        SectionCheck::Hash
    })
    .map_err(|e| e.to_string())?;
    let session = NativePilotSession::from_pack(&pack, RuntimeAdmissionLoadMode::HostCache)?;
    let prepared = session.sysml_definition()?;
    for (name, text) in [
        (
            "constant",
            r#"rcore 1; package m10.probe version "1"; import model "https://www.omg.org/spec/SysML/20250201" as k; pub fn constant(e:k.Element)->Bool = true;"#,
        ),
        ("viewKind", include_str!("../assets/view-navigation.rcore")),
    ] {
        let mut source = DefinitionSource::new(
            "package:/m10/constant.rcore",
            SourceKind::RcoreBehavior,
            text,
        );
        source.dependencies.push(prepared.selection().ecore.clone());
        let mut bundle = prepared.sources().clone();
        bundle.sources.push(source.clone());
        let limits = ParseLimits {
            source_bytes: 1_000_000,
        };
        let observation = InvocationContext::default();
        let mut indexing = ModuleIndexPreparation::begin(&bundle, &source.id, limits, &observation)
            .map_err(|e| format!("{e:?}"))?;
        while !indexing
            .advance(&observation)
            .map_err(|e| format!("{e:?}"))?
        {}
        let index = indexing.finish().map_err(|e| format!("{e:?}"))?;
        let symbol = index
            .module(&source.id)?
            .symbols
            .iter()
            .find(|s| s.name == [name])
            .ok_or("Missing probe")?
            .id
            .clone();
        let metamodel = session.frozen_metamodel()?;
        let class = metamodel.class("ViewUsage").ok_or("Missing ViewUsage")?;
        let mut builder = ModelBuilder::new(metamodel);
        let object = builder
            .create("probe", class, "memory:probe")
            .map_err(|e| format!("{e:?}"))?;
        let element_id = builder
            .metamodel()
            .feature(class, "elementId")
            .ok_or("Missing elementId")?;
        builder
            .set_str(object, element_id, "probe")
            .map_err(|e| format!("{e:?}"))?;
        let model = builder
            .link(&ById)
            .map_err(|e| format!("{e:?}"))?
            .validate()
            .map_err(|e| format!("{e:?}"))?;
        let mut store = FrozenStore::from_model(model);
        storage::atomic(&mut store, |s| s.set_revision(&prepared.schema().revision))?;
        for storage_bytes in [16_777_216, 67_108_864] {
            let start = Instant::now();
            let function = SourceModelFunction::from_prepared(
                &prepared,
                vec![source.clone()],
                &EntrySelection {
                    symbol: symbol.clone(),
                    types: vec![],
                    arguments: vec![],
                },
                &Limits::default(),
                ExecutionLimits {
                    max_model_storage_bytes: storage_bytes,
                    ..ExecutionLimits::default()
                },
                limits,
            )?;
            eprintln!(
                "M10 {name} compile storage={storage_bytes} {:.6}s",
                start.elapsed().as_secs_f64()
            );
            let start = Instant::now();
            let mut budget = QueryBudget::new(2_000_000);
            let result = function.invoke_model(&store, "probe", vec![], &mut budget);
            eprintln!(
                "M10 {name} invoke storage={storage_bytes} {:.6}s work={} result={result:?}",
                start.elapsed().as_secs_f64(),
                budget.reads
            );
            let value = result?;
            let DataWire::Scalar(value) = value.data else {
                return Err("Probe returned a non-scalar".into());
            };
            assert_eq!(
                value.payload,
                if name == "constant" {
                    json!(true)
                } else {
                    json!("unsupported")
                }
            );
        }
    }
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn actual_view_kind_with_public_limits_is_diagnostic() -> Result<(), String> {
    use intersect_view_host::SourceLoader;
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    let (loaded, _) = Loaded::from_pack(
        std::fs::read(path).map_err(|e| e.to_string())?,
        false,
        &BTreeMap::new(),
        &mut |_| {},
    )?;
    let loader = crate::view_loading::Loader {
        loaded: loaded.clone(),
        project: Rc::new(RefCell::new(Default::default())),
    };
    let result = loader.load(&[
        Source {
            uri: "file:///m10/system.sysml".into(),
            text: include_str!("../tests/fixtures/system.sysml").into(),
        },
        Source {
            uri: "file:///m10/views.sysml".into(),
            text: include_str!("../tests/fixtures/views.sysml").into(),
        },
    ])?;
    assert!(
        result.load.diagnostics.is_empty(),
        "{:?}",
        result.load.diagnostics
    );
    let Some(intersect_view_host::LoadedModel::Frozen(model)) = result.load.model else {
        return Err("No accepted diagnostic model".into());
    };
    let owner = model
        .objects()
        .find(|o| name(*o) == Some("structure"))
        .ok_or("Missing structure")?;
    let owner = owner.identity().to_owned();
    let drive = model
        .objects()
        .find(|o| name(*o) == Some("drive"))
        .ok_or("Missing drive")?
        .identity()
        .to_owned();
    let mut store = FrozenStore::from_model(*model);
    let prepared = loaded.session.sysml_definition()?;
    storage::atomic(&mut store, |s| s.set_revision(&prepared.schema().revision))?;
    let mut source = DefinitionSource::new(
        "package:/m10/navigation.rcore",
        SourceKind::RcoreBehavior,
        include_str!("../assets/view-navigation.rcore"),
    );
    source.dependencies.push(prepared.selection().ecore.clone());
    let mut bundle = prepared.sources().clone();
    bundle.sources.push(source.clone());
    let limits = ParseLimits {
        source_bytes: 1_000_000,
    };
    let observation = InvocationContext::default();
    let mut index = ModuleIndexPreparation::begin(&bundle, &source.id, limits, &observation)
        .map_err(|e| format!("{e:?}"))?;
    while !index.advance(&observation).map_err(|e| format!("{e:?}"))? {}
    let index = index.finish().map_err(|e| format!("{e:?}"))?;
    for selected in [
        "viewKind",
        "viewSeeds",
        "viewHasFilters",
        "viewHasImportedExposes",
        "viewHasAliases",
        "ambiguous",
    ] {
        let symbol = index
            .module(&source.id)?
            .symbols
            .iter()
            .find(|s| s.name == [selected])
            .ok_or("Missing viewKind")?
            .id
            .clone();
        for storage_bytes in if selected == "viewKind" {
            vec![16_777_216, 67_108_864]
        } else {
            vec![67_108_864]
        } {
            let started = Instant::now();
            let function = SourceModelFunction::from_prepared(
                &prepared,
                vec![source.clone()],
                &EntrySelection {
                    symbol: symbol.clone(),
                    types: vec![],
                    arguments: vec![],
                },
                &Limits::default(),
                ExecutionLimits {
                    max_model_storage_bytes: storage_bytes,
                    ..ExecutionLimits::default()
                },
                limits,
            )?;
            eprintln!(
                "M10 actual {selected} compile storage={storage_bytes} {:.6}s",
                started.elapsed().as_secs_f64()
            );
            let started = Instant::now();
            let mut budget = QueryBudget::new(2_000_000);
            let receiver = if selected == "ambiguous" {
                drive.clone()
            } else {
                owner.clone()
            };
            let result = function.invoke_model(&store, &receiver, vec![], &mut budget);
            eprintln!(
                "M10 actual {selected} invoke storage={storage_bytes} {:.6}s work={} result={result:?}",
                started.elapsed().as_secs_f64(),
                budget.reads
            );
            if storage_bytes == 16_777_216 {
                assert!(result.is_err_and(|e| e.contains("Retained model storage limit exceeded")));
            } else {
                let value = result?;
                if selected == "viewSeeds" {
                    let DataWire::Collection(values) = value.data else {
                        return Err("View seeds returned a non-collection".into());
                    };
                    assert_eq!(values.len(), 1);
                } else {
                    let DataWire::Scalar(value) = value.data else {
                        return Err("Catalog predicate returned a non-scalar".into());
                    };
                    assert_eq!(
                        value.payload,
                        if selected == "viewKind" {
                            json!("interconnection")
                        } else {
                            json!(false)
                        }
                    );
                }
            }
        }
    }
    Ok(())
}

#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn m13_native_rendering_semantic_gate() -> Result<(), String> {
    use intersect_view_host::{ModelView, ModelViewContext, ModelViews};
    type Observation = Vec<(String, Vec<(String, Vec<String>, Vec<String>)>)>;
    struct Probe(Rc<RefCell<Observation>>);
    impl ModelViews for Probe {
        fn views(&self, c: &ModelViewContext<'_>) -> Result<Vec<ModelView>, String> {
            let mut observations = Vec::new();
            for object in c.model().objects().filter(|o| {
                crate::views::is(c.model(), *o, "ViewUsage")
                    && c.editable(*o)
                    && crate::views::name(*o).is_some()
            }) {
                let mut members = Vec::new();
                for member in c.elements("renderingMembers", object)? {
                    let targets = c
                        .elements("renderingTargets", member)?
                        .into_iter()
                        .map(|o| c.string("label", o))
                        .collect::<Result<Vec<_>, String>>()?;
                    let origins = c
                        .elements("renderingOrigin", member)?
                        .into_iter()
                        .map(|o| c.string("label", o))
                        .collect::<Result<Vec<_>, String>>()?;
                    members.push((c.string("label", member)?, targets, origins));
                }
                if members.len() > 1 {
                    let location = c.binding(object)?;
                    return Err(format!(
                        "{}:{}:{}: ambiguous rendering memberships",
                        location.uri,
                        location.range.start.line + 1,
                        location.range.start.character + 1
                    ));
                }
                observations.push((
                    crate::views::name(object).ok_or("Unnamed view")?.to_owned(),
                    members,
                ));
            }
            *self.0.borrow_mut() = observations;
            Ok(vec![])
        }
    }
    let path = std::env::var("INTERSECT_IDE_TEST_PACK").map_err(|e| e.to_string())?;
    let mut host = Host::default();
    host.load_pack_for_analysis(std::fs::read(path).map_err(|e| e.to_string())?)?;
    let observations = Rc::new(RefCell::new(vec![]));
    let engine = host.views.take().ok_or("No shared host")?;
    host.views = Some(engine.with_model_views(Box::new(Probe(observations.clone()))));
    let source = r#"package R13 {
      private import StandardViewDefinitions::*;
      private import Views::*;
      part def Thing; part unit : Thing;
      view def Base :> GeneralView { render rendering choice :> Views::asTreeDiagram; }
      view def Replaced :> Base { render asInterconnectionDiagram :>> Base::choice; }
      view explicit : GeneralView { render asTreeDiagram; expose unit; }
      view inherited : Base { expose unit; }
      view replaced : Replaced { expose unit; }
      view absent : GeneralView { expose unit; }
    }"#;
    let analysis=host.dispatch(json!({"method":"workspace/analyze","params":{"revision":1,"projectNodesOnly":true,"sources":[{"uri":"file:///m13/dispatch.sysml","text":source}]}}))?;
    assert_eq!(analysis["clean"], true, "{analysis}");
    host.view_catalog(1)?;
    let observed = observations.borrow().clone();
    eprintln!("M13 rendering gate: {observed:#?}");
    for (view, target, origin) in [
        ("explicit", "asTreeDiagram", "explicit"),
        ("inherited", "asTreeDiagram", "Base"),
        ("replaced", "asInterconnectionDiagram", "Replaced"),
    ] {
        let record = observed
            .iter()
            .find(|(name, _)| name == view)
            .ok_or("Missing view")?;
        assert_eq!(record.1.len(), 1, "{record:?}");
        assert!(record.1[0].1.iter().any(|s| s == target), "{record:?}");
        assert!(record.1[0].2.iter().any(|s| s == origin), "{record:?}");
        if view == "replaced" {
            assert!(
                !record.1[0].1.iter().any(|s| s == "asTreeDiagram"),
                "{record:?}"
            );
        }
    }
    assert!(
        observed
            .iter()
            .find(|(name, _)| name == "absent")
            .ok_or("Missing absent view")?
            .1
            .is_empty()
    );
    let unresolved = source.replace("render asTreeDiagram;", "render missingRendering;");
    let invalid=host.dispatch(json!({"method":"workspace/analyze","params":{"revision":2,"projectNodesOnly":true,"sources":[{"uri":"file:///m13/dispatch.sysml","text":unresolved}]}}))?;
    assert_eq!(invalid["clean"], false, "{invalid}");
    assert!(
        !invalid["diagnostics"]
            .as_array()
            .ok_or("Missing diagnostics")?
            .is_empty()
    );
    let competing=source.replace("view absent : GeneralView { expose unit; }","view competing : GeneralView { render asTreeDiagram; render asInterconnectionDiagram; expose unit; }");
    let result=host.dispatch(json!({"method":"workspace/analyze","params":{"revision":3,"projectNodesOnly":true,"sources":[{"uri":"file:///m13/dispatch.sysml","text":competing}]}}))?;
    if result["clean"] == true {
        let error = host
            .view_catalog(3)
            .err()
            .ok_or("Competing renderings were accepted")?;
        assert!(
            error.contains("file:///m13/dispatch.sysml") && error.contains("ambiguous rendering"),
            "{error}"
        );
    } else {
        assert!(
            !result["diagnostics"]
                .as_array()
                .ok_or("Missing conflict diagnostics")?
                .is_empty()
        );
    }

    Ok(())
}
