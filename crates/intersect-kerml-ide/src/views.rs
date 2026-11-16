//! Handwritten language-owned bounded visualization provider.
//! SysML interpretation stays here; the scene consumer sees no language classes.
//! The supported profile is explicit exposes, standard General/Interconnection
//! inheritance, stored typing and project-owned concrete parts/ports. Unsupported
//! filters, ambiguous identity paths and unsupported semantic commands fail closed.
use super::{Host, name, position_byte};
use intersect_model::core::frozen::{Model, Obj, ValueRef};
use intersect_text::grammar;
use intersect_views::{
    Binding, Compartment, Edge, Field, LayoutHints, Node, Owner, Palette, Port, Position, Range,
    Scene, SourcePatch, VERSION, View,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MAX_NODES: usize = 500;
const MAX_DEPTH: usize = 8;

pub(super) fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
pub(super) fn is(model: &Model, o: Obj<'_>, class: &str) -> bool {
    model
        .metamodel()
        .class(class)
        .is_some_and(|c| model.metamodel().is_a(o.class(), c))
}
pub(super) fn refs<'m>(o: Obj<'m>, field: &str) -> Vec<Obj<'m>> {
    o.feature(field)
        .map(|f| o.refs(f).collect())
        .unwrap_or_default()
}
fn flag(o: Obj<'_>, field: &str) -> bool {
    o.feature(field).and_then(|f| o.get(f)).is_some_and(|v| {
        matches!(
            v,
            ValueRef::Bool(true) | ValueRef::Default(Value::Bool(true))
        )
    })
}
pub(super) fn children<'m>(model: &'m Model, o: Obj<'m>) -> Vec<Obj<'m>> {
    refs(o, "ownedRelationship")
        .into_iter()
        .filter(|r| is(model, *r, "OwningMembership"))
        .flat_map(|r| refs(r, "ownedRelatedElement"))
        .collect()
}
fn generals<'m>(model: &'m Model, o: Obj<'m>) -> Vec<Obj<'m>> {
    refs(o, "ownedRelationship")
        .into_iter()
        .filter(|r| is(model, *r, "Specialization"))
        .flat_map(|r| refs(r, "general"))
        .collect()
}
fn closure<'m>(model: &'m Model, root: Obj<'m>) -> Result<Vec<Obj<'m>>, String> {
    let mut result = Vec::new();
    let mut stack = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(o) = stack.pop() {
        if !seen.insert(o.identity().to_owned()) {
            continue;
        }
        if seen.len() > 2048 {
            return Err("View type traversal budget exceeded".into());
        }
        result.push(o);
        stack.extend(generals(model, o));
    }
    Ok(result)
}
fn declared_parent(mut o: Obj<'_>) -> Option<Obj<'_>> {
    while let Some((parent, _)) = o.container() {
        if name(parent).is_some() {
            return Some(parent);
        }
        o = parent;
    }
    None
}
/// Name/class paths survive offset changes and unrelated insertions. They are
/// intentionally not claimed to preserve identity across arbitrary renames.
fn path(o: Obj<'_>) -> Result<Vec<(String, String)>, String> {
    let mut result = Vec::new();
    let mut current = Some(o);
    let mut visited = BTreeSet::new();
    while let Some(o) = current {
        if !visited.insert(o.identity().to_owned()) || visited.len() > 64 {
            return Err("Cyclic or deep semantic identity path".into());
        }
        if let Some(n) = name(o) {
            result.push((o.class_key().to_owned(), n.to_owned()));
        }
        current = declared_parent(o);
    }
    result.reverse();
    Ok(result)
}
pub(super) fn key(o: Obj<'_>) -> Result<String, String> {
    if name(o).is_none() {
        return Err("An unnamed element needs an explicit visual context identity".into());
    }
    Ok(format!(
        "element:{}",
        hash(&serde_json::to_string(&(o.resource(), path(o)?)).map_err(|e| e.to_string())?)
    ))
}
fn qualified(o: Obj<'_>) -> Result<String, String> {
    Ok(path(o)?
        .into_iter()
        .map(|(_, n)| n)
        .collect::<Vec<_>>()
        .join("::"))
}
fn catalog(model: &Model, writable: &BTreeSet<String>) -> Result<BTreeMap<String, String>, String> {
    let mut map = BTreeMap::new();
    for o in model
        .objects()
        .filter(|o| name(*o).is_some() && writable.contains(o.resource()))
    {
        let k = key(o)?;
        if map.insert(k, o.identity().to_owned()).is_some() {
            return Err(
                "Ambiguous declaration paths cannot be bound to persistent diagrams".into(),
            );
        }
    }
    Ok(map)
}
fn position(text: &str, offset: usize) -> Position {
    let v = super::byte_position(text, offset);
    Position {
        line: v["line"].as_u64().unwrap_or(0) as usize,
        character: v["character"].as_u64().unwrap_or(0) as usize,
    }
}

struct Projection<'a> {
    host: &'a Host,
    model: &'a Model,
    view_key: String,
    count: usize,
    problems: Vec<String>,
    edges: Vec<Edge>,
    endpoints: BTreeMap<(String, String), String>,
}
impl Projection<'_> {
    fn binding(&self, o: Obj<'_>, context: &[String]) -> Result<Binding, String> {
        let text = self
            .host
            .source(o.resource())
            .ok_or_else(|| format!("No source text for {}", o.resource()))?;
        let (start, end) = o.span();
        let k = if name(o).is_some() {
            key(o)?
        } else {
            format!(
                "anonymous:{}",
                hash(&format!(
                    "{}:{}:{}:{context:?}",
                    o.resource(),
                    o.class_key(),
                    start
                ))
            )
        };
        Ok(Binding {
            key: k,
            element_id: o.identity().into(),
            context: context.to_vec(),
            uri: o.resource().into(),
            range: Range {
                start: position(text, start),
                end: position(text, end),
            },
            read_only: !self.host.editable.contains(o.resource()),
        })
    }
    fn symbol_id(&self, semantic: &str, context: &[String], role: &str) -> String {
        format!(
            "symbol:{}",
            hash(&format!("{}:{semantic}:{context:?}:{role}", self.view_key))
        )
    }
    fn check_budget(&mut self, depth: usize) -> Result<(), String> {
        self.count += 1;
        if self.count > MAX_NODES || depth > MAX_DEPTH {
            return Err(
                "Diagram exceeds 500 symbols or 8 nesting levels; narrow the view exposure".into(),
            );
        }
        Ok(())
    }
    fn type_label(&self, o: Obj<'_>) -> String {
        generals(self.model, o)
            .into_iter()
            .find(|g| is(self.model, *g, "Definition") && name(*g).is_some())
            .and_then(name)
            .unwrap_or("")
            .into()
    }
    fn effective_features<'m>(&self, o: Obj<'m>) -> Result<Vec<Obj<'m>>, String>
    where
        Self: 'm,
    {
        let mut by_name = BTreeMap::new();
        for ty in closure(self.model, o)? {
            for child in children(self.model, ty) {
                if let Some(n) = name(child) {
                    if flag(child, "isAbstract") || !is(self.model, child, "Feature") {
                        continue;
                    }
                    if by_name.contains_key(n) {
                        if ["PartUsage", "PortUsage", "AttributeUsage"]
                            .iter()
                            .any(|c| is(self.model, child, c))
                        {
                            return Err(format!(
                                "Ambiguous or redefined feature {n} requires upstream effective-feature evaluation"
                            ));
                        }
                        continue;
                    }
                    by_name.insert(n.to_owned(), child);
                }
            }
        }
        Ok(by_name.into_values().collect())
    }
    fn general_node(&mut self, o: Obj<'_>) -> Result<Node, String> {
        self.check_budget(0)?;
        let binding = self.binding(o, &[])?;
        let mut groups: BTreeMap<&str, Vec<Field>> = BTreeMap::new();
        for child in children(self.model, o) {
            if name(child).is_none() {
                continue;
            }
            let group = if is(self.model, child, "PortUsage") {
                "ports"
            } else if is(self.model, child, "AttributeUsage") {
                "attributes"
            } else if is(self.model, child, "PartUsage") {
                "parts"
            } else {
                continue;
            };
            let ty = self.type_label(child);
            groups.entry(group).or_default().push(Field {
                label: format!(
                    "{}{}{}",
                    name(child).unwrap_or(""),
                    if ty.is_empty() { "" } else { " : " },
                    ty
                ),
                binding: self.binding(child, &[])?,
            });
        }
        let title = name(o).unwrap_or(o.class_key()).to_owned();
        let notation = if is(self.model, o, "Package") {
            "package"
        } else if is(self.model, o, "Definition") {
            "definition"
        } else {
            "usage"
        };
        Ok(Node {
            id: self.symbol_id(&binding.key, &[], "node"),
            notation: notation.into(),
            title,
            subtitle: o.class_key().into(),
            binding,
            compartments: groups
                .into_iter()
                .map(|(label, rows)| Compartment {
                    label: label.into(),
                    rows,
                })
                .collect(),
            children: vec![],
            ports: vec![],
        })
    }
    fn structure_node(
        &mut self,
        o: Obj<'_>,
        parent_context: &[String],
        depth: usize,
    ) -> Result<Node, String> {
        self.check_budget(depth)?;
        let mut context = parent_context.to_vec();
        context.push(key(o)?);
        let binding = self.binding(o, &context)?;
        let id = self.symbol_id(&binding.key, &context, "node");
        self.endpoints
            .insert((context.join("/"), o.identity().into()), id.clone());
        let mut ports = Vec::new();
        let mut nodes = Vec::new();
        let mut rows = Vec::new();
        for child in self.effective_features(o)? {
            if is(self.model, child, "PortUsage") {
                self.check_budget(depth + 1)?;
                let b = self.binding(child, &context)?;
                let port_id = self.symbol_id(&b.key, &context, "port");
                self.endpoints.insert(
                    (context.join("/"), child.identity().into()),
                    port_id.clone(),
                );
                let side = if refs(child, "ownedRelationship")
                    .iter()
                    .any(|r| is(self.model, *r, "Conjugation"))
                {
                    "WEST"
                } else {
                    "EAST"
                };
                ports.push(Port {
                    id: port_id,
                    label: name(child).unwrap_or("").into(),
                    side: side.into(),
                    binding: b,
                });
            } else if is(self.model, child, "PartUsage") {
                nodes.push(self.structure_node(child, &context, depth + 1)?);
            } else if is(self.model, child, "AttributeUsage") {
                let ty = self.type_label(child);
                rows.push(Field {
                    label: format!(
                        "{}{}{}",
                        name(child).unwrap_or(""),
                        if ty.is_empty() { "" } else { " : " },
                        ty
                    ),
                    binding: self.binding(child, &context)?,
                });
            }
        }
        let title = if self.type_label(o).is_empty() {
            name(o).unwrap_or("").into()
        } else {
            format!("{} : {}", name(o).unwrap_or(""), self.type_label(o))
        };
        // Endpoints are engine-resolved feature chains in the usage context.
        // Children must be projected first so every visible endpoint is known.
        for ty in closure(self.model, o)? {
            for connection in children(self.model, ty)
                .into_iter()
                .filter(|c| is(self.model, *c, "ConnectionUsage"))
            {
                let ends: Vec<_> = refs(connection, "ownedRelationship")
                    .into_iter()
                    .filter(|r| is(self.model, *r, "EndFeatureMembership"))
                    .flat_map(|r| refs(r, "ownedRelatedElement"))
                    .collect();
                let resolve = |end: Obj<'_>| -> Result<String, String> {
                    let target = refs(end, "ownedRelationship")
                        .into_iter()
                        .filter(|r| is(self.model, *r, "ReferenceSubsetting"))
                        .flat_map(|r| refs(r, "referencedFeature"))
                        .next()
                        .ok_or("Unresolved connection end")?;
                    let mut chain: Vec<_> = refs(target, "ownedRelationship")
                        .into_iter()
                        .filter(|r| is(self.model, *r, "FeatureChaining"))
                        .flat_map(|r| refs(r, "chainingFeature"))
                        .collect();
                    if chain.is_empty() {
                        chain.push(target);
                    }
                    let mut ctx = context.clone();
                    let last = chain.pop().ok_or("Empty connection chain")?;
                    for part in chain {
                        if !is(self.model, part, "PartUsage") {
                            return Err("Only part/port connection chains are supported".into());
                        }
                        ctx.push(key(part)?);
                    }
                    if is(self.model, last, "PartUsage") {
                        ctx.push(key(last)?);
                    }
                    self.endpoints
                        .get(&(ctx.join("/"), last.identity().into()))
                        .cloned()
                        .ok_or_else(|| {
                            "Connection endpoint is outside this supported view projection".into()
                        })
                };
                if ends.len() != 2 {
                    self.problems
                        .push("Only binary connections are supported in this profile".into());
                    continue;
                }
                match (resolve(ends[0]), resolve(ends[1])) {
                    (Ok(source), Ok(target)) => self.edges.push(Edge {
                        id: self.symbol_id(
                            &self.binding(connection, &context)?.key,
                            &context,
                            "connection",
                        ),
                        notation: "connection".into(),
                        label: name(connection).unwrap_or("").into(),
                        source,
                        target,
                        binding: self.binding(connection, &context)?,
                    }),
                    (a, b) => self.problems.push(a.err().or(b.err()).unwrap_or_default()),
                }
            }
        }
        Ok(Node {
            id,
            notation: "structure".into(),
            title,
            subtitle: o.class_key().into(),
            binding,
            compartments: if rows.is_empty() {
                vec![]
            } else {
                vec![Compartment {
                    label: "attributes".into(),
                    rows,
                }]
            },
            children: nodes,
            ports,
        })
    }
}

fn view_kind(model: &Model, o: Obj<'_>) -> Result<String, String> {
    let ancestors = closure(model, o)?;
    if ancestors.iter().any(|a| {
        qualified(*a).ok().as_deref() == Some("StandardViewDefinitions::InterconnectionView")
    }) {
        return Ok("interconnection".into());
    }
    if ancestors
        .iter()
        .any(|a| qualified(*a).ok().as_deref() == Some("StandardViewDefinitions::GeneralView"))
    {
        return Ok("general".into());
    }
    Err("Only views specializing StandardViewDefinitions::GeneralView or InterconnectionView are supported".into())
}
fn exposed<'m>(model: &'m Model, view: Obj<'m>) -> Result<Vec<Obj<'m>>, String> {
    let mut result = BTreeMap::new();
    for ty in closure(model, view)? {
        for relation in refs(ty, "ownedRelationship") {
            if is(model, relation, "ElementFilterMembership") {
                return Err("View filter evaluation is not qualified in this profile; the view is not rendered".into());
            }
            if is(model, relation, "MembershipExpose") {
                for membership in refs(relation, "importedMembership") {
                    let elements = refs(membership, "ownedRelatedElement");
                    if elements.is_empty() {
                        return Err(
                            "Referenced/aliased exposure is not supported by this profile".into(),
                        );
                    }
                    for o in elements {
                        result.insert(o.identity().to_owned(), o);
                    }
                }
            } else if is(model, relation, "NamespaceExpose") {
                for namespace in refs(relation, "importedNamespace") {
                    if refs(namespace, "ownedRelationship")
                        .iter()
                        .any(|r| is(model, *r, "Import"))
                    {
                        return Err("Namespace exposes with imported members require upstream exposure evaluation; use explicit member exposes".into());
                    }
                    let mut stack = children(model, namespace);
                    let mut work = 0;
                    while let Some(o) = stack.pop() {
                        work += 1;
                        if work > 2048 {
                            return Err("Exposure budget exceeded".into());
                        }
                        if name(o).is_some() {
                            result.insert(o.identity().into(), o);
                        }
                        if flag(relation, "isRecursive") {
                            stack.extend(children(model, o));
                        }
                    }
                }
            }
        }
    }
    if result.len() > MAX_NODES {
        return Err("View exposes more than 500 elements".into());
    }
    Ok(result.into_values().collect())
}

impl Host {
    pub(super) fn views_catalog(&self) -> Result<Value, String> {
        let model = self.current.as_ref().ok_or("No accepted project model")?;
        if self.baseline.is_none() {
            return Err("Views require a clean, current project model".into());
        }
        let _ = catalog(model, &self.editable)?;
        let mut result = Vec::new();
        for view in model.objects().filter(|o| {
            is(model, *o, "ViewUsage") && self.editable.contains(o.resource()) && name(*o).is_some()
        }) {
            let p = Projection {
                host: self,
                model,
                view_key: key(view)?,
                count: 0,
                problems: vec![],
                edges: vec![],
                endpoints: BTreeMap::new(),
            };
            let mut problems = Vec::new();
            let kind = view_kind(model, view).unwrap_or_else(|e| {
                problems.push(e);
                "unsupported".into()
            });
            let exposures = exposed(model, view).unwrap_or_else(|e| {
                problems.push(e);
                vec![]
            });
            let mut owners = BTreeMap::new();
            let mut details = Vec::new();
            for ty in closure(model, view)? {
                for child in children(model, ty).into_iter().filter(|c| {
                    is(model, *c, "ViewpointUsage") && self.editable.contains(c.resource())
                }) {
                    let label = format!(
                        "Viewpoint · {}{}",
                        name(child).unwrap_or("(unnamed)"),
                        generals(model, child)
                            .into_iter()
                            .filter(|g| is(model, *g, "ViewpointDefinition"))
                            .filter_map(name)
                            .map(|n| format!(" : {n}"))
                            .collect::<String>()
                    );
                    details.push(Field {
                        label,
                        binding: p.binding(child, &[])?,
                    });
                }
            }
            for o in &exposures {
                if ["Package", "PartDefinition", "PartUsage"]
                    .iter()
                    .any(|c| is(model, *o, c))
                    && !is(model, *o, "ViewUsage")
                    && self.editable.contains(o.resource())
                {
                    owners.insert(
                        key(*o)?,
                        Owner {
                            key: key(*o)?,
                            label: qualified(*o)?,
                        },
                    );
                }
            }
            result.push(View {
                key: key(view)?,
                name: name(view).unwrap_or("").into(),
                kind,
                binding: p.binding(view, &[])?,
                owners: owners.into_values().collect(),
                details,
                problems,
            });
        }
        let catalog = intersect_views::ViewCatalog {
            schema_version: VERSION,
            revision: self.revision,
            views: result,
        };
        catalog.validate()?;
        serde_json::to_value(catalog).map_err(|e| e.to_string())
    }
    pub(super) fn views_scene(&self, params: &Value) -> Result<Value, String> {
        self.check_revision(params)?;
        let requested = params["viewKey"].as_str().ok_or("Missing view key")?;
        let catalog = self.views_catalog()?;
        let view: View = serde_json::from_value(
            catalog["views"]
                .as_array()
                .ok_or("Invalid view catalog")?
                .iter()
                .find(|v| v["key"].as_str() == Some(requested))
                .ok_or("Unknown view")?
                .clone(),
        )
        .map_err(|e| e.to_string())?;
        if !view.problems.is_empty() {
            return Err(view.problems.join("\n"));
        }
        let model = self.current.as_ref().ok_or("No accepted model")?;
        let object = model
            .lookup(&view.binding.element_id)
            .ok_or("View no longer exists")?;
        let mut palette: Palette = if params["palette"].is_null() {
            serde_json::from_str(include_str!("../assets/structure.palette.json"))
        } else {
            serde_json::from_value(params["palette"].clone())
        }
        .map_err(|e| format!("Invalid palette: {e}"))?;
        palette.validate()?;
        // Unknown language commands remain visible and disabled in capabilities.
        let mut p = Projection {
            host: self,
            model,
            view_key: requested.into(),
            count: 0,
            problems: vec![],
            edges: vec![],
            endpoints: BTreeMap::new(),
        };
        let mut nodes = Vec::new();
        for o in exposed(model, object)? {
            if name(o).is_none() {
                continue;
            }
            if view.kind == "general" {
                nodes.push(p.general_node(o)?);
            } else if is(model, o, "PartUsage") && !is(model, o, "ViewUsage") {
                nodes.push(p.structure_node(o, &[], 0)?);
            } else {
                p.problems.push(format!(
                    "{} is not a part usage supported by the interconnection profile",
                    name(o).unwrap_or(o.class_key())
                ));
            }
        }
        if view.kind == "general" {
            let symbols: BTreeMap<_, _> = nodes
                .iter()
                .map(|n| (n.binding.element_id.clone(), n.id.clone()))
                .collect();
            for o in exposed(model, object)? {
                for r in refs(o, "ownedRelationship")
                    .into_iter()
                    .filter(|r| is(model, *r, "Specialization"))
                {
                    for target in refs(r, "general") {
                        if let (Some(source), Some(target)) =
                            (symbols.get(o.identity()), symbols.get(target.identity()))
                        {
                            p.edges.push(Edge {
                                id: p.symbol_id(
                                    &format!("{}:{target}", p.binding(r, &[])?.key),
                                    &[],
                                    "specialization",
                                ),
                                notation: "specialization".into(),
                                label: if is(model, r, "FeatureTyping") {
                                    "defined by"
                                } else {
                                    "specializes"
                                }
                                .into(),
                                source: source.clone(),
                                target: target.clone(),
                                binding: p.binding(r, &[])?,
                            });
                        }
                    }
                }
            }
        }
        // Declaration of a command is not proof of implementation. Capabilities
        // make unsupported profile commands explicit before any gesture.
        let capabilities:Vec<_>=palette.groups.iter_mut().flat_map(|g|g.entries.iter_mut()).map(|t|{
            let supported=matches!(t.command.as_str(),"presentation.select"|"presentation.fit"|"presentation.layout"|"sysml.createPartUsage"|"sysml.createPortUsage");
            json!({"toolId":t.id,"enabled":supported&&(!t.command.starts_with("sysml.")||!view.owners.is_empty()),"reason":if !supported {"Command is not implemented by the language service"}else if t.command.starts_with("sysml.")&&view.owners.is_empty(){"This view has no writable supported owner"}else{""}})
        }).collect();
        let layout = LayoutHints {
            direction: if view.kind == "general" {
                "DOWN"
            } else {
                "RIGHT"
            }
            .into(),
        };
        let scene = Scene {
            schema_version: VERSION,
            revision: self.revision,
            layout,
            view,
            nodes,
            edges: p.edges,
            palette,
            problems: p.problems,
        };
        let response: intersect_views::SceneResponse =
            serde_json::from_value(json!({"scene":scene,"capabilities":capabilities}))
                .map_err(|e| e.to_string())?;
        response.validate()?;
        serde_json::to_value(response).map_err(|e| e.to_string())
    }
    pub(super) fn views_locate(&self, params: &Value) -> Result<Value, String> {
        self.check_revision(params)?;
        let model = self.current.as_ref().ok_or("No accepted model")?;
        let uri = params["uri"].as_str().ok_or("Missing source URI")?;
        let offset = position_byte(
            self.source(uri).ok_or("Unknown source")?,
            &params["position"],
        )?;
        let mut candidates: Vec<_> = model
            .objects()
            .filter(|o| o.resource() == uri && o.span().0 <= offset && offset < o.span().1)
            .collect();
        candidates.sort_by_key(|o| o.span().1.saturating_sub(o.span().0));
        for o in candidates {
            // This consumes resolved engine relationships, never a spelling search.
            if is(model, o, "Specialization")
                && let Some(target) = refs(o, "general").into_iter().find(|o| name(*o).is_some())
            {
                return Ok(json!({"elementId":target.identity(),"key":key(target)?}));
            }
            if name(o).is_some() {
                return Ok(json!({"elementId":o.identity(),"key":key(o)?}));
            }
        }
        Ok(Value::Null)
    }
}

pub(super) type Command = intersect_views::PrepareCommand;
pub(super) fn prepare_patch(
    host: &Host,
    command: &Command,
) -> Result<(SourcePatch, String, String), String> {
    let requested_name = command
        .arguments
        .get("name")
        .ok_or("Missing name argument")?;
    if command.revision != host.revision {
        return Err("Diagram command is stale".into());
    }
    let response = host.views_scene(
        &json!({"revision":command.revision,"viewKey":command.view_key,"palette":command.palette}),
    )?;
    let scene: Scene =
        serde_json::from_value(response["scene"].clone()).map_err(|e| e.to_string())?;
    let tool = scene
        .palette
        .groups
        .iter()
        .flat_map(|g| &g.entries)
        .find(|t| t.id == command.tool_id)
        .ok_or("Unknown palette tool")?;
    let keyword = match tool.command.as_str() {
        "sysml.createPartUsage" => "part",
        "sysml.createPortUsage" => "port",
        _ => return Err("This tool is not a supported model-editing command".into()),
    };
    if requested_name.len() > 128
        || !command
            .arguments
            .get("name")
            .ok_or("Missing name")?
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        || !requested_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err("Use a simple identifier of at most 128 ASCII letters, digits or underscores, starting with a letter or underscore".into());
    }
    let model = host.current.as_ref().ok_or("No accepted model")?;
    let owner_id = catalog(model, &host.editable)?
        .get(&command.owner_key)
        .cloned()
        .ok_or("Owner no longer exists")?;
    let owner = model.lookup(&owner_id).ok_or("Owner no longer exists")?;
    let mut allowed = scene.view.owners.iter().any(|o| o.key == command.owner_key);
    fn contains(nodes: &[Node], key: &str) -> bool {
        nodes
            .iter()
            .any(|n| n.binding.key == key || contains(&n.children, key))
    }
    allowed |= contains(&scene.nodes, &command.owner_key);
    if !allowed
        || !host.editable.contains(owner.resource())
        || is(model, owner, "ViewUsage")
        || !["Package", "PartDefinition", "PartUsage"]
            .iter()
            .any(|c| is(model, owner, c))
    {
        return Err("Owner is read-only, outside this view or unsupported for creation".into());
    }
    if closure(model, owner)?
        .into_iter()
        .flat_map(|o| children(model, o))
        .any(|o| name(o) == Some(requested_name.as_str()))
    {
        return Err("An owned or inherited member already has that name".into());
    }
    let source = host
        .source(owner.resource())
        .ok_or("Missing owner source")?;
    let (start, end) = owner.span();
    let fragment = source
        .get(start..end)
        .ok_or("Owner source span is invalid")?;
    // Shared core lexer supplies a lexical token anchor, including correct
    // handling of braces in comments/quoted strings. There is no second parser.
    let tokens = grammar::lex_with_limit(fragment, Some(200_000)).map_err(|(_, e)| e)?;
    let last = tokens.last().ok_or("Owner has no lexical terminator")?;
    if !["}", ";"].contains(&last.text.as_str()) {
        return Err("Unsupported owner body terminator".into());
    }
    let anchor = start + last.start;
    let before_line = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let indentation: String = source[before_line..start]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let declaration = format!("{keyword} {requested_name};");
    let (patch_start, original, replacement) = if last.text == ";" {
        (
            anchor,
            ";".to_owned(),
            format!(" {{{newline}{indentation}    {declaration}{newline}{indentation}}}"),
        )
    } else {
        let line_start = source[..anchor].rfind('\n').map_or(anchor, |i| i + 1);
        if line_start >= start
            && source[line_start..anchor]
                .chars()
                .all(|c| c == ' ' || c == '\t')
        {
            (
                line_start,
                String::new(),
                format!("{indentation}    {declaration}{newline}"),
            )
        } else {
            (
                anchor,
                String::new(),
                format!("{newline}{indentation}    {declaration}{newline}{indentation}"),
            )
        }
    };
    let mut after = source.to_owned();
    after.replace_range(patch_start..patch_start + original.len(), &replacement);
    let patch = SourcePatch {
        uri: owner.resource().into(),
        range: Range {
            start: position(source, patch_start),
            end: position(source, patch_start + original.len()),
        },
        original,
        replacement,
        before_hash: hash(source),
        after_hash: hash(&after),
    };
    patch.validate()?;
    Ok((
        patch,
        after,
        format!("{}::{requested_name}", command.owner_key),
    ))
}
