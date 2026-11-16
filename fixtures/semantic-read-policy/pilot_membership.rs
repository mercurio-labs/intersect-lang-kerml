//! Value-getter parity only. Native raw graphs below are not constructor/validity proof.
use intersect_kerml::membership_reads::OwningMembershipReads;
use intersect_model::{
    core::{
        frozen::FrozenStore,
        query::QueryBudget,
        storage::{ModelRead, ModelStore},
    },
    definitions::{
        bundle::{DefinitionBundle, DefinitionSource, SourceKind},
        compiler,
        ecore::ParseLimits,
        ecore_migration::BundleMigrationReport,
        grammar::Profile,
        preparation::PreparedLanguageDefinition,
        schema::{BundleCompilation, CompilationSourcePolicy},
    },
    semantic_reads::{SemanticReadPolicy, SemanticReadSession},
};
use serde_json::{Value, json};
const ECORE: &str = include_str!("../xtext/pinned-kerml-operands/SysML.ecore");
fn inputs() -> Result<(BundleCompilation, BundleMigrationReport), String> {
    inputs_with_uri("org.omg.sysml/model/SysML.rcore")
}
fn inputs_with_uri(uri: &str) -> Result<(BundleCompilation, BundleMigrationReport), String> {
    let source = DefinitionSource::new("org.omg.sysml/model/SysML.ecore", SourceKind::Ecore, ECORE);
    assert_eq!(source.id.sha256, intersect_kerml::PILOT_SCHEMA_SHA256);
    let mut rules = DefinitionSource::new(
        "memory:member-reads/behavior.rcore",
        SourceKind::RcoreBehavior,
        "rcore 1; package memberReads version \"1\";",
    );
    rules.dependencies = vec![source.id.clone()];
    let original = BundleCompilation {
        bundle: DefinitionBundle {
            version: 1,
            sources: vec![source.clone(), rules.clone()],
            namespaces: vec![],
        },
        ecore: source.id.clone(),
        xtext: None,
        rcore_behavior: rules.id,
        namespace: "https://www.omg.org/spec/SysML/20250201".into(),
        materialized_types: vec![],
    };
    let migration = intersect_model::migration::migrate_ecore_bundle_with_limits(
        original.bundle.clone(),
        &[(source.id, uri.into())].into(),
        ParseLimits {
            source_bytes: ECORE.len(),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok((migration.remap_compilation(&original)?, migration))
}
fn prepare<'a>(
    input: &'a BundleCompilation,
    origin: &'a BundleMigrationReport,
) -> Result<PreparedLanguageDefinition<'a>, String> {
    let policy = CompilationSourcePolicy {
        structure_bytes: ECORE.len(),
        ..CompilationSourcePolicy::default()
    };
    compiler::prepare_bundle_with_borrowed_authored_validation(
        input,
        origin,
        Profile::NativeV67,
        &[],
        None,
        policy,
        policy,
        &|_, _, _| Ok(()),
    )
    .map_err(|e| format!("{e:?}"))
}
pub fn exact_owning_membership_getters_match_independent_pilot_observations() -> Result<(), String>
{
    let (input, origin) = inputs()?;
    let prepared = prepare(&input, &origin)?;
    let evidence: Value =
        serde_json::from_str(include_str!("pilot-observations.json")).map_err(|e| e.to_string())?;
    let mut store = FrozenStore::for_schema(prepared.schema()).map_err(|e| e.to_string())?;
    intersect_model::core::storage::atomic(&mut store, |s| {
        s.create_element(
            prepared.schema(),
            "owner",
            "OwningMembership",
            "memory:native-getter",
            0,
            12,
        )?;
        for id in [
            "oracle:constructed#2",
            "oracle:constructed#3",
            "oracle:constructed#4",
        ] {
            s.create_element(
                prepared.schema(),
                id,
                "Package",
                "memory:native-getter",
                0,
                12,
            )?;
        }
        Ok(())
    })?;
    for row in evidence["construction_controls"]
        .as_array()
        .ok_or("Missing oracle rows")?
    {
        let order = row["owned_related_element_order"]
            .as_array()
            .ok_or("Missing oracle sequence")?
            .iter()
            .map(|item| item["object_uri"].clone())
            .collect::<Vec<_>>();
        intersect_model::core::storage::atomic(&mut store, |s| {
            s.set_value("owner", "ownedRelatedElement", Some(json!(order)))?;
            s.set_value("owner", "memberElement", Some(Value::Null))
        })?;
        let session = SemanticReadSession::new(&prepared, &store, &OwningMembershipReads)?;
        let expected = row["member_element_getter"]
            .as_object()
            .map(|value| value["object_uri"].clone())
            .unwrap_or(Value::Null);
        let actual = session.value(
            "OwningMembership",
            "owner",
            "memberElement",
            &mut QueryBudget::new(20_000_000),
        )?;
        assert_eq!(actual, expected, "{}", row["step"]);
        assert_eq!(
            session.value(
                "OwningMembership",
                "owner",
                "ownedMemberElement",
                &mut QueryBudget::new(20_000_000)
            )?,
            expected
        );
        // Null in the fresh/cleared cases is an observed getter output on an
        // invalid required-member graph, not a valid model/constructor award.
        assert!(
            session
                .value(
                    "OwningMembership",
                    "owner",
                    "memberName",
                    &mut QueryBudget::new(20_000_000)
                )
                .is_err()
        );
    }
    // Contradictory native physical data is adversarial; not Java-reachable state.
    intersect_model::core::storage::atomic(&mut store, |s| {
        s.set_value(
            "owner",
            "ownedRelatedElement",
            Some(json!(["oracle:constructed#2", "oracle:constructed#3"])),
        )?;
        s.set_value(
            "owner",
            "memberElement",
            Some(json!("oracle:constructed#4")),
        )
    })?;
    let session = SemanticReadSession::new(&prepared, &store, &OwningMembershipReads)?;
    assert_eq!(
        session.value(
            "OwningMembership",
            "owner",
            "memberElement",
            &mut QueryBudget::new(20_000_000)
        )?,
        json!("oracle:constructed#2")
    );
    assert_eq!(
        store.value("owner", "memberElement")?,
        Some(json!("oracle:constructed#4"))
    );
    assert!(
        session
            .value(
                "Membership",
                "owner",
                "memberElement",
                &mut QueryBudget::new(20_000_000)
            )
            .is_err()
    );
    Ok(())
}

/// Getter observations over genuine Ecore sources. These raw fixture graphs
/// intentionally do not award constructor, structural validation or ResourceSet
/// admission. The native host separately checks those complete graph stages.
pub fn native_owned_membership_families_share_source_typed_first_getters() -> Result<(), String> {
    let (input, origin) = inputs_with_uri("org.omg.sysml/model/SysML.ecore.rcore")?;
    let prepared = prepare(&input, &origin)?;
    let policy = intersect_kerml::membership_reads::native_owned_membership_reads(
        &prepared,
        &mut QueryBudget::new(20_000_000),
    )?;
    let rows = [
        ("OwningMembership", "Package"),
        ("FeatureMembership", "Feature"),
        ("EndFeatureMembership", "Feature"),
        ("ParameterMembership", "Feature"),
        ("ReturnParameterMembership", "Feature"),
        ("ElementFilterMembership", "LiteralInteger"),
        ("FeatureValue", "LiteralInteger"),
        ("ResultExpressionMembership", "LiteralInteger"),
        ("ActorMembership", "PartUsage"),
        ("StakeholderMembership", "PartUsage"),
        ("SubjectMembership", "Usage"),
        ("VariantMembership", "Usage"),
        ("ObjectiveMembership", "RequirementUsage"),
        ("RequirementConstraintMembership", "ConstraintUsage"),
        ("RequirementVerificationMembership", "RequirementUsage"),
        ("FramedConcernMembership", "ConcernUsage"),
        ("StateSubactionMembership", "ActionUsage"),
        ("TransitionFeatureMembership", "Step"),
        ("ViewRenderingMembership", "RenderingUsage"),
    ];
    let mut store = FrozenStore::for_schema(prepared.schema()).map_err(|e| e.to_string())?;
    intersect_model::storage::atomic(&mut store, |s| {
        s.create_element(
            prepared.schema(),
            "wrong-kind",
            "Package",
            "memory:native-membership-getters",
            0,
            1,
        )?;
        for (actual, target) in rows {
            s.create_element(
                prepared.schema(),
                actual,
                actual,
                "memory:native-membership-getters",
                0,
                1,
            )?;
            s.create_element(
                prepared.schema(),
                &format!("target-{actual}"),
                target,
                "memory:native-membership-getters",
                0,
                1,
            )?;
        }
        Ok(())
    })?;
    for (actual, _) in rows {
        let target = format!("target-{actual}");
        intersect_model::storage::atomic(&mut store, |s| {
            s.set_value(actual, "ownedRelatedElement", Some(json!([target])))
        })?;
        assert_eq!(
            SemanticReadSession::new(&prepared, &store, &policy)?.value(
                actual,
                actual,
                "memberElement",
                &mut QueryBudget::new(20_000_000),
            )?,
            json!(target),
            "{actual}"
        );
        intersect_model::storage::atomic(&mut store, |s| {
            s.set_value(
                actual,
                "ownedRelatedElement",
                Some(json!(["wrong-kind", target])),
            )
        })?;
        let expected = if actual == "OwningMembership" {
            json!("wrong-kind")
        } else {
            Value::Null
        };
        assert_eq!(
            SemanticReadSession::new(&prepared, &store, &policy)?.value(
                actual,
                actual,
                "memberElement",
                &mut QueryBudget::new(20_000_000),
            )?,
            expected,
            "first-before-kind for {actual}"
        );
        intersect_model::storage::atomic(&mut store, |s| {
            s.set_value(actual, "ownedRelatedElement", Some(json!([])))
        })?;
        assert_eq!(
            SemanticReadSession::new(&prepared, &store, &policy)?.value(
                actual,
                actual,
                "memberElement",
                &mut QueryBudget::new(20_000_000),
            )?,
            Value::Null,
            "empty {actual}"
        );
    }
    assert!(
        SemanticReadSession::new(&prepared, &store, &policy)?
            .value(
                "FeatureValue",
                "FeatureValue",
                "featureWithValue",
                &mut QueryBudget::new(20_000_000),
            )
            .is_err()
    );
    Ok(())
}

/// A bound metadata policy belongs to one immutable prepared owner. Reusing it
/// must neither admit an equal foreign owner nor retain any model getter result.
pub fn native_bound_membership_owner_and_live_values() -> Result<(), String> {
    let (input, origin) = inputs_with_uri("org.omg.sysml/model/SysML.ecore.rcore")?;
    let prepared = prepare(&input, &origin)?;
    let foreign = prepare(&input, &origin)?;
    assert_eq!(
        serde_json::to_value(prepared.schema()).map_err(|error| error.to_string())?,
        serde_json::to_value(foreign.schema()).map_err(|error| error.to_string())?,
        "the foreign-owner control has genuinely identical schema metadata"
    );
    let policy = intersect_kerml::membership_reads::native_owned_membership_reads(
        &prepared,
        &mut QueryBudget::new(20_000_000),
    )?;
    policy.validate(&prepared)?;
    let error = policy
        .validate(&foreign)
        .err()
        .ok_or("equal foreign prepared owner was accepted")?;
    assert!(
        error.contains("SAME genuine prepared source owner"),
        "{error}"
    );
    // The public unbound policy retains its ordinary full source validation.
    OwningMembershipReads.validate(&foreign)?;

    let mut store = FrozenStore::for_schema(prepared.schema()).map_err(|e| e.to_string())?;
    intersect_model::storage::atomic(&mut store, |s| {
        for (id, actual) in [
            ("membership", "FeatureMembership"),
            ("first", "Feature"),
            ("second", "Feature"),
            ("other-kind", "Package"),
        ] {
            s.create_element(
                prepared.schema(),
                id,
                actual,
                "memory:bound-native-membership",
                0,
                1,
            )?;
        }
        s.set_value("membership", "ownedRelatedElement", Some(json!(["first"])))
    })?;
    assert!(SemanticReadSession::new(&foreign, &store, &policy).is_err());
    let read = |store: &FrozenStore, budget: &mut QueryBudget| {
        SemanticReadSession::new(&prepared, store, &policy)?.value(
            "FeatureMembership",
            "membership",
            "memberElement",
            budget,
        )
    };
    let before = json!({"row": store.get("membership")?, "revision": store.revision()?});
    let mut initial_budget = QueryBudget::new(20_000_000);
    assert_eq!(read(&store, &mut initial_budget)?, json!("first"));
    for _ in 0..3 {
        let mut repeated_budget = QueryBudget::new(20_000_000);
        assert_eq!(read(&store, &mut repeated_budget)?, json!("first"));
        assert_eq!(repeated_budget.reads, initial_budget.reads);
    }
    assert!(initial_budget.reads > 0);
    assert_eq!(
        json!({"row": store.get("membership")?, "revision": store.revision()?}),
        before,
        "repeated source metadata reuse is read-only"
    );
    for (related, expected) in [
        (json!(["second", "first"]), json!("second")),
        (json!(["other-kind", "first"]), Value::Null),
        (json!([]), Value::Null),
        (json!(["first"]), json!("first")),
    ] {
        intersect_model::storage::atomic(&mut store, |s| {
            s.set_value("membership", "ownedRelatedElement", Some(related.clone()))
        })?;
        assert_eq!(read(&store, &mut QueryBudget::new(20_000_000))?, expected);
        assert_eq!(
            store.value("membership", "ownedRelatedElement")?,
            Some(related)
        );
    }
    let original = store.get("first")?.ok_or("missing target fixture")?;
    intersect_model::storage::atomic(&mut store, |s| {
        let mut corrupt = original.clone();
        corrupt.type_identity = "foreign:Feature".into();
        s.put(corrupt)
    })?;
    assert!(read(&store, &mut QueryBudget::new(20_000_000)).is_err());
    intersect_model::storage::atomic(&mut store, |s| s.put(original.clone()))?;
    assert_eq!(
        read(&store, &mut QueryBudget::new(20_000_000))?,
        json!("first")
    );

    for related in [
        None,
        Some(Value::Null),
        Some(json!(["missing"])),
        Some(json!([13])),
    ] {
        intersect_model::storage::atomic(&mut store, |s| {
            s.set_value("membership", "ownedRelatedElement", related.clone())
        })?;
        let before = json!({"row": store.get("membership")?, "revision": store.revision()?});
        assert!(read(&store, &mut QueryBudget::new(20_000_000)).is_err());
        assert_eq!(
            json!({"row": store.get("membership")?, "revision": store.revision()?}),
            before,
            "failed live reads do not change the model"
        );
    }
    intersect_model::storage::atomic(&mut store, |s| {
        s.set_value("membership", "ownedRelatedElement", Some(json!(["first"])))
    })?;
    assert!(read(&store, &mut QueryBudget::new(0)).is_err());
    assert!(
        SemanticReadSession::new(&prepared, &store, &policy)?
            .value(
                "OwningMembership",
                "membership",
                "memberElement",
                &mut QueryBudget::new(20_000_000),
            )
            .is_err(),
        "bound metadata does not replace the actual receiver check"
    );

    let (bad_input, bad_origin) = inputs_with_uri("memory:foreign-native-membership.rcore")?;
    let bad_source = prepare(&bad_input, &bad_origin)?;
    OwningMembershipReads.validate(&bad_source)?;
    let error = intersect_kerml::membership_reads::native_owned_membership_reads(
        &bad_source,
        &mut QueryBudget::new(20_000_000),
    )
    .err()
    .ok_or("native binding accepted the wrong converted source URI")?;
    assert!(error.contains("actual converted Pilot source"), "{error}");
    Ok(())
}
