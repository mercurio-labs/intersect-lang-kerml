//! Handwritten Pilot text layout and lossless JSON examples.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{
    Outcome, Reason, Value,
    format::{java_double, outcome_json, pilot_text},
};
#[test]
fn fix_7_java_subnormal_selection_uses_closest_one_or_two_digit_decimal() {
    // Each small value is n * 2^-1074. Decimal candidates are selected by
    // the JDK 19+ length/closest/ties-even rule, not by running a JVM.
    for (bits, expected) in [
        (1, "4.9E-324"),
        (2, "9.9E-324"),
        (3, "1.5E-323"),
        (4, "2.0E-323"),
        (5, "2.5E-323"),
        (6, "3.0E-323"),
        (7, "3.5E-323"),
        (8, "4.0E-323"),
        (9, "4.4E-323"),
        (10, "4.9E-323"),
        (12, "5.9E-323"),
        (20, "9.9E-323"),
        (100, "4.94E-322"),
    ] {
        let x = f64::from_bits(bits);
        assert_eq!(java_double(x), expected);
        assert_eq!(java_double(-x), format!("-{expected}"));
        assert_eq!(expected.parse::<f64>().map(f64::to_bits), Ok(bits));
    }
}
#[test]
fn fix_7_extremes_and_normal_subnormal_boundary_round_trip() {
    for (bits, expected) in [
        (0x000fffffffffffff, "2.225073858507201E-308"),
        (0x0010000000000000, "2.2250738585072014E-308"),
        (0x0010000000000001, "2.225073858507202E-308"),
        (0x0008000000000000, "1.1125369292536007E-308"),
        (0x7feffffffffffffe, "1.7976931348623155E308"),
        (0x7fefffffffffffff, "1.7976931348623157E308"),
    ] {
        assert_eq!(java_double(f64::from_bits(bits)), expected);
        assert_eq!(expected.parse::<f64>().map(f64::to_bits), Ok(bits));
    }
    assert_eq!(java_double(1e23), "1.0E23");
    for bits in 1..=1024 {
        let x = f64::from_bits(bits);
        for value in [x, -x] {
            assert_eq!(
                java_double(value).parse::<f64>().map(f64::to_bits),
                Ok(value.to_bits())
            );
        }
    }
}
#[test]
fn fix_6_element_text_uses_effective_names_through_redefinitions() {
    let mut b = FixtureBuilder::new();
    let p = b.package("Review");
    let t = b.part_def(p, "T");
    let base = b.attribute(t, "m", None);
    let unnamed = b.object("AttributeUsage");
    b.member(t, "FeatureMembership", unnamed);
    b.redefines(unnamed, base);
    let again = b.object("AttributeUsage");
    b.member(t, "FeatureMembership", again);
    b.redefines(again, unnamed);
    let renamed = b.attribute(t, "own", None);
    b.redefines(renamed, again);
    let cyclic = b.object("AttributeUsage");
    let other = b.object("AttributeUsage");
    b.redefines(cyclic, other);
    b.redefines(other, cyclic);
    let short_base = b.attribute(t, "named", None);
    b.set(short_base, "declaredShortName", "N".into());
    let short = b.object("AttributeUsage");
    b.redefines(short, short_base);
    let reference = b.feature_ref_to(again);
    let m = b.build();
    for (e, expected) in [
        (unnamed, "AttributeUsage m"),
        (again, "AttributeUsage m"),
        (renamed, "AttributeUsage own"),
        (cyclic, "AttributeUsage"),
        (short, "AttributeUsage <<N>> named"),
        (reference, "FeatureReferenceExpression m"),
    ] {
        assert_eq!(pilot_text(&m, &Value::Element(e)), expected);
    }
}
#[test]
fn java_double_layout_examples() {
    for (n, text) in [
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (4.0, "4.0"),
        (1e-3, "0.001"),
        (1e-4, "1.0E-4"),
        (1e10, "1.0E10"),
        (6.02e23, "6.02E23"),
        (0.1 + 0.2, "0.30000000000000004"),
        (1e7, "1.0E7"),
        (9999999.0, "9999999.0"),
        (9.99e-4, "9.99E-4"),
        (2.0 / 3.0, "0.6666666666666666"),
        (f64::MAX, "1.7976931348623157E308"),
    ] {
        assert_eq!(java_double(n), text);
    }
}
#[test]
fn pilot_text_scalars_and_model_elements() {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let f = b.attribute(p, "a", None);
    b.set(f, "declaredShortName", "A".into());
    let a = b.int(1);
    let c = b.int(2);
    let op = b.op("+", &[a, c]);
    let reference = b.feature_ref_to(f);
    let m = b.build();
    for (value, text) in [
        (Value::Int(7), "LiteralInteger 7"),
        (Value::Real(2.0 / 3.0), "LiteralRational 0.6666666666666666"),
        (Value::Str("b".into()), "LiteralString b"),
        (Value::Bool(true), "LiteralBoolean true"),
        (Value::Infinity, "LiteralInfinity *"),
        (Value::Element(f), "AttributeUsage <<A>> a"),
        (
            Value::Metaclass(f),
            "MetadataFeature AttributeUsage <<A>> a",
        ),
        (Value::Element(reference), "FeatureReferenceExpression a"),
        (
            Value::Unevaluated {
                at: op,
                reason: Reason::UnknownFunction { invocation: op },
            },
            "OperatorExpression +",
        ),
    ] {
        assert_eq!(pilot_text(&m, &value), text);
    }
}
#[test]
fn json_preserves_real_bits_and_all_value_kinds() {
    let mut b = FixtureBuilder::new();
    let p = b.package("P");
    let e = b.attribute(p, "x", None);
    let m = b.build();
    let values = vec![
        Value::Bool(true),
        Value::Int(i64::MAX),
        Value::Real(-0.0),
        Value::Real(f64::from_bits(0x7ff8000000000001)),
        Value::Str("a\nb".into()),
        Value::Infinity,
        Value::Element(e),
        Value::Metaclass(e),
        Value::Unevaluated {
            at: e,
            reason: Reason::NoValue {
                feature: e,
                frame: Some(p),
            },
        },
    ];
    let json = outcome_json(&m, &Outcome::Values(values));
    assert_eq!(json["values"][1]["value"], i64::MAX);
    assert_eq!(json["values"][2]["bits"], "8000000000000000");
    assert_eq!(json["values"][3]["bits"], "7ff8000000000001");
    assert_eq!(json["values"][3]["text"], "NaN");
    assert_eq!(json["values"][8]["reason"]["reason"], "no-value");
    assert_eq!(json["values"][6]["class"], "AttributeUsage");
    assert_eq!(
        outcome_json(
            &m,
            &Outcome::NotEvaluable(Reason::NoReferent { expression: e })
        )["outcome"],
        "not-evaluable"
    );
}
