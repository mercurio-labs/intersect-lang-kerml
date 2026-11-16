//! One source-bound value getter. Raw fixtures are not constructor/validity proofs.
use intersect_kerml::package_name_reads::{
    PACKAGE_NAME_SOURCE, PACKAGE_NAME_SOURCE_URI, PackageNameReads, package_name_binding,
    package_name_provenance,
};
use intersect_model::{
    core::{
        frozen::{FrozenStore, Metamodel},
        query::*,
        storage::{self, IndexPolicy, ModelRead, ModelStore},
    },
    definitions::{
        bundle::{DefinitionBundle, DefinitionSource, SourceId, SourceKind},
        compiler,
        ecore::ParseLimits,
        ecore_migration::BundleMigrationReport,
        grammar::Profile,
        preparation::PreparedLanguageDefinition,
        schema::{BundleCompilation, CompilationSourcePolicy, MemberIdentity, Schema},
    },
    semantic_reads::{ReadMember, ReadPlan, SemanticReadPolicy, SemanticReadSession},
};
use serde_json::{Value, json};
use std::cell::RefCell;
#[allow(dead_code)]
#[path = "../semantic-read-policy/controls.rs"]
mod shared;
const ECORE: &str = include_str!("../xtext/pinned-kerml-operands/SysML.ecore");
fn inputs(uri: &str) -> Result<(BundleCompilation, BundleMigrationReport), String> {
    let source = DefinitionSource::new(uri, SourceKind::Ecore, ECORE);
    assert_eq!(source.id.sha256, intersect_kerml::PILOT_SCHEMA_SHA256);
    let mut rules = DefinitionSource::new(
        "memory:package-name/rules.rcore",
        SourceKind::RcoreBehavior,
        "rcore 1; package packageName version \"1\";",
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
        &[(source.id, "org.omg.sysml/model/SysML.ecore.rcore".into())].into(),
        ParseLimits {
            source_bytes: ECORE.len(),
        },
    )
    .map_err(|e| e.to_string())?;
    Ok((migration.remap_compilation(&original)?, migration))
}
fn prepare<'a>(
    input: &'a BundleCompilation,
    migration: &'a BundleMigrationReport,
) -> Result<PreparedLanguageDefinition<'a>, String> {
    let policy = CompilationSourcePolicy {
        structure_bytes: ECORE.len(),
        ..CompilationSourcePolicy::default()
    };
    compiler::prepare_bundle_with_borrowed_authored_validation(
        input,
        migration,
        Profile::NativeV67,
        &[],
        None,
        policy,
        policy,
        &|_, _, _| Ok(()),
    )
    .map_err(|e| format!("{e:?}"))
}
fn raw_store(schema: &Schema, indexes: IndexPolicy) -> Result<FrozenStore, String> {
    let metamodel = Metamodel::freeze(schema).map_err(|e| e.to_string())?;
    let mut store = FrozenStore::with_index_policy(std::sync::Arc::new(metamodel), indexes);
    storage::atomic(&mut store, |s| {
        for (id, class) in [("p", "Package"), ("library", "LibraryPackage")] {
            s.create_element(schema, id, class, "memory:raw-name", 0, 12)?;
            s.set_value(id, "declaredName", Some(json!("P")))?;
            // Adversarial physical derived slot, not a Java-reachable or valid model claim.
            s.set_value(id, "name", Some(json!("raw lie")))?;
        }
        Ok(())
    })?;
    Ok(store)
}
fn query(schema: &Schema) -> QueryPlan {
    QueryPlan {
        schema_uri: schema.uri.clone(),
        schema_revision: schema.revision.clone(),
        from: "Package".into(),
        filter: Some(Predicate::Compare {
            left: Expression::Field {
                path: "name".into(),
            },
            comparison: Comparison::Eq,
            right: Expression::Literal { value: json!("P") },
        }),
        select: vec![Column {
            name: "name".into(),
            expression: Expression::Field {
                path: "name".into(),
            },
        }],
        order_by: vec![],
        limit: 10,
    }
}
pub fn independent_pilot_package_name_and_null_ignore_conflicting_raw_name() -> Result<(), String> {
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let definition = prepare(&input, &migration)?;
    let evidence: Value =
        serde_json::from_str(include_str!("oracle.json")).map_err(|e| e.to_string())?;
    assert_eq!(evidence["named_package"]["e_get_resolve_true"], "P");
    assert_eq!(
        evidence["root_namespace"]["e_get_resolve_true"],
        Value::Null
    );
    let mut store = raw_store(definition.schema(), IndexPolicy::AllValues)?;
    {
        let session = SemanticReadSession::new(&definition, &store, &PackageNameReads)?;
        assert_eq!(
            session.value("Package", "p", "name", &mut QueryBudget::new(20_000_000))?,
            evidence["named_package"]["e_get_resolve_true"]
        );
        assert_eq!(
            session.navigate("Package", "p", "name", &mut QueryBudget::new(20_000_000))?,
            "P"
        );
        assert_eq!(
            session.project(
                "Package",
                "p",
                &["name", "declaredName"],
                &mut QueryBudget::new(20_000_000)
            )?,
            vec![
                ("name".into(), json!("P")),
                ("declaredName".into(), json!("P"))
            ]
        );
    }
    assert_eq!(store.value("p", "name")?, Some(json!("raw lie")));
    storage::atomic(&mut store, |s| {
        s.set_value("p", "declaredName", Some(Value::Null))
    })?;
    let session = SemanticReadSession::new(&definition, &store, &PackageNameReads)?;
    assert_eq!(
        session.value("Package", "p", "name", &mut QueryBudget::new(20_000_000))?,
        Value::Null
    );
    // Null Package is a source-derived test of base effectiveName, not a new Java observation.
    assert_eq!(store.value("p", "name")?, Some(json!("raw lie")));
    assert_eq!(package_name_provenance().len(), 3);
    let binding = package_name_binding()?;
    assert_eq!(
        binding.source(),
        &SourceId::for_text(PACKAGE_NAME_SOURCE_URI, PACKAGE_NAME_SOURCE)
    );
    assert!(binding.contract_id().starts_with("sha256:"));
    Ok(())
}
pub fn exact_source_member_actual_receiver_and_subtype_guards_remain_closed() -> Result<(), String>
{
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let definition = prepare(&input, &migration)?;
    let store = raw_store(definition.schema(), IndexPolicy::AllValues)?;
    let selected = definition.schema().effective_feature("Package", "name")?;
    let mut wrong = selected.identity.clone();
    let MemberIdentity::Source { declaration } = &mut wrong else {
        return Err("Missing source identity".into());
    };
    declaration.path = vec![49, 1];
    assert_eq!(
        PackageNameReads.plan(ReadMember {
            actual: "Package",
            owner: selected.owner,
            identity: &wrong,
            feature: selected.feature
        }),
        ReadPlan::Unsupported
    );
    declaration_source_guard(&selected.identity, selected.owner, selected.feature)?;
    let trace = shared::Trace {
        inner: &store,
        calls: RefCell::new(vec![]),
    };
    let session = SemanticReadSession::new(&definition, &trace, &PackageNameReads)?;
    for actual in ["LibraryPackage", "Namespace", "Feature"] {
        assert!(
            session
                .value(actual, "library", "name", &mut QueryBudget::new(20_000_000))
                .is_err()
        );
        assert!(trace.calls.borrow().is_empty());
    }
    assert!(
        session
            .value(
                "Package",
                "library",
                "name",
                &mut QueryBudget::new(20_000_000)
            )
            .is_err()
    );
    assert!(!trace.calls.borrow().iter().any(|s| s.starts_with("value:")));
    let mut model = storage::export(&store)?;
    model.schema_revision = "wrong".into();
    let stale = storage::ModelView::new(&model)?;
    let session = SemanticReadSession::new(&definition, &stale, &PackageNameReads)?;
    assert!(
        session
            .value("Package", "p", "name", &mut QueryBudget::new(20_000_000))
            .is_err()
    );
    let mut model = storage::export(&store)?;
    model
        .elements
        .iter_mut()
        .find(|e| e.id == "p")
        .ok_or("Missing p")?
        .type_identity = "wrong".into();
    let wrong = storage::ModelView::new(&model)?;
    let session = SemanticReadSession::new(&definition, &wrong, &PackageNameReads)?;
    assert!(
        session
            .value("Package", "p", "name", &mut QueryBudget::new(20_000_000))
            .is_err()
    );
    let (input, migration) = inputs("memory:wrong/original.ecore")?;
    let wrong = prepare(&input, &migration)?;
    assert!(SemanticReadSession::new(&wrong, &store, &PackageNameReads).is_err());
    Ok(())
}
fn declaration_source_guard(
    identity: &MemberIdentity,
    owner: &str,
    feature: &intersect_model::definitions::schema::Feature,
) -> Result<(), String> {
    let mut wrong = identity.clone();
    let MemberIdentity::Source { declaration } = &mut wrong else {
        return Err("Missing declaration".into());
    };
    declaration.source.sha256 = "wrong".into();
    assert_eq!(
        PackageNameReads.plan(ReadMember {
            actual: "Package",
            owner,
            identity: &wrong,
            feature
        }),
        ReadPlan::Unsupported
    );
    Ok(())
}
pub fn zero_exact_one_short_recovery_and_index_modes_share_the_boundary() -> Result<(), String> {
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let definition = prepare(&input, &migration)?;
    let mut observed = Vec::new();
    for indexes in [
        IndexPolicy::AllValues,
        IndexPolicy::selected(Vec::<String>::new()),
    ] {
        let store = raw_store(definition.schema(), indexes)?;
        let before = storage::export(&store)?;
        let trace = shared::Trace {
            inner: &store,
            calls: RefCell::new(vec![]),
        };
        let session = SemanticReadSession::new(&definition, &trace, &PackageNameReads)?;
        let mut zero = QueryBudget::new(0);
        assert!(session.value("Package", "p", "name", &mut zero).is_err());
        assert_eq!(zero.reads, 0);
        assert!(trace.calls.borrow().is_empty());
        let mut measured = QueryBudget::new(20_000_000);
        let result = session.value("Package", "p", "name", &mut measured)?;
        assert_eq!(result, "P");
        assert!(measured.reads > 1);
        let complete_calls = trace.calls.borrow().clone();
        trace.calls.borrow_mut().clear();
        let mut short = QueryBudget::new(measured.reads - 1);
        assert!(session.value("Package", "p", "name", &mut short).is_err());
        assert_eq!(short.reads, measured.reads - 1);
        // The source function runs after its admitted scalar dependency read.
        // Exhaustion may occur in that function; it must retain a deterministic
        // read prefix, return no value and leave the graph unchanged.
        assert!(complete_calls.starts_with(trace.calls.borrow().as_slice()));
        trace.calls.borrow_mut().clear();
        let mut exact = QueryBudget::new(measured.reads);
        assert_eq!(session.value("Package", "p", "name", &mut exact)?, result);
        assert_eq!(trace.calls.borrow().as_slice(), complete_calls);
        trace.calls.borrow_mut().clear();
        // Package queries include LibraryPackage; preserve conservative subtype admission.
        let error = session
            .query(
                &query(definition.schema()),
                &Parameters::new(),
                QueryLimits {
                    max_steps: 20_000_000,
                    max_results: 10,
                },
            )
            .err()
            .ok_or("Expected subtype rejection")?;
        assert!(error.contains("LibraryPackage"), "{error}");
        assert!(trace.calls.borrow().is_empty());
        assert!(
            session
                .project(
                    "Package",
                    "p",
                    &["name", "qualifiedName"],
                    &mut QueryBudget::new(20_000_000)
                )
                .is_err()
        );
        assert!(trace.calls.borrow().is_empty());
        assert_eq!(storage::export(&store)?, before);
        observed.push(json!({"result":result,"work":exact.reads,"callbacks":complete_calls,"query_error":error}));
    }
    assert_eq!(observed[0], observed[1]);
    println!("PACKAGE_NAME_BOUNDARY={}", observed[0]);
    Ok(())
}
