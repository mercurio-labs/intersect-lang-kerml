//! Handwritten evaluator integration suite; cases are path modules.

#[path = "base.rs"]
mod base;
#[path = "conditions.rs"]
mod conditions;
#[path = "control.rs"]
mod control;
#[path = "core.rs"]
mod core;
#[path = "data.rs"]
mod data;
#[path = "evaluable.rs"]
mod evaluable;
#[path = "extended.rs"]
mod extended;
#[path = "extended_functions.rs"]
mod extended_functions;
#[path = "features.rs"]
mod features;
#[path = "support/fixture.rs"]
mod fixture;
#[path = "fixtures.rs"]
mod fixtures;
#[path = "format.rs"]
mod format;
#[path = "metadata.rs"]
mod metadata;
#[path = "sites.rs"]
mod sites;
#[path = "text.rs"]
mod text;
#[path = "trace.rs"]
mod trace;
