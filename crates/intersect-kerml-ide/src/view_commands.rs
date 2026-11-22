//! Language-owned lexical source commands. Shared host owns all transactions.
use crate::views::{declared_parent, is, key, name};
use intersect_view_host::{
    CommandContext, CommandError, ProjectModel, insert_member, source_patch,
};
use intersect_views::{Binding, Node, PrepareCommand, SourcePatch};
use std::collections::BTreeMap;
pub(super) struct Commands;
fn fail(message: impl Into<String>) -> CommandError {
    CommandError::new("sysml.command", message)
}
impl intersect_view_host::Commands for Commands {
    fn supports(&self, command: &str) -> bool {
        matches!(
            command,
            "sysml.createPartUsage" | "sysml.createPortUsage" | "sysml.createConnectionUsage"
        )
    }
    fn prepare(
        &self,
        context: &CommandContext<'_>,
        command: &PrepareCommand,
    ) -> Result<Vec<SourcePatch>, CommandError> {
        let requested = command
            .arguments
            .get("name")
            .ok_or_else(|| fail("Missing name argument"))?;
        if requested.len() > 128
            || !requested
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
            || !requested
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(fail(
                "Use a simple ASCII identifier of at most 128 characters",
            ));
        }
        let owner = context
            .keys
            .get(&command.owner_key)
            .and_then(|id| context.model.lookup(id))
            .ok_or_else(|| fail("Owner is not writable"))?;
        let keyword = match context.command {
            "sysml.createPartUsage" => "part",
            "sysml.createPortUsage" => "port",
            "sysml.createConnectionUsage" => "connection",
            _ => return Err(fail("Unsupported command")),
        };
        if !context.sources.contains_key(owner.resource())
            || is(context.model, owner, "ViewUsage")
            || is(context.model, owner, "ConnectionUsage")
            || !["Package", "PartDefinition", "PartUsage"]
                .iter()
                .any(|c| is(context.model, owner, c))
        {
            return Err(fail("Owner is read-only or unsupported for creation"));
        }
        if keyword == "port" && is(context.model, owner, "Package") {
            return Err(fail("Drop a port on a part or part definition"));
        }
        // Use the very same cached Rcore membership/label functions as the rules.
        for member in context.functions.elements("members", owner).map_err(fail)? {
            if context.functions.string("label", member).map_err(fail)? == *requested {
                return Err(fail("An owned or inherited member already has that name"));
            }
        }
        let declaration = if keyword == "connection" {
            let graph = context.scene.graph().map_err(fail)?;
            let source = command
                .arguments
                .get("source")
                .ok_or_else(|| fail("Choose a source endpoint"))?;
            let target = command
                .arguments
                .get("target")
                .ok_or_else(|| fail("Choose a target endpoint"))?;
            let (connection_owner, source, target) =
                connection_paths(context.model, graph, context.keys, source, target)
                    .map_err(fail)?;
            if connection_owner != command.owner_key {
                return Err(fail(
                    "Connection must use the endpoints' common visible scope",
                ));
            }
            format!("connection {requested} connect {source} to {target};")
        } else {
            format!("{keyword} {requested};")
        };
        let source = context
            .sources
            .get(owner.resource())
            .ok_or_else(|| fail("Missing owner source"))?;
        let (range, replacement) =
            insert_member(source, owner.span(), &declaration).map_err(fail)?;
        Ok(vec![
            source_patch(owner.resource(), source, range, replacement).map_err(fail)?,
        ])
    }
}
/// Resolve displayed occurrence IDs to source paths in their common scope.
/// Never accept a client-authored source fragment as a connection endpoint.
fn connection_paths(
    model: ProjectModel<'_>,
    scene: &intersect_views::GraphScene,
    declarations: &BTreeMap<String, String>,
    source: &str,
    target: &str,
) -> Result<(String, String, String), String> {
    fn find<'a>(nodes: &'a [Node], id: &str) -> Option<&'a Binding> {
        for n in nodes {
            if n.id == id {
                return Some(&n.binding);
            }
            if let Some(p) = n.ports.iter().find(|p| p.id == id) {
                return Some(&p.binding);
            }
            if let Some(regions) = &n.regions {
                for region in &regions.items {
                    if let Some(binding) = find(&region.children, id) {
                        return Some(binding);
                    }
                }
            }
            if let Some(b) = find(&n.children, id) {
                return Some(b);
            }
        }
        None
    }
    let endpoint = |id| -> Result<Vec<String>, String> {
        let b = find(&scene.nodes, id).ok_or("Endpoint is outside this view")?;
        let object = model
            .lookup(&b.element_id)
            .ok_or("Endpoint no longer exists")?;
        if (!is(model, object, "PartUsage") || is(model, object, "ViewUsage"))
            && !is(model, object, "PortUsage")
        {
            return Err("Only parts and ports can be connected".into());
        }
        let mut path = b.context.clone();
        if path.last() != Some(&b.key) {
            path.push(b.key.clone());
        }
        Ok(path)
    };
    let a = endpoint(source)?;
    let b = endpoint(target)?;
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    if common == a.len() || common == b.len() {
        return Err("Choose distinct endpoints in sibling scopes".into());
    }
    let lookup = |key: &String| {
        declarations
            .get(key)
            .and_then(|id| model.lookup(id))
            .ok_or("Endpoint scope is not writable or supported")
    };
    let owner = if common > 0 {
        lookup(&a[common - 1])?
    } else {
        let parent = declared_parent(lookup(&a[0])?).ok_or("Endpoint has no source scope")?;
        let other = declared_parent(lookup(&b[0])?).ok_or("Endpoint has no source scope")?;
        if parent.identity() != other.identity() || !is(model, parent, "Package") {
            return Err("Endpoints need a common visible part or package".into());
        }
        parent
    };
    let reference = |path: &[String]| -> Result<String, String> {
        path[common..]
            .iter()
            .map(|k| {
                let object = lookup(k)?;
                // Quoting preserves source names, including reserved words and spaces.
                name(object)
                    .map(|n| format!("'{}'", n.replace('\\', "\\\\").replace('\'', "\\'")))
                    .ok_or_else(|| "An endpoint is unnamed".into())
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|v| v.join("."))
    };
    Ok((key(owner)?, reference(&a)?, reference(&b)?))
}
