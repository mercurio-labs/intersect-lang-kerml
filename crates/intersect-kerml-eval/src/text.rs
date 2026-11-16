//! Handwritten expression wrappers and result discovery; parsing stays in the host.
use crate::{EvalError, Model, ObjId, SourceMap, nav::Nav, schema::EvalSchema};
use intersect_text::Source;

/// Construct the Pilot wrapper, rejecting text that could escape its import.
pub fn eval_source(
    target: Option<&str>,
    expression: &str,
    n: usize,
) -> Result<(Source, SourceMap), EvalError> {
    let prefix = if let Some(target) = target {
        if canonical_name(target).is_none() {
            return Err(EvalError::Model {
                object: target.into(),
                message: "invalid target qualified name".into(),
            });
        }
        format!("calc {{private import {target}::*;\n")
    } else {
        "calc {\n".into()
    };
    let uri = format!("memory:/intersect-eval/{n}.sysml");
    let map = SourceMap {
        uri: uri.clone(),
        prefix_bytes: prefix.len(),
        expression_len: expression.len(),
    };
    Ok((
        Source {
            uri,
            text: format!("{prefix}{expression}}}"),
        },
        map,
    ))
}

// Escaped names are single-quoted; backslash escapes only a quote or backslash.
// Separators inside a quoted segment remain part of its name.
pub(crate) fn canonical_name(name: &str) -> Option<String> {
    let mut chars = name.chars().peekable();
    let mut parts = Vec::new();
    loop {
        let mut part = String::new();
        match chars.next() {
            Some('\'') => loop {
                match chars.next() {
                    Some('\'') if !part.is_empty() => break,
                    Some('\\') => match chars.next() {
                        Some(c @ ('\'' | '\\')) => part.push(c),
                        _ => return None,
                    },
                    Some(c) if c != '\'' && !c.is_control() => part.push(c),
                    _ => return None,
                }
            },
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                part.push(c);
                while chars
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    if let Some(c) = chars.next() {
                        part.push(c);
                    }
                }
            }
            _ => return None,
        }
        parts.push(crate::nav::escape(&part));
        match chars.next() {
            None => return Some(parts.join("::")),
            Some(':') if chars.next() == Some(':') => {}
            _ => return None,
        }
    }
}

/// Find the wrapper's top-level calc result, never a nested body's result.
pub fn wrapper_expression(model: &Model, uri: &str) -> Result<ObjId, EvalError> {
    let schema = EvalSchema::new(model.metamodel())?;
    let nav = Nav::new(model, &schema);
    let mut candidates = Vec::new();
    for obj in model.objects().filter(|o| o.resource() == uri) {
        if !nav.is(obj.id(), "CalculationUsage")? {
            continue;
        }
        let mut ancestor = obj.container();
        let mut nested = false;
        while let Some((parent, _)) = ancestor {
            if nav.is(parent.id(), "CalculationUsage")? {
                nested = true;
                break;
            }
            ancestor = parent.container();
        }
        if !nested {
            candidates.push(obj.id());
        }
    }
    if let [calc] = candidates.as_slice()
        && let Some(expression) = nav.owned_result_expression(*calc)?
    {
        return Ok(expression);
    }
    Err(EvalError::Model {
        object: uri.into(),
        message: "wrapper must contain exactly one top-level calc with a result expression".into(),
    })
}
