//! Handwritten evaluator errors, distinct from unevaluated expression values.

/// Failure to access a model or complete a bounded evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    Schema(String),
    Model { object: String, message: String },
    NotAnExpression(String),
    StepLimit(u64),
    DepthLimit(u32),
    Stopped(u64),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(name) => write!(f, "missing evaluator schema item: {name}"),
            Self::Model { object, message } => write!(f, "{object}: {message}"),
            Self::NotAnExpression(object) => write!(f, "not an expression: {object}"),
            Self::StepLimit(limit) => write!(f, "evaluation step limit exceeded: {limit}"),
            Self::DepthLimit(limit) => write!(f, "evaluation depth limit exceeded: {limit}"),
            Self::Stopped(step) => write!(f, "evaluation stopped by observer at step {step}"),
        }
    }
}

impl std::error::Error for EvalError {}
