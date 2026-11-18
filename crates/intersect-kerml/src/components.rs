//! SysML/KerML component sessions over the same native parser and authored
//! Rcore programs as the ordinary host. No acquisition or editor I/O lives here.
use crate::{frozen_load, native_pilot::NativePilotSession, sysml_host::SysmlBindings};
use intersect_model::{
    components::{ComponentImage, MountedDraft, SharedSchema},
    definitions::conversion::InvocationContext,
    definitions::schema::Diagnostic,
    frozen::{FrozenStore, Model as FrozenModel, Severity},
    frozen_scopes::RcoreScopes,
};
use intersect_text::{
    Reference, Source,
    components::{ComponentAnalysis, ComponentLanguage, ParsedComponentSource},
};
use std::sync::Arc;

type Parse<'a> = dyn Fn(&Source, &mut FrozenStore) -> Result<ParsedComponentSource, String> + 'a;

pub struct SysmlComponentLanguage<'a> {
    context: Arc<SharedSchema>,
    parse: &'a Parse<'a>,
    scopes: &'a RcoreScopes<'a>,
}

impl SysmlComponentLanguage<'_> {
    /// Explicit cold preparation of a closed KPAR library. The ordinary native
    /// frozen loader performs linking, completion, structural and language checks.
    pub fn prepare_component(
        &self,
        uri: &str,
        sources: &[Source],
    ) -> Result<Arc<ComponentImage>, String> {
        ComponentImage::prepare(self.context.clone(), uri, self.prepare_model(sources)?)
    }

    pub fn prepare_component_with_snapshot(
        &self,
        uri: &str,
        sources: &[Source],
    ) -> Result<(Arc<ComponentImage>, Vec<u8>), String> {
        ComponentImage::prepare_with_snapshot(
            self.context.clone(),
            uri,
            self.prepare_model(sources)?,
        )
    }

    /// Resolve and validate a closed textual library before host-owned packaging.
    pub fn prepare_model(&self, sources: &[Source]) -> Result<FrozenModel, String> {
        let mut load = frozen_load::load_frozen(
            FrozenStore::new(self.context.metamodel()),
            sources,
            |source, store| {
                let parsed = (self.parse)(source, store)?;
                Ok((parsed.evaluation, parsed.pending.len(), parsed.consumed))
            },
            self.scopes,
            &|_| {},
        )?;
        if !load.is_clean() {
            return Err(format!(
                "Component preparation failed: {:?}",
                load.diagnostics
            ));
        }
        load.model
            .take()
            .ok_or_else(|| "Prepared component has no validated model".into())
    }
}

impl ComponentLanguage for SysmlComponentLanguage<'_> {
    fn context(&self) -> Arc<SharedSchema> {
        self.context.clone()
    }

    fn analyze(
        &self,
        sources: &[Source],
        usages: &[Arc<ComponentImage>],
    ) -> Result<ComponentAnalysis, String> {
        let mut draft = MountedDraft::new(self.context.clone(), usages.to_vec())?;
        let mut diagnostics = Vec::new();
        let mut pending = Vec::new();
        for source in sources {
            match (self.parse)(source, draft.project_store_mut()) {
                Ok(parsed) => {
                    diagnostics.extend(parsed.evaluation.diagnostics);
                    pending.extend(parsed.pending);
                }
                Err(error) => {
                    diagnostics.push(Diagnostic::new(&source.uri, 0, source.text.len(), error))
                }
            }
        }
        let mut references = Vec::new();
        if !diagnostics.is_empty() {
            return Ok(ComponentAnalysis {
                model: None,
                diagnostics,
                references,
            });
        }
        draft.reconcile_opposites()?;
        // Retry dependent scopes after other project references make progress.
        // Components themselves are already resolved and never enter this list.
        let mut remaining = pending;
        loop {
            let mut deferred = Vec::new();
            let mut progress = false;
            for reference in remaining {
                match draft.scope_candidates(
                    self.scopes,
                    &reference.owner,
                    &reference.feature,
                    &reference.spelling,
                ) {
                    Ok(Some(candidates)) if candidates.len() == 1 => {
                        let target = &candidates[0];
                        let actual = draft
                            .reader()
                            .class(target)?
                            .ok_or("Missing scope candidate")?;
                        let owner_class = draft
                            .reader()
                            .class(&reference.owner)?
                            .ok_or("Missing reference owner")?;
                        if !draft.schema().conforms(&actual, &reference.target_class)
                            || !draft.schema().reference_conforms(
                                &owner_class,
                                &reference.feature,
                                &actual,
                            )?
                        {
                            diagnostics.push(Diagnostic::new(
                                &reference.uri,
                                reference.start,
                                reference.end,
                                "Reference candidate has an incompatible type",
                            ));
                            continue;
                        }
                        draft.resolve_reference(
                            &reference.owner,
                            &reference.feature,
                            reference.index,
                            target,
                        )?;
                        references.push(Reference {
                            uri: reference.uri,
                            start: reference.start,
                            end: reference.end,
                            target: target.clone(),
                        });
                        progress = true;
                    }
                    Ok(Some(candidates)) if candidates.len() > 1 => {
                        diagnostics.push(Diagnostic::new(
                            &reference.uri,
                            reference.start,
                            reference.end,
                            format!(
                                "Ambiguous reference '{}' ({} candidates)",
                                reference.spelling,
                                candidates.len()
                            ),
                        ))
                    }
                    Ok(_) => deferred.push(reference),
                    Err(error) => diagnostics.push(Diagnostic::new(
                        &reference.uri,
                        reference.start,
                        reference.end,
                        error,
                    )),
                }
            }
            if deferred.is_empty() {
                break;
            }
            if !progress {
                for reference in deferred {
                    diagnostics.push(Diagnostic::new(&reference.uri, reference.start, reference.end,
                        format!("Unresolved reference '{}' (missing, inaccessible, or cyclic dependency)", reference.spelling)));
                }
                break;
            }
            draft.reconcile_opposites()?;
            remaining = deferred;
        }
        if !diagnostics.is_empty() {
            return Ok(ComponentAnalysis {
                model: None,
                diagnostics,
                references,
            });
        }
        draft.reconcile_opposites()?;
        draft.complete(self.scopes)?;
        for (owner, issue) in draft.issues(self.scopes)? {
            if issue.severity != Severity::Error {
                continue;
            }
            let element = draft
                .reader()
                .get(&owner)?
                .ok_or("Missing validation receiver")?;
            diagnostics.push(Diagnostic::new(
                &element.resource,
                element.start,
                element.end,
                format!("{}: {}", issue.code, issue.message),
            ));
        }
        if !diagnostics.is_empty() {
            return Ok(ComponentAnalysis {
                model: None,
                diagnostics,
                references,
            });
        }
        match draft.finish() {
            Ok(model) => Ok(ComponentAnalysis {
                model: Some(model),
                diagnostics,
                references,
            }),
            Err(error) => Ok(ComponentAnalysis {
                model: None,
                diagnostics: vec![Diagnostic::new(
                    sources.first().map_or("model", |s| s.uri.as_str()),
                    0,
                    0,
                    error,
                )],
                references,
            }),
        }
    }
}

/// Keep parser capabilities and prepared direct bindings alive for every edit
/// in the callback. Neither grammar compilation nor binding compilation repeats.
pub fn with_component_language<T>(
    session: &NativePilotSession,
    preparation: &InvocationContext<'_>,
    bindings: SysmlBindings<'_>,
    body: impl FnOnce(&SysmlComponentLanguage<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let metamodel = session.frozen_metamodel()?;
    let context = SharedSchema::from_metamodel(metamodel)?;
    with_shared_component_language(session, preparation, bindings, context, body)
}

/// Rebind parser capabilities while retaining the exact schema context of
/// previously prepared images. Hosts own the context, never parser lifetimes.
pub fn with_shared_component_language<T>(
    session: &NativePilotSession,
    preparation: &InvocationContext<'_>,
    bindings: SysmlBindings<'_>,
    context: Arc<SharedSchema>,
    body: impl FnOnce(&SysmlComponentLanguage<'_>) -> Result<T, String>,
) -> Result<T, String> {
    if context.metamodel().digest().map_err(|e| e.to_string())?
        != session
            .frozen_metamodel()?
            .digest()
            .map_err(|e| e.to_string())?
    {
        return Err("Component context does not match the installed language".into());
    }
    let scopes = RcoreScopes::new(&context.metamodel(), bindings.bundle, bindings.scopes)
        .map_err(|e| e.to_string())?;
    session.with_definitions(|kerml, kerml_contracts, sysml, sysml_contracts| {
        frozen_load::with_kerml(kerml, kerml_contracts, |kerml| {
            frozen_load::with_sysml(sysml, sysml_contracts, preparation, |sysml| {
                let parse = |source: &Source, store: &mut FrozenStore| {
                    let mut budget = intersect_model::core::query::QueryBudget::new(usize::MAX);
                    if source.uri.ends_with(".kerml") {
                        kerml.language.parse_component_into_with_settings(
                            source,
                            frozen_load::limits(),
                            Some(&kerml.settings),
                            store,
                            &mut budget,
                        )
                    } else {
                        sysml.language.parse_component_into_with_settings(
                            source,
                            frozen_load::limits(),
                            Some(&sysml.settings),
                            store,
                            &mut budget,
                        )
                    }
                };
                body(&SysmlComponentLanguage {
                    context: context.clone(),
                    parse: &parse,
                    scopes: &scopes,
                })
            })
        })
    })
}
