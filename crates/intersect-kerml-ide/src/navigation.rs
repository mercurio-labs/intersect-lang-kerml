//! Reference navigation over the current upstream linker result.
use crate::{Host, byte_position, position_byte};
use serde_json::{Value, json};

impl Host {
    /// Resolve an authored reference occurrence in the accepted analysis.
    /// Native consumers use the same endpoint as the embedded LSP facade.
    pub fn definition(&self, params: &Value) -> Result<Value, String> {
        self.check_revision(params)?;
        let model = self.current.as_ref().ok_or("No current project model")?;
        if !self.documents.matches_sources(|uri| self.source(uri)) {
            return Err("Open documents differ from the current analysis".into());
        }
        let uri = params["textDocument"]["uri"]
            .as_str()
            .ok_or("Missing document URI")?;
        let text = self.source(uri).ok_or("Unknown source")?;
        if !self
            .documents
            .matches(uri, text, params["textDocument"]["version"].as_i64())
        {
            return Err("Document differs from the current analysis".into());
        }
        let offset = position_byte(text, &params["position"])?;
        let Some(reference) = self
            .navigation
            .iter()
            .filter(|r| r.uri == uri && r.start <= offset && offset <= r.end)
            .min_by_key(|r| r.end - r.start)
        else {
            return Ok(Value::Null);
        };
        let Some(target) = model.lookup(&reference.target) else {
            return Ok(Value::Null);
        };
        let target_text = self
            .source(target.resource())
            .ok_or("Target source is unavailable")?;
        let (start, end) = target.span();
        if start >= end || target_text.get(start..end).is_none() {
            return Ok(Value::Null);
        }
        let range =
            json!({"start":byte_position(target_text,start),"end":byte_position(target_text,end)});
        Ok(
            json!([{"originSelectionRange":{"start":byte_position(text,reference.start),"end":byte_position(text,reference.end)},
            "targetUri":target.resource(),"targetRange":range,"targetSelectionRange":range}]),
        )
    }
}
