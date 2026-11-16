//! Handwritten result values and Pilot-style equality (REF section 2.3).

use crate::ObjId;
use std::rc::Rc;

/// One value in an evaluation result sequence.
#[derive(Clone, Debug)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(Rc<str>),
    Infinity,
    Element(ObjId),
    Unevaluated { at: ObjId, reason: Reason },
    Metaclass(ObjId),
}

/// An absent result list is distinct from an empty KerML sequence.
#[derive(Clone, Debug)]
pub enum Outcome {
    NotEvaluable(Reason),
    Values(Vec<Value>),
}

/// Why evaluation could not reduce an expression or feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    UnknownFunction {
        invocation: ObjId,
    },
    NotModelLevelEvaluable {
        expression: ObjId,
    },
    OperandNotSingle {
        invocation: ObjId,
        operand: usize,
        count: usize,
    },
    OperandKinds {
        invocation: ObjId,
        kinds: Vec<&'static str>,
    },
    OperationUndefined {
        invocation: ObjId,
    },
    NoValue {
        feature: ObjId,
        frame: Option<ObjId>,
    },
    NoReferent {
        expression: ObjId,
    },
    NoResultExpression {
        expression: ObjId,
    },
    Overflow {
        invocation: ObjId,
    },
    BadIndex {
        invocation: ObjId,
    },
    TypeArgumentMissing {
        invocation: ObjId,
    },
    Unsupported {
        element: ObjId,
        what: &'static str,
    },
}

fn identity(value: &Value) -> Option<ObjId> {
    match value {
        Value::Element(id) | Value::Metaclass(id) | Value::Unevaluated { at: id, .. } => Some(*id),
        _ => None,
    }
}

fn double_bits(value: f64) -> u64 {
    if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits()
    }
}

/// Boxed scalar equality; model values compare by variant-independent identity.
pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Real(a), Value::Real(b)) => double_bits(*a) == double_bits(*b),
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Infinity, Value::Infinity) => true,
        _ => matches!((identity(a), identity(b)), (Some(a), Some(b)) if a == b),
    }
}

/// Ordered, pairwise equality, including sequence length.
pub fn equal_seq(a: &[Value], b: &[Value]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_and_real_are_distinct() {
        assert!(!equal(&Value::Int(1), &Value::Real(1.0)));
    }

    #[test]
    fn reals_use_java_double_equality() {
        assert!(equal(&Value::Real(2.0), &Value::Real(2.0)));
        assert!(!equal(&Value::Real(2.0), &Value::Real(3.0)));
        assert!(equal(
            &Value::Real(f64::NAN),
            &Value::Real(f64::from_bits(0xfff0_0000_0000_0001))
        ));
        assert!(!equal(&Value::Real(0.0), &Value::Real(-0.0)));
    }

    #[test]
    fn infinity_equals_itself() {
        assert!(equal(&Value::Infinity, &Value::Infinity));
        assert!(!equal(&Value::Infinity, &Value::Real(f64::INFINITY)));
    }

    #[test]
    fn elements_compare_by_identity_across_variants_and_ignore_reason() -> Result<(), String> {
        use intersect_model::core::frozen::{ById, Metamodel, ModelBuilder};
        use intersect_model::definitions::dynamic::SchemaBuilder;
        use std::sync::Arc;

        let mut schema = SchemaBuilder::new("equality", "urn:intersect:eval:equality");
        schema.class("Node").map_err(|e| e.to_string())?;
        let schema = schema.build().map_err(|e| e.to_string())?;
        let mm = Arc::new(Metamodel::freeze(schema.as_schema()).map_err(|e| e.to_string())?);
        let node = mm.class("Node").ok_or("Node class")?;
        let mut builder = ModelBuilder::new(mm);
        let a = builder
            .create("a", node, "memory:/equality")
            .map_err(|e| e.to_string())?;
        let b = builder
            .create("b", node, "memory:/equality")
            .map_err(|e| e.to_string())?;
        let _model = builder
            .link(&ById)
            .map_err(|e| e.to_string())?
            .validate()
            .map_err(|e| e.to_string())?;
        let variants = [
            Value::Element(a),
            Value::Metaclass(a),
            Value::Unevaluated {
                at: a,
                reason: Reason::NoValue {
                    feature: a,
                    frame: None,
                },
            },
            Value::Unevaluated {
                at: a,
                reason: Reason::UnknownFunction { invocation: a },
            },
        ];
        for left in &variants {
            for right in &variants {
                assert!(equal(left, right));
            }
            assert!(!equal(left, &Value::Element(b)));
            assert!(!equal(left, &Value::Int(1)));
        }
        Ok(())
    }

    #[test]
    fn scalar_values_compare_within_their_kind() {
        assert!(equal(&Value::Bool(true), &Value::Bool(true)));
        assert!(!equal(&Value::Bool(true), &Value::Bool(false)));
        assert!(equal(&Value::Int(7), &Value::Int(7)));
        assert!(!equal(&Value::Int(7), &Value::Int(8)));
        assert!(equal(&Value::Str("a".into()), &Value::Str("a".into())));
        assert!(!equal(&Value::Str("a".into()), &Value::Str("b".into())));
    }

    #[test]
    fn sequences_require_same_length_and_order() {
        assert!(equal_seq(&[], &[]));
        assert!(equal_seq(
            &[Value::Int(1), Value::Int(2)],
            &[Value::Int(1), Value::Int(2)]
        ));
        assert!(!equal_seq(&[Value::Int(1)], &[]));
        assert!(!equal_seq(
            &[Value::Int(1), Value::Int(2)],
            &[Value::Int(2), Value::Int(1)]
        ));
    }
}
