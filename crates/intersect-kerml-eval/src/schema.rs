//! Handwritten schema resolution, once per evaluator (never per expression).
#![allow(dead_code)] // Used progressively by the ordered M1 evaluator tasks.
use crate::EvalError;
use intersect_model::core::frozen::{ClassId, FeatureId, Metamodel};
use std::collections::BTreeMap;

pub(crate) const CLASSES: &[&str] = &[
    "Element",
    "Namespace",
    "Package",
    "Membership",
    "OwningMembership",
    "FeatureMembership",
    "ParameterMembership",
    "ReturnParameterMembership",
    "ResultExpressionMembership",
    "Relationship",
    "Type",
    "Classifier",
    "Feature",
    "FeatureValue",
    "FeatureTyping",
    "Specialization",
    "Subsetting",
    "Redefinition",
    "Subclassification",
    "Conjugation",
    "FeatureChaining",
    "TypeFeaturing",
    "Expression",
    "Function",
    "BooleanExpression",
    "Predicate",
    "InstantiationExpression",
    "InvocationExpression",
    "OperatorExpression",
    "FeatureChainExpression",
    "IndexExpression",
    "SelectExpression",
    "CollectExpression",
    "FeatureReferenceExpression",
    "MetadataAccessExpression",
    "ConstructorExpression",
    "NullExpression",
    "LiteralExpression",
    "LiteralBoolean",
    "LiteralInteger",
    "LiteralRational",
    "LiteralString",
    "LiteralInfinity",
    "MetadataFeature",
    "Metaclass",
    "MultiplicityRange",
    "CalculationUsage",
    "ConstraintUsage",
    "TriggerInvocationExpression",
    "AnnotatingElement",
    "Annotation",
];
pub(crate) const FIELDS: &[(&str, &str)] = &[
    ("Element", "ownedRelationship"),
    ("Element", "declaredName"),
    ("Element", "declaredShortName"),
    ("Relationship", "ownedRelatedElement"),
    ("Membership", "memberElement"),
    ("OperatorExpression", "operator"),
    ("Feature", "direction"),
    ("LiteralBoolean", "value"),
    ("LiteralInteger", "value"),
    ("LiteralRational", "value"),
    ("LiteralString", "value"),
    ("FeatureTyping", "type"),
    ("Specialization", "general"),
    ("Specialization", "specific"),
    ("Subsetting", "subsettedFeature"),
    ("Redefinition", "redefinedFeature"),
    ("Subclassification", "superclassifier"),
    ("Conjugation", "originalType"),
    ("FeatureChaining", "chainingFeature"),
    ("TypeFeaturing", "featuringType"),
    ("AnnotatingElement", "annotatedElement"),
    ("Annotation", "annotatedElement"),
];

/// Every field except `AnnotatingElement.annotatedElement` is stored.
/// The latter is resolved for schema admission but never read by M1.
/// Derived operand/argument/referent/value/result/type collections are not read;
/// `nav` reconstructs them from the stored relationships above.
#[derive(Clone)]
pub(crate) struct EvalSchema {
    classes: BTreeMap<&'static str, ClassId>,
    fields: BTreeMap<(&'static str, &'static str), FeatureId>,
    pub directions: Vec<(&'static str, serde_json::Value)>,
}
impl EvalSchema {
    pub fn new(mm: &Metamodel) -> Result<Self, EvalError> {
        let mut classes = BTreeMap::new();
        for name in CLASSES {
            let id = mm
                .schema()
                .classes
                .iter()
                .find(|(_, c)| c.name == *name)
                .and_then(|(key, _)| mm.class(key))
                .ok_or_else(|| EvalError::Schema((*name).into()))?;
            classes.insert(*name, id);
        }
        let mut fields = BTreeMap::new();
        for (class, name) in FIELDS {
            let id = classes
                .get(class)
                .and_then(|c| mm.feature(*c, name))
                .ok_or_else(|| EvalError::Schema(format!("{class}.{name}")))?;
            fields.insert((*class, *name), id);
        }
        let direction = fields
            .get(&("Feature", "direction"))
            .and_then(|f| mm.definition(*f))
            .and_then(|f| mm.schema().enumerations.get(&f.target))
            .ok_or_else(|| EvalError::Schema("Feature.direction enum".into()))?;
        let mut directions = vec![];
        for name in ["in", "out", "inout"] {
            let value = direction.parse(name).map_err(EvalError::Schema)?;
            directions.push((
                name,
                serde_json::to_value(value).map_err(|e| EvalError::Schema(e.to_string()))?,
            ));
        }
        Ok(Self {
            classes,
            fields,
            directions,
        })
    }
    pub fn class(&self, name: &str) -> Result<ClassId, EvalError> {
        self.classes
            .get(name)
            .copied()
            .ok_or_else(|| EvalError::Schema(name.into()))
    }
    pub fn field(&self, class: &'static str, name: &'static str) -> Result<FeatureId, EvalError> {
        self.fields
            .get(&(class, name))
            .copied()
            .ok_or_else(|| EvalError::Schema(format!("{class}.{name}")))
    }
}
