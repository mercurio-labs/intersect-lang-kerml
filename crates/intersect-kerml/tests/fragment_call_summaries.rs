//! Public compiler controls for the transient V70 analysis. Handwritten test
//! source; the frozen native DTO reference is a regression/cross-target witness,
//! not an independent Xtext oracle or a full-language support award.
use intersect_model::definitions::bundle::DefinitionBundle;
use intersect_model::definitions::bundle::DefinitionSource;
use intersect_model::definitions::bundle::SourceKind;
use intersect_text::grammar::GrammarAnalysisWorkBudget;
use intersect_text::grammar::Profile;
use intersect_text::package::BundleCompilation;
use intersect_text::package::CompilationSourcePolicy;
use serde_json::{Value, json};

fn schema(count: usize, mismatch: bool) -> BundleCompilation {
    let mut text = "<e:EPackage xmlns:e='http://www.eclipse.org/emf/2002/Ecore' xmlns:x='http://www.w3.org/2001/XMLSchema-instance' name='summaries' nsURI='urn:fragment-call-summaries-public-v1' nsPrefix='summaries'>".to_owned();
    text.push_str("<eClassifiers x:type='e:EClass' name='Base'/>");
    for index in 0..count {
        let parent = if mismatch && index == 1 {
            ""
        } else {
            " eSuperTypes='#//Base'"
        };
        text.push_str(&format!(
            "<eClassifiers x:type='e:EClass' name='A{index:02}'{parent}/>"
        ));
    }
    text.push_str("</e:EPackage>");
    let ecore = DefinitionSource::new(
        "memory://summaries/metamodel.ecore",
        SourceKind::Ecore,
        text,
    );
    let mut behavior = DefinitionSource::new(
        "memory://summaries/empty.rcore",
        SourceKind::RcoreBehavior,
        "rcore 1; package emptyRules version \"1\";",
    );
    behavior.dependencies = vec![ecore.id.clone()];
    BundleCompilation {
        ecore: ecore.id.clone(),
        materialized_types: vec![],
        xtext: None,
        rcore_behavior: behavior.id.clone(),
        namespace: "urn:fragment-call-summaries-public-v1".into(),
        bundle: DefinitionBundle {
            version: 1,
            sources: vec![ecore, behavior],
            namespaces: vec![],
        },
    }
}
fn source(count: usize, width: usize) -> String {
    let mut source="grammar intersect.fixture.CallSummaries\nimport \"urn:fragment-call-summaries-public-v1\" as m\n".to_owned();
    for index in 0..count {
        source.push_str(&format!("A{index:02} returns m::A{index:02}: F;\n"));
    }
    source.push_str(&format!(
        "fragment F returns m::Base: {};\n",
        vec!["'x'"; width].join(" ")
    ));
    source
}
fn compile(
    source: &str,
    base: &BundleCompilation,
    profile: Profile,
) -> Result<intersect_text::package::Package, Vec<intersect_model::definitions::schema::Diagnostic>>
{
    compile_with_budget(source, base, profile, None)
}
fn compile_with_budget(
    source: &str,
    base: &BundleCompilation,
    profile: Profile,
    budget: Option<GrammarAnalysisWorkBudget>,
) -> Result<intersect_text::package::Package, Vec<intersect_model::definitions::schema::Diagnostic>>
{
    let mut compilation = base.clone();
    let mut grammar = DefinitionSource::new(
        "memory://summaries/language.xtext",
        SourceKind::Xtext,
        source,
    );
    grammar.dependencies = vec![compilation.ecore.clone()];
    compilation.xtext = Some(grammar.id.clone());
    compilation.bundle.sources.push(grammar);
    let targets = [(
        compilation.ecore.clone(),
        "memory://summaries/metamodel.rcore".into(),
    )]
    .into();
    let migration = intersect_model::migration::migrate_ecore_bundle_with_limits(
        compilation.bundle.clone(),
        &targets,
        intersect_model::definitions::ecore::ParseLimits::default(),
    )
    .unwrap();
    let compilation =
        intersect_text::migration::remap_compilation(&migration, &compilation).unwrap();
    let policy = CompilationSourcePolicy::default();
    match budget {
        None => intersect_text::ordinary::compile_with_context(
            compilation,
            profile,
            &[],
            None,
            policy,
            policy,
        ),
        Some(budget) => intersect_text::ordinary::compile_with_context_and_budget(
            compilation,
            profile,
            &[],
            None,
            policy,
            policy,
            budget,
        ),
    }
}
fn success(source: &str, schema: &BundleCompilation, profile: Profile) -> Value {
    match compile(source, schema, profile) {
        Ok(package) => serde_json::to_value(package).unwrap(),
        Err(error) => panic!(
            "Unexpected compiler rejection: {}",
            serde_json::to_string(&error).unwrap()
        ),
    }
}
fn rejection(source: &str, schema: &BundleCompilation, profile: Profile) -> Value {
    rejection_with_budget(source, schema, profile, None)
}
/// The V65-V69 large-source witnesses are historical capacity rejections: they
/// request the historical 10k grant explicitly (the default grant has been
/// REALISTIC since 1e5cd75b).
fn legacy_budget_rejection(source: &str, schema: &BundleCompilation, profile: Profile) -> Value {
    rejection_with_budget(
        source,
        schema,
        profile,
        Some(GrammarAnalysisWorkBudget::LEGACY_10K),
    )
}
fn rejection_with_budget(
    source: &str,
    schema: &BundleCompilation,
    profile: Profile,
    budget: Option<GrammarAnalysisWorkBudget>,
) -> Value {
    match compile_with_budget(source, schema, profile, budget) {
        Ok(_) => panic!("Expected explicit compiler rejection"),
        Err(error) => serde_json::to_value(error).unwrap(),
    }
}
fn reference() -> Value {
    serde_json::from_str(include_str!(
        "../../../fixtures/xtext/fragment-call-summaries-v70/native-reference.json"
    ))
    .unwrap()
}

/// Verification-only capture helper used by a separate native public consumer.
/// The assertion controls below never write files or consult host resources.
pub fn capture_native_public_reference() -> Value {
    let large = source(60, 200);
    let large_schema = schema(60, false);
    let small = source(3, 3);
    let small_schema = schema(3, false);
    let bad = source(2, 1);
    let bad_schema = schema(2, true);
    let profiles = [
        Profile::NativeV65,
        Profile::NativeV66,
        Profile::NativeV67,
        Profile::NativeV68,
        Profile::NativeV69,
    ];
    json!({
        "scope":"native regression DTO/program reference; no independent Java/Xtext oracle and no WASM execution yet",
        "large":{"source":large,"compilation":large_schema,"legacy_errors":profiles.into_iter().map(|profile|json!({"profile":profile,"error":legacy_budget_rejection(&large,&large_schema,profile)})).collect::<Vec<_>>(),"v70":success(&large,&large_schema,Profile::NativeV70)},
        "small":{"source":small,"compilation":small_schema,"v69":success(&small,&small_schema,Profile::NativeV69),"v70":success(&small,&small_schema,Profile::NativeV70)},
        "class_mismatch":{"source":bad,"compilation":bad_schema,"v69":rejection(&bad,&bad_schema,Profile::NativeV69),"v70":rejection(&bad,&bad_schema,Profile::NativeV70)},
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
pub fn public_authored_many_class_summary_witness_matches_complete_native_program() {
    let expected = reference();
    let row = &expected["large"];
    let source = source(60, 200);
    let schema = schema(60, false);
    assert_eq!(json!(source), row["source"]);
    assert_eq!(json!(schema), row["compilation"]);
    for (index, profile) in [
        Profile::NativeV65,
        Profile::NativeV66,
        Profile::NativeV67,
        Profile::NativeV68,
        Profile::NativeV69,
    ]
    .into_iter()
    .enumerate()
    {
        let error = legacy_budget_rejection(&source, &schema, profile);
        assert_eq!(error[0]["message"], "Grammar prediction budget exceeded");
        assert_eq!(error, row["legacy_errors"][index]["error"]);
    }
    let actual = success(&source, &schema, Profile::NativeV70);
    assert_eq!(actual["grammar"]["entry"], "A00");
    assert_eq!(actual["grammar"]["rules"].as_object().unwrap().len(), 61);
    assert_eq!(actual, row["v70"]); // Entire retained grammar and prediction programs.
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
pub fn public_small_source_keeps_entire_legacy_program_except_explicit_profile() {
    let expected = reference();
    let row = &expected["small"];
    let source = source(3, 3);
    let schema = schema(3, false);
    assert_eq!(json!(source), row["source"]);
    assert_eq!(json!(schema), row["compilation"]);
    let old = success(&source, &schema, Profile::NativeV69);
    let new = success(&source, &schema, Profile::NativeV70);
    assert_eq!(old, row["v69"]);
    assert_eq!(new, row["v70"]);
    let mut projected = new;
    projected["grammar"]["profile"] = json!(Profile::NativeV69);
    assert_eq!(projected, old);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
pub fn public_imported_class_mismatch_cannot_repair_authoritative_source() {
    let expected = reference();
    let row = &expected["class_mismatch"];
    let source = source(2, 1);
    let schema = schema(2, true);
    assert_eq!(json!(source), row["source"]);
    assert_eq!(json!(schema), row["compilation"]);
    let old = rejection(&source, &schema, Profile::NativeV69);
    let new = rejection(&source, &schema, Profile::NativeV70);
    assert_eq!(
        old[0]["message"],
        "Cannot add a supertype to an imported class"
    );
    assert_eq!(old[0]["source"], "memory://summaries/language.xtext");
    assert_eq!(old[0]["start"], 135);
    assert_eq!(old[0]["end"], 136);
    assert_eq!(&source.as_bytes()[135..136], b"F");
    assert_eq!(old, row["v69"]);
    assert_eq!(new, row["v70"]);
    assert_eq!(new, old);
    // This public rejection occurs during authored-source validation. The private
    // literal controls separately prove cache hits cannot mask class-pair errors.
}
