//! Handwritten preparation of the pinned original KerML owner (not
//! generated): the source recipe check, the Ecore migration, the structure
//! compile and the seven `operand` contracts. `NativeKerMLSession` and the
//! language pack use it; parsing is `crate::frozen_load`.
use super::*;
use intersect_model::definitions::bundle::SourceKind;
use intersect_model::definitions::setting::ComputedSettingContract;
use intersect_model::definitions::setting::SettingOperation;
use intersect_text::compiler;
use intersect_text::package::BundleCompilation;
use intersect_text::package::CompilationSourcePolicy;

/// A prepared owner's inputs: the remapped original compilation, its
/// migration report, its computed-setting contracts and its source policy.
pub(crate) type MinimalDraftInputs = (
    BundleCompilation,
    intersect_model::definitions::ecore_migration::BundleMigrationReport,
    Vec<ComputedSettingContract>,
    CompilationSourcePolicy,
);
fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
type CompletedMinimalDraftInputs = (
    BundleCompilation,
    intersect_model::migration::CompletedBundleMigration,
    Vec<ComputedSettingContract>,
    CompilationSourcePolicy,
);
/// The literal original R2 KerML source policy of the pinned recipe.
pub(super) const KERML_POLICY: CompilationSourcePolicy = CompilationSourcePolicy {
    structure_bytes: 629_610,
    xtext_bytes: 29_731,
    xtext_declarations: 284,
};
/// The pinned four-source original KerML recipe: exact identities, kinds,
/// namespace and selected roles. Shared by preparation and the language-pack
/// load (which rebuilds the original selection from the restored sources).
pub(super) fn validate_kerml_original(input: &BundleCompilation) -> Result<(), String> {
    input.bundle.validate()?;
    let sources = &input.bundle.sources;
    let expected = [
        (SCHEMA, crate::PILOT_SCHEMA_SHA256, SourceKind::Ecore),
        (EXPRESSIONS, EXPRESSION_SHA, SourceKind::Xtext),
        (KERML, KERML_SHA, SourceKind::Xtext),
        (
            "probe-behavior.rcore",
            EMPTY_BEHAVIOR_SHA,
            SourceKind::RcoreBehavior,
        ),
    ];
    require(
        sources.len() == expected.len()
            && input.materialized_types.is_empty()
            && input.namespace == "https://www.omg.org/spec/SysML/20250201",
        "Minimal draft requires the complete pinned original source recipe",
    )?;
    for (uri, hash, kind) in expected {
        require(
            sources
                .iter()
                .any(|s| s.id.uri == uri && s.id.sha256 == hash && s.kind == kind),
            "Minimal draft original source identity/kind mismatch",
        )?;
    }
    require(
        input.ecore.uri == SCHEMA
            && input.ecore.sha256 == crate::PILOT_SCHEMA_SHA256
            && input
                .xtext
                .as_ref()
                .is_some_and(|id| id.uri == KERML && id.sha256 == KERML_SHA)
            && input.rcore_behavior.uri == "probe-behavior.rcore"
            && input.rcore_behavior.sha256 == EMPTY_BEHAVIOR_SHA,
        "Minimal draft selected original roles changed",
    )
}
/// The seven pinned `InvocationExpression::operand` contracts over the
/// converted bundle, each validated by its actual provider constructor.
/// Shared by both recipes and the language-pack load, which requires the
/// restored grammar's computed settings to EQUAL this derivation.
pub(super) fn operand_contracts(
    schema: &intersect_model::definitions::schema::Schema,
    bundle: &intersect_model::definitions::bundle::DefinitionBundle,
) -> Result<Vec<ComputedSettingContract>, String> {
    let mut contracts = Vec::new();
    let mut validated = None;
    for class in schema
        .classes
        .keys()
        .filter(|class| schema.conforms(class, "InvocationExpression"))
    {
        let contract = ComputedSettingContract {
            version: 1,
            schema_revision: schema.revision.clone(),
            class: class.clone(),
            class_identity: schema.identity(class),
            feature: "operand".into(),
            member: schema.effective_feature(class, "operand")?.identity,
            provider: intersect_model::definitions::conversion::ProviderId {
                name: crate::OPERAND_PROVIDER.into(),
                version: "1".into(),
            },
            sources: bundle
                .sources
                .iter()
                .map(|source| source.id.clone())
                .collect(),
            operations: vec![SettingOperation::Append],
        };
        // Validated at the first contract, as before, and then once only.
        let validated = match validated {
            Some(validated) => validated,
            None => *validated.insert(bundle.validated()?),
        };
        crate::OperandSettings::new_in(schema, &validated, contract.clone())?;
        contracts.push(contract);
    }
    require(
        contracts.len() == 7,
        "Expected seven exact admission declarations",
    )?;
    Ok(contracts)
}
pub(super) fn prepare_minimal_inputs_completed_with_progress(
    input: &BundleCompilation,
    observe: &impl Fn(&'static str),
) -> Result<CompletedMinimalDraftInputs, String> {
    validate_kerml_original(input)?;
    let policy = KERML_POLICY;
    let targets = [(input.ecore.clone(), format!("{SCHEMA}.rcore"))].into();
    observe("native_migration_begin");
    let migration = intersect_model::migration::migrate_ecore_bundle_completed_with_limits(
        input.bundle.clone(),
        &targets,
        intersect_model::definitions::ecore::ParseLimits {
            source_bytes: policy.structure_bytes,
        },
    )
    .map_err(|error| error.to_string())?;
    observe("native_migration_end");
    let input = intersect_text::migration::remap_compilation(migration.report(), input)?;
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
    let contracts = operand_contracts(&schema, &input.bundle)?;
    observe("operand_contracts_end");
    Ok((input, migration, contracts, policy))
}
