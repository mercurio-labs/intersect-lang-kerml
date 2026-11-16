//! Shared native/WASM fixture: the complete pinned original KerML definition
//! sources (`kerml-draft-pinned/`) as a `BundleCompilation`.
use intersect_model::definitions::bundle::{DefinitionBundle, DefinitionSource, SourceKind};
use intersect_text::package::BundleCompilation;

const STRUCTURE: &str = "org.omg.sysml/model/SysML.ecore";
const EXPRESSIONS: &str =
    "org.omg.kerml.expressions.xtext/src/org/omg/kerml/expressions/xtext/KerMLExpressions.xtext";
const KERML: &str = "org.omg.kerml.xtext/src/org/omg/kerml/xtext/KerML.xtext";

fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
pub fn original_inputs() -> Result<BundleCompilation, String> {
    let ecore = DefinitionSource::new(
        STRUCTURE,
        SourceKind::Ecore,
        include_str!("kerml-draft-pinned/SysML.ecore"),
    );
    let mut expressions = DefinitionSource::new(
        EXPRESSIONS,
        SourceKind::Xtext,
        include_str!("kerml-draft-pinned/KerMLExpressions.xtext"),
    );
    let mut grammar = DefinitionSource::new(
        KERML,
        SourceKind::Xtext,
        include_str!("kerml-draft-pinned/KerML.xtext"),
    );
    require(
        ecore.id.sha256 == intersect_kerml::PILOT_SCHEMA_SHA256
            && expressions.id.sha256
                == "7bea42879a913f7613ac5e32779174db01e8fd4fe75694f0845c495aa0191aa8"
            && grammar.id.sha256
                == "cc02be703955603d2daa91f878f299f950195fcd2e3c59563ae17643ba91dfaa",
        "Complete original fixture pins changed",
    )?;
    expressions.dependencies = vec![ecore.id.clone()];
    grammar.dependencies = vec![ecore.id.clone(), expressions.id.clone()];
    let mut behavior = DefinitionSource::new(
        "probe-behavior.rcore",
        SourceKind::RcoreBehavior,
        r#"rcore 1; package emptyRules version "1";"#,
    );
    behavior.dependencies = vec![ecore.id.clone()];
    let input = BundleCompilation {
        ecore: ecore.id.clone(),
        materialized_types: vec![],
        xtext: Some(grammar.id.clone()),
        rcore_behavior: behavior.id.clone(),
        namespace: "https://www.omg.org/spec/SysML/20250201".into(),
        bundle: DefinitionBundle {
            version: 1,
            sources: vec![ecore, expressions, grammar, behavior],
            namespaces: vec![],
        },
    };
    Ok(input)
}
