//! Handwritten library-name index and pinned operator precedence by package.
#![allow(dead_code)] // Consumed by the ordered M1 dispatch task.
use crate::{EvalError, Model, ObjId, nav::Nav, schema::EvalSchema};
use std::collections::BTreeMap;

pub(crate) struct LibraryIndex {
    names: BTreeMap<String, ObjId>,
    objects: BTreeMap<ObjId, String>,
}
impl LibraryIndex {
    pub fn new(model: &Model, schema: &EvalSchema) -> Result<Self, EvalError> {
        let nav = Nav::new(model, schema);
        let mut names = BTreeMap::new();
        let mut objects = BTreeMap::new();
        for object in model.objects() {
            if let Some(name) = nav.qualified_name(object.id())? {
                let root = name.split("::").next();
                if matches!(
                    root,
                    Some(
                        "BaseFunctions"
                            | "DataFunctions"
                            | "ControlFunctions"
                            | "SequenceFunctions"
                            | "StringFunctions"
                            | "NumericalFunctions"
                            | "TrigFunctions"
                            | "Base"
                            | "Metaobjects"
                            | "Collections"
                            | "ScalarValues"
                            | "KerML"
                            | "SysML"
                    )
                ) {
                    names.entry(name.clone()).or_insert(object.id());
                    objects.insert(object.id(), name);
                }
            }
        }
        Ok(Self { names, objects })
    }
    pub fn get(&self, name: &str) -> Option<ObjId> {
        self.names.get(name).copied()
    }
    pub fn name(&self, object: ObjId) -> Option<&str> {
        self.objects.get(&object).map(String::as_str)
    }
}

/// Manually transcribed from the pinned Base/Data/ControlFunctions.kerml.
/// Base wins over Data for equality; alphabetic names are unquoted.
pub(crate) const OPERATORS: &[(&str, &str)] = &[
    ("==", "BaseFunctions::'=='"),
    ("!=", "BaseFunctions::'!='"),
    ("===", "BaseFunctions::'==='"),
    ("!==", "BaseFunctions::'!=='"),
    (",", "BaseFunctions::','"),
    ("#", "BaseFunctions::'#'"),
    ("[", "BaseFunctions::'['"),
    ("istype", "BaseFunctions::istype"),
    ("hastype", "BaseFunctions::hastype"),
    ("@", "BaseFunctions::'@'"),
    ("@@", "BaseFunctions::'@@'"),
    ("as", "BaseFunctions::as"),
    ("meta", "BaseFunctions::meta"),
    ("all", "BaseFunctions::all"),
    ("+", "DataFunctions::'+'"),
    ("-", "DataFunctions::'-'"),
    ("*", "DataFunctions::'*'"),
    ("/", "DataFunctions::'/'"),
    ("**", "DataFunctions::'**'"),
    ("^", "DataFunctions::'^'"),
    ("%", "DataFunctions::'%'"),
    ("<", "DataFunctions::'<'"),
    ("<=", "DataFunctions::'<='"),
    (">", "DataFunctions::'>'"),
    (">=", "DataFunctions::'>='"),
    ("..", "DataFunctions::'..'"),
    ("not", "DataFunctions::not"),
    ("&", "DataFunctions::'&'"),
    ("|", "DataFunctions::'|'"),
    ("xor", "DataFunctions::xor"),
    ("~", "DataFunctions::'~'"),
    (".", "ControlFunctions::'.'"),
    ("if", "ControlFunctions::if"),
    ("and", "ControlFunctions::and"),
    ("or", "ControlFunctions::or"),
    ("implies", "ControlFunctions::implies"),
    ("??", "ControlFunctions::'??'"),
    ("collect", "ControlFunctions::collect"),
    ("select", "ControlFunctions::select"),
];
pub(crate) fn operator_function(op: &str) -> Option<&'static str> {
    OPERATORS
        .iter()
        .find(|(name, _)| *name == op)
        .map(|(_, function)| *function)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_handles_missing_library_and_exact_function_names() -> Result<(), EvalError> {
        let b = crate::test_fixture::FixtureBuilder::new();
        let plus = b.named("DataFunctions::+");
        let model = b.build();
        let schema = EvalSchema::new(model.metamodel())?;
        let index = LibraryIndex::new(&model, &schema)?;
        assert_eq!(index.get("DataFunctions::'+'"), Some(plus));
        assert_eq!(index.name(plus), Some("DataFunctions::'+'"));
        assert_eq!(index.get("ScalarFunctions::'+'"), None);
        assert_eq!(operator_function("=="), Some("BaseFunctions::'=='"));
        assert_eq!(operator_function("not"), Some("DataFunctions::not"));
        assert_eq!(operator_function("unregistered"), None);
        let empty = crate::test_fixture::FixtureBuilder::empty().build();
        let schema = EvalSchema::new(empty.metamodel())?;
        assert!(
            LibraryIndex::new(&empty, &schema)?
                .get("DataFunctions::'+'")
                .is_none()
        );
        Ok(())
    }
    #[test]
    #[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
    fn text_operator_table_resolves_with_pinned_package_precedence() -> Result<(), String> {
        let Some(model) = crate::test_text_fixture::load("package OperatorProbe {}")? else {
            return Ok(());
        };
        let schema = EvalSchema::new(model.metamodel()).map_err(|e| e.to_string())?;
        let index = LibraryIndex::new(&model, &schema).map_err(|e| e.to_string())?;
        for (op, name) in OPERATORS {
            assert!(index.get(name).is_some(), "missing {name}");
            for package in ["BaseFunctions", "DataFunctions", "ControlFunctions"] {
                if name.starts_with(&format!("{package}::")) {
                    break;
                }
                let earlier = format!("{package}::{}", crate::nav::escape(op));
                assert!(index.get(&earlier).is_none(), "{earlier} precedes {name}");
            }
        }
        Ok(())
    }
}
