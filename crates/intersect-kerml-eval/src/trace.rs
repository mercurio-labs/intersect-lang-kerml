//! Handwritten observer events and deterministic traces; source text is host supplied.
use crate::{
    Model, ObjId, Outcome, Value,
    context::{Context, Frame},
    format::{identity, pilot_text, reason_text, values_text},
};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Continue,
    Stop,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Value(ObjId),
    Unbound(ObjId),
    Bound,
    NotHere,
}
#[derive(Clone, Debug)]
pub enum FrameView {
    Type {
        ty: ObjId,
    },
    Arguments {
        expression: ObjId,
        bound: Vec<(ObjId, Vec<Value>)>,
    },
}
impl From<&Frame> for FrameView {
    fn from(f: &Frame) -> Self {
        match f {
            Frame::Type(ty) => Self::Type { ty: *ty },
            Frame::Arguments { expression, bound } => Self::Arguments {
                expression: *expression,
                bound: bound.clone(),
            },
            Frame::Call {
                invocation, bound, ..
            } => Self::Arguments {
                expression: *invocation,
                bound: bound.clone(),
            },
            Frame::Metaclass {
                element,
                self_feature,
            } => Self::Arguments {
                expression: *element,
                bound: self_feature
                    .iter()
                    .map(|feature| (*feature, vec![Value::Metaclass(*element)]))
                    .collect(),
            },
        }
    }
}
pub struct ContextView<'c> {
    frames: &'c [Frame],
    counter: Rc<Cell<u64>>,
}
impl<'c> ContextView<'c> {
    pub(crate) fn new(cx: &'c Context<'_>) -> Self {
        Self {
            frames: &cx.frames,
            counter: cx.run.steps.clone(),
        }
    }
    pub fn frames(&self) -> impl Iterator<Item = FrameView> + '_ {
        self.frames.iter().map(FrameView::from)
    }
}
#[allow(unused_variables)]
pub trait EvalObserver {
    fn enter(
        &mut self,
        step: u64,
        depth: u32,
        expression: ObjId,
        context: &ContextView<'_>,
    ) -> Control {
        Control::Continue
    }
    fn exit(&mut self, step: u64, expression: ObjId, outcome: &Outcome) {}
    fn apply(&mut self, invocation: ObjId, function: &'static str, arguments: &[Option<&Outcome>]) {
    }
    fn skipped(&mut self, invocation: ObjId, operand: usize) {}
    fn lookup(&mut self, feature: ObjId, frame: &FrameView, found: Lookup) {}
}
#[derive(Clone, Debug)]
pub enum TraceEvent {
    Enter {
        step: u64,
        depth: u32,
        expression: ObjId,
        context: Vec<FrameView>,
    },
    Exit {
        step: u64,
        depth: u32,
        expression: ObjId,
        outcome: Outcome,
    },
    Apply {
        step: u64,
        depth: u32,
        invocation: ObjId,
        function: &'static str,
        arguments: Vec<Option<Outcome>>,
    },
    Skipped {
        step: u64,
        depth: u32,
        invocation: ObjId,
        operand: usize,
    },
    Lookup {
        step: u64,
        depth: u32,
        feature: ObjId,
        frame: FrameView,
        found: Lookup,
    },
}
#[derive(Default)]
pub struct Trace {
    pub events: Vec<TraceEvent>,
    sources: BTreeMap<String, String>,
    stack: Vec<(ObjId, u32)>,
    counter: Option<Rc<Cell<u64>>>,
}
impl Trace {
    /// Host-provided original source (including any expression wrapper).
    /// No file or network access occurs during tracing or rendering.
    pub fn with_source(mut self, uri: impl Into<String>, text: impl Into<String>) -> Self {
        self.sources.insert(uri.into(), text.into());
        self
    }
    fn position(&self) -> (u64, u32) {
        (
            self.counter.as_ref().map_or(0, |c| c.get()),
            self.stack.last().map_or(0, |(_, d)| *d),
        )
    }
}
impl EvalObserver for Trace {
    fn enter(
        &mut self,
        step: u64,
        depth: u32,
        expression: ObjId,
        context: &ContextView<'_>,
    ) -> Control {
        self.counter = Some(context.counter.clone());
        // A stopped/failed run has no outcome to exit with. A new root starts
        // a new stack while retaining the recorded events for inspection.
        if depth == 0 {
            self.stack.clear();
        }
        self.stack.push((expression, depth));
        self.events.push(TraceEvent::Enter {
            step,
            depth,
            expression,
            context: context.frames().collect(),
        });
        Control::Continue
    }
    fn exit(&mut self, step: u64, expression: ObjId, outcome: &Outcome) {
        let (_, depth) = self.position();
        self.events.push(TraceEvent::Exit {
            step,
            depth,
            expression,
            outcome: outcome.clone(),
        });
        self.stack.pop();
    }
    fn apply(&mut self, invocation: ObjId, function: &'static str, arguments: &[Option<&Outcome>]) {
        let (step, depth) = self.position();
        self.events.push(TraceEvent::Apply {
            step,
            depth,
            invocation,
            function,
            arguments: arguments.iter().map(|a| a.cloned()).collect(),
        });
    }
    fn skipped(&mut self, invocation: ObjId, operand: usize) {
        let (step, depth) = self.position();
        self.events.push(TraceEvent::Skipped {
            step,
            depth,
            invocation,
            operand,
        });
    }
    fn lookup(&mut self, feature: ObjId, frame: &FrameView, found: Lookup) {
        let (step, depth) = self.position();
        self.events.push(TraceEvent::Lookup {
            step,
            depth,
            feature,
            frame: frame.clone(),
            found,
        });
    }
}
/// Span-only wrapper mapping needed by trace rendering. Creating/loading a
/// wrapper is M2.2 and is intentionally not implemented here.
pub struct SourceMap {
    pub uri: String,
    pub prefix_bytes: usize,
    pub expression_len: usize,
}
impl SourceMap {
    pub fn to_user(&self, span: (usize, usize)) -> Option<(usize, usize)> {
        let end = self.prefix_bytes.checked_add(self.expression_len)?;
        if span.0 < span.1 && span.0 >= self.prefix_bytes && span.1 <= end {
            Some((
                span.0.checked_sub(self.prefix_bytes)?,
                span.1.checked_sub(self.prefix_bytes)?,
            ))
        } else {
            None
        }
    }
}
fn element_text(model: &Model, e: ObjId) -> String {
    pilot_text(model, &Value::Element(e))
}
fn outcome_text(model: &Model, outcome: &Outcome) -> String {
    match outcome {
        Outcome::NotEvaluable(reason) => {
            format!("(not evaluable: {})", reason_text(model, reason))
        }
        Outcome::Values(_) => values_text(model, outcome),
    }
}
fn frame_text(model: &Model, frame: &FrameView) -> String {
    match frame {
        FrameView::Type { ty } => element_text(model, *ty),
        FrameView::Arguments { expression, bound } => format!(
            "arguments of {} [{}]",
            element_text(model, *expression),
            bound
                .iter()
                .map(|(p, v)| format!(
                    "{} = {}",
                    element_text(model, *p),
                    values_text(model, &Outcome::Values(v.clone()))
                ))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
}
fn location(model: &Model, trace: &Trace, e: ObjId, source: Option<&SourceMap>) -> String {
    let Some(object) = model.obj(e) else {
        return identity(model, e);
    };
    let uri = object.resource();
    let span = object.span();
    let text = trace.sources.get(uri);
    if let Some(map) = source.filter(|s| s.uri == uri)
        && let Some(user) = map.to_user(span)
    {
        if let Some(text) = text
            && let Some(end) = map.prefix_bytes.checked_add(map.expression_len)
            && let Some(user_text) = text.get(map.prefix_bytes..end)
        {
            let (line, column) = intersect_kerml::sysml_host::line_column(user_text, user.0);
            return format!("<expression>:{line}:{column}");
        }
        return format!("<expression>:bytes {}..{}", user.0, user.1);
    }
    if let Some(text) = text {
        let (line, column) = intersect_kerml::sysml_host::line_column(text, span.0);
        format!("{uri}:{line}:{column}")
    } else {
        format!("{uri}:bytes {}..{}", span.0, span.1)
    }
}
/// Human-readable trace: one line per event, indented by depth. Elements are
/// named by their Pilot-style text and values use `values_text`; JSON traces
/// (`outcome_json`) remain the lossless form.
///
/// `apply` is reported when a library function starts, before its operands
/// are evaluated (operand evaluation is part of the function, and lazy
/// functions evaluate only some operands). Operand values appear on the
/// nested `exit` lines; arguments already bound by an enclosing call are
/// shown on the `apply` line.
pub fn trace_text(model: &Model, trace: &Trace, source: Option<&SourceMap>) -> String {
    let mut lines = Vec::new();
    for event in &trace.events {
        let (step, depth, text) = match event {
            TraceEvent::Enter {
                step,
                depth,
                expression,
                context,
            } => {
                let mut text = format!(
                    "enter {} at {}",
                    element_text(model, *expression),
                    location(model, trace, *expression, source),
                );
                if !context.is_empty() {
                    text.push_str(" in ");
                    text.push_str(
                        &context
                            .iter()
                            .map(|f| frame_text(model, f))
                            .collect::<Vec<_>>()
                            .join(" / "),
                    );
                }
                (*step, *depth, text)
            }
            TraceEvent::Exit {
                step,
                depth,
                expression,
                outcome,
            } => (
                *step,
                *depth,
                format!(
                    "exit {} = {}",
                    element_text(model, *expression),
                    outcome_text(model, outcome)
                ),
            ),
            TraceEvent::Apply {
                step,
                depth,
                invocation,
                function,
                arguments,
            } => {
                let mut text = format!("apply {function} for {}", element_text(model, *invocation));
                if arguments.iter().any(Option::is_some) {
                    text.push_str(&format!(
                        " to ({})",
                        arguments
                            .iter()
                            .map(|a| a
                                .as_ref()
                                .map_or_else(|| "...".into(), |o| outcome_text(model, o)))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
                (*step, *depth, text)
            }
            TraceEvent::Skipped {
                step,
                depth,
                invocation,
                operand,
            } => (
                *step,
                *depth,
                format!(
                    "skip operand {operand} of {} (not evaluated)",
                    element_text(model, *invocation)
                ),
            ),
            TraceEvent::Lookup {
                step,
                depth,
                feature,
                frame,
                found,
            } => {
                let found = match found {
                    Lookup::Value(e) => format!(
                        "value {} at {}",
                        element_text(model, *e),
                        location(model, trace, *e, source)
                    ),
                    Lookup::Unbound(e) => format!("no value (unbound {})", element_text(model, *e)),
                    Lookup::Bound => "bound argument".into(),
                    Lookup::NotHere => "not here".into(),
                };
                (
                    *step,
                    *depth,
                    format!(
                        "lookup {} in {} -> {found}",
                        element_text(model, *feature),
                        frame_text(model, frame)
                    ),
                )
            }
        };
        let indent = usize::try_from(depth)
            .ok()
            .and_then(|n| n.checked_mul(2))
            .map_or(String::new(), |n| " ".repeat(n));
        lines.push(format!("{indent}[{step}] {text}"));
    }
    lines.join(
        "
",
    )
}
