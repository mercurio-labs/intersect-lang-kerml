//! Handwritten CLI presentation of evaluator results and trace events.
use super::{Options, UserFile};
use intersect_kerml::sysml_host::line_column;
use intersect_kerml_eval::{
    Evaluator, FrameView, Lookup, Model, ObjId, Outcome, Trace, TraceEvent, Value,
    format::{outcome_json, reason_text, values_text},
    trace_text,
};
use serde_json::{Value as Json, json};

pub(super) fn run(options: &Options, user: &[UserFile], model: &Model) -> Result<(), String> {
    let evaluation = Evaluation {
        options,
        user,
        model,
        evaluator: Evaluator::new(model, options.mode).map_err(|e| e.to_string())?,
    };
    let target = options
        .target
        .as_deref()
        .map(|name| evaluation.evaluator.resolve_target(name))
        .transpose()
        .map_err(|e| e.to_string())?;
    if options.all {
        let resources: Vec<_> = user.iter().map(|f| f.source.uri.as_str()).collect();
        let (mut sites, mut failed) = (0usize, 0usize);
        for site in intersect_kerml_eval::sites(model, &resources).map_err(|e| e.to_string())? {
            let object = model
                .obj(site.expression)
                .ok_or("site expression is absent")?;
            let file = user
                .iter()
                .find(|f| f.source.uri == object.resource())
                .ok_or("site resource has no source text")?;
            let (line, column) = source_position(file, object.span().0);
            let mut owner = object
                .container()
                .and_then(|(rel, _)| rel.container())
                .map(|(owner, _)| owner);
            let mut name = None;
            while let Some(element) = owner {
                name = evaluation
                    .evaluator
                    .qualified_name(element.id())
                    .map_err(|e| e.to_string())?;
                if name.is_some() {
                    break;
                }
                owner = element.container().map(|(parent, _)| parent);
            }
            let (kind, kind_label) = match site.kind {
                intersect_kerml_eval::SiteKind::FeatureValue => ("feature-value", ""),
                intersect_kerml_eval::SiteKind::ResultExpression => {
                    ("result-expression", " [result]")
                }
                intersect_kerml_eval::SiteKind::MultiplicityBound => {
                    ("multiplicity-bound", " [multiplicity]")
                }
            };
            // Show the file as the user named it, not its (percent-encoded) URI.
            let path = file.path.to_string_lossy().replace('\\', "/");
            let target = target.or(site.target);
            let description = json!({"resource":object.resource(),"path":path,"line":line,"column":column,
                "span":object.span(),"kind":kind,"expression":object.identity(),"feature":name,
                "target":target.map(|id|identity(model,id))});
            let label = format!(
                "{path}:{line}:{column} {}{kind_label}",
                name.as_deref().unwrap_or("(anonymous)")
            );
            sites += 1;
            if !evaluation.print(site.expression, target, description, Some(label))? {
                failed += 1;
            }
        }
        // Every site is reported; an evaluator error at one site does not hide
        // the others, but still makes the command fail (exit status 2).
        if failed > 0 {
            return Err(format!(
                "{failed} of {sites} site(s) raised an evaluation error"
            ));
        }
        return Ok(());
    }
    let wrapper = user
        .iter()
        .find(|f| f.source_map.is_some())
        .ok_or("missing expression wrapper")?;
    let expression = intersect_kerml_eval::wrapper_expression(model, &wrapper.source.uri)
        .map_err(|e| e.to_string())?;
    let site = json!({"kind":"expression","expression":identity(model,expression),
        "location":location_json(model,user,expression),"target":target.map(|id|identity(model,id))});
    evaluation.print(expression, target, site, None).map(|_| ())
}

struct Evaluation<'a> {
    options: &'a Options,
    user: &'a [UserFile],
    model: &'a Model,
    evaluator: Evaluator<'a>,
}

impl Evaluation<'_> {
    fn print(
        &self,
        expression: ObjId,
        target: Option<ObjId>,
        site: Json,
        label: Option<String>,
    ) -> Result<bool, String> {
        let Self {
            options,
            user,
            model,
            evaluator,
        } = self;
        let evaluable = evaluator
            .is_model_level_evaluable(expression)
            .map_err(|e| e.to_string())?;
        let outcome = if options.trace {
            let mut trace = Trace::default();
            for file in *user {
                trace = trace.with_source(&file.source.uri, &file.source.text);
            }
            let result = evaluator.evaluate_observed(expression, target, options.gated, &mut trace);
            if options.json {
                for event in &trace.events {
                    eprintln!("{}", trace_json(model, user, event));
                }
            } else {
                let source_map = user.iter().find_map(|f| f.source_map.as_ref());
                eprintln!("{}", trace_text(model, &trace, source_map));
            }
            result
        } else if options.gated {
            evaluator.evaluate_gated(expression, target)
        } else {
            evaluator.evaluate(expression, target)
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) if options.all => {
                if options.json {
                    println!(
                        "{}",
                        json!({"site":site,"evaluable":evaluable,
                            "outcome":{"outcome":"exception","message":error.to_string()}})
                    );
                } else {
                    let label = label.unwrap_or_default();
                    println!("{label} = (error: {error})");
                }
                return Ok(false);
            }
            Err(error) => return Err(error.to_string()),
        };
        if options.json {
            let mut row =
                json!({"site":site,"evaluable":evaluable,"outcome":outcome_json(model,&outcome)});
            if options.explain {
                row["explanation"] = json!(explanations(model, &outcome));
            }
            println!("{row}");
        } else {
            let values = values_text(model, &outcome);
            let mut text =
                label.map_or_else(|| values.clone(), |label| format!("{label} = {values}"));
            if options.explain {
                let reasons = explanations(model, &outcome);
                if !reasons.is_empty() {
                    text.push_str(&format!(" [why: {}]", reasons.join("; ")));
                }
            }
            println!("{text}");
        }
        Ok(true)
    }
}

/// Wrapper diagnostics include zero-width EOF spans; map positions inclusively.
pub(super) fn source_position(file: &UserFile, start: usize) -> (usize, usize) {
    if let Some(map) = &file.source_map
        && let Some(end) = map.prefix_bytes.checked_add(map.expression_len)
        && (map.prefix_bytes..=end).contains(&start)
        && let Some(text) = file.source.text.get(map.prefix_bytes..end)
    {
        return line_column(text, start.saturating_sub(map.prefix_bytes));
    }
    line_column(&file.source.text, start)
}

fn identity(model: &Model, id: ObjId) -> String {
    model.obj(id).map_or_else(
        || format!("object-index:{}", id.index()),
        |o| o.identity().into(),
    )
}

fn explanations(model: &Model, outcome: &Outcome) -> Vec<String> {
    match outcome {
        Outcome::NotEvaluable(reason) => vec![reason_text(model, reason)],
        Outcome::Values(values) => values
            .iter()
            .filter_map(|v| match v {
                Value::Unevaluated { reason, .. } => Some(reason_text(model, reason)),
                _ => None,
            })
            .collect(),
    }
}

fn frame_json(model: &Model, frame: &FrameView) -> Json {
    match frame {
        FrameView::Type { ty } => json!({"kind":"type","type":identity(model,*ty)}),
        FrameView::Arguments { expression, bound } => json!({
            "kind":"arguments", "expression":identity(model,*expression),
            "bound":bound.iter().map(|(p,v)| json!({"parameter":identity(model,*p),"outcome":outcome_json(model,&Outcome::Values(v.clone()))})).collect::<Vec<_>>()
        }),
    }
}

fn location_json(model: &Model, user: &[UserFile], id: ObjId) -> Json {
    let Some(object) = model.obj(id) else {
        return Json::Null;
    };
    let (start, end) = object.span();
    if let Some(file) = user.iter().find(|f| f.source.uri == object.resource()) {
        let (line, column) = source_position(file, start);
        let user_span = file
            .source_map
            .as_ref()
            .and_then(|map| map.to_user((start, end)));
        return json!({"resource":object.resource(),"file":file.path,"line":line,"column":column,
            "span":user_span.unwrap_or((start,end))});
    }
    json!({"resource":object.resource(),"span":[start,end]})
}

fn trace_json(model: &Model, user: &[UserFile], event: &TraceEvent) -> Json {
    let (step, depth, element, mut value) = match event {
        TraceEvent::Enter {
            step,
            depth,
            expression,
            context,
        } => (
            *step,
            *depth,
            *expression,
            json!({"event":"enter","expression":identity(model,*expression),"context":context.iter().map(|f|frame_json(model,f)).collect::<Vec<_>>()}),
        ),
        TraceEvent::Exit {
            step,
            depth,
            expression,
            outcome,
        } => (
            *step,
            *depth,
            *expression,
            json!({"event":"exit","expression":identity(model,*expression),"outcome":outcome_json(model,outcome)}),
        ),
        TraceEvent::Apply {
            step,
            depth,
            invocation,
            function,
            arguments,
        } => (
            *step,
            *depth,
            *invocation,
            json!({"event":"apply","invocation":identity(model,*invocation),"function":function,"arguments":arguments.iter().map(|o|o.as_ref().map(|o|outcome_json(model,o))).collect::<Vec<_>>()}),
        ),
        TraceEvent::Skipped {
            step,
            depth,
            invocation,
            operand,
        } => (
            *step,
            *depth,
            *invocation,
            json!({"event":"skipped","invocation":identity(model,*invocation),"operand":operand}),
        ),
        TraceEvent::Lookup {
            step,
            depth,
            feature,
            frame,
            found,
        } => {
            let found = match found {
                Lookup::Value(v) => json!({"kind":"value","expression":identity(model,*v)}),
                Lookup::Unbound(f) => json!({"kind":"unbound","feature":identity(model,*f)}),
                Lookup::Bound => json!({"kind":"bound"}),
                Lookup::NotHere => json!({"kind":"not-here"}),
            };
            (
                *step,
                *depth,
                *feature,
                json!({"event":"lookup","feature":identity(model,*feature),"frame":frame_json(model,frame),"found":found}),
            )
        }
    };
    value["step"] = json!(step);
    value["depth"] = json!(depth);
    value["location"] = location_json(model, user, element);
    value
}
