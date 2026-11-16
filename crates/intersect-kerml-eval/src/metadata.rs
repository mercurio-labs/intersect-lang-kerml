//! Handwritten metadata navigation over stored annotations (REF 1.4/1.6).
//! Reflected enum values are explicitly unsupported (approved representation limit).
use crate::{
    EvalError, Evaluator, ObjId, Outcome, Reason, Value,
    context::{Context, Frame},
    eval::unevaluated,
    nav::Nav,
};
use intersect_model::core::frozen::ValueRef;

impl Nav<'_> {
    pub(crate) fn annotated_elements(&self, metadata: ObjId) -> Result<Vec<ObjId>, EvalError> {
        if !self.is(metadata, "AnnotatingElement")? {
            return Err(self.error(metadata, "expected an AnnotatingElement"));
        }
        let mut annotations = vec![];
        // Pinned Pilot quirk (PilotEvalMetadataTest): annotation's inverse
        // owningAnnotatingRelationship uses the Relationship container
        // delegate. It omits a containing Annotation for a MetadataFeature.
        // Match getAnnotation(), rather than inferring that inverse here.
        for relationship in self.relationships(metadata)? {
            if self.is(relationship, "Annotation")?
                && !self
                    .refs(relationship, "Annotation", "annotatedElement")?
                    .contains(&metadata)
            {
                annotations.push(relationship);
            }
        }
        if annotations.is_empty() {
            // Without an Annotation, an owned metadata member implicitly
            // annotates its owning namespace. Detached metadata annotates none.
            if let Some((membership, _)) = self.object(metadata)?.container()
                && self.is(membership.id(), "OwningMembership")?
                && let Some((namespace, _)) = membership.container()
                && self.is(namespace.id(), "Namespace")?
            {
                return Ok(vec![namespace.id()]);
            }
            return Ok(vec![]);
        }
        let mut elements = vec![];
        for annotation in annotations {
            elements.extend(self.refs(annotation, "Annotation", "annotatedElement")?);
        }
        Ok(elements)
    }

    pub(crate) fn metadata_features(&self, element: ObjId) -> Result<Vec<ObjId>, EvalError> {
        let relationships = self.relationships(element)?;
        let mut metadata = vec![];
        // Pilot puts owned annotations before metadata owned as members, even
        // when their physical containment relationships are interleaved.
        for relationship in &relationships {
            if self.is(*relationship, "Annotation")?
                && self
                    .refs(*relationship, "Annotation", "annotatedElement")?
                    .contains(&element)
            {
                for child in self.owned(*relationship)? {
                    if self.is(child, "AnnotatingElement")? {
                        if self.is(child, "MetadataFeature")? {
                            metadata.push(child);
                        }
                        break;
                    }
                }
            }
        }
        if self.is(element, "Namespace")? {
            for relationship in relationships {
                if self.is(relationship, "OwningMembership")? {
                    for child in self.owned(relationship)? {
                        if self.is(child, "MetadataFeature")?
                            && self.annotated_elements(child)?.contains(&element)
                        {
                            metadata.push(child);
                        }
                    }
                }
            }
        }
        Ok(metadata)
    }
}

impl Evaluator<'_> {
    pub(crate) fn metaclass_context<'o>(&self, element: ObjId, cx: &Context<'o>) -> Context<'o> {
        cx.pushed(Frame::Metaclass {
            element,
            self_feature: self.library.get("Base::Anything::self"),
        })
    }
    pub(crate) fn is_annotated_element(&self, feature: ObjId) -> Result<bool, EvalError> {
        match self
            .library
            .get("Metaobjects::Metaobject::annotatedElement")
        {
            Some(base) => self.nav.specializes(feature, base),
            None => Ok(false),
        }
    }
    pub(crate) fn metaclass_feature(
        &self,
        feature: ObjId,
        element: ObjId,
    ) -> Result<Option<Outcome>, EvalError> {
        if self.nav.is(feature, "Expression")? {
            return Ok(None);
        }
        let Some(name) = self.nav.string(feature, "Element", "declaredName")? else {
            return Ok(None);
        };
        let object = self.nav.object(element)?;
        let mm = self.nav.model.metamodel();
        let Some(field) = mm.feature(object.class(), name) else {
            return Ok(None);
        };
        let info = mm.feature_info(field).ok_or_else(|| {
            self.nav
                .error(element, "reflective field information is absent")
        })?;
        let definition = mm.definition(field).ok_or_else(|| {
            self.nav
                .error(element, "reflective field definition is absent")
        })?;
        if info.derived {
            return Ok(Some(unevaluated(
                feature,
                Reason::Unsupported {
                    element,
                    what: "derived metaclass feature",
                },
            )));
        }
        let enum_field = mm.schema().enumerations.contains_key(&definition.target);
        let mut values = vec![];
        let mut pending: Vec<_> = object.get(field).into_iter().collect();
        while let Some(value) = pending.pop() {
            if enum_field
                && !matches!(
                    value,
                    ValueRef::Null
                        | ValueRef::Many(_)
                        | ValueRef::Default(serde_json::Value::Null | serde_json::Value::Array(_))
                        | ValueRef::Json(serde_json::Value::Null | serde_json::Value::Array(_))
                )
            {
                // A present enum becomes [Java null] in Pilot. An unset enum
                // instead becomes [], so flatten nulls/collections normally.
                return Ok(Some(unevaluated(
                    feature,
                    Reason::Unsupported {
                        element,
                        what: "enum reflection produces a null item in the Pilot",
                    },
                )));
            }
            match value {
                ValueRef::Null => {}
                ValueRef::Bool(v) => values.push(Value::Bool(v)),
                ValueRef::Int(v) => values.push(Value::Int(v)),
                ValueRef::Str(v) => values.push(Value::Str(v.into())),
                ValueRef::Ref(v) => {
                    self.nav.object(v)?;
                    values.push(Value::Metaclass(v));
                }
                ValueRef::Many(items) => {
                    let items: Vec<_> = items.iter().collect();
                    pending.extend(items.into_iter().rev());
                }
                ValueRef::Default(v) | ValueRef::Json(v) => match v {
                    serde_json::Value::Null => {}
                    serde_json::Value::Bool(v) => values.push(Value::Bool(*v)),
                    serde_json::Value::String(v) => values.push(Value::Str(v.as_str().into())),
                    serde_json::Value::Number(v) => {
                        let n = v.as_i64().ok_or_else(|| {
                            self.nav
                                .error(element, "reflective integer is not representable as i64")
                        })?;
                        values.push(Value::Int(n));
                    }
                    serde_json::Value::Array(items) => {
                        pending.extend(items.iter().rev().map(ValueRef::Json))
                    }
                    serde_json::Value::Object(_)
                        if v.get("kind").and_then(|v| v.as_str()) == Some("float64") =>
                    {
                        let bits = v
                            .get("bits")
                            .and_then(|v| v.as_str())
                            .filter(|s| s.len() == 16)
                            .and_then(|s| u64::from_str_radix(s, 16).ok())
                            .ok_or_else(|| {
                                self.nav
                                    .error(element, "invalid reflective float64 encoding")
                            })?;
                        values.push(Value::Real(f64::from_bits(bits)));
                    }
                    _ => {
                        return Err(self
                            .nav
                            .error(element, "unsupported reflective attribute encoding"));
                    }
                },
            }
        }
        Ok(Some(Outcome::Values(values)))
    }
    pub(crate) fn metadata_values(&self, element: ObjId) -> Result<Vec<Value>, EvalError> {
        let mut values: Vec<_> = self
            .nav
            .metadata_features(element)?
            .into_iter()
            .map(Value::Element)
            .collect();
        values.push(Value::Metaclass(element));
        Ok(values)
    }
    pub(crate) fn metaclass_type(&self, element: ObjId) -> Result<Option<ObjId>, EvalError> {
        let object = self.nav.object(element)?;
        let definition = self
            .nav
            .model
            .metamodel()
            .schema()
            .classes
            .get(object.class_key())
            .ok_or_else(|| {
                self.nav
                    .error(element, "metaclass schema definition is absent")
            })?;
        // Exact pinned ElementUtil.getMetaclassOf search order. The public
        // SysML/KerML imports expose shorter spellings in text, but stored
        // declarations live in these nested packages.
        for package in [
            "KerML::Root",
            "KerML::Core",
            "KerML::Kernel",
            "SysML::Systems",
        ] {
            if let Some(ty) = self.library.get(&format!(
                "{package}::{}",
                crate::nav::escape(&definition.name)
            )) && self.nav.is(ty, "Metaclass")?
            {
                return Ok(Some(ty));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EvalMode, test_fixture::FixtureBuilder, value::equal_seq};

    fn annotation(b: &mut FixtureBuilder, owner: ObjId, target: ObjId, child: Option<ObjId>) {
        let annotation = b.object("Annotation");
        b.reference(owner, "ownedRelationship", annotation);
        b.reference(annotation, "annotatedElement", target);
        if let Some(child) = child {
            b.reference(annotation, "ownedRelatedElement", child);
        }
    }
    #[test]
    fn annotations_precede_owned_metadata_members_and_metaclass_is_last() -> Result<(), EvalError> {
        let mut b = FixtureBuilder::new();
        let p = b.package("P");
        let other = b.package("Other");
        let member = b.object("MetadataUsage");
        b.member(p, "OwningMembership", member);
        let external = b.object("MetadataUsage");
        b.member(p, "OwningMembership", external);
        annotation(&mut b, external, other, None);
        let first = b.object("MetadataFeature");
        annotation(&mut b, p, p, Some(first));
        let second = b.object("MetadataUsage");
        annotation(&mut b, p, p, Some(second));
        let comment = b.object("Comment");
        b.set(comment, "body", "text".into());
        annotation(&mut b, p, p, Some(comment));
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
        assert!(equal_seq(
            &ev.metadata_values(p)?,
            &[
                Value::Element(first),
                Value::Element(second),
                Value::Element(member),
                Value::Metaclass(p)
            ]
        ));
        assert!(ev.nav.annotated_elements(first)?.is_empty());
        assert_eq!(ev.nav.annotated_elements(member)?, [p]);
        assert_eq!(ev.nav.annotated_elements(external)?, [other]);
        Ok(())
    }
    #[test]
    fn metadata_targets_omit_containing_annotations_but_include_owned_annotations()
    -> Result<(), EvalError> {
        let mut b = FixtureBuilder::new();
        let p = b.package("P");
        let q = b.package("Q");
        let metadata = b.object("MetadataFeature");
        annotation(&mut b, p, p, Some(metadata));
        annotation(&mut b, metadata, q, None);
        let detached = b.object("MetadataFeature");
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
        assert_eq!(ev.nav.annotated_elements(metadata)?, [q]);
        assert!(ev.nav.annotated_elements(detached)?.is_empty());
        assert!(ev.nav.annotated_elements(p).is_err());
        Ok(())
    }
    #[test]
    fn library_metaclass_lookup_uses_the_objects_concrete_class() -> Result<(), EvalError> {
        let mut b = FixtureBuilder::new();
        let sysml = b.package("SysML");
        let kerml = b.package("KerML");
        let systems = b.object("Package");
        b.name(systems, Some(sysml), "Systems");
        b.member(sysml, "OwningMembership", systems);
        let root = b.object("Package");
        b.name(root, Some(kerml), "Root");
        b.member(kerml, "OwningMembership", root);
        let part = b.object("Metaclass");
        b.name(part, Some(systems), "PartDefinition");
        b.member(systems, "OwningMembership", part);
        let package = b.object("Metaclass");
        b.name(package, Some(root), "Package");
        b.member(root, "OwningMembership", package);
        let p = b.package("P");
        let t = b.part_def(p, "T");
        let literal = b.int(1);
        let model = b.build();
        let ev = Evaluator::new(&model, EvalMode::ModelLevel)?;
        assert_eq!(ev.metaclass_type(t)?, Some(part));
        assert_eq!(ev.metaclass_type(p)?, Some(package));
        assert_eq!(ev.metaclass_type(literal)?, None);
        Ok(())
    }
}
