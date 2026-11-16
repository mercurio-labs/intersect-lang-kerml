//! Contextual reference completion. Parser occurrences select the reference
//! feature; every suggested spelling is checked by the SAME authored scopes as
//! the linker. The spelling inventory is only a finite set of query seeds.
use crate::{Host, Observer, Source, byte_position, editing};
use intersect_kerml::sysml_host::{SysmlBindings, with_sysml_host};
use intersect_model::{
    core::{
        frozen::{FeatureId, FrozenError, FrozenStore, LinkView, ObjId},
        query::QueryBudget,
        storage,
    },
    definitions::conversion::InvocationContext,
    frozen_scopes::RcoreScopes,
};
use intersect_text::{EvaluationLimits, grammar::RuntimePredictionMemoMode};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

const MARKER: &str = "__intersect_reference_cursor__";
const MAX_QUERIES: usize = 128;
const PARSE_WORK: usize = 20_000_000;

impl Host {
    /// Current-buffer completion shared by native hosts and the worker endpoint.
    /// Speculative parses are isolated and never replace the accepted analysis.
    pub fn complete(&self, params: &Value) -> Result<Value, String> {
        let lexical = self.documents.complete(params)?;
        let Some(context) = self.documents.context(params)? else {
            return Ok(lexical);
        };
        let Some(loaded) = &self.loaded else {
            return Ok(lexical);
        };
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or("Missing document URI")?;
        if context.text.contains(MARKER) || context.text.len() > 256 * 1024 {
            return Ok(lexical);
        }
        let mut sources = self.sources.clone();
        self.documents.overlay(&mut sources);
        sources.insert(uri.to_owned(), context.text.to_owned());
        if sources.len() > 256 || sources.values().map(String::len).sum::<usize>() > 2 * 1024 * 1024
        {
            return Ok(lexical);
        }
        let probes = probe_texts(context.text, context.start, context.end);
        let preparation = InvocationContext::new(Rc::new(Observer));
        let semantic = with_sysml_host(
            &loaded.session,
            &preparation,
            SysmlBindings {
                bundle: &loaded.bundle,
                scopes: &loaded.scopes,
                identity: &loaded.identity,
            },
            RuntimePredictionMemoMode::On,
            |host| {
                let resolvers = RcoreScopes::new(host.metamodel(), &loaded.bundle, &loaded.scopes)
                    .map_err(|e| e.to_string())?
                    .with_budget(100_000);
                // All parse attempts share one caller pool, including failures.
                let mut work = QueryBudget::new(PARSE_WORK);
                for probe in probes {
                    let mut store = FrozenStore::from_model(loaded.library.clone());
                    let mut site = None;
                    for (source_uri, text) in &sources {
                        let source = Source {
                            uri: source_uri.clone(),
                            text: if source_uri == uri {
                                probe.clone()
                            } else {
                                text.clone()
                            },
                        };
                        let (parsed, _, _) = host.parse_with(
                            &source,
                            &mut store,
                            EvaluationLimits {
                                steps_per_resource: Some(PARSE_WORK),
                                nesting: 256,
                                resources: Some(256),
                                input_bytes: Some(2 * 1024 * 1024),
                                ..EvaluationLimits::workbench()
                            },
                            &mut work,
                        )?;
                        if source_uri == uri && parsed.diagnostics.is_empty() {
                            site = parsed.unresolved_references.into_iter().find(|r| {
                                r.start <= context.start
                                    && context.start < r.end
                                    && r.spelling.contains(MARKER)
                            });
                        }
                    }
                    let Some(site) = site else {
                        continue;
                    };
                    let Some(qualifier) = site.spelling.strip_suffix(MARKER) else {
                        continue;
                    };
                    let mut names = BTreeSet::new();
                    storage::visit_views(&store, |object| {
                        for field in [
                            "declaredName",
                            "declaredShortName",
                            "memberName",
                            "memberShortName",
                        ] {
                            if let Some(name) = object.value(field).and_then(|value| value.as_str())
                                && name.starts_with(context.prefix)
                                && let Some(spelling) = editing::name_spelling(name)
                            {
                                names.insert((name.to_owned(), spelling));
                            }
                        }
                        Ok(())
                    })?;
                    let incomplete = names.len() > MAX_QUERIES;
                    let results = RefCell::new(Vec::new());
                    let resolve = |view: &LinkView<'_>,
                                   owner: ObjId,
                                   feature: FeatureId,
                                   spelling: &str|
                     -> Result<Option<ObjId>, FrozenError> {
                        if view.identity(owner) == Some(site.owner.as_str())
                            && view.metamodel().feature_name(feature) == site.feature
                            && spelling == site.spelling
                        {
                            for (label, insertion) in names.iter().take(MAX_QUERIES) {
                                let spelling = format!("{qualifier}{insertion}");
                                // Never suggest an ambiguous, out-of-scope or wrong-type name.
                                if let Ok(found) =
                                    resolvers.candidates(view, owner, feature, &spelling)
                                    && let [target] = found.as_slice()
                                    && let Some(class) = view.class(*target)
                                {
                                    results.borrow_mut().push(json!({"label":label,"kind":if view.metamodel().class_key(class).ends_with("Package") {9} else {7},
                                        "detail":view.metamodel().class_key(class),"sortText":format!("0{label}"),
                                        "textEdit":{"range":{"start":byte_position(context.text,context.start),"end":byte_position(context.text,context.end)},"newText":insertion}}));
                                }
                            }
                            return Ok(None);
                        }
                        let found = resolvers.candidates(view, owner, feature, spelling)?;
                        Ok(if found.len() == 1 {
                            found.first().copied()
                        } else {
                            None
                        })
                    };
                    // The cursor marker deliberately remains unresolved. Only its
                    // scoped query results escape; no speculative model is published.
                    let _ = store
                        .into_builder()
                        .map_err(|e| e.to_string())?
                        .link(&resolve);
                    return Ok(Some(
                        json!({"isIncomplete":incomplete,"items":results.into_inner()}),
                    ));
                }
                Ok(None)
            },
        );
        // Unsupported/incomplete grammar contexts keep existing lexical editing.
        Ok(semantic.ok().flatten().unwrap_or(lexical))
    }
}

// Bounded syntax repairs for an editing probe, always checked by the real parser.
// Full-buffer attempts preserve declarations after the cursor. Prefix attempts
// let an unfinished tail be edited without trusting the previous analysis.
fn probe_texts(text: &str, start: usize, end: usize) -> Vec<String> {
    let prefix = format!("{}{MARKER}", &text[..start]);
    let full = format!("{prefix}{}", &text[end..]);
    let terminated = format!("{prefix};{}", &text[end..]);
    let mut probes = vec![full.clone()];
    for candidate in [terminated, format!("{prefix};"), prefix] {
        if let Some(tail) = closing_suffix(&candidate) {
            let repaired = format!("{candidate}\n{tail}");
            if !probes.contains(&repaired) {
                probes.push(repaired);
            }
        }
    }
    probes
}

fn closing_suffix(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut stack = Vec::new();
    let mut quote = None;
    let (mut line, mut block, mut i) = (false, false, 0);
    while i < bytes.len() {
        let byte = bytes[i];
        let pair = bytes.get(i..i + 2);
        if line {
            if byte == b'\n' {
                line = false;
            }
        } else if block {
            if pair == Some(b"*/") {
                block = false;
                i += 1;
            }
        } else if let Some(q) = quote {
            if byte == b'\\' {
                i += 1;
            } else if byte == q {
                quote = None;
            }
        } else if pair == Some(b"//") {
            line = true;
            i += 1;
        } else if pair == Some(b"/*") {
            block = true;
            i += 1;
        } else {
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'{' => stack.push('}'),
                b'(' => stack.push(')'),
                b'[' => stack.push(']'),
                b'}' | b')' | b']' if stack.pop() != Some(byte as char) => return None,
                _ => {}
            }
        }
        i += 1;
    }
    if quote.is_some() || block {
        return None;
    }
    Some(stack.into_iter().rev().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_repairs_are_bounded_and_ignore_non_code_delimiters() {
        assert_eq!(
            closing_suffix("package P { /* } */ 'x}' // }"),
            Some("}".into())
        );
        assert_eq!(closing_suffix("package P { 'unfinished"), None);
        assert_eq!(closing_suffix("package P { )"), None);
        let text = "package P { part p : Ve";
        let probes = probe_texts(text, text.len() - 2, text.len());
        assert!(probes.len() <= 4);
        assert!(probes.iter().any(|p| p.ends_with(";\n}")));
        assert_eq!(editing::name_spelling("part"), Some("'part'".into()));
        assert_eq!(
            editing::name_spelling("fuel tank"),
            Some("'fuel tank'".into())
        );
    }
}
