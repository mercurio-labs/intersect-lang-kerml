//! Handwritten, source-proven Pilot SysML Usage reference setting.
//!
//! The registered Rust implementation is executable authority. The compiled-in
//! delegate identity records its original provenance; Java tooling and upstream
//! source files are never required for activation or package reload.
use crate::PILOT_RCORE_SCHEMA_SHA256;
use intersect_model::core::{
    query::{DerivedValues, QueryBudget},
    storage::{ComputedEdit, ModelRead, SettingEdit, SettingValues},
};
use intersect_model::definitions::{
    bundle::{DefinitionBundle, SourceId},
    schema::{MemberIdentity, Schema},
    setting::{ComputedSettingContract, SettingOperation},
};
use intersect_model::setting::SourceSettingValues;
use serde_json::Value;

pub const USAGE_REFERENCE_PROVIDER: &str = "intersect.sysml.pilot.usage-reference";
pub const USAGE_REFERENCE_DELEGATE_URI: &str = "org.omg.sysml.logic/src/main/java/org/omg/sysml/delegate/setting/Usage_isReference_SettingDelegate.java";
pub const USAGE_REFERENCE_DELEGATE_SHA256: &str =
    "26e20c319aad6b7fab9c2c06a3138c26d5b1c4b4a28e127cce0db52f01355c30";

/// Provenance only; this identity does not install or authorize a provider.
pub fn usage_reference_delegate_source() -> SourceId {
    SourceId {
        uri: USAGE_REFERENCE_DELEGATE_URI.into(),
        sha256: USAGE_REFERENCE_DELEGATE_SHA256.into(),
    }
}

/// Optional portable verification of original source data, never activation I/O.
pub fn verify_usage_reference_delegate_source(text: &str) -> Result<SourceId, String> {
    let actual = SourceId::for_text(USAGE_REFERENCE_DELEGATE_URI, text);
    if actual != usage_reference_delegate_source() {
        return Err("Pinned Usage reference delegate source mismatch".into());
    }
    Ok(actual)
}

/// Shared Boolean involution. The native semantic-read policy uses this after
/// its own fallible dependency reader; this function grants no raw store access.
pub fn invert_usage_reference_flag(value: bool) -> bool {
    !value
}

/// Exact schema-bound legacy setting/read adapter. Implementing DerivedValues
/// does not activate or supply a proof for any prepared semantic-read session.
pub struct UsageReferenceSettings<'a> {
    schema: &'a Schema,
    contract: ComputedSettingContract,
}
impl<'a> UsageReferenceSettings<'a> {
    pub fn new(
        schema: &'a Schema,
        bundle: &DefinitionBundle,
        contract: ComputedSettingContract,
    ) -> Result<Self, String> {
        Self::new_in(schema, &bundle.validated()?, contract)
    }
    /// `new` over a bundle validated once for all the contracts of a loop.
    pub fn new_in(
        schema: &'a Schema,
        bundle: &intersect_model::definitions::bundle::ValidatedBundle<'_>,
        contract: ComputedSettingContract,
    ) -> Result<Self, String> {
        contract.validate_in(schema, bundle)?;
        let member = schema.effective_feature(&contract.class, "isReference")?;
        let backing = schema.effective_feature(&contract.class, "isComposite")?;
        let field = member.feature;
        let stored = backing.feature;
        if schema.uri != "https://www.omg.org/spec/SysML/20250201"
            || contract.provider.name != USAGE_REFERENCE_PROVIDER
            || contract.provider.version != "1"
            || contract.operations != [SettingOperation::Set]
            || contract.feature != "isReference"
            || !schema.conforms(&contract.class, "Usage")
            || member.owner != "Usage"
            || backing.owner != "Feature"
            || field.reference
            || field.containment
            || !field.derived
            || !field.volatile
            || !field.transient
            || !field.changeable
            || field.lower != 1
            || field.upper != 1
            || field.unsettable
            || schema.runtime_attribute_lexical_target(&contract.class, "isReference")?
                != Some("EBoolean")
            || stored.reference
            || stored.derived
            || !stored.changeable
            || stored.lower != 1
            || stored.upper != 1
            || stored.default != Value::Bool(false)
            || schema.runtime_attribute_lexical_target(&contract.class, "isComposite")?
                != Some("EBoolean")
            || !matches!(&member.identity, MemberIdentity::Source { declaration } if declaration.source.sha256 == PILOT_RCORE_SCHEMA_SHA256)
            || !matches!(&backing.identity, MemberIdentity::Source { declaration } if declaration.source.sha256 == PILOT_RCORE_SCHEMA_SHA256)
            || schema
                .declarations
                .get(&contract.class)
                .is_none_or(|declaration| declaration.source.sha256 != PILOT_RCORE_SCHEMA_SHA256)
        {
            return Err("Unsupported pinned Usage reference contract".into());
        }
        Ok(Self { schema, contract })
    }

    fn receiver(
        &self,
        store: &dyn ModelRead,
        id: &str,
        feature: &str,
        budget: &mut QueryBudget,
    ) -> Result<(), String> {
        budget.charge()?;
        if store.revision()? != self.schema.revision {
            return Err("Usage reference model belongs to a different schema revision".into());
        }
        budget.charge()?;
        if feature != "isReference" {
            return Err("Unknown Usage reference setting".into());
        }
        let mut found = false;
        store.inspect(id, &mut |object| {
            if object.class() != self.contract.class
                || object.type_identity() != self.schema.identity(object.class())
            {
                return Err("Wrong Usage reference receiver".into());
            }
            found = true;
            Ok(())
        })?;
        if !found {
            return Err("Missing Usage reference receiver".into());
        }
        Ok(())
    }
}
impl SourceSettingValues for UsageReferenceSettings<'_> {
    fn contract(&self) -> &ComputedSettingContract {
        &self.contract
    }
}
impl DerivedValues for UsageReferenceSettings<'_> {
    fn schema_revision(&self) -> Option<&str> {
        Some(&self.schema.revision)
    }
    fn supports(&self, class: &str, feature: &str) -> bool {
        class == self.contract.class && feature == "isReference"
    }
    fn read(
        &self,
        store: &dyn ModelRead,
        id: &str,
        feature: &str,
        budget: &mut QueryBudget,
    ) -> Result<Value, String> {
        self.receiver(store, id, feature, budget)?;
        budget.charge()?;
        // Only this exact source-proven stored dependency is read by this legacy
        // adapter. Missing storage uses the verified fresh-object false default.
        let composite = match store.value(id, "isComposite")? {
            None => false,
            Some(Value::Bool(value)) => value,
            Some(_) => return Err("Malformed Usage isComposite dependency".into()),
        };
        Ok(Value::Bool(invert_usage_reference_flag(composite)))
    }
}
impl SettingValues for UsageReferenceSettings<'_> {
    fn plan(
        &self,
        store: &dyn ModelRead,
        id: &str,
        feature: &str,
        edit: &ComputedEdit,
        budget: &mut QueryBudget,
    ) -> Result<Vec<SettingEdit>, String> {
        self.receiver(store, id, feature, budget)?;
        let ComputedEdit::Set(Value::Bool(value)) = edit else {
            return Err("Usage reference requires Set of a Boolean".into());
        };
        budget.charge()?;
        Ok(vec![SettingEdit::Set {
            id: id.into(),
            feature: "isComposite".into(),
            value: Value::Bool(invert_usage_reference_flag(*value)),
        }])
    }
}
