//! Handwritten source-site selection and presentation over the shared evaluator.
use super::{Host, name};
use intersect_kerml_eval::{
    EvalError, EvalLimits, EvalMode, Evaluator, Outcome, Site, SiteKind, Value as EvalValue,
    format::{outcome_json, reason_text},
};
use serde_json::{Value, json};

#[cfg(test)]
#[path = "../../intersect-kerml-eval/tests/support/fixture.rs"]
mod fixture;

impl Host {
    fn evaluation_model(&self, params: &Value) -> Result<&intersect_kerml_eval::Model, String> {
        self.check_revision(params)?;
        self.current
            .as_ref()
            .ok_or_else(|| "No current project model; analyze the current buffers first".into())
    }

    pub(super) fn evaluation_sites(&self, params: &Value) -> Result<Value, String> {
        let model = self.evaluation_model(params)?;
        let uri = params["uri"]
            .as_str()
            .ok_or("Missing evaluation source URI")?;
        if !self.sources.contains_key(uri) {
            return Err("Evaluation requires a project or dependency source".into());
        }
        let evaluator = Evaluator::new(model, EvalMode::ModelLevel).map_err(|e| e.to_string())?;
        let sites = intersect_kerml_eval::sites(model, &[uri]).map_err(|e| e.to_string())?;
        let rows = sites
            .into_iter()
            .map(|site| self.site_json(site, &evaluator))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({"revision":self.revision,"sites":rows}))
    }

    fn site_json(&self, site: Site, evaluator: &Evaluator<'_>) -> Result<Value, String> {
        let model = self.current.as_ref().ok_or("No current evaluation model")?;
        let expression = model
            .obj(site.expression)
            .ok_or("Evaluation expression no longer exists")?;
        let (start, end) = expression.span();
        let text = self
            .source(expression.resource())
            .and_then(|text| text.get(start..end))
            .ok_or("Expression has no valid source span")?;
        let mut owner = expression.container().map(|(parent, _)| parent);
        while let Some(element) = owner {
            if name(element).is_some() {
                break;
            }
            owner = element.container().map(|(parent, _)| parent);
        }
        let context = site.target.and_then(|id| model.obj(id));
        let kind = match site.kind {
            SiteKind::FeatureValue => "feature-value",
            SiteKind::ResultExpression => "result-expression",
            SiteKind::MultiplicityBound => "multiplicity-bound",
        };
        // The language owns re-selection identity. Snapshot object IDs are never
        // reused by clients as if they were durable across model rebuilds.
        // Multiple bounds can share this key; clients must refuse ambiguity.
        let key = owner
            .map(super::views::key)
            .transpose()?
            .map(|key| format!("evaluation:{kind}:{key}"));
        let label = owner
            .map(|o| evaluator.qualified_name(o.id()))
            .transpose()
            .map_err(|e| e.to_string())?
            .flatten();
        let context_name = context
            .map(|o| evaluator.qualified_name(o.id()))
            .transpose()
            .map_err(|e| e.to_string())?
            .flatten();
        Ok(
            json!({"id":expression.identity(),"key":key,"kind":kind,"uri":expression.resource(),"range":self.range(expression),
            "text":text,"label":label.unwrap_or_else(|| "Anonymous expression".into()),
            "ownerId":owner.map(|o|o.identity()),"ownerRange":owner.map(|o|self.range(o)),
            "context":{"id":context.map(|o|o.identity()),"name":context_name.unwrap_or_else(|| "No target frame".into())}}),
        )
    }

    pub(super) fn evaluate_site(&self, params: &Value) -> Result<Value, String> {
        let model = self.evaluation_model(params)?;
        let id = params["expressionId"]
            .as_str()
            .ok_or("Missing evaluation expression id")?;
        let object = model
            .lookup(id)
            .ok_or("Evaluation expression no longer exists")?;
        if !self.sources.contains_key(object.resource()) {
            return Err("Evaluation requires a project or dependency expression".into());
        }
        let site = intersect_kerml_eval::sites(model, &[object.resource()])
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|site| site.expression == object.id())
            .ok_or("Element is not a supported source evaluation site")?;
        let evaluator = Evaluator::new(model, EvalMode::ModelLevel)
            .map_err(|e| e.to_string())?
            .with_limits(EvalLimits {
                steps: 100_000,
                depth: 128,
            });
        let description = self.site_json(site, &evaluator)?;
        let result = evaluator.evaluate(site.expression, site.target);
        let mut response = json!({"revision":self.revision,"site":description});
        match result {
            Ok(mut outcome) => {
                let mut partial = false;
                if let Outcome::Values(values) = &mut outcome {
                    partial = values
                        .iter()
                        .any(|value| matches!(value, EvalValue::Unevaluated { .. }));
                    response["totalValues"] = json!(values.len());
                    response["truncated"] = json!(values.len() > 256);
                    values.truncate(256);
                }
                let mut wire = outcome_json(model, &outcome);
                // i64 values must survive JSON transport into JavaScript exactly.
                if let Outcome::Values(values) = &outcome {
                    for (row, value) in wire["values"]
                        .as_array_mut()
                        .ok_or("Missing evaluator values")?
                        .iter_mut()
                        .zip(values)
                    {
                        if let EvalValue::Int(value) = value {
                            row["value"] = json!(value.to_string());
                        }
                    }
                }
                let reasons = match &outcome {
                    Outcome::NotEvaluable(reason) => vec![reason_text(model, reason)],
                    Outcome::Values(values) => values
                        .iter()
                        .filter_map(|v| match v {
                            EvalValue::Unevaluated { reason, .. } => {
                                Some(reason_text(model, reason))
                            }
                            _ => None,
                        })
                        .collect(),
                };
                response["status"] = json!(if matches!(outcome, Outcome::NotEvaluable(_)) {
                    "not-evaluable"
                } else if reasons.is_empty() && !partial {
                    "values"
                } else {
                    "partial"
                });
                response["messages"] = json!(reasons);
                response["outcome"] = wire;
            }
            Err(error) => {
                response["status"] = json!(if matches!(
                    error,
                    EvalError::StepLimit(_) | EvalError::DepthLimit(_)
                ) {
                    "budget-exceeded"
                } else {
                    "error"
                });
                response["messages"] = json!([error.to_string()]);
                response["outcome"] = Value::Null;
            }
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(not(feature = "milestone-tests"), ignore = "milestone")]
    fn retained_model_evaluation_preserves_integer_precision_and_context() -> Result<(), String> {
        let mut b = fixture::FixtureBuilder::new();
        let package = b.package("P");
        let rover = b.part_def(package, "Rover");
        let a = b.int(2_147_483_647);
        let multiplier = b.int(4_194_304);
        let product = b.op("*", &[a, multiplier]);
        let remainder = b.int(4_194_305);
        let big = b.op("+", &[product, remainder]);
        b.attribute(rover, "big", Some(big));
        let unset = b.attribute(rover, "unset", None);
        let missing = b.feature_ref_to(unset);
        b.attribute(rover, "missing", Some(missing));
        let cycle = b.attribute(rover, "cycle", None);
        let recursive = b.feature_ref_to(cycle);
        b.value(cycle, recursive);
        fn sequence(
            b: &mut fixture::FixtureBuilder,
            start: i64,
            end: i64,
        ) -> intersect_kerml_eval::ObjId {
            if end - start == 1 {
                return b.int(start);
            }
            let middle = start + (end - start) / 2;
            let left = sequence(b, start, middle);
            let right = sequence(b, middle, end);
            b.op(",", &[left, right])
        }
        let many = sequence(&mut b, 0, 300);
        b.attribute(rover, "many", Some(many));
        let model = b.build();
        let id = |id| {
            model
                .obj(id)
                .map(|o| o.identity().to_owned())
                .ok_or("Missing fixture object")
        };
        let (big, missing, recursive, many) = (id(big)?, id(missing)?, id(recursive)?, id(many)?);
        let host = Host {
            current: Some(model),
            revision: 7,
            sources: std::collections::BTreeMap::from([(
                "memory:/eval.kerml".into(),
                String::new(),
            )]),
            ..Host::default()
        };
        let result = host.evaluate_site(&json!({"revision":7,"expressionId":big}))?;
        assert_eq!(result["status"], "values");
        assert_eq!(result["outcome"]["values"][0]["value"], "9007199254740993");
        assert_eq!(result["site"]["context"]["name"], "P::Rover");
        let result = host.evaluate_site(&json!({"revision":7,"expressionId":missing}))?;
        assert_eq!(result["status"], "partial", "{result}");
        assert!(
            !result["messages"]
                .as_array()
                .ok_or("Missing messages")?
                .is_empty()
        );
        let result = host.evaluate_site(&json!({"revision":7,"expressionId":recursive}))?;
        assert_eq!(result["status"], "budget-exceeded", "{result}");
        let result = host.evaluate_site(&json!({"revision":7,"expressionId":many}))?;
        assert_eq!(result["status"], "values", "{result}");
        assert_eq!(result["totalValues"], 300);
        assert_eq!(result["truncated"], true);
        assert_eq!(
            result["outcome"]["values"]
                .as_array()
                .ok_or("Missing sequence")?
                .len(),
            256
        );
        Ok(())
    }
}
