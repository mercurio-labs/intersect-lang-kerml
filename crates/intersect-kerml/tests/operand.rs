use intersect_kerml::{OPERAND_PROVIDER, OperandSettings, PILOT_SCHEMA_SHA256};
use intersect_model::core::{
    frozen::FrozenStore,
    query::{DerivedValues, QueryBudget},
    storage::{self, ComputedEdit, ComputedSettings, ModelRead, ModelStore},
};
use intersect_model::definitions::{
    bundle::{DefinitionBundle, DefinitionSource, SourceKind},
    compiler,
    conversion::ProviderId,
    ecore::ParseLimits,
    schema::Schema,
    setting::{ComputedSettingContract, SettingOperation},
};
use intersect_model::setting::SourceBoundSettings;
use serde_json::{Value, json};
const ECORE: &str = include_str!("../../../fixtures/xtext/pinned-kerml-operands/SysML.ecore");
fn fixture() -> (Schema, DefinitionBundle) {
    let source = DefinitionSource::new("org.omg.sysml/model/SysML.ecore", SourceKind::Ecore, ECORE);
    assert_eq!(source.id.sha256, PILOT_SCHEMA_SHA256);
    let report = intersect_model::migration::migrate_ecore_with_limits(
        source,
        "org.omg.sysml/model/SysML.rcore",
        ParseLimits {
            source_bytes: ECORE.len(),
        },
    )
    .unwrap();
    let source = report
        .converted
        .unwrap_or_else(|| panic!("{:?}", report.issues));
    assert_eq!(source.id.sha256, intersect_kerml::PILOT_RCORE_SCHEMA_SHA256);
    let bundle = DefinitionBundle {
        version: 1,
        sources: vec![source.clone()],
        namespaces: vec![],
    };
    let schema = compiler::compile_structure_bundle_with_limits(
        &bundle,
        &source.id,
        "https://www.omg.org/spec/SysML/20250201",
        ParseLimits {
            source_bytes: ECORE.len(),
        },
    )
    .unwrap();
    (schema, bundle)
}
fn contract(schema: &Schema, bundle: &DefinitionBundle, class: &str) -> ComputedSettingContract {
    ComputedSettingContract {
        version: 1,
        schema_revision: schema.revision.clone(),
        class: class.into(),
        class_identity: schema.identity(class),
        feature: "operand".into(),
        member: schema.effective_feature(class, "operand").unwrap().identity,
        provider: ProviderId {
            name: OPERAND_PROVIDER.into(),
            version: "1".into(),
        },
        sources: vec![bundle.sources[0].id.clone()],
        operations: vec![SettingOperation::Append],
    }
}
fn store(schema: &Schema, class: &str) -> FrozenStore {
    let mut store = FrozenStore::for_schema(schema).unwrap();
    storage::atomic(&mut store, |s| {
        s.create_element(schema, "owner", class, "model", 0, 5)?;
        // Pinned FeatureImpl.DIRECTION_EDEFAULT is null. This is explicit input
        // construction for the adapter comparison, not generic enum-default policy.
        storage::set_with_opposite(s, schema, "owner", "direction", Value::Null)?;
        for id in ["left", "right"] {
            s.create_element(schema, id, "LiteralInteger", "model", 1, 2)?;
            storage::set_with_opposite(s, schema, id, "direction", Value::Null)?;
            storage::set_with_opposite(s, schema, id, "value", json!(2))?;
        }
        Ok(())
    })
    .unwrap();
    store
}
fn tree(schema: &Schema, store: &dyn ModelRead, id: &str) -> Value {
    let class = store.class(id).unwrap().unwrap();
    let mut result = json!({"kind":class,"relationships":[]});
    for (feature, key) in [
        ("ownedRelationship", "relationships"),
        ("ownedRelatedElement", "elements"),
    ] {
        if let Some(Value::Array(ids)) = store.value(id, feature).unwrap()
            && (key == "elements" || !ids.is_empty())
        {
            result[key] = Value::Array(
                ids.iter()
                    .map(|v| {
                        let child = v.as_str().unwrap();
                        let opposite = if feature == "ownedRelationship" {
                            "owningRelatedElement"
                        } else {
                            "owningRelationship"
                        };
                        assert_eq!(store.value(child, opposite).unwrap(), Some(json!(id)));
                        tree(schema, store, child)
                    })
                    .collect(),
            );
        }
    }
    for field in ["visibility", "direction", "value"] {
        if let Some(value) = store.value(id, field).unwrap()
            && !value.is_null()
        {
            result[field] = if let Ok(feature) = schema.effective_feature(&class, field) {
                if let Some(enumeration) = schema.enumerations.get(&feature.feature.target) {
                    json!(
                        enumeration
                            .format(&serde_json::from_value(value).unwrap())
                            .unwrap()
                    )
                } else {
                    value
                }
            } else {
                value
            };
        }
    }
    result
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
pub fn seven_pilot_operand_trees_match_with_exact_source_contracts() {
    let (schema, bundle) = fixture();
    let evidence: Value = serde_json::from_str(include_str!(
        "../../../fixtures/xtext/pinned-kerml-operands/observations.json"
    ))
    .unwrap();
    let cases = evidence["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 7);
    for case in cases {
        let class = case["class"].as_str().unwrap();
        let contract = contract(&schema, &bundle, class);
        let provider = OperandSettings::new(&schema, &bundle, contract.clone()).unwrap();
        let mut store = store(&schema, class);
        let service = SourceBoundSettings {
            schema: &schema,
            bundle: &bundle,
            expected: &contract,
            provider: &provider,
            conversions: None,
        };
        for id in ["left", "right"] {
            service
                .apply(
                    &mut store,
                    "owner",
                    ComputedEdit::Append(json!(id)),
                    &mut QueryBudget::new(10000),
                )
                .unwrap();
        }
        assert_eq!(tree(&schema, &store, "owner"), case["tree"], "{class}");
        assert_eq!(
            provider
                .read(&store, "owner", "operand", &mut QueryBudget::new(10))
                .unwrap(),
            json!([])
        );
        assert!(store.value("owner", "operand").unwrap().is_none());
        assert!(store.value("owner", "argument").unwrap().is_none());
        assert_eq!(storage::export(&store).unwrap().elements.len(), 9);
    }
}
#[cfg_attr(not(target_arch = "wasm32"), test)]
pub fn invalid_operands_and_every_provider_budget_cutoff_restore_graph() {
    let (schema, bundle) = fixture();
    let contract = contract(&schema, &bundle, "OperatorExpression");
    let provider = OperandSettings::new(&schema, &bundle, contract.clone()).unwrap();
    let service = ComputedSettings {
        schema: &schema,
        provider: &provider,
        conversions: None,
    };
    let mut store = store(&schema, "OperatorExpression");
    storage::atomic(&mut store, |s| {
        s.create_element(&schema, "wrong", "Package", "model", 0, 0)
    })
    .unwrap();
    let original = storage::export(&store).unwrap();
    let frame = store.begin().unwrap();
    let mut budget = QueryBudget::new(10000);
    service
        .apply(
            &mut store,
            "owner",
            "operand",
            ComputedEdit::Append(json!("left")),
            &mut budget,
        )
        .unwrap();
    let reads = budget.reads;
    store.rollback(&frame).unwrap();
    for limit in 0..reads {
        assert!(
            service
                .apply(
                    &mut store,
                    "owner",
                    "operand",
                    ComputedEdit::Append(json!("left")),
                    &mut QueryBudget::new(limit)
                )
                .is_err(),
            "{limit}"
        );
        assert_eq!(storage::export(&store).unwrap(), original, "{limit}");
    }
    for edit in [
        ComputedEdit::Append(json!("missing")),
        ComputedEdit::Append(json!("wrong")),
        ComputedEdit::Append(json!("owner")),
        ComputedEdit::Append(json!(3)),
        ComputedEdit::Set(json!([])),
        ComputedEdit::Unset,
    ] {
        assert!(
            service
                .apply(
                    &mut store,
                    "owner",
                    "operand",
                    edit,
                    &mut QueryBudget::new(10000)
                )
                .is_err()
        );
        assert_eq!(storage::export(&store).unwrap(), original);
    }
    let collision_frame = store.begin().unwrap();
    store
        .create_element(
            &schema,
            "owner.operand.0.membership",
            "Package",
            "model",
            0,
            0,
        )
        .unwrap();
    let collision_state = storage::export(&store).unwrap();
    assert!(
        service
            .apply(
                &mut store,
                "owner",
                "operand",
                ComputedEdit::Append(json!("left")),
                &mut QueryBudget::new(10000)
            )
            .unwrap_err()
            .contains("collision")
    );
    assert_eq!(storage::export(&store).unwrap(), collision_state);
    store.rollback(&collision_frame).unwrap();
    assert_eq!(storage::export(&store).unwrap(), original);
    service
        .apply(
            &mut store,
            "owner",
            "operand",
            ComputedEdit::Append(json!("left")),
            &mut QueryBudget::new(10000),
        )
        .unwrap();
    let before = storage::export(&store).unwrap();
    assert!(
        service
            .apply(
                &mut store,
                "owner",
                "operand",
                ComputedEdit::Append(json!("left")),
                &mut QueryBudget::new(10000)
            )
            .is_err()
    );
    assert_eq!(storage::export(&store).unwrap(), before);
    let mut stale = contract;
    stale.provider.version = "other".into();
    assert!(OperandSettings::new(&schema, &bundle, stale).is_err());
}
