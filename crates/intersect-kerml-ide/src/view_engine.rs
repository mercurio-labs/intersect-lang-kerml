//! SysML registration and model-defined view adapter. Scene projection is shared.
#[cfg(test)]
#[path = "view_engine/m14.rs"]
mod m14;
use crate::{
    Host, Loaded,
    view_loading::{Loader, ProjectEnvironment},
};
use intersect_text::preparation::PreparedLanguageDefinition;
use intersect_view_host::{
    LanguageDefinition, ModelView, ModelViewContext, ModelViews, PreparedLanguageProvider,
    ProjectModel, ProjectObject,
};
use intersect_views::{Field, Owner, Scope, ScopeOp, View, ViewDocument};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

pub(super) fn key<'a>(object: impl Into<ProjectObject<'a>>) -> Result<String, String> {
    intersect_view_host::element_key(
        object,
        &["declaredName".into(), "declaredShortName".into()],
        &[],
    )
}
pub(super) fn is<'a>(
    model: impl Into<ProjectModel<'a>>,
    object: impl Into<ProjectObject<'a>>,
    class: &str,
) -> bool {
    let model = model.into();
    let object = object.into();
    model
        .metamodel()
        .class(class)
        .is_some_and(|c| model.metamodel().is_a(object.class(), c))
}
pub(super) fn name(object: ProjectObject<'_>) -> Option<&str> {
    ["declaredName", "declaredShortName"]
        .into_iter()
        .find_map(|f| {
            object
                .feature(f)
                .and_then(|f| object.str(f))
                .filter(|s| !s.is_empty())
        })
}
pub(super) fn declared_parent(mut object: ProjectObject<'_>) -> Option<ProjectObject<'_>> {
    while let Some(parent) = object.container() {
        if name(parent).is_some() {
            return Some(parent);
        }
        object = parent;
    }
    None
}
pub(super) fn qualified<'a>(object: impl Into<ProjectObject<'a>>) -> String {
    let mut names = vec![];
    let mut current = Some(object.into());
    while let Some(object) = current {
        if let Some(name) = name(object) {
            names.push(name);
        }
        current = declared_parent(object);
    }
    names.reverse();
    names.join("::")
}
struct Definition(Rc<Loaded>);
impl PreparedLanguageProvider for Definition {
    fn prepare(&self) -> Result<PreparedLanguageDefinition<'_>, String> {
        self.0.session.sysml_definition()
    }
}
pub(super) fn host(
    loaded: Rc<Loaded>,
    project: Rc<RefCell<ProjectEnvironment>>,
) -> Result<(intersect_view_host::Host, String, String), String> {
    let canonical = crate::view_rendering::Registry::prepare(&loaded)?;
    let definition = {
        let prepared = loaded.session.sysml_definition()?;
        let source = |id: &intersect_model::definitions::bundle::SourceId| {
            prepared
                .sources()
                .sources
                .iter()
                .find(|s| s.id == *id)
                .cloned()
                .ok_or_else(|| format!("Missing prepared source {}", id.uri))
        };
        let structure = source(prepared.selection().ecore)?;
        let grammar = source(
            prepared
                .selection()
                .xtext
                .ok_or("SysML definition has no grammar")?,
        )?;
        let bindings = loaded
            .bundle
            .sources
            .iter()
            .find(|s| s.kind == intersect_model::definitions::bundle::SourceKind::RcoreBehavior)
            .cloned()
            .ok_or("No checked language bindings source")?;
        // A private milestone probe may re-read this same authored source after
        // compilation, avoiding repeated native linking during query diagnosis.
        // Production builds always use the immutable embedded module.
        #[cfg(test)]
        let navigation_override = std::env::var("M14_NAVIGATION_SOURCE")
            .ok()
            .map(std::fs::read_to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        let navigation_source = navigation_override
            .as_deref()
            .unwrap_or(include_str!("../assets/view-navigation.rcore"));
        #[cfg(not(test))]
        let navigation_source = include_str!("../assets/view-navigation.rcore");
        let navigation_source = format!("{navigation_source}\n{}", canonical.control_functions()?);
        let mut functions = intersect_model::definitions::bundle::DefinitionSource::new(
            "package:/sysml/views/navigation.rcore",
            intersect_model::definitions::bundle::SourceKind::RcoreBehavior,
            &navigation_source,
        );
        functions.dependencies.push(structure.id.clone());
        LanguageDefinition {
            id: "sysml".into(),
            name: "KerML / SysML".into(),
            namespace: prepared.selection().namespace.into(),
            file_extensions: vec!["sysml".into(), "kerml".into()],
            structure,
            grammar,
            bindings,
            view_functions: vec![functions],
            view_execution_limits: intersect_model::rcore::ExecutionLimits {
                // Qualified by the one-function real-schema diagnostic. Keep
                // provider work limits unchanged; aggregate work is explicit below.
                max_model_storage_bytes: 64 * 1024 * 1024,
                ..Default::default()
            },
            view_function_work: 32_000_000,
            extra_functions: [
                "closure",
                "viewExposures",
                "structuralViewSeeds",
                "viewHasExplicitRelationshipExpose",
                "supportedPartRoot",
                "supportedStructuralRoot",
                "supportedActionRoot",
                "supportedStateRoot",
                "viewHasRecursiveExposes",
                "renderingMembers",
                "renderingTargets",
                "renderingOrigin",
                "viewDefinitionTypes",
                "viewHasFilters",
                "viewHasImportedExposes",
                "viewHasAliases",
                "ambiguous",
                "viewOwners",
                "viewpoints",
                "viewpointLabel",
                "features",
                "members",
                "m14FeatureCandidates",
                "m14ConcreteCandidates",
                "m14RedefinedTargets",
                "behaviorActions",
                "actionNodes",
                "stateNodes",
                "behaviorControlKind",
                "behaviorParameterSide",
                "behaviorStates",
                "behaviorParameters",
                "behaviorFlows",
                "behaviorSuccessions",
                "behaviorTransitions",
                "successionSourcePath",
                "successionTargetPath",
                "flowSourcePath",
                "flowTargetPath",
                "transitionSourcePath",
                "transitionTargetPath",
                "transitionTrigger",
                "transitionGuard",
                "transitionEffect",
                "stateEntry",
                "stateDo",
                "stateExit",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            // These private templates prepare the shared checked function cache.
            // Public projection rules enter only through the admitted package.
            rule_sets: [
                include_str!("../assets/general.viewrules.json"),
                include_str!("../assets/interconnection.viewrules.json"),
                include_str!("../assets/actionFlow.viewrules.json"),
                include_str!("../assets/stateTransition.viewrules.json"),
            ]
            .into_iter()
            .map(|text| {
                let mut rule: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
                rule["id"] =
                    format!("{}.template", rule["id"].as_str().ok_or("Missing rule ID")?).into();
                serde_json::to_string(&rule).map_err(|e| e.to_string())
            })
            .chain(
                [
                    include_str!("../assets/requirements-table.viewrules.json"),
                    include_str!("../assets/browser.viewrules.json"),
                    include_str!("../assets/sequence.viewrules.json"),
                ]
                .map(|rules| Ok(rules.to_owned())),
            )
            .collect::<Result<Vec<_>, String>>()?,
            notation: include_str!("../assets/sysml.notation.json").into(),
            palette: include_str!("../assets/structure.palette.json").into(),
            name_features: vec!["declaredName".into(), "declaredShortName".into()],
        }
    };
    let metamodel = loaded.session.frozen_metamodel()?;
    let mut host = intersect_view_host::Host::from_prepared(
        definition,
        Box::new(Definition(loaded.clone())),
        metamodel,
        Box::new(Loader { loaded, project }),
        Box::new(crate::view_commands::Commands),
    )?
    .with_model_views(Box::new(Catalog(canonical)));
    let initialized =
        host.dispatch(json!({"method":"initialize","params":{"protocolVersion":2}}))?;
    let fingerprint = initialized["language"]["fingerprint"]
        .as_str()
        .ok_or("Missing language fingerprint")?;
    use sha2::{Digest, Sha256};
    let resources = [
        (
            "sysml.compartments",
            "ruleSet",
            include_str!("../assets/general.viewrules.json"),
        ),
        (
            "sysml.interconnection",
            "ruleSet",
            include_str!("../assets/interconnection.viewrules.json"),
        ),
        (
            "sysml.partsTree",
            "ruleSet",
            include_str!("../assets/parts-tree.viewrules.json"),
        ),
        (
            "sysml.actionFlow",
            "ruleSet",
            include_str!("../assets/actionFlow.viewrules.json"),
        ),
        (
            "sysml.stateTransition",
            "ruleSet",
            include_str!("../assets/stateTransition.viewrules.json"),
        ),
        (
            "sysml.symbols",
            "symbolLibrary",
            include_str!("../assets/sysml.symbols.json"),
        ),
        (
            "sysml.renderings",
            "renderingRegistry",
            include_str!("../assets/renderings.json"),
        ),
    ];
    let hash = |bytes: &[u8]| format!("{:x}", Sha256::digest(bytes));
    let mut input: intersect_views::PresentationInput = serde_json::from_value(json!({
        "root": "sysml.presentation@1",
        "manifests": [{
            "schemaVersion": 1, "id": "sysml.presentation", "version": "1",
            "language": "https://www.omg.org/spec/SysML/20250201",
            "languageFingerprint": fingerprint,
            "requires": ["rules.graph.v8", "views.routing.v1", "views.labelPlacement.v1",
                         "graphics.composition.v1", "graphics.text.v1", "views.rendering.v1"],
            "dependencies": [],
            "resources": resources.iter().map(|(id, kind, content)|
                json!({"id": id, "kind": kind, "sha256": hash(content.as_bytes())})
            ).collect::<Vec<_>>()
        }],
        "resources": resources.iter().map(|(id, _, content)|
            json!({"id": id, "content": content})
        ).collect::<Vec<_>>()
    }))
    .map_err(|error| error.to_string())?;

    // Recreate the exact shipped Wave 1 package identity. Symbol descriptors
    // are additive and preserve its projections and occurrence identifiers.
    let mut legacy = input.clone();
    let previous_symbols = include_str!("../assets/sysml.symbols-v4.json");
    for resource in &mut legacy.resources {
        if resource.id == "sysml.symbols" {
            resource.content = previous_symbols.into();
        }
    }
    for resource in &mut legacy.manifests[0].resources {
        if resource.id == "sysml.symbols" {
            resource.sha256 = hash(previous_symbols.as_bytes());
        }
    }
    let legacy_fingerprint = legacy
        .resolve(
            "https://www.omg.org/spec/SysML/20250201",
            fingerprint,
            &hash,
        )?
        .fingerprint;

    input.manifests[0]
        .requires
        .insert(3, "views.symbolProperties.v1".into());
    let presentation =
        host.dispatch(json!({"method":"views/preparePresentation","params":input}))?;
    let fingerprint = presentation["presentation"]["fingerprint"]
        .as_str()
        .ok_or("No presentation fingerprint")?
        .to_owned();
    Ok((host, fingerprint, legacy_fingerprint))
}
// The grid default uses a checked language rule outside the graph package.
use crate::view_rendering::{BROWSER, REQUIREMENTS_TABLE, SEQUENCE};

fn supports_root(
    context: &ModelViewContext<'_>,
    registration: &str,
    object: ProjectObject<'_>,
) -> Result<bool, String> {
    if registration == REQUIREMENTS_TABLE {
        return Ok(is(context.model(), object, "RequirementUsage")
            && object
                .feature("declaredName")
                .and_then(|feature| object.str(feature))
                .is_some_and(|name| !name.is_empty()));
    }
    if registration == SEQUENCE {
        return Ok(is(context.model(), object, "OccurrenceDefinition") && name(object).is_some());
    }
    context.boolean(root_predicate(registration), object)
}
fn root_predicate(registration: &str) -> &'static str {
    match registration {
        "sysml.compartments" | BROWSER => "supportedStructuralRoot",
        "sysml.actionFlow" => "supportedActionRoot",
        "sysml.stateTransition" => "supportedStateRoot",
        _ => "supportedPartRoot",
    }
}
struct Catalog(crate::view_rendering::Registry);
impl ModelViews for Catalog {
    fn validate_scope(
        &self,
        context: &ModelViewContext<'_>,
        view: &View,
        scope: &Scope,
    ) -> Result<(), String> {
        for op in &scope.ops {
            if let ScopeOp::AddSeed { element } = op {
                let object = context.element(element).ok_or_else(|| {
                    format!(
                        "{}:{}:{}: rendering root is absent from accepted model",
                        view.binding.uri,
                        view.binding.range.start.line + 1,
                        view.binding.range.start.character + 1
                    )
                })?;
                if !supports_root(context, &view.kind, object)? {
                    let binding = context.binding(object)?;
                    return Err(format!(
                        "{}:{}:{}: root is incompatible with selected rendering",
                        binding.uri,
                        binding.range.start.line + 1,
                        binding.range.start.character + 1
                    ));
                }
            }
        }
        Ok(())
    }
    fn views(&self, context: &ModelViewContext<'_>) -> Result<Vec<ModelView>, String> {
        let mut result = vec![];
        for object in context.model().objects().filter(|o| {
            is(context.model(), *o, "ViewUsage") && context.editable(*o) && name(*o).is_some()
        }) {
            let key = context.key(object)?;
            let mut selection = self.0.intent(context, object).and_then(|intent| {
                let registration = intent.registration_id;
                let selection = if matches!(registration, REQUIREMENTS_TABLE | BROWSER | SEQUENCE) {
                    None
                } else {
                    Some(Box::new(self.0.resolve(context, intent)?))
                };
                Ok((registration, selection))
            });
            if selection.is_ok() {
                let problem = if context.boolean("viewHasFilters", object)? {
                    Some("View filter evaluation is not qualified in this profile")
                } else if context.boolean("viewHasImportedExposes", object)? {
                    Some("Namespace exposes with imports are unsupported")
                } else if context.boolean("viewHasAliases", object)? {
                    Some("Referenced or aliased exposure is unsupported")
                } else if context.boolean("viewHasExplicitRelationshipExpose", object)? {
                    Some("Explicit relationship exposes are incompatible structural roots")
                } else if context.boolean("viewHasRecursiveExposes", object)? {
                    Some("Recursive exposure is unsupported")
                } else {
                    None
                };
                if let Some(problem) = problem {
                    selection = Err(problem.into());
                }
            }
            let seeds = if selection.is_ok() {
                context.elements(
                    if selection
                        .as_ref()
                        .is_ok_and(|(registration, _)| *registration == "sysml.compartments")
                    {
                        "structuralViewSeeds"
                    } else {
                        "viewExposures"
                    },
                    object,
                )?
            } else {
                vec![]
            };
            if selection.as_ref().is_ok_and(|(id, _)| *id == SEQUENCE) && seeds.len() != 1 {
                selection = Err(
                    "This SequenceView profile requires one exposed occurrence definition".into(),
                );
            }
            if let Ok((registration, _)) = &selection {
                let registration = *registration;
                for seed in &seeds {
                    if !supports_root(context, registration, *seed)? {
                        selection = Err(
                            "Exposed roots are incompatible with the selected rendering".into(),
                        );
                        break;
                    }
                    if !matches!(registration, REQUIREMENTS_TABLE | BROWSER | SEQUENCE)
                        && context.boolean("ambiguous", *seed)?
                    {
                        selection =
                            Err("Ambiguous inherited members require explicit redefinition".into());
                        break;
                    }
                }
            }
            let kind = selection
                .as_ref()
                .map(|(registration, _)| (*registration).to_owned())
                .unwrap_or_else(|_| "unsupported".into());
            let mut owners = BTreeMap::new();
            let mut candidates = if selection.is_ok() {
                context.elements("viewOwners", object)?
            } else {
                vec![]
            };
            if kind == "sysml.compartments" {
                // Explicit member exposes can show sibling endpoints without
                // exposing their package. The connection still belongs there.
                let model = context.model();
                let mut package_keys = BTreeSet::new();
                for seed in seeds
                    .iter()
                    .copied()
                    .filter(|seed| is(model, *seed, "PartUsage") && !is(model, *seed, "ViewUsage"))
                {
                    if let Some(package) =
                        declared_parent(seed).filter(|parent| is(model, *parent, "Package"))
                        && !package_keys.insert(context.key(package)?)
                    {
                        candidates.push(package);
                    }
                }
            }
            for owner in candidates.into_iter().filter(|o| context.editable(*o)) {
                if name(owner).is_some() {
                    owners.insert(
                        context.key(owner)?,
                        Owner {
                            key: context.key(owner)?,
                            label: qualified(owner),
                        },
                    );
                }
            }
            let mut owners: Vec<_> = owners.into_values().collect();
            owners.sort_by(|a, b| {
                a.label
                    .matches("::")
                    .count()
                    .cmp(&b.label.matches("::").count())
                    .then_with(|| a.label.cmp(&b.label))
            });
            let details = context
                .elements("viewpoints", object)?
                .into_iter()
                .filter(|o| context.editable(*o))
                .map(|o| {
                    Ok(Field {
                        label: context.string("viewpointLabel", o)?,
                        binding: context.binding(o)?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let view = View {
                key: key.clone(),
                name: name(object).unwrap_or("").into(),
                kind: kind.clone(),
                binding: context.binding(object)?,
                owners,
                details,
                problems: selection.as_ref().err().cloned().into_iter().collect(),
            };
            let (registration, selection) = match selection {
                Ok(selection) => selection,
                Err(message) => {
                    let diagnostic = intersect_views::RenderingDiagnostic {
                        code: "sysml.rendering.unsupported".into(),
                        message,
                        binding: view.binding.clone(),
                    };
                    result.push(ModelView::Unsupported {
                        view,
                        diagnostics: vec![diagnostic],
                    });
                    continue;
                }
            };
            let document = ViewDocument {
                schema_version: 1,
                key,
                name: view.name.clone(),
                rule_set: registration.into(),
                scope: Scope {
                    schema_version: 1,
                    ops: seeds
                        .into_iter()
                        .map(|e| {
                            Ok(ScopeOp::AddSeed {
                                element: context.key(e)?,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                },
            };
            result.push(ModelView::Resolved {
                document,
                view,
                selection,
            });
        }
        Ok(result)
    }
}
impl Host {
    pub(super) fn views_catalog(&self) -> Result<Value, String> {
        serde_json::to_value(
            self.views
                .as_ref()
                .ok_or("Load a language pack first")?
                .view_catalog(self.revision)?,
        )
        .map_err(|e| e.to_string())
    }
    pub(super) fn views_scene(&self, params: &Value) -> Result<Value, String> {
        self.check_revision(params)?;
        if params.as_object().is_none_or(|p| {
            p.keys()
                .any(|k| !["revision", "viewKey", "scope", "palette"].contains(&k.as_str()))
        }) {
            return Err("Invalid scene request fields".into());
        }
        let scope = params
            .get("scope")
            .map(|s| serde_json::from_value(s.clone()))
            .transpose()
            .map_err(|e| e.to_string())?;
        let mut palette: intersect_views::Palette = if params["palette"].is_null() {
            serde_json::from_str(include_str!("../assets/structure.palette.json"))
        } else {
            serde_json::from_value(params["palette"].clone())
        }
        .map_err(|e| format!("Invalid palette: {e}"))?;
        palette.validate()?;
        let engine = self.views.as_ref().ok_or("Load a language pack first")?;
        let model = engine.accepted_project().ok_or("No accepted model")?;
        // Preserve the legacy language-owned suggestion policy. Explicit command
        // arguments remain exact and duplicate names are still rejected.
        let names: std::collections::BTreeSet<_> = model.objects().filter_map(name).collect();
        for tool in palette.groups.iter_mut().flat_map(|g| &mut g.entries) {
            if matches!(
                tool.command.as_str(),
                "sysml.createPartUsage" | "sysml.createPortUsage" | "sysml.createConnectionUsage"
            ) && let Some(input) = tool.inputs.iter_mut().find(|i| i.id == "name")
            {
                let base = input.default.clone();
                let mut suffix = 2;
                while names.contains(input.default.as_str()) {
                    input.default = format!("{base}{suffix}");
                    suffix += 1;
                }
            }
        }
        let response = self
            .views
            .as_ref()
            .ok_or("Load a language pack first")?
            .view_scene(
                self.revision,
                params["viewKey"].as_str().ok_or("Missing view key")?,
                scope.as_ref(),
                Some(palette),
            )?;
        if let intersect_views::Scene::Graph(scene) = &response.scene
            && matches!(
                scene.rule_set.as_str(),
                "sysml.actionFlow" | "sysml.stateTransition" | SEQUENCE
            )
        {
            // Generic exploration can carry partial structural scenes. A behavior
            // profile must never publish a graph with a missing semantic edge.
            // The engine's explicit Hide count is a visibility receipt, not a
            // semantic projection failure.
            let problems = scene
                .problems
                .iter()
                .filter(|problem| !problem.starts_with("Hidden "))
                .collect::<Vec<_>>();
            if !problems.is_empty() {
                return Err(format!(
                    "{}:{}:{}: Behavioral projection refused: {}",
                    scene.view.binding.uri,
                    scene.view.binding.range.start.line + 1,
                    scene.view.binding.range.start.character + 1,
                    problems
                        .iter()
                        .map(|p| p.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
        serde_json::to_value(response).map_err(|e| e.to_string())
    }
    pub(super) fn view_dispatch(&mut self, mut request: Value) -> Result<Value, String> {
        let supplied = request["params"].get("presentationFingerprint").is_some();
        if let Some(params) = request["params"].as_object_mut() {
            params
                .entry("presentationFingerprint")
                .or_insert_with(|| self.view_presentation_fingerprint.clone().into());
        }
        let commit = request["method"] == "views/commitCommand";
        let mut result = self
            .views
            .as_mut()
            .ok_or("Load a language pack first")?
            .dispatch(request)?;
        if !supplied && let Some(object) = result.as_object_mut() {
            object.remove("presentationFingerprint");
        }
        if commit {
            self.sync_analysis(&mut result)?;
        }
        Ok(result)
    }
    pub(super) fn prepare_command(
        &mut self,
        command: intersect_views::PrepareCommand,
    ) -> Result<Value, String> {
        let result =
            self.view_dispatch(json!({"method":"views/prepareCommand","params":command}))?;
        if let Some(code) = result["code"].as_str() {
            return Err(format!(
                "{code}: {}",
                result["message"].as_str().unwrap_or("Command refused")
            ));
        }
        Ok(result)
    }
    pub(super) fn commit_command(&mut self, params: &Value) -> Result<Value, String> {
        self.view_dispatch(json!({"method":"views/commitCommand","params":params}))
    }
}
