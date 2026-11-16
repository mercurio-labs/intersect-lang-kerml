//! Handwritten parser-free site ordering and target tests.
use super::fixture::FixtureBuilder;
use intersect_kerml_eval::{Site, SiteKind, sites};

#[test]
fn sites_use_containment_order_and_each_kind_has_its_own_target_rule() {
    let mut b = FixtureBuilder::new();
    let pkg = b.package("P");
    let ty = b.part_def(pkg, "T");
    // Allocate in reverse of containment order to catch object-table traversal.
    let bound = b.int(4);
    let result = b.int(3);
    let value = b.int(2);
    b.attribute(ty, "m", Some(value));
    let calc = b.object("Function");
    b.member(ty, "OwningMembership", calc);
    b.member(calc, "ResultExpressionMembership", result);
    let mult = b.object("MultiplicityRange");
    b.member(ty, "OwningMembership", mult);
    b.member(mult, "OwningMembership", bound);
    let model = b.build();
    assert_eq!(
        sites(&model, &["memory:/eval.kerml"]).unwrap(),
        vec![
            Site {
                expression: value,
                target: Some(ty),
                kind: SiteKind::FeatureValue
            },
            Site {
                expression: result,
                target: Some(calc),
                kind: SiteKind::ResultExpression
            },
            Site {
                expression: bound,
                target: Some(ty),
                kind: SiteKind::MultiplicityBound
            },
        ]
    );
}

#[test]
fn nested_arithmetic_is_one_site_and_package_values_have_no_type_target() {
    let mut b = FixtureBuilder::new();
    let pkg = b.package("P");
    let one = b.int(1);
    let two = b.int(2);
    let three = b.int(3);
    let product = b.op("*", &[two, three]);
    let sum = b.op("+", &[one, product]);
    b.attribute(pkg, "m", Some(sum));
    let model = b.build();
    assert_eq!(
        sites(&model, &["memory:/eval.kerml"]).unwrap(),
        vec![Site {
            expression: sum,
            target: None,
            kind: SiteKind::FeatureValue,
        }]
    );
    assert!(sites(&model, &["memory:/absent"]).unwrap().is_empty());
}

#[test]
fn resources_follow_requested_order_and_library_is_opt_in() {
    let mut b = FixtureBuilder::empty();
    let mut expressions = Vec::new();
    for uri in ["library:/base.kerml", "memory:/a.sysml", "memory:/b.sysml"] {
        let feature = b.object_at("Feature", uri, (0, 1));
        let value = b.object_at("FeatureValue", uri, (0, 1));
        let expr = b.object_at("LiteralInteger", uri, (0, 1));
        b.set(expr, "value", 1.into());
        b.reference(feature, "ownedRelationship", value);
        b.reference(value, "ownedRelatedElement", expr);
        b.reference(value, "memberElement", expr);
        expressions.push(expr);
    }
    let model = b.build();
    let selected = sites(&model, &["memory:/b.sysml", "memory:/a.sysml"]).unwrap();
    assert_eq!(
        selected.iter().map(|s| s.expression).collect::<Vec<_>>(),
        vec![expressions[2], expressions[1]]
    );
    assert_eq!(
        sites(&model, &["library:/base.kerml"]).unwrap()[0].expression,
        expressions[0]
    );
}
