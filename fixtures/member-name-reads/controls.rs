//! Exact getter capability controls. Physical fixtures are not constructor/validity proof.
use intersect_kerml::membership_name_reads::{
    OwningMembershipNameReads, membership_name_provenance,
};
use intersect_model::{
    core::{
        frozen::{FrozenStore, Metamodel},
        query::*,
        storage::{self, IndexPolicy, ModelRead, ModelStore},
    },
    definitions::{
        bundle::{DefinitionBundle, DefinitionSource, SourceKind},
        compiler,
        ecore::ParseLimits,
        ecore_migration::BundleMigrationReport,
        grammar::Profile,
        preparation::PreparedLanguageDefinition,
        schema::{BundleCompilation, CompilationSourcePolicy, MemberIdentity, Schema},
    },
    semantic_reads::{
        ReadDependencies, ReadMember, ReadPlan, RelatedNull, RelatedRead, SemanticReadPolicy,
        SemanticReadSession,
    },
};
use serde_json::{Value, json};
use std::cell::RefCell;
#[path = "generic.rs"]
mod generic;
#[allow(dead_code)]
#[path = "../semantic-read-policy/controls.rs"]
mod shared;
const ECORE: &str = include_str!("../xtext/pinned-kerml-operands/SysML.ecore");
fn inputs(uri: &str) -> Result<(BundleCompilation, BundleMigrationReport), String> {
    let source = DefinitionSource::new(uri, SourceKind::Ecore, ECORE);
    assert_eq!(source.id.sha256, intersect_kerml::PILOT_SCHEMA_SHA256);
    let mut rules = DefinitionSource::new(
        "memory:member-name/rules.rcore",
        SourceKind::RcoreBehavior,
        "rcore 1; package memberName version \"1\";",
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
    .map_err(|error| error.to_string())?;
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
    .map_err(|error| format!("{error:?}"))
}
fn store(schema: &Schema, indexes: IndexPolicy) -> Result<FrozenStore, String> {
    let metamodel = Metamodel::freeze(schema).map_err(|e| e.to_string())?;
    let mut store = FrozenStore::with_index_policy(std::sync::Arc::new(metamodel), indexes);
    storage::atomic(&mut store, |s| {
        for (id, class) in [
            ("m", "OwningMembership"),
            ("p", "Package"),
            ("library", "LibraryPackage"),
            ("other", "Namespace"),
        ] {
            s.create_element(schema, id, class, "memory:member-name", 0, 12)?;
        }
        s.set_value("p", "declaredName", Some(json!("P")))?;
        s.set_value("p", "name", Some(json!("raw target lie")))?;
        s.set_value("m", "ownedRelatedElement", Some(json!(["p"])))?;
        for name in [
            "memberName",
            "ownedMemberName",
            "memberElement",
            "ownedMemberElement",
        ] {
            s.set_value("m", name, Some(json!("raw source lie")))?;
        }
        Ok(())
    })?;
    Ok(store)
}
fn set(store: &mut FrozenStore, id: &str, name: &str, value: Value) -> Result<(), String> {
    storage::atomic(store, |s| s.set_value(id, name, Some(value)))
}
fn value(
    session: &SemanticReadSession<'_, impl SemanticReadPolicy>,
    feature: &str,
    budget: &mut QueryBudget,
) -> Result<Value, String> {
    session.value("OwningMembership", "m", feature, budget)
}
pub fn exact_pilot_names_null_branch_and_raw_conflicts() -> Result<(), String> {
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let prepared = prepare(&input, &migration)?;
    let oracle: Value =
        serde_json::from_str(include_str!("oracle.json")).map_err(|error| error.to_string())?;
    let mut raw = store(prepared.schema(), IndexPolicy::AllValues)?;
    {
        let session = SemanticReadSession::new(&prepared, &raw, &OwningMembershipNameReads)?;
        for name in ["memberName", "ownedMemberName"] {
            assert_eq!(
                value(&session, name, &mut QueryBudget::new(20_000_000))?,
                oracle[name]["e_get_resolve_true"]
            );
        }
        assert_eq!(
            session.navigate(
                "OwningMembership",
                "m",
                "memberName",
                &mut QueryBudget::new(20_000_000)
            )?,
            "P"
        );
        assert_eq!(
            session.project(
                "OwningMembership",
                "m",
                &["memberName", "ownedMemberName"],
                &mut QueryBudget::new(20_000_000)
            )?,
            vec![
                ("memberName".into(), json!("P")),
                ("ownedMemberName".into(), json!("P"))
            ]
        );
    }
    assert_eq!(raw.value("m", "memberName")?, Some(json!("raw source lie")));
    assert_eq!(raw.value("p", "name")?, Some(json!("raw target lie")));
    set(&mut raw, "p", "declaredName", Value::Null)?;
    let session = SemanticReadSession::new(&prepared, &raw, &OwningMembershipNameReads)?;
    assert_eq!(
        value(&session, "memberName", &mut QueryBudget::new(20_000_000))?,
        Value::Null
    );
    // Source-derived null target name; this is not a new Java Package-null measurement.
    set(&mut raw, "m", "ownedRelatedElement", json!([]))?;
    let trace = shared::Trace {
        inner: &raw,
        calls: RefCell::new(vec![]),
    };
    let session = SemanticReadSession::new(&prepared, &trace, &OwningMembershipNameReads)?;
    assert_eq!(
        value(&session, "memberName", &mut QueryBudget::new(20_000_000))?,
        Value::Null
    );
    assert!(
        !trace
            .calls
            .borrow()
            .iter()
            .any(|call| call.ends_with(":p") || call.starts_with("value:p:"))
    );
    // Exact source getter branch, while the required one-valued graph is invalid.
    assert_eq!(
        prepared
            .schema()
            .effective_feature("OwningMembership", "ownedMemberElement")?
            .feature
            .lower,
        1
    );
    assert_eq!(membership_name_provenance().len(), 2);
    Ok(())
}
pub fn exact_source_member_and_actual_contracts() -> Result<(), String> {
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let prepared = prepare(&input, &migration)?;
    for name in [
        "memberName",
        "ownedMemberName",
        "ownedMemberElement",
        "ownedRelatedElement",
    ] {
        let selected = prepared
            .schema()
            .effective_feature("OwningMembership", name)?;
        for mismatch in [0, 1, 2] {
            let mut identity = selected.identity.clone();
            let MemberIdentity::Source { declaration } = &mut identity else {
                return Err("Missing source member".into());
            };
            match mismatch {
                0 => declaration.source.uri = "wrong".into(),
                1 => declaration.source.sha256 = "wrong".into(),
                _ => declaration.path = vec![0, 0],
            }
            assert_eq!(
                OwningMembershipNameReads.plan(ReadMember {
                    actual: "OwningMembership",
                    owner: selected.owner,
                    identity: &identity,
                    feature: selected.feature
                }),
                ReadPlan::Unsupported
            );
        }
        for actual in ["Membership", "FeatureMembership", "Namespace"] {
            assert_eq!(
                OwningMembershipNameReads.plan(ReadMember {
                    actual,
                    owner: selected.owner,
                    identity: &selected.identity,
                    feature: selected.feature
                }),
                ReadPlan::Unsupported
            );
        }
    }
    let raw = store(prepared.schema(), IndexPolicy::AllValues)?;
    let trace = shared::Trace {
        inner: &raw,
        calls: RefCell::new(vec![]),
    };
    let session = SemanticReadSession::new(&prepared, &trace, &OwningMembershipNameReads)?;
    assert!(
        session
            .project(
                "OwningMembership",
                "m",
                &["memberName", "qualifiedName"],
                &mut QueryBudget::new(20_000_000)
            )
            .is_err()
    );
    assert!(trace.calls.borrow().is_empty());
    let (wrong_input, wrong_migration) = inputs("memory:wrong/schema.ecore")?;
    let wrong = prepare(&wrong_input, &wrong_migration)?;
    assert!(SemanticReadSession::new(&wrong, &trace, &OwningMembershipNameReads).is_err());
    assert!(trace.calls.borrow().is_empty());
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Fault {
    Unknown,
    WrongGetter,
    Duplicate,
    Nonzero,
    UnsupportedTarget,
    NonReference,
    Collection,
    StoredCaller,
    RejectNull,
}
struct FaultPolicy(Fault);
const NORMAL: RelatedRead<'static> = RelatedRead {
    key: "owned-member-package-name",
    reference: "ownedMemberElement",
    target_actual: "Package",
    target_feature: "name",
    null: RelatedNull::ReturnNull,
};
impl SemanticReadPolicy for FaultPolicy {
    fn validate(&self, d: &PreparedLanguageDefinition<'_>) -> QueryResult<()> {
        OwningMembershipNameReads.validate(d)
    }
    fn plan(&self, m: ReadMember<'_>) -> ReadPlan {
        if matches!(self.0, Fault::StoredCaller)
            && m.actual == "OwningMembership"
            && m.feature.name == "memberName"
        {
            ReadPlan::Stored
        } else {
            OwningMembershipNameReads.plan(m)
        }
    }
    fn dependencies(&self, m: ReadMember<'_>) -> &[&str] {
        OwningMembershipNameReads.dependencies(m)
    }
    fn related_dependencies(&self, m: ReadMember<'_>) -> &[RelatedRead<'_>] {
        if matches!(self.0, Fault::StoredCaller) && m.feature.name == "memberName" {
            return &[NORMAL];
        }
        if m.actual != "OwningMembership" || m.feature.name != "ownedMemberName" {
            return &[];
        }
        match self.0 {
            Fault::Duplicate => &[NORMAL, NORMAL],
            Fault::UnsupportedTarget => &[RelatedRead {
                target_feature: "qualifiedName",
                ..NORMAL
            }],
            Fault::NonReference => &[RelatedRead {
                reference: "memberName",
                ..NORMAL
            }],
            Fault::Collection => &[RelatedRead {
                reference: "ownedRelatedElement",
                ..NORMAL
            }],
            Fault::RejectNull => &[RelatedRead {
                null: RelatedNull::Reject,
                ..NORMAL
            }],
            _ => &[NORMAL],
        }
    }
    fn read(
        &self,
        m: ReadMember<'_>,
        d: &dyn ReadDependencies,
        b: &mut QueryBudget,
    ) -> QueryResult<Value> {
        if matches!(self.0, Fault::WrongGetter) && m.feature.name == "memberName" {
            return d.read_related(NORMAL.key, 0, b);
        }
        if m.feature.name == "ownedMemberName" {
            return d.read_related(
                if matches!(self.0, Fault::Unknown) {
                    "wrong-getter-key"
                } else {
                    NORMAL.key
                },
                usize::from(matches!(self.0, Fault::Nonzero)),
                b,
            );
        }
        OwningMembershipNameReads.read(m, d, b)
    }
}
pub fn declared_edge_keys_preflight_and_default_hook_reject() -> Result<(), String> {
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let prepared = prepare(&input, &migration)?;
    let mut raw = store(prepared.schema(), IndexPolicy::AllValues)?;
    for fault in [
        Fault::Duplicate,
        Fault::UnsupportedTarget,
        Fault::NonReference,
        Fault::Collection,
        Fault::StoredCaller,
    ] {
        let trace = shared::Trace {
            inner: &raw,
            calls: RefCell::new(vec![]),
        };
        let policy = FaultPolicy(fault);
        let session = SemanticReadSession::new(&prepared, &trace, &policy)?;
        let error = value(&session, "memberName", &mut QueryBudget::new(20_000_000))
            .err()
            .ok_or("Expected static edge refusal")?;
        assert!(trace.calls.borrow().is_empty(), "{fault:?}: {error}");
    }
    for (fault, expected) in [
        (Fault::Unknown, "Undeclared related"),
        (Fault::WrongGetter, "Unsupported related"),
        (Fault::Nonzero, "occurrence zero"),
    ] {
        let trace = shared::Trace {
            inner: &raw,
            calls: RefCell::new(vec![]),
        };
        let policy = FaultPolicy(fault);
        let session = SemanticReadSession::new(&prepared, &trace, &policy)?;
        let error = value(&session, "memberName", &mut QueryBudget::new(20_000_000))
            .err()
            .ok_or("Expected edge refusal")?;
        assert!(error.contains(expected), "{fault:?}: {error}");
        assert!(
            !trace
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("value:"))
        );
    }
    set(&mut raw, "m", "ownedRelatedElement", json!([]))?;
    let policy = FaultPolicy(Fault::RejectNull);
    let session = SemanticReadSession::new(&prepared, &raw, &policy)?;
    assert!(
        value(&session, "memberName", &mut QueryBudget::new(20_000_000))
            .err()
            .ok_or("Expected absent refusal")?
            .contains("absent")
    );
    Ok(())
}
pub fn malformed_dangling_subtype_identity_and_revision_failures() -> Result<(), String> {
    let (input, migration) = inputs("org.omg.sysml/model/SysML.ecore")?;
    let prepared = prepare(&input, &migration)?;
    for target in [
        json!(42),
        json!({"id":"p"}),
        json!(["p"]),
        json!("missing"),
        json!("library"),
        json!("other"),
    ] {
        let mut raw = store(prepared.schema(), IndexPolicy::AllValues)?;
        set(
            &mut raw,
            "m",
            "ownedRelatedElement",
            json!([target.clone()]),
        )?;
        let trace = shared::Trace {
            inner: &raw,
            calls: RefCell::new(vec![]),
        };
        let session = SemanticReadSession::new(&prepared, &trace, &OwningMembershipNameReads)?;
        let error = value(&session, "memberName", &mut QueryBudget::new(20_000_000))
            .err()
            .ok_or("Expected target refusal")?;
        assert!(
            !trace
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with("value:p:")
                    || call.starts_with("value:library:")
                    || call.starts_with("value:other:")),
            "{target}: {error}"
        );
        if !target.is_string() {
            assert!(error.contains("non-identity"), "{error}")
        }
    }
    let raw = store(prepared.schema(), IndexPolicy::AllValues)?;
    for id in ["m", "p"] {
        let mut exported = storage::export(&raw)?;
        exported
            .elements
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or("Missing raw receiver")?
            .type_identity = "wrong".into();
        let wrong = storage::ModelView::new(&exported)?;
        let trace = shared::Trace {
            inner: &wrong,
            calls: RefCell::new(vec![]),
        };
        let session = SemanticReadSession::new(&prepared, &trace, &OwningMembershipNameReads)?;
        assert!(value(&session, "memberName", &mut QueryBudget::new(20_000_000)).is_err());
        assert!(
            !trace
                .calls
                .borrow()
                .iter()
                .any(|call| call.starts_with(&format!("value:{id}:")))
        );
    }
    let mut exported = storage::export(&raw)?;
    exported.schema_revision = "wrong".into();
    let stale = storage::ModelView::new(&exported)?;
    let trace = shared::Trace {
        inner: &stale,
        calls: RefCell::new(vec![]),
    };
    let session = SemanticReadSession::new(&prepared, &trace, &OwningMembershipNameReads)?;
    assert!(value(&session, "memberName", &mut QueryBudget::new(20_000_000)).is_err());
    assert_eq!(trace.calls.borrow().as_slice(), ["revision"]);
    Ok(())
}

pub fn shared_reference_domain_is_checked_before_target_value() -> Result<(), String> {
    generic::shared_reference_domain_is_checked_before_target_value()
}
