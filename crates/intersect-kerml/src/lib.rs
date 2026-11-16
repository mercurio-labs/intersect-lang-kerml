//! Handwritten KerML language policies over shared definitions, text and storage.
//! Operand construction follows the pinned Mercurio/Pilot evidence in
//! fixtures/xtext/pinned-kerml-operands. No Java is used at runtime.
#[cfg(test)]
extern crate self as intersect_kerml;

use intersect_model::core::{
    query::{DerivedValues, QueryBudget},
    storage::{ComputedEdit, ModelRead, SettingEdit, SettingValues},
};
use intersect_model::definitions::{
    bundle::DefinitionBundle,
    schema::{MemberIdentity, Schema},
    setting::{ComputedSettingContract, SettingOperation},
};
use intersect_model::setting::SourceSettingValues;
use serde_json::{Value, json};

pub mod standard_library;

pub const PILOT_SCHEMA_SHA256: &str =
    "22719934c65588180fc19658f8a7c891cb209458e4027bf90ddf005c80ddc17f";
/// Exact output of the native migration of the pinned Pilot schema.
pub const PILOT_RCORE_SCHEMA_SHA256: &str =
    "9e4020fa8602419130a0133aa30bd8a2558191b343d615ff7212a84d2110599a";
pub const OPERAND_PROVIDER: &str = "intersect.kerml.pilot.operand";

/// Deterministic identity of this crate's compiler: a SHA-256 (64 lowercase hex
/// digits) over its version, enabled features, the workspace `Cargo.toml`, its
/// manifest, build script, every `src/` file, every file it embeds or mounts as
/// a module from outside `src/`, and the workspace `Cargo.lock`, computed by
/// the Model-owned `intersect-build-support` host dependency. The complete
/// language-pack compiler identity separately composes the installed Model
/// and Text identities; this package never scans their source checkouts.
/// Equal identities mean the same compiler inputs, so saved compiler output
/// keyed by it may be reused; it is a cache key for the host's own output, not
/// a signature.
pub const SOURCE_IDENTITY: &str = env!("INTERSECT_KERML_SOURCE_IDENTITY");
pub mod sdk;

/// Explicit append-only adapter for the pinned Pilot parsing extension.
/// Its read facade is empty; this does not implement normative argument queries.
pub struct OperandSettings<'a> {
    schema: &'a Schema,
    contract: ComputedSettingContract,
    /// The three fixed operand enumeration literals, resolved once against
    /// the borrowed (hence unchanged) schema; a failure is kept as is.
    literals: std::cell::OnceCell<Result<[Value; 3], String>>,
}
impl<'a> OperandSettings<'a> {
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
        let member = schema.effective_feature(&contract.class, "operand")?;
        let field = member.feature;
        if contract.feature != "operand"
            || contract.provider.name != OPERAND_PROVIDER
            || contract.provider.version != "1"
            || contract.operations != [SettingOperation::Append]
            || member.owner != "InvocationExpression"
            || !schema.conforms(&contract.class, "InvocationExpression")
            || !field.reference
            || !field.containment
            || !field.derived
            || !field.transient
            || !field.volatile
            || !field.ordered
            || !field.unique
            || field.opposite.is_some()
            || field.lower != 0
            || field.upper != -1
            || !matches!(&contract.member, MemberIdentity::Source { declaration } if declaration.source.sha256 == PILOT_RCORE_SCHEMA_SHA256)
            || schema
                .declarations
                .get(&contract.class)
                .is_none_or(|declaration| declaration.source.sha256 != PILOT_RCORE_SCHEMA_SHA256)
        {
            return Err("Unsupported pinned KerML operand contract".into());
        }
        Ok(Self {
            schema,
            contract,
            literals: std::cell::OnceCell::new(),
        })
    }
    /// Membership visibility, parameter direction and value visibility.
    fn literals(&self) -> Result<&[Value; 3], String> {
        self.literals
            .get_or_init(|| {
                Ok([
                    self.enumeration("ParameterMembership", "visibility", "private")?,
                    self.enumeration("Feature", "direction", "in")?,
                    self.enumeration("FeatureValue", "visibility", "public")?,
                ])
            })
            .as_ref()
            .map_err(Clone::clone)
    }
    fn enumeration(&self, class: &str, feature: &str, spelling: &str) -> Result<Value, String> {
        let field = self.schema.effective_feature(class, feature)?;
        let enumeration = self
            .schema
            .enumerations
            .get(&field.feature.target)
            .ok_or("Missing pinned enum")?;
        serde_json::to_value(enumeration.parse(spelling)?).map_err(|error| error.to_string())
    }
    fn receiver(
        &self,
        store: &dyn ModelRead,
        id: &str,
        feature: &str,
        budget: &mut QueryBudget,
    ) -> Result<(String, usize, usize), String> {
        budget.charge()?;
        if feature != "operand" {
            return Err("Unknown KerML setting".into());
        }
        let mut metadata = None;
        store.inspect(id, &mut |object| {
            if object.class() != self.contract.class
                || object.type_identity() != self.schema.identity(object.class())
            {
                return Err("Wrong KerML operand receiver".into());
            }
            metadata = Some((object.resource().to_owned(), object.start(), object.end()));
            Ok(())
        })?;
        metadata.ok_or_else(|| "Missing KerML operand receiver".into())
    }
}
/// One operand provider per prepared contract (the seven `InvocationExpression`
/// classes), for binding as the grammar's computed-setting providers.
pub fn operand_settings<'a>(
    definition: &'a intersect_text::preparation::PreparedLanguageDefinition<'_>,
    contracts: &[ComputedSettingContract],
) -> Result<Vec<OperandSettings<'a>>, String> {
    if contracts.is_empty() {
        return Ok(Vec::new());
    }
    // One bundle validation for all the contracts, not one each.
    let bundle = definition.sources().validated()?;
    contracts
        .iter()
        .map(|contract| OperandSettings::new_in(definition.schema(), &bundle, contract.clone()))
        .collect()
}
impl SourceSettingValues for OperandSettings<'_> {
    fn contract(&self) -> &ComputedSettingContract {
        &self.contract
    }
}
impl DerivedValues for OperandSettings<'_> {
    fn schema_revision(&self) -> Option<&str> {
        Some(&self.schema.revision)
    }
    fn supports(&self, class: &str, feature: &str) -> bool {
        class == self.contract.class && feature == "operand"
    }
    fn read(
        &self,
        store: &dyn ModelRead,
        id: &str,
        feature: &str,
        budget: &mut QueryBudget,
    ) -> Result<Value, String> {
        self.receiver(store, id, feature, budget)?;
        Ok(json!([]))
    }
}
impl SettingValues for OperandSettings<'_> {
    fn plan(
        &self,
        store: &dyn ModelRead,
        id: &str,
        feature: &str,
        edit: &ComputedEdit,
        budget: &mut QueryBudget,
    ) -> Result<Vec<SettingEdit>, String> {
        let (resource, start, end) = self.receiver(store, id, feature, budget)?;
        let ComputedEdit::Append(Value::String(operand)) = edit else {
            return Err("KerML operand requires append of an expression identity".into());
        };
        budget.charge()?;
        let class = store.class(operand)?.ok_or("Missing operand")?;
        if operand == id || !self.schema.conforms(&class, "Expression") {
            return Err("Operand must be a distinct Expression".into());
        }
        for field in ["owningRelationship", "owningRelatedElement"] {
            budget.charge()?;
            if store.value(operand, field)?.is_some_and(|v| !v.is_null()) {
                return Err("Operand must be uncontained".into());
            }
        }
        budget.charge()?;
        if store.value(id, "operand")?.is_some() {
            return Err("Operand is a parsing adapter, not stored data".into());
        }
        budget.charge()?;
        let mut relationships = match store.value(id, "ownedRelationship")? {
            None => Vec::new(),
            Some(Value::Array(items)) => items,
            Some(_) => return Err("Malformed owned relationships".into()),
        };
        for item in &relationships {
            budget.charge()?;
            if !item.is_string() {
                return Err("Malformed relationship identity".into());
            }
        }
        let prefix = format!("{id}.operand.{}", relationships.len());
        let membership = format!("{prefix}.membership");
        let parameter = format!("{prefix}.parameter");
        let value = format!("{prefix}.value");
        let mut plan = Vec::new();
        for (identity, class) in [
            (&membership, "ParameterMembership"),
            (&parameter, "Feature"),
            (&value, "FeatureValue"),
        ] {
            budget.charge()?;
            if store.class(identity)?.is_some() {
                return Err("Operand construction identity collision".into());
            }
            plan.push(SettingEdit::Create {
                id: identity.clone(),
                class: class.into(),
                resource: resource.clone(),
                start,
                end,
            });
        }
        let set = |id: &str, feature: &str, value: Value| SettingEdit::Set {
            id: id.into(),
            feature: feature.into(),
            value,
        };
        let [membership_visibility, direction, value_visibility] = self.literals()?.clone();
        plan.extend([
            set(&membership, "visibility", membership_visibility),
            set(&parameter, "direction", direction),
            set(&value, "visibility", value_visibility),
            set(&value, "ownedRelatedElement", json!([operand])),
            set(&parameter, "ownedRelationship", json!([value])),
            set(&membership, "ownedRelatedElement", json!([parameter])),
        ]);
        relationships.push(json!(membership));
        plan.push(set(id, "ownedRelationship", Value::Array(relationships)));
        Ok(plan)
    }
}
/// Source-proven SysML policy over the pinned Pilot metamodel.
pub mod sysml_usage;

/// Preparation of the pinned original KerML and SysML owners.
pub mod draft;

/// The capability pieces of the frozen-engine text load (KerML and SysML).
pub mod text_capability;

/// The frozen-engine KerML text load: parse, link, complete and validate.
pub mod frozen_load;

/// The KerML Rcore binding recipe (scopes, defaults, implicit elements).
pub mod kerml_bindings;

/// The combined KerML+SysML Rcore binding recipe the SysML hosts use.
pub mod sysml_bindings;

/// SysML v2 with its standard library on the frozen engine: the host core of
/// the `intersect` CLI.
pub mod sysml_host;

pub use draft::language_pack;
pub use draft::native_pilot;
