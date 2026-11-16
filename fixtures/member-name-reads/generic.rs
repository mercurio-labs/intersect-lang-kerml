//! Authored generic engine refusal control; not a Pilot semantic contract.
use super::*;
const SCHEMA: &str = r##"<ecore:EPackage xmlns:ecore="http://www.eclipse.org/emf/2002/Ecore" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" name="edge" nsURI="urn:edge" nsPrefix="e">
<eClassifiers xsi:type="ecore:EClass" name="Actor"><eStructuralFeatures xsi:type="ecore:EAttribute" name="label" eType="ecore:EDataType http://www.eclipse.org/emf/2002/Ecore#//EString"/><eStructuralFeatures xsi:type="ecore:EReference" name="link" eType="#//Other"/></eClassifiers>
<eClassifiers xsi:type="ecore:EClass" name="Target"><eStructuralFeatures xsi:type="ecore:EAttribute" name="backing" eType="ecore:EDataType http://www.eclipse.org/emf/2002/Ecore#//EString"/></eClassifiers>
<eClassifiers xsi:type="ecore:EClass" name="Other"/></ecore:EPackage>"##;
struct Policy {
    source: intersect_model::definitions::bundle::SourceId,
}
impl SemanticReadPolicy for Policy {
    fn validate(&self, d: &PreparedLanguageDefinition<'_>) -> QueryResult<()> {
        if d.selection().ecore == self.source {
            Ok(())
        } else {
            Err("Authored edge source mismatch".into())
        }
    }
    fn plan(&self, m: ReadMember<'_>) -> ReadPlan {
        if !matches!(m.identity,MemberIdentity::Source{declaration} if declaration.source==self.source)
        {
            return ReadPlan::Unsupported;
        }
        match (m.actual, m.feature.name.as_str()) {
            ("Actor", "label") => ReadPlan::Delegated,
            ("Actor", "link") | ("Target", "backing") => ReadPlan::Stored,
            _ => ReadPlan::Unsupported,
        }
    }
    fn dependencies(&self, _: ReadMember<'_>) -> &[&str] {
        &[]
    }
    fn related_dependencies(&self, m: ReadMember<'_>) -> &[RelatedRead<'_>] {
        if m.actual == "Actor" && m.feature.name == "label" {
            &[RelatedRead {
                key: "declared-edge",
                reference: "link",
                target_actual: "Target",
                target_feature: "backing",
                null: RelatedNull::Reject,
            }]
        } else {
            &[]
        }
    }
    fn read(
        &self,
        _: ReadMember<'_>,
        d: &dyn ReadDependencies,
        b: &mut QueryBudget,
    ) -> QueryResult<Value> {
        d.read_related("declared-edge", 0, b)
    }
}
pub fn shared_reference_domain_is_checked_before_target_value() -> Result<(), String> {
    let source = DefinitionSource::new("memory:edge/schema.ecore", SourceKind::Ecore, SCHEMA);
    let mut behavior = DefinitionSource::new(
        "memory:edge/behavior.rcore",
        SourceKind::RcoreBehavior,
        "rcore 1; package edge version \"1\";",
    );
    behavior.dependencies = vec![source.id.clone()];
    let original = BundleCompilation {
        bundle: DefinitionBundle {
            version: 1,
            sources: vec![source.clone(), behavior.clone()],
            namespaces: vec![],
        },
        ecore: source.id.clone(),
        xtext: None,
        rcore_behavior: behavior.id,
        namespace: "urn:edge".into(),
        materialized_types: vec![],
    };
    let migration = intersect_model::migration::migrate_ecore_bundle(
        original.bundle.clone(),
        &[(source.id, "memory:edge/schema.rcore".into())].into(),
    )
    .map_err(|error| error.to_string())?;
    let input = migration.remap_compilation(&original)?;
    let prepared = compiler::prepare_bundle_with_borrowed_authored_validation(
        &input,
        &migration,
        Profile::NativeV67,
        &[],
        None,
        CompilationSourcePolicy::default(),
        CompilationSourcePolicy::default(),
        &|_, _, _| Ok(()),
    )
    .map_err(|error| format!("{error:?}"))?;
    let mut raw = FrozenStore::for_schema(prepared.schema()).map_err(|e| e.to_string())?;
    storage::atomic(&mut raw, |s| {
        s.create_element(prepared.schema(), "a", "Actor", "memory:edge", 0, 1)?;
        s.create_element(prepared.schema(), "t", "Target", "memory:edge", 0, 1)?;
        s.set_value("a", "link", Some(json!("t")))?;
        s.set_value("t", "backing", Some(json!("must not read")))
    })?;
    let trace = shared::Trace {
        inner: &raw,
        calls: RefCell::new(vec![]),
    };
    let policy = Policy {
        source: input.ecore.clone(),
    };
    let session = SemanticReadSession::new(&prepared, &trace, &policy)?;
    let mut budget = QueryBudget::new(1_000_000);
    let error = session
        .value("Actor", "a", "label", &mut budget)
        .err()
        .ok_or("Expected reference-domain refusal")?;
    assert!(
        error.contains("reference target does not conform"),
        "{error}"
    );
    assert!(
        !trace
            .calls
            .borrow()
            .iter()
            .any(|call| call == "value:t:backing")
    );
    assert!(trace.calls.borrow().iter().any(|call| call == "get:t"));
    assert!(budget.reads > 0);
    Ok(())
}
