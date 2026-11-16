//! Bounded source-defined local scope over untouched migrated nominal KerML types.
//! This is not original-grammar construction or full KerML name-resolution evidence.
use intersect_model::{
    core::{
        frozen::FrozenStore,
        query::QueryBudget,
        storage::{self, ModelStore},
    },
    definitions::{
        bundle::{DefinitionBundle, DefinitionSource, SourceKind},
        compiler,
        ecore::ParseLimits,
        rcore_behavior::ir_v2::Limits,
        rcore_scope::ScopeBindingDescriptor,
        schema::Semantics,
    },
    rcore_behavior_functions::ExecutionLimits,
};
use intersect_text::{
    linking::{ReferenceCandidates, ReferenceRequest},
    package::Package,
    rcore_scopes::RcoreScopes,
};
use serde_json::json;
use std::collections::BTreeMap;

const ECORE: &str = include_str!("../../../fixtures/xtext/pinned-kerml-operands/SysML.ecore");
const SCOPE: &str = include_str!("../semantics/local-feature-typing-scope.rcore");
const NAMESPACE: &str = "https://www.omg.org/spec/SysML/20250201";
// Host-selected finite fixture admission, not a package-supplied grant.
const SOURCE_LIMIT: usize = 2_000_000;

fn fixture() -> Result<(Package, DefinitionBundle, FrozenStore), String> {
    let original =
        DefinitionSource::new("org.omg.sysml/model/SysML.ecore", SourceKind::Ecore, ECORE);
    assert_eq!(original.id.sha256, intersect_kerml::PILOT_SCHEMA_SHA256);
    let migrated = intersect_model::migration::migrate_ecore_with_limits(
        original,
        "org.omg.sysml/model/SysML.rcore",
        ParseLimits {
            source_bytes: SOURCE_LIMIT,
        },
    )
    .map_err(|error| error.to_string())?;
    let structure = migrated
        .converted
        .ok_or_else(|| format!("{:?}", migrated.issues))?;
    assert_eq!(
        structure.id.sha256,
        intersect_kerml::PILOT_RCORE_SCHEMA_SHA256
    );
    let mut behavior = DefinitionSource::new(
        "package:/kerml/local-feature-typing-scope.rcore",
        SourceKind::RcoreBehavior,
        SCOPE,
    );
    behavior.dependencies.push(structure.id.clone());
    let bundle = DefinitionBundle {
        version: 1,
        sources: vec![structure.clone(), behavior.clone()],
        namespaces: vec![],
    };
    let schema = compiler::compile_structure_bundle_with_limits(
        &bundle,
        &structure.id,
        NAMESPACE,
        ParseLimits {
            source_bytes: SOURCE_LIMIT,
        },
    )
    .map_err(|error| error.to_string())?;
    let package = Package {
        version: 1,
        xtext_inference: None,
        source_policy: None,
        schema,
        grammar: None,
        semantics: Semantics {
            scope_bindings: vec![ScopeBindingDescriptor {
                source: behavior.id.clone(),
                declaration: 1,
            }],
            rule_conversions: vec![],
            version: 1,
            scope: "unique_names".into(),
            derived: vec![],
            initializers: vec![],
            validations: vec![],
        },
        sources: BTreeMap::from([
            (structure.id.uri.clone(), structure.text),
            (behavior.id.uri.clone(), behavior.text),
        ]),
        compilation: None,
    };
    let mut store = FrozenStore::for_schema(&package.schema).map_err(|e| e.to_string())?;
    storage::atomic(&mut store, |store| {
        for (object, class) in [
            ("outer", "Package"),
            ("inner", "Package"),
            ("other", "Package"),
            ("outerType", "Class"),
            ("localType", "Class"),
            ("otherType", "Class"),
            ("feature", "Feature"),
            ("typing", "FeatureTyping"),
            ("outerTypeMembership", "OwningMembership"),
            ("innerMembership", "OwningMembership"),
            ("localTypeMembership", "OwningMembership"),
            ("featureMembership", "OwningMembership"),
            ("otherTypeMembership", "OwningMembership"),
        ] {
            store.create_element(&package.schema, object, class, "memory:scope.kerml", 0, 1)?;
        }
        for (object, name) in [
            ("outer", "Outer"),
            ("inner", "Inner"),
            ("other", "Other"),
            ("outerType", "T"),
            ("localType", "T"),
            ("otherType", "T"),
            ("feature", "f"),
        ] {
            storage::set_with_opposite(
                store,
                &package.schema,
                object,
                "declaredName",
                json!(name),
            )?;
        }
        for (owner, field, children) in [
            (
                "outer",
                "ownedRelationship",
                vec!["outerTypeMembership", "innerMembership"],
            ),
            (
                "outerTypeMembership",
                "ownedRelatedElement",
                vec!["outerType"],
            ),
            ("innerMembership", "ownedRelatedElement", vec!["inner"]),
            (
                "inner",
                "ownedRelationship",
                vec!["localTypeMembership", "featureMembership"],
            ),
            (
                "localTypeMembership",
                "ownedRelatedElement",
                vec!["localType"],
            ),
            ("featureMembership", "ownedRelatedElement", vec!["feature"]),
            ("feature", "ownedRelationship", vec!["typing"]),
            ("other", "ownedRelationship", vec!["otherTypeMembership"]),
            (
                "otherTypeMembership",
                "ownedRelatedElement",
                vec!["otherType"],
            ),
        ] {
            storage::set_with_opposite(store, &package.schema, owner, field, json!(children))?;
        }
        Ok(())
    })?;
    Ok((package, bundle, store))
}

#[test]
fn original_nominal_feature_typing_scope_executes_rcore_local_visibility_and_shadowing()
-> Result<(), String> {
    let (package, bundle, mut store) = fixture()?;
    let scope = RcoreScopes::from_bundle_with_source_limits(
        &package,
        &bundle,
        &Limits::default(),
        ExecutionLimits::default(),
        ParseLimits {
            source_bytes: SOURCE_LIMIT,
        },
    )?;
    let request = ReferenceRequest {
        owner: "typing",
        feature: "type",
        spelling: "T",
        target_class: "Type",
        uri: "memory:scope.kerml",
        start: 0,
        end: 1,
    };
    let mut work = QueryBudget::new(1_000_000);
    assert_eq!(
        scope.candidates(&store, &request, &mut work)?,
        ["localType"]
    );

    storage::set_with_opposite(
        &mut store,
        &package.schema,
        "localType",
        "declaredName",
        json!("U"),
    )?;
    assert_eq!(
        scope.candidates(&store, &request, &mut work)?,
        ["outerType"]
    );

    storage::set_with_opposite(
        &mut store,
        &package.schema,
        "outerType",
        "declaredName",
        json!("U"),
    )?;
    assert!(
        scope.candidates(&store, &request, &mut work)?.is_empty(),
        "Unrelated Other::T must not become a global fallback"
    );

    storage::set_with_opposite(
        &mut store,
        &package.schema,
        "localType",
        "declaredName",
        json!("T"),
    )?;
    storage::atomic(&mut store, |store| {
        store.create_element(
            &package.schema,
            "duplicateMembership",
            "OwningMembership",
            "memory:scope.kerml",
            0,
            1,
        )?;
        store.create_element(
            &package.schema,
            "duplicateType",
            "Class",
            "memory:scope.kerml",
            0,
            1,
        )?;
        storage::set_with_opposite(
            store,
            &package.schema,
            "duplicateType",
            "declaredName",
            json!("T"),
        )?;
        storage::set_with_opposite(
            store,
            &package.schema,
            "duplicateMembership",
            "ownedRelatedElement",
            json!(["duplicateType"]),
        )?;
        storage::set_with_opposite(
            store,
            &package.schema,
            "inner",
            "ownedRelationship",
            json!([
                "localTypeMembership",
                "featureMembership",
                "duplicateMembership"
            ]),
        )?;
        Ok(())
    })?;
    assert_eq!(
        scope.candidates(&store, &request, &mut work)?,
        ["localType", "duplicateType"],
        "Same-scope ambiguity must survive candidate production"
    );
    assert!(
        scope
            .candidates(&store, &request, &mut QueryBudget::new(0))
            .is_err()
    );
    Ok(())
}

/// The same verified scope on the frozen engine. At every step of the store
/// test above, linking the frozen model must see exactly the candidates the
/// store path returns.
#[test]
#[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
fn frozen_engine_matches_store_candidates_for_local_feature_typing_scope() -> Result<(), String> {
    use intersect_model::{
        core::frozen::{FrozenError, LinkView, Metamodel, ModelBuilder, ObjId},
        definitions::rcore_scope::compile_scope_with_source_limits,
        frozen_scopes::{Execution, RcoreScopes as FrozenScopes},
    };
    use std::{cell::RefCell, sync::Arc};

    let (package, bundle, mut store) = fixture()?;
    let limits = ParseLimits {
        source_bytes: SOURCE_LIMIT,
    };
    let old = RcoreScopes::from_bundle_with_source_limits(
        &package,
        &bundle,
        &Limits::default(),
        ExecutionLimits::default(),
        limits,
    )?;
    let behavior = bundle
        .sources
        .get(1)
        .ok_or("Missing behavior source")?
        .id
        .clone();
    let verified = vec![
        compile_scope_with_source_limits(&bundle, &behavior, 1, &Limits::default(), limits)
            .map_err(|e| format!("{e:?}"))?,
    ];
    let mm = Arc::new(Metamodel::freeze(&package.schema).map_err(|e| e.to_string())?);
    let typing_class = mm.class("FeatureTyping").ok_or("Missing FeatureTyping")?;
    let type_feature = mm.feature(typing_class, "type").ok_or("Missing type")?;
    let request = ReferenceRequest {
        owner: "typing",
        feature: "type",
        spelling: "T",
        target_class: "Type",
        uri: "memory:scope.kerml",
        start: 0,
        end: 1,
    };
    let frozen = |store: &FrozenStore| -> Result<Vec<String>, String> {
        let model = storage::export(store)?;
        let mut b = ModelBuilder::import(mm.clone(), &model).map_err(|e| e.to_string())?;
        let typing = b.lookup("typing").ok_or("Missing typing")?;
        b.add_proxy(typing, type_feature, "T")
            .map_err(|e| e.to_string())?;
        let scopes = FrozenScopes::new(&mm, &bundle, &verified)
            .map_err(|e| e.to_string())?
            .with_execution(Execution::Shadow);
        let seen = RefCell::new(None);
        let record = |view: &LinkView<'_>,
                      owner: ObjId,
                      feature,
                      spelling: &str|
         -> Result<Option<ObjId>, FrozenError> {
            let found = scopes.candidates(view, owner, feature, spelling)?;
            let ids = found
                .iter()
                .filter_map(|o| view.identity(*o))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            *seen.borrow_mut() = Some(ids);
            Ok(found.first().copied())
        };
        // Only the candidates matter here; an empty result leaves the link unresolved.
        let _ = b.link(&record);
        // Shadow mode: both executors ran and agreed; none deferred.
        let stats = scopes.stats();
        if stats.fast != 1 || stats.deferred != 0 {
            return Err(format!("Direct executor did not run the scope: {stats:?}"));
        }
        seen.into_inner()
            .ok_or_else(|| "The scope was never invoked".into())
    };
    let mut work = QueryBudget::new(1_000_000);
    let mut compare = |store: &FrozenStore, expected: &[&str]| -> Result<(), String> {
        let from_store = old.candidates(store, &request, &mut work)?;
        assert_eq!(from_store, expected);
        assert_eq!(frozen(store)?, from_store);
        Ok(())
    };

    compare(&store, &["localType"])?;
    storage::set_with_opposite(
        &mut store,
        &package.schema,
        "localType",
        "declaredName",
        json!("U"),
    )?;
    compare(&store, &["outerType"])?;
    storage::set_with_opposite(
        &mut store,
        &package.schema,
        "outerType",
        "declaredName",
        json!("U"),
    )?;
    compare(&store, &[])?;
    storage::set_with_opposite(
        &mut store,
        &package.schema,
        "localType",
        "declaredName",
        json!("T"),
    )?;
    storage::atomic(&mut store, |store| {
        store.create_element(
            &package.schema,
            "duplicateMembership",
            "OwningMembership",
            "memory:scope.kerml",
            0,
            1,
        )?;
        store.create_element(
            &package.schema,
            "duplicateType",
            "Class",
            "memory:scope.kerml",
            0,
            1,
        )?;
        storage::set_with_opposite(
            store,
            &package.schema,
            "duplicateType",
            "declaredName",
            json!("T"),
        )?;
        storage::set_with_opposite(
            store,
            &package.schema,
            "duplicateMembership",
            "ownedRelatedElement",
            json!(["duplicateType"]),
        )?;
        storage::set_with_opposite(
            store,
            &package.schema,
            "inner",
            "ownedRelationship",
            json!([
                "localTypeMembership",
                "featureMembership",
                "duplicateMembership"
            ]),
        )?;
        Ok(())
    })?;
    compare(&store, &["localType", "duplicateType"])?;
    Ok(())
}
