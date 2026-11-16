//! Handwritten evaluation-site enumeration in resource and containment order.
use crate::{EvalError, Model, ObjId, nav::Nav, schema::EvalSchema};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SiteKind {
    FeatureValue,
    ResultExpression,
    MultiplicityBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Site {
    pub expression: ObjId,
    pub target: Option<ObjId>,
    pub kind: SiteKind,
}

/// Enumerate only the named resources, in caller order. A site's expression
/// subtree is consumed by that site, including any nested values or results.
pub fn sites(model: &Model, resources: &[&str]) -> Result<Vec<Site>, EvalError> {
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(model, &schema);
    let mut result = Vec::new();
    let mut visited = BTreeSet::new();
    for uri in resources {
        let mut stack: Vec<_> = model
            .objects()
            .filter(|o| {
                o.resource() == *uri && o.container().is_none_or(|(p, _)| p.resource() != *uri)
            })
            .map(|o| o.id())
            .collect();
        stack.reverse();
        while let Some(id) = stack.pop() {
            if !visited.insert(id) {
                continue;
            }
            if let Some(site) = site(&nav, id)? {
                result.push(site);
                continue;
            }
            // These are the stored Element/Relationship containment fields.
            // Their sequence order, not object allocation order, is authoritative.
            let mut children = nav.relationships(id)?;
            if nav.is(id, "Relationship")? {
                children.extend(nav.owned(id)?);
            }
            for child in children.into_iter().rev() {
                if nav.object(child)?.resource() == *uri {
                    stack.push(child);
                }
            }
        }
    }
    Ok(result)
}

fn ancestor(nav: &Nav<'_>, start: ObjId, class: &str) -> Result<Option<ObjId>, EvalError> {
    let mut current = nav.object(start)?.container();
    let mut visited = BTreeSet::new();
    while let Some((obj, _)) = current {
        if !visited.insert(obj.id()) {
            return Err(nav.error(start, "containment cycle in site target"));
        }
        if nav.is(obj.id(), class)? {
            return Ok(Some(obj.id()));
        }
        current = obj.container();
    }
    Ok(None)
}

fn site(nav: &Nav<'_>, expression: ObjId) -> Result<Option<Site>, EvalError> {
    if !nav.is(expression, "Expression")? {
        return Ok(None);
    }
    let Some((rel, _)) = nav.object(expression)?.container() else {
        return Ok(None);
    };
    let Some((owner, _)) = rel.container() else {
        return Ok(None);
    };
    let (kind, target) = if nav.is(rel.id(), "FeatureValue")? {
        (SiteKind::FeatureValue, ancestor(nav, owner.id(), "Type")?)
    } else if nav.is(rel.id(), "ResultExpressionMembership")? {
        (SiteKind::ResultExpression, ancestor(nav, rel.id(), "Type")?)
    } else if nav.is(rel.id(), "OwningMembership")? && nav.is(owner.id(), "MultiplicityRange")? {
        (
            SiteKind::MultiplicityBound,
            ancestor(nav, owner.id(), "Namespace")?,
        )
    } else {
        return Ok(None);
    };
    Ok(Some(Site {
        expression,
        target,
        kind,
    }))
}
