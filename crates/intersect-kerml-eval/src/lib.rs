//! Handwritten KerML expression evaluation over the frozen model.
//!
//! Implementation scope: `docs/kerml-expression-evaluation-plan.md`.
//! Behavioral reference: `docs/pilot-expression-evaluator-reference.md`.
//! Evaluated results and independently verified Pilot agreement are separate claims.

pub mod analysis_case;
pub mod analysis_execution;
mod condition;
mod context;
mod error;
mod eval;
mod evaluable;
pub mod format;
mod functions;
mod library;
mod metadata;
mod nav;
mod schema;
mod sites;
mod text;
mod trace;
pub mod value;

#[cfg(test)]
#[path = "../tests/support/fixture.rs"]
mod test_fixture;
#[cfg(test)]
#[path = "../tests/support/text_fixture.rs"]
mod test_text_fixture;

pub use error::EvalError;
pub use eval::{EvalLimits, EvalMode, Evaluator};
pub use intersect_model::core::frozen::{Model, ObjId};
pub use sites::{Site, SiteKind, sites};
pub use text::{eval_source, wrapper_expression};
pub use trace::{
    ContextView, Control, EvalObserver, FrameView, Lookup, SourceMap, Trace, TraceEvent, trace_text,
};
pub use value::{Outcome, Reason, Value};
