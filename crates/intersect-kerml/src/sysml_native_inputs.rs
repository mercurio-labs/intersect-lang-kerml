//! Handwritten promotion of the existing original SysML input-control recipe.
//! Native migration and actual provider validation are shared implementations;
//! this creates preparation inputs only, with no grammar/semantic award.
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::schema::Schema;
use intersect_model::definitions::setting::ComputedSettingContract;
use intersect_text::compiler;
use intersect_text::grammar::SourceLimits;
use intersect_text::grammar::syntax;
use intersect_text::package::BundleCompilation;
use intersect_text::package::CompilationSourcePolicy;

pub(super) const SYSML: &str = "org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext";
pub(super) const SYSML_SHA: &str =
    "7d9142742c454f5810fe4db61a089abef5e0b8d61ad8bf552631d9136b04f298";
const EXPRESSIONS: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";
const EXPRESSIONS_SHA: &str = "7bea42879a913f7613ac5e32779174db01e8fd4fe75694f0845c495aa0191aa8";
const SCHEMA: &str = "org.omg.sysml/model/SysML.ecore";
const BEHAVIOR: &str = "probe-behavior.rcore";
const BEHAVIOR_SHA: &str = "5eaff959a081947c82b71b13afd07e0547e2acb92cbc3bf98efba24511760c8c";

/// The literal original R2 SysML source policy; not the smaller KerML policy.
pub(super) const SYSML_POLICY: CompilationSourcePolicy = CompilationSourcePolicy {
    structure_bytes: 629_610,
    xtext_bytes: 61_789,
    xtext_declarations: 551,
};

/// The pinned four-source original SysML recipe. Shared by preparation and the
/// language-pack load (which rebuilds the original selection it restores).
pub(super) fn validate_original(original: &BundleCompilation) -> Result<(), String> {
    original.bundle.validate()?;
    if original.bundle.sources.len() != 4
        || !original.materialized_types.is_empty()
        || original.namespace != "https://www.omg.org/spec/SysML/20250201"
        || original.ecore.uri != SCHEMA
        || original.ecore.sha256 != crate::PILOT_SCHEMA_SHA256
        || original.rcore_behavior.uri != BEHAVIOR
        || original.rcore_behavior.sha256 != BEHAVIOR_SHA
        || !original
            .xtext
            .as_ref()
            .is_some_and(|id| id.uri == SYSML && id.sha256 == SYSML_SHA)
    {
        return Err("Native SysML requires its exact original four-source recipe".into());
    }
    for (uri, hash, kind) in [
        (SCHEMA, crate::PILOT_SCHEMA_SHA256, SourceKind::Ecore),
        (EXPRESSIONS, EXPRESSIONS_SHA, SourceKind::Xtext),
        (SYSML, SYSML_SHA, SourceKind::Xtext),
        (BEHAVIOR, BEHAVIOR_SHA, SourceKind::RcoreBehavior),
    ] {
        let mut rows = original
            .bundle
            .sources
            .iter()
            .filter(|source| source.id.uri == uri);
        let source = rows.next().ok_or("Missing original native SysML source")?;
        if rows.next().is_some() || source.id.sha256 != hash || source.kind != kind {
            return Err("Foreign native SysML original source identity/kind".into());
        }
        let expected = if uri == SCHEMA {
            Vec::new()
        } else if uri == SYSML {
            let expressions = original
                .bundle
                .sources
                .iter()
                .find(|source| source.id.uri == EXPRESSIONS)
                .ok_or("Missing original SysML expression dependency")?;
            vec![original.ecore.clone(), expressions.id.clone()]
        } else {
            vec![original.ecore.clone()]
        };
        if source.dependencies != expected {
            return Err("Original native SysML source dependency closure differs".into());
        }
    }
    Ok(())
}

/// Progress markers match the KerML recipe's names (draft_entry.rs), plus the
/// SysML-only syntax inventory and Usage contract phases; they observe only.
pub(super) fn prepare_native_sysml_inputs(
    original: &BundleCompilation,
    observe: &impl Fn(&'static str),
) -> Result<crate::draft::MinimalDraftInputs, String> {
    validate_original(original)?;
    let policy = SYSML_POLICY;
    let entry = original.xtext.as_ref().ok_or("Missing SysML entry")?;
    if original.bundle.sources.len() != 4
        || entry.uri != SYSML
        || entry.sha256 != SYSML_SHA
        || original.ecore.sha256 != crate::PILOT_SCHEMA_SHA256
    {
        return Err("Original SysML four-source closure changed".into());
    }
    observe("syntax_inventory_begin");
    for (uri, expected) in [
        (
            "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext",
            108,
        ),
        (SYSML, 443),
    ] {
        let source = original
            .bundle
            .sources
            .iter()
            .find(|source| source.id.uri == uri)
            .ok_or("Missing complete original syntax input")?;
        let parsed = syntax::parse(
            source,
            SourceLimits {
                source_bytes: policy.xtext_bytes,
                declarations: policy.xtext_declarations,
            },
        )
        .map_err(|error| format!("Original syntax inventory failed: {error:?}"))?;
        if parsed.rules.len() != expected {
            return Err("Original authored declaration inventory changed".into());
        }
    }
    observe("syntax_inventory_end");
    let targets = [(
        original.ecore.clone(),
        format!("{}.rcore", original.ecore.uri),
    )]
    .into();
    observe("native_migration_begin");
    let migration = intersect_model::migration::migrate_ecore_bundle_with_limits(
        original.bundle.clone(),
        &targets,
        intersect_model::definitions::ecore::ParseLimits {
            source_bytes: policy.structure_bytes,
        },
    )
    .map_err(|error| error.to_string())?;
    observe("native_migration_end");
    let input = intersect_text::migration::remap_compilation(&migration, original)?;
    observe("contract_schema_compile_begin");
    let mut schema = compiler::compile_structure_bundle_with_limits(
        &input.bundle,
        &input.ecore,
        &input.namespace,
        intersect_model::definitions::ecore::ParseLimits {
            source_bytes: policy.structure_bytes,
        },
    )
    .map_err(|errors| format!("{errors:?}"))?;
    intersect_text::grammar::prepare_ecore_reference_context(&mut schema)?;
    observe("contract_schema_compile_end");
    observe("operand_contracts_begin");
    let mut contracts = super::entry::operand_contracts(&schema, &input.bundle)?;
    observe("operand_contracts_end");
    observe("usage_contracts_begin");
    contracts.extend(usage_contracts(&schema, &input.bundle)?);
    if contracts.len() != 57 {
        return Err("Expected seven operand plus fifty original Usage settings".into());
    }
    observe("usage_contracts_end");
    Ok((input, migration, contracts, policy))
}

/// The fifty pinned `Usage::isReference` contracts over the converted bundle,
/// each validated by its actual provider constructor. Shared by preparation
/// and the language-pack load.
fn usage_contracts(
    schema: &Schema,
    bundle: &DefinitionBundle,
) -> Result<Vec<ComputedSettingContract>, String> {
    let mut contracts = Vec::new();
    let mut validated = None;
    for class in schema
        .classes
        .keys()
        .filter(|class| schema.conforms(class, "Usage"))
    {
        let contract = ComputedSettingContract {
            version: 1,
            schema_revision: schema.revision.clone(),
            class: class.clone(),
            class_identity: schema.identity(class),
            feature: "isReference".into(),
            member: schema.effective_feature(class, "isReference")?.identity,
            provider: intersect_model::definitions::conversion::ProviderId {
                name: crate::sysml_usage::USAGE_REFERENCE_PROVIDER.into(),
                version: "1".into(),
            },
            sources: bundle
                .sources
                .iter()
                .map(|source| source.id.clone())
                .collect(),
            operations: vec![intersect_model::definitions::setting::SettingOperation::Set],
        };
        // Validated at the first contract, as before, and then once only.
        let validated = match validated {
            Some(validated) => validated,
            None => *validated.insert(bundle.validated()?),
        };
        crate::sysml_usage::UsageReferenceSettings::new_in(schema, &validated, contract.clone())?;
        contracts.push(contract);
    }
    Ok(contracts)
}

/// The exact 7 + 50 SysML contract derivation over a schema and its converted
/// bundle, as preparation produces it.
pub(super) fn sysml_contracts(
    schema: &Schema,
    bundle: &DefinitionBundle,
) -> Result<Vec<ComputedSettingContract>, String> {
    let mut contracts = super::entry::operand_contracts(schema, bundle)?;
    contracts.extend(usage_contracts(schema, bundle)?);
    if contracts.len() != 57 {
        return Err("Expected seven operand plus fifty original Usage settings".into());
    }
    Ok(contracts)
}
