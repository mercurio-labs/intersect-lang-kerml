//! Handwritten source gates of the exact pinned original SysML NativeV80
//! definition (not generated), shared by the frozen text load and the native
//! SysML draft profiles. They check source identities and grammar shape only.
use intersect_model::core::query::QueryResult;
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::bundle::SourceId;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::conversion::InvocationContext;
use intersect_text::grammar::Profile;
use intersect_text::grammar::RuntimeGuardedPolicy;
use intersect_text::preparation::PreparedLanguageDefinition;
use std::collections::BTreeMap;

const SYSML: &str = "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext";
const SYSML_SHA: &str = "7d9142742c454f5810fe4db61a089abef5e0b8d61ad8bf552631d9136b04f298";

pub(crate) fn source_closure(
    definition: &PreparedLanguageDefinition<'_>,
) -> QueryResult<(Vec<SourceId>, Vec<SourceId>)> {
    definition.original_sources().validate()?;
    definition.sources().validate()?;
    let original = definition.original_sources();
    let converted = definition.sources();
    for (bundle, structure_uri, structure_sha, structure_kind) in [
        (
            original,
            super::SCHEMA,
            crate::PILOT_SCHEMA_SHA256,
            SourceKind::Ecore,
        ),
        (
            converted,
            "org.omg.sysml/model/SysML.ecore.rcore",
            crate::PILOT_RCORE_SCHEMA_SHA256,
            SourceKind::Rcore,
        ),
    ] {
        let wanted: BTreeMap<_, _> = [
            (structure_uri, (structure_sha, structure_kind)),
            (
                super::EXPRESSIONS,
                (super::EXPRESSION_SHA, SourceKind::Xtext),
            ),
            (SYSML, (SYSML_SHA, SourceKind::Xtext)),
            (
                "probe-behavior.rcore",
                (super::EMPTY_BEHAVIOR_SHA, SourceKind::RcoreBehavior),
            ),
        ]
        .into();
        if bundle.sources.len() != wanted.len()
            || bundle.sources.iter().any(|source| {
                wanted
                    .get(source.id.uri.as_str())
                    .is_none_or(|(sha, kind)| {
                        *sha != source.id.sha256.as_str() || kind != &source.kind
                    })
            })
        {
            return Err("Unsupported original SysML Package source closure".into());
        }
        let structure = bundle
            .sources
            .iter()
            .find(|s| s.id.uri == structure_uri)
            .ok_or("Missing original SysML structure")?;
        let expressions = bundle
            .sources
            .iter()
            .find(|s| s.id.uri == super::EXPRESSIONS)
            .ok_or("Missing original Expressions dependency")?;
        let entry = bundle
            .sources
            .iter()
            .find(|s| s.id.uri == SYSML)
            .ok_or("Missing original SysML entry")?;
        if expressions.dependencies.as_slice() != std::slice::from_ref(&structure.id)
            || entry.dependencies != [structure.id.clone(), expressions.id.clone()]
        {
            return Err("SysML Package source dependency order changed".into());
        }
    }
    let schema = definition.schema();
    let grammar = definition
        .grammar()
        .ok_or("Missing complete SysML grammar")?;
    let admission = definition
        .runtime_admission()
        .ok_or("SysML Package requires the actual compiler-owned DRAFT runtime proof")?;
    let prediction = grammar
        .parser_prediction
        .as_ref()
        .ok_or("Missing exact SysML prediction program")?;
    if schema.revision != super::REVISION
        || schema.classes.len() != 175
        || grammar.profile != Profile::NativeV80
        || grammar.entry != "RootNamespace"
        || grammar.rules.len() != 540
        || grammar.terminals.len() != 9
        || grammar.lexical_prediction.is_none()
        || admission.policy() != RuntimeGuardedPolicy::InputCheckedCompletionScopes
        || admission.decision_count() == 0
        || !prediction.requires_runtime_capability()
        || prediction.input_checked_completion_obligation_count() != admission.decision_count()
        || prediction.strict_completion_obligation_count() != 0
    {
        return Err(
            "SysML Package requires the complete exact original Native80 DRAFT definition".into(),
        );
    }
    let source = |uri: &str| original.sources.iter().find(|s| s.id.uri == uri);
    let entry = source(SYSML).ok_or("Missing original SysML entry identity")?;
    let expressions = source(super::EXPRESSIONS).ok_or("Missing Expressions identity")?;
    if grammar.declaration_sources.len() != 549
        || grammar
            .shadowed_declarations
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            != ["ExpressionBody", "ResultExpressionMember"]
        || grammar.shadowed_declarations.values().any(|rows| {
            rows.len() != 1
                || rows.iter().any(|span| {
                    span.source != expressions.id
                        || span.start >= span.end
                        || span.end > expressions.text.len()
                })
        })
    {
        return Err("SysML Package declaration/shadowing closure changed".into());
    }
    for (name, span) in &grammar.declaration_sources {
        let producer = [entry, expressions]
            .into_iter()
            .find(|s| s.id == span.source)
            .ok_or("SysML declaration has an unsupported producer")?;
        if span.start >= span.end
            || span.end > producer.text.len()
            || (!grammar.rules.contains_key(name) && !grammar.terminals.contains_key(name))
            || (grammar.terminals.contains_key(name) && span.source != expressions.id)
        {
            return Err("SysML Package declaration source/range changed".into());
        }
    }
    Ok((
        original.sources.iter().map(|s| s.id.clone()).collect(),
        converted.sources.iter().map(|s| s.id.clone()).collect(),
    ))
}

fn exact_sources(
    bundle: &DefinitionBundle,
    wanted: &[(&str, &str); 4],
    preparation: &InvocationContext<'_>,
) -> Result<(), String> {
    preparation.work(1)?;
    if bundle.sources.len() != wanted.len() {
        return Err("SysML physical profile requires its exact four-source closure".into());
    }
    // PreparedLanguageDefinition is an immutable compiler-owned source view;
    // no caller-authored SourceId or equal serialized schema replaces it here.
    for source in &bundle.sources {
        preparation.work(
            wanted.len()
                * (2 + source.id.uri.len().div_ceil(64) + source.id.sha256.len().div_ceil(64)),
        )?;
        if !wanted
            .iter()
            .any(|(uri, hash)| source.id.uri == *uri && source.id.sha256 == *hash)
        {
            return Err("Foreign original/converted SysML physical source identity".into());
        }
    }
    for (uri, _) in wanted {
        preparation.work(bundle.sources.len() * (1 + uri.len().div_ceil(64)))?;
        if bundle.sources.iter().filter(|s| s.id.uri == *uri).count() != 1 {
            return Err("Duplicate or missing SysML physical source identity".into());
        }
    }
    Ok(())
}

pub(crate) fn check(
    definition: &PreparedLanguageDefinition<'_>,
    preparation: &InvocationContext<'_>,
) -> Result<(), String> {
    preparation.work(1)?;
    // This opaque view is created only by the canonical compiler/preparation
    // source validator and borrows its immutable bundles. Reuse that authority;
    // rehashing every source here would be a new, separately payable source
    // traversal. The complete source-ID/override checks below still run.
    exact_sources(
        definition.original_sources(),
        &[
            (super::SCHEMA, crate::PILOT_SCHEMA_SHA256),
            (super::EXPRESSIONS, super::EXPRESSION_SHA),
            (SYSML, SYSML_SHA),
            ("probe-behavior.rcore", super::EMPTY_BEHAVIOR_SHA),
        ],
        preparation,
    )?;
    exact_sources(
        definition.sources(),
        &[
            (
                "org.omg.sysml/model/SysML.ecore.rcore",
                crate::PILOT_RCORE_SCHEMA_SHA256,
            ),
            (super::EXPRESSIONS, super::EXPRESSION_SHA),
            (SYSML, SYSML_SHA),
            ("probe-behavior.rcore", super::EMPTY_BEHAVIOR_SHA),
        ],
        preparation,
    )?;
    let grammar = definition
        .grammar()
        .ok_or("Missing original SysML grammar")?;
    let program = grammar
        .parser_prediction
        .as_ref()
        .ok_or("Missing actual SysML prediction program")?;
    let admission = definition
        .runtime_admission()
        .ok_or("Missing same-owner SysML runtime admission")?;
    preparation.work(12)?;
    if definition.schema().revision != super::REVISION
        || definition.schema().classes.len() != 175
        || grammar.profile != Profile::NativeV80
        || grammar.entry != "RootNamespace"
        || grammar.rules.len() != 540
        || grammar.terminals.len() != 9
        || grammar.declaration_sources.len() != 549
        || admission.policy() != RuntimeGuardedPolicy::InputCheckedCompletionScopes
        || admission.decision_count() == 0
        || program.input_checked_completion_obligation_count() != admission.decision_count()
        || program.strict_completion_obligation_count() != 0
    {
        return Err(
            "SysML physical profile requires its genuine complete Native80 DRAFT owner".into(),
        );
    }
    preparation.work(3)?;
    if !grammar
        .shadowed_declarations
        .keys()
        .map(String::as_str)
        .eq(["ExpressionBody", "ResultExpressionMember"])
    {
        return Err("SysML physical profile original override closure differs".into());
    }
    preparation.checkpoint()?;
    Ok(())
}
