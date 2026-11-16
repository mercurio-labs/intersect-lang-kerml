//! Handwritten equality, collection, index and type-test cases.
use super::{data::check, fixture::FixtureBuilder};
use intersect_kerml_eval::{
    EvalError, EvalMode, Evaluator, ObjId, Outcome, Reason, Value, value::equal_seq,
};
#[test]
fn fix_2_type_target_self_has_its_context_type_for_all_type_operators() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let p = b.package("Review");
    let t = b.part_def(p, "T");
    let other = b.part_def(p, "Other");
    let mut cases = Vec::new();
    for op in ["istype", "hastype", "@", "as"] {
        for ty in [t, other] {
            let this = b.feature_ref("Base::Anything::self");
            cases.push((b.type_test(op, this, ty), op, ty == t));
        }
    }
    let feature = b.part(t, "part", t);
    let this = b.feature_ref("Base::Anything::self");
    let feature_test = b.type_test("hastype", this, t);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, op, matches) in cases {
        let expected = if op == "as" {
            if matches {
                vec![Value::Element(t)]
            } else {
                vec![]
            }
        } else {
            vec![Value::Bool(matches)]
        };
        assert!(matches!(ev.evaluate(e,Some(t))?,Outcome::Values(v) if equal_seq(&v,&expected)));
    }
    assert!(
        matches!(ev.evaluate(feature_test,Some(feature))?,Outcome::Values(v) if equal_seq(&v,&[Value::Bool(true)]))
    );
    Ok(())
}
pub fn seq(b: &mut FixtureBuilder, ns: &[i64]) -> ObjId {
    let mut e = b.null();
    for n in ns {
        let v = b.int(*n);
        e = b.op(",", &[e, v]);
    }
    e
}
#[test]
fn equality_boxed_kinds_null_infinity_and_identity() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let mut cases = Vec::new();
    for op in ["==", "===", "!=", "!=="] {
        let a = b.int(1);
        let c = b.real(1.0);
        cases.push((b.op(op, &[a, c]), matches!(op, "!=" | "!==")));
    }
    let a = b.null();
    let c = b.null();
    cases.push((b.op("==", &[a, c]), true));
    let a = b.infinity();
    let c = b.infinity();
    cases.push((b.op("==", &[a, c]), true));
    let x = b.object("Expression");
    let one = b.int(1);
    cases.push((b.op("==", &[x, one]), false));
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, expected) in cases {
        assert!(
            matches!(ev.evaluate(e,None)?,Outcome::Values(v) if equal_seq(&v,&[Value::Bool(expected)]))
        );
    }
    Ok(())
}
#[test]
fn concat_flattens_nested_lists() -> Result<(), EvalError> {
    check(
        |b| {
            let one = b.int(1);
            let rest = seq(b, &[2, 3]);
            b.op(",", &[one, rest])
        },
        &[Value::Int(1), Value::Int(2), Value::Int(3)],
    )
}
#[test]
fn sequence_index_is_one_based() -> Result<(), EvalError> {
    for (i, expected) in [
        (2, vec![Value::Int(20)]),
        (4, vec![]),
        (0, vec![]),
        (-1, vec![]),
    ] {
        check(
            |b| {
                let s = seq(b, &[10, 20, 30]);
                let i = b.int(i);
                b.index(s, i)
            },
            &expected,
        )?;
    }
    Ok(())
}
pub fn collection(b: &mut FixtureBuilder, kind: &str, ns: &[i64], dims: &[i64]) -> ObjId {
    let object = b.object("Expression");
    let ty = b.named(&format!("Collections::{kind}"));
    b.typed(object, ty);
    let values = seq(b, ns);
    let feature = b.attribute(object, "items", Some(values));
    let base = b.named("Collections::Collection::elements");
    b.redefines(feature, base);
    if kind == "Array" {
        let values = seq(b, dims);
        let feature = b.attribute(object, "shape", Some(values));
        let base = b.named("Collections::Array::dimensions");
        b.redefines(feature, base);
    }
    object
}
#[test]
fn array_index_row_major_and_pilot_nonpositive_quirk() -> Result<(), EvalError> {
    for (indices, expected) in [
        ([2, 1], vec![Value::Int(30)]),
        ([0, 1], vec![]),
        ([1, 0], vec![]),
    ] {
        check(
            |b| {
                let a = collection(b, "Array", &[10, 20, 30, 40], &[2, 2]);
                let i = seq(b, &indices);
                b.index(a, i)
            },
            &expected,
        )?;
    }
    Ok(())
}
#[test]
fn ordered_collection_index_and_collection_equality() -> Result<(), EvalError> {
    check(
        |b| {
            let a = collection(b, "OrderedCollection", &[10, 20, 30], &[]);
            let i = b.int(2);
            b.index(a, i)
        },
        &[Value::Int(20)],
    )?;
    check(
        |b| {
            let a = collection(b, "Collection", &[1, 2], &[]);
            let c = collection(b, "Collection", &[1, 2], &[]);
            b.op("==", &[a, c])
        },
        &[Value::Bool(true)],
    )
}
#[test]
fn type_tests_use_primitive_hierarchy_and_empty_quantifiers() -> Result<(), EvalError> {
    for (op, ty, empty, want) in [
        ("istype", "Real", false, true),
        ("istype", "Natural", false, false),
        ("istype", "Integer", true, true),
        ("@", "Integer", true, false),
        ("hastype", "Real", false, false),
        ("hastype", "Integer", false, true),
    ] {
        check(
            |b| {
                let a = if empty { b.null() } else { b.int(1) };
                let ty = b.named(&format!("ScalarValues::{ty}"));
                b.type_test(op, a, ty)
            },
            &[Value::Bool(want)],
        )?;
    }
    Ok(())
}
#[test]
fn as_filters_in_order() -> Result<(), EvalError> {
    check(
        |b| {
            let a = b.int(1);
            let c = b.string("a");
            let s = b.op(",", &[a, c]);
            let ty = b.named("ScalarValues::Integer");
            b.type_test("as", s, ty)
        },
        &[Value::Int(1)],
    )
}
#[test]
fn index_shape_type_argument_and_comma_arithmetic_reasons() -> Result<(), EvalError> {
    let mut b = FixtureBuilder::new();
    let s = seq(&mut b, &[1, 2]);
    let i = b.string("x");
    let index = b.index(s, i);
    let s = b.int(1);
    let missing = b.op("istype", &[s]);
    let s = seq(&mut b, &[1, 2]);
    let one = b.int(1);
    let sum = b.op("+", &[s, one]);
    let m = b.build();
    let ev = Evaluator::new(&m, EvalMode::ModelLevel)?;
    for (e, want) in [
        (index, Reason::BadIndex { invocation: index }),
        (
            missing,
            Reason::TypeArgumentMissing {
                invocation: missing,
            },
        ),
        (
            sum,
            Reason::OperandNotSingle {
                invocation: sum,
                operand: 0,
                count: 2,
            },
        ),
    ] {
        assert!(
            matches!(ev.evaluate(e,None)?,Outcome::Values(v) if matches!(v.as_slice(),[Value::Unevaluated{at,reason}] if *at==e && *reason==want))
        );
    }
    Ok(())
}
