//! Handwritten fixture acceptance, independent of expression evaluation.
use super::fixture::{FixtureBuilder, metamodel};

#[test]
fn full_schema_fixture_builds_one_plus_two_under_two_seconds_when_warm() {
    let _ = metamodel();
    let start = std::time::Instant::now();
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let one = b.int(1);
    let two = b.int(2);
    let sum = b.op("+", &[one, two]);
    let a = b.attribute(p, "a", Some(sum));
    let model = b.build();
    assert!(model.obj(a).is_some());
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn fixture_constructors_and_specializations_validate() {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let ty = b.part_def(p, "T");
    let sub = b.part_def(p, "S");
    b.subclassifies(sub, ty);
    let v = b.real(1.5);
    let a = b.attribute(ty, "a", Some(v));
    let v = b.string("ok");
    let a2 = b.attribute(sub, "a", Some(v));
    b.redefines(a2, a);
    b.part(sub, "part", ty);
    let v = b.bool(true);
    b.attribute(p, "bool", Some(v));
    let v = b.infinity();
    b.attribute(p, "inf", Some(v));
    let v = b.null();
    b.attribute(p, "null", Some(v));
    let v = b.feature_ref("P::T::a");
    b.attribute(p, "ref", Some(v));
    let v = b.int(4);
    let v = b.invoke("DataFunctions::'+'", &[v]);
    b.attribute(p, "invoke", Some(v));
    let v = b.feature_ref("P::T::a");
    let v = b.chain(v, "P::T::a");
    b.attribute(p, "chain", Some(v));
    let v = b.int(1);
    let i = b.int(1);
    let v = b.index(v, i);
    b.attribute(p, "index", Some(v));
    let v = b.int(1);
    let body = b.body(&[], v);
    let seq = b.null();
    let e = b.collect(seq, body);
    b.attribute(p, "collect", Some(e));
    let v = b.bool(true);
    let body = b.body(&[], v);
    let seq = b.null();
    let e = b.select(seq, body);
    b.attribute(p, "select", Some(e));
    let v = b.int(1);
    let v = b.constructor(ty, &[v]);
    b.attribute(p, "ctor", Some(v));
    let v = b.metadata_access("P::T");
    b.attribute(p, "metadata", Some(v));
    let condition = b.bool(true);
    let yes = b.int(1);
    let no = b.int(2);
    let e = b.op("if", &[condition, yes, no]);
    b.attribute(p, "conditional", Some(e));
    let model = b.build();
    assert!(!model.is_empty());
}
