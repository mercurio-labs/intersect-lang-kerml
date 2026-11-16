//! Handwritten parser-free expression fixtures over the cached full SysML schema.
//! Rcore structure compilation is setup only; no KerML/SysML parser or pack is used.
#![allow(dead_code)] // The ordered evaluator tasks progressively consume this API.

use intersect_model::{
    core::frozen::{ById, ClassId, FeatureId, Metamodel, Model, ModelBuilder, ObjId},
    definitions::{
        bundle::{DefinitionBundle, DefinitionSource, SourceKind},
        ecore::ParseLimits,
    },
};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

pub type Expr = ObjId;
const STRUCTURE: &str = include_str!("../../../intersect-kerml/metamodel/SysML.ecore.rcore");
pub fn metamodel() -> Arc<Metamodel> {
    static MM: OnceLock<Arc<Metamodel>> = OnceLock::new();
    MM.get_or_init(|| {
        let source = DefinitionSource::new(
            intersect_kerml::kerml_bindings::STRUCTURE_URI,
            SourceKind::Rcore,
            STRUCTURE,
        );
        let entry = source.id.clone();
        let bundle = DefinitionBundle {
            version: 1,
            sources: vec![source],
            namespaces: vec![],
        };
        let schema = intersect_model::definitions::compiler::compile_structure_bundle_with_limits(
            &bundle,
            &entry,
            "https://www.omg.org/spec/SysML/20250201",
            ParseLimits {
                source_bytes: STRUCTURE.len(),
            },
        )
        .expect("stored SysML structure compiles");
        Arc::new(Metamodel::freeze(&schema).expect("stored SysML schema freezes"))
    })
    .clone()
}

pub struct FixtureBuilder {
    pub model: ModelBuilder,
    classes: BTreeMap<ObjId, ClassId>,
    paths: BTreeMap<ObjId, String>,
    names: BTreeMap<String, ObjId>,
    pending: Vec<(ObjId, String, String)>,
}
impl FixtureBuilder {
    pub fn empty() -> Self {
        Self {
            model: ModelBuilder::new(metamodel()),
            classes: BTreeMap::new(),
            paths: BTreeMap::new(),
            names: BTreeMap::new(),
            pending: vec![],
        }
    }
    pub fn new() -> Self {
        let mut b = Self::empty();
        b.library();
        b
    }
    pub fn class(&self, name: &str) -> ClassId {
        let mm = self.model.metamodel();
        mm.schema()
            .classes
            .iter()
            .find(|(_, c)| c.name == name)
            .and_then(|(key, _)| mm.class(key))
            .expect("fixture metaclass")
    }
    pub fn object(&mut self, name: &str) -> ObjId {
        self.object_at(name, "memory:/eval.kerml", (0, 0))
    }
    pub fn object_at(&mut self, name: &str, uri: &str, span: (usize, usize)) -> ObjId {
        let class = self.class(name);
        let id = format!("fixture:{}", self.classes.len());
        let obj = self
            .model
            .create_at(&id, class, uri, span.0, span.1)
            .expect("fixture object");
        self.classes.insert(obj, class);
        self.set(obj, "elementId", id.into());
        obj
    }
    pub fn field(&self, obj: ObjId, name: &str) -> FeatureId {
        self.classes
            .get(&obj)
            .and_then(|c| self.model.metamodel().feature(*c, name))
            .expect("fixture feature")
    }
    pub fn set(&mut self, obj: ObjId, name: &str, value: serde_json::Value) {
        let field = self.field(obj, name);
        self.model
            .set_attribute(obj, field, value)
            .expect("fixture attribute");
    }
    pub fn enumeration(&mut self, obj: ObjId, field: &str, literal: &str) {
        let mm = self.model.metamodel();
        let f = mm
            .definition(self.field(obj, field))
            .expect("enum definition");
        let value = mm
            .schema()
            .enumerations
            .get(&f.target)
            .expect("enum type")
            .parse(literal)
            .expect("enum literal");
        let value = serde_json::to_value(value).expect("enum JSON");
        self.set(obj, field, value);
    }
    pub fn reference(&mut self, obj: ObjId, name: &str, target: ObjId) {
        let field = self.field(obj, name);
        self.model
            .add_ref(obj, field, target)
            .expect("fixture reference");
    }
    pub fn member(&mut self, owner: ObjId, kind: &str, child: ObjId) -> ObjId {
        let m = self.object(kind);
        self.reference(owner, "ownedRelationship", m);
        self.reference(m, "ownedRelatedElement", child);
        // The validated full schema also requires Membership.memberElement.
        // Operand containment itself is exactly the OperandSettings path.
        self.reference(m, "memberElement", child);
        m
    }
    pub fn name(&mut self, obj: ObjId, owner: Option<ObjId>, name: &str) {
        self.set(obj, "declaredName", name.into());
        let path = owner
            .and_then(|o| self.paths.get(&o))
            .map_or_else(|| name.to_owned(), |p| format!("{p}::{name}"));
        self.paths.insert(obj, path.clone());
        self.names.insert(path, obj);
    }
    pub fn named(&self, path: &str) -> ObjId {
        *self
            .names
            .get(&Self::path(path))
            .expect("named fixture element")
    }
    fn path(path: &str) -> String {
        path.split("::")
            .map(|s| s.trim_matches('\''))
            .collect::<Vec<_>>()
            .join("::")
    }
    pub fn package(&mut self, name: &str) -> ObjId {
        let p = self.object("Package");
        self.name(p, None, name);
        p
    }
    pub fn parameter(&mut self, name: &str) -> ObjId {
        let p = self.object("Feature");
        self.name(p, None, name);
        self.enumeration(p, "direction", "in");
        p
    }
    pub fn function(&mut self, pkg: ObjId, name: &str, params: &[&str]) -> ObjId {
        let f = self.object("Function");
        self.name(f, Some(pkg), name);
        self.member(pkg, "OwningMembership", f);
        for name in params {
            let p = self.parameter(name);
            self.name(p, Some(f), name);
            self.member(f, "ParameterMembership", p);
        }
        let result = self.object("Feature");
        self.name(result, Some(f), "result");
        self.enumeration(result, "direction", "out");
        self.member(f, "ReturnParameterMembership", result);
        f
    }
    pub fn attribute(&mut self, owner: ObjId, name: &str, value: Option<Expr>) -> ObjId {
        let f = self.object("AttributeUsage");
        self.name(f, Some(owner), name);
        let membership = if self.model.metamodel().is_a(
            *self.classes.get(&owner).expect("owner"),
            self.class("Type"),
        ) {
            "FeatureMembership"
        } else {
            "OwningMembership"
        };
        self.member(owner, membership, f);
        if let Some(value) = value {
            self.value(f, value);
        }
        f
    }
    pub fn value(&mut self, feature: ObjId, value: Expr) -> ObjId {
        self.member(feature, "FeatureValue", value)
    }
    pub fn part_def(&mut self, owner: ObjId, name: &str) -> ObjId {
        let p = self.object("PartDefinition");
        self.name(p, Some(owner), name);
        self.member(owner, "OwningMembership", p);
        p
    }
    pub fn part(&mut self, owner: ObjId, name: &str, ty: ObjId) -> ObjId {
        let p = self.object("PartUsage");
        self.name(p, Some(owner), name);
        self.member(owner, "FeatureMembership", p);
        self.typed(p, ty);
        p
    }
    fn specialization(
        &mut self,
        kind: &str,
        specific: ObjId,
        general: ObjId,
        fields: &[(&str, ObjId)],
    ) -> ObjId {
        let r = self.object(kind);
        self.reference(specific, "ownedRelationship", r);
        self.reference(r, "specific", specific);
        self.reference(r, "general", general);
        for (field, target) in fields {
            self.reference(r, field, *target);
        }
        r
    }
    pub fn redefines(&mut self, f: ObjId, g: ObjId) -> ObjId {
        self.specialization(
            "Redefinition",
            f,
            g,
            &[
                ("redefiningFeature", f),
                ("redefinedFeature", g),
                ("subsettingFeature", f),
                ("subsettedFeature", g),
            ],
        )
    }
    pub fn subclassifies(&mut self, a: ObjId, b: ObjId) -> ObjId {
        self.specialization(
            "Subclassification",
            a,
            b,
            &[("subclassifier", a), ("superclassifier", b)],
        )
    }
    pub fn typed(&mut self, f: ObjId, t: ObjId) -> ObjId {
        self.specialization("FeatureTyping", f, t, &[("typedFeature", f), ("type", t)])
    }
    pub fn int(&mut self, n: i64) -> Expr {
        let e = self.object("LiteralInteger");
        self.set(e, "value", n.into());
        e
    }
    pub fn real(&mut self, n: f64) -> Expr {
        let e = self.object("LiteralRational");
        self.set(
            e,
            "value",
            serde_json::json!({"kind":"float64", "bits":format!("{:016x}", n.to_bits())}),
        );
        e
    }
    pub fn string(&mut self, s: &str) -> Expr {
        let e = self.object("LiteralString");
        self.set(e, "value", s.into());
        e
    }
    pub fn bool(&mut self, b: bool) -> Expr {
        let e = self.object("LiteralBoolean");
        self.set(e, "value", b.into());
        e
    }
    pub fn infinity(&mut self) -> Expr {
        self.object("LiteralInfinity")
    }
    pub fn null(&mut self) -> Expr {
        self.object("NullExpression")
    }
    pub fn operand(&mut self, inv: ObjId, value: Expr) -> ObjId {
        let p = self.object("Feature");
        self.enumeration(p, "direction", "in");
        self.value(p, value);
        let m = self.member(inv, "ParameterMembership", p);
        self.enumeration(m, "visibility", "private");
        p
    }
    pub fn op(&mut self, operator: &str, operands: &[Expr]) -> Expr {
        let e = self.object("OperatorExpression");
        self.set(e, "operator", operator.into());
        for (i, value) in operands.iter().enumerate() {
            let lazy = (operator == "if" && i > 0)
                || (matches!(operator, "and" | "or" | "implies" | "??") && i == 1);
            let value = if lazy {
                let body = self.body(&[], *value);
                self.owned_expression_ref(body)
            } else {
                *value
            };
            self.operand(e, value);
        }
        e
    }
    fn referent_to(&mut self, expression: Expr, target: ObjId) {
        let m = self.object("Membership");
        self.reference(expression, "ownedRelationship", m);
        self.reference(m, "memberElement", target);
    }
    fn referent_path(&mut self, expression: Expr, path: &str) {
        let m = self.object("Membership");
        self.reference(expression, "ownedRelationship", m);
        self.pending
            .push((m, "memberElement".into(), Self::path(path)));
    }
    pub fn feature_ref_to(&mut self, target: ObjId) -> Expr {
        let e = self.object("FeatureReferenceExpression");
        self.referent_to(e, target);
        e
    }
    fn owned_expression_ref(&mut self, body: Expr) -> Expr {
        let reference = self.object("FeatureReferenceExpression");
        self.member(reference, "FeatureMembership", body);
        reference
    }
    pub fn feature_ref(&mut self, path: &str) -> Expr {
        let e = self.object("FeatureReferenceExpression");
        self.referent_path(e, path);
        e
    }
    pub fn invoke(&mut self, function_path: &str, args: &[Expr]) -> Expr {
        let e = self.object("InvocationExpression");
        self.referent_path(e, function_path);
        for arg in args {
            self.operand(e, *arg);
        }
        e
    }
    pub fn chain(&mut self, source: Expr, path: &str) -> Expr {
        let e = self.object("FeatureChainExpression");
        self.set(e, "operator", ".".into());
        self.operand(e, source);
        self.referent_path(e, path);
        e
    }
    pub fn index(&mut self, seq: Expr, i: Expr) -> Expr {
        let e = self.object("IndexExpression");
        self.set(e, "operator", "#".into());
        self.operand(e, seq);
        self.operand(e, i);
        e
    }
    fn control(&mut self, class: &str, op: &str, seq: Expr, body: Expr) -> Expr {
        let e = self.object(class);
        self.set(e, "operator", op.into());
        self.operand(e, seq);
        let reference = self.owned_expression_ref(body);
        self.operand(e, reference);
        e
    }
    pub fn collect(&mut self, seq: Expr, body: Expr) -> Expr {
        self.control("CollectExpression", "collect", seq, body)
    }
    pub fn select(&mut self, seq: Expr, body: Expr) -> Expr {
        self.control("SelectExpression", "select", seq, body)
    }
    pub fn body(&mut self, params: &[ObjId], expr: Expr) -> Expr {
        let e = self.object("Expression");
        for p in params {
            self.member(e, "ParameterMembership", *p);
        }
        self.member(e, "ResultExpressionMembership", expr);
        e
    }
    pub fn constructor(&mut self, ty: ObjId, args: &[Expr]) -> Expr {
        let e = self.object("ConstructorExpression");
        self.referent_to(e, ty);
        for arg in args {
            self.operand(e, *arg);
        }
        let result = self.object("Feature");
        self.enumeration(result, "direction", "out");
        self.typed(result, ty);
        self.member(e, "ReturnParameterMembership", result);
        e
    }
    pub fn metadata_access(&mut self, path: &str) -> Expr {
        let e = self.object("MetadataAccessExpression");
        self.referent_path(e, path);
        e
    }
    pub fn type_test(&mut self, op: &str, value: Expr, ty: ObjId) -> Expr {
        let e = self.op(op, &[value]);
        let p = self.object("Feature");
        self.enumeration(p, "direction", "in");
        self.typed(p, ty);
        self.member(e, "ParameterMembership", p);
        e
    }
    pub fn build(mut self) -> Model {
        for (object, field, path) in std::mem::take(&mut self.pending) {
            let target = self.named(&path);
            let field = self.field(object, &field);
            self.model
                .add_proxy(object, field, &format!("fixture:{}", target.index()))
                .expect("fixture proxy");
        }
        self.model
            .link(&ById)
            .expect("fixture links")
            .validate()
            .expect("fixture validates")
    }
    fn datatype(&mut self, package: ObjId, name: &str) -> ObjId {
        let t = self.object("DataType");
        self.name(t, Some(package), name);
        self.member(package, "OwningMembership", t);
        t
    }
    fn library(&mut self) {
        let base = self.package("Base");
        let anything = self.object("Classifier");
        self.name(anything, Some(base), "Anything");
        self.member(base, "OwningMembership", anything);
        self.attribute(anything, "self", None);
        let data = self.datatype(base, "DataValue");
        self.subclassifies(data, anything);
        let scalar = self.package("ScalarValues");
        let sv = self.datatype(scalar, "ScalarValue");
        self.subclassifies(sv, data);
        for name in ["Boolean", "String"] {
            let t = self.datatype(scalar, name);
            self.subclassifies(t, sv);
        }
        let mut parent = sv;
        for name in [
            "NumericalValue",
            "Number",
            "Complex",
            "Real",
            "Rational",
            "Integer",
            "Natural",
            "Positive",
        ] {
            let t = self.datatype(scalar, name);
            self.subclassifies(t, parent);
            parent = t;
        }
        let collections = self.package("Collections");
        let collection = self.datatype(collections, "Collection");
        self.subclassifies(collection, data);
        let elements = self.attribute(collection, "elements", None);
        self.set(elements, "isUnique", false.into());
        let ordered = self.datatype(collections, "OrderedCollection");
        self.subclassifies(ordered, collection);
        let ordered_elements = self.attribute(ordered, "elements", None);
        self.redefines(ordered_elements, elements);
        self.set(ordered_elements, "isOrdered", true.into());
        self.set(ordered_elements, "isUnique", false.into());
        let array = self.datatype(collections, "Array");
        self.subclassifies(array, ordered);
        let dims = self.attribute(array, "dimensions", None);
        let positive = self.named("ScalarValues::Positive");
        self.typed(dims, positive);
        self.set(dims, "isOrdered", true.into());
        self.set(dims, "isUnique", false.into());
        for (package, names) in [
            (
                "BaseFunctions",
                "== != === !== , # istype hastype @ as @@ meta [",
            ),
            (
                "DataFunctions",
                "+ - * / ** ^ % < <= > >= .. not & | xor max min",
            ),
            (
                "ControlFunctions",
                ". if and or implies ?? collect select reduce forAll exists reject selectOne maximize minimize",
            ),
            (
                "SequenceFunctions",
                "size isEmpty notEmpty includes includesOnly excludes equals same including union includingAt excluding intersection excludingAt subsequence head last tail",
            ),
            ("StringFunctions", "Length Substring"),
            ("NumericalFunctions", "sum product"),
            (
                "TrigFunctions",
                "sin cos tan cot arcsin arccos arctan deg rad",
            ),
        ] {
            let p = self.package(package);
            for name in names.split_whitespace() {
                let params: &[&str] = if name == "if" {
                    &["x", "y", "z"]
                } else if matches!(
                    name,
                    "not"
                        | "size"
                        | "isEmpty"
                        | "notEmpty"
                        | "head"
                        | "last"
                        | "tail"
                        | "Length"
                        | "sum"
                        | "product"
                        | "sin"
                        | "cos"
                        | "tan"
                        | "cot"
                        | "arcsin"
                        | "arccos"
                        | "arctan"
                        | "deg"
                        | "rad"
                ) {
                    &["x"]
                } else {
                    &["x", "y"]
                };
                self.function(p, name, params);
            }
            if package == "TrigFunctions" {
                self.attribute(p, "pi", None);
            }
        }
    }
}
