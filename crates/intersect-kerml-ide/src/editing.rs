//! Bounded lexical editing support.
use crate::{byte_position, position_byte};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Documents(BTreeMap<String, Document>);
struct Document {
    text: String,
    language: String,
    version: i64,
}

// Lexical suggestions only. These are language-owned spellings, not a claim
// that each keyword is valid at the cursor or that references are in scope.
const COMMON: &str = "abstract alias all and as assoc behavior binding bool by class classifier comment composite conjugate conjugates conjugation connector datatype dependency derived differences disjoining disjoint doc else end false feature featured featuring filter first flow for from function hastype if import in inout interaction intersects inv inverse inverting istype language library member meta metadata multiplicity namespace new nonunique not null of or ordered out package portion predicate private protected public readonly redefines references rep return specializes step struct subsets succession then to true type typed typing union unions var when xor";
const SYSML: &str = "about accept action actor allocation analysis assert assign assume at attribute bind calc case concern connect connection constraint decide def default defined entry enum event exhibit exit expose fork include individual interface item join loop merge objective occurrence parallel part perform port ref render rendering require requirement satisfy send snapshot specialization state subject terminate timeslice transition until use variation variant verification verify view viewpoint while";
const SNIPPETS: &[(&str, &str, &str, bool)] = &[
    (
        "package declaration",
        "package ${1:Name} {\n\t$0\n}",
        "Package",
        false,
    ),
    (
        "import namespace",
        "private import ${1:Namespace}::*;",
        "Namespace import",
        false,
    ),
    (
        "part definition",
        "part def ${1:Name} {\n\t$0\n}",
        "Part definition",
        true,
    ),
    (
        "part usage",
        "part ${1:name} : ${2:Type};$0",
        "Part usage",
        true,
    ),
    (
        "attribute usage",
        "attribute ${1:name} : ${2:Type};$0",
        "Attribute usage",
        true,
    ),
    (
        "port definition",
        "port def ${1:Name} {\n\t$0\n}",
        "Port definition",
        true,
    ),
    (
        "class declaration",
        "class ${1:Name} {\n\t$0\n}",
        "Class",
        false,
    ),
    (
        "feature declaration",
        "feature ${1:name} : ${2:Type};$0",
        "Feature",
        false,
    ),
];

impl Documents {
    fn budget(&self, uri: &str, text: &str) -> Result<(), String> {
        let others = self.0.iter().filter(|(key, _)| key.as_str() != uri);
        let (count, bytes) = others.fold((1, text.len()), |(count, bytes), (_, doc)| {
            (count + 1, bytes + doc.text.len())
        });
        if count > 256 || bytes > 8 * 1024 * 1024 {
            return Err("Open documents exceed 256 sources or 8 MiB of text".into());
        }
        Ok(())
    }
    pub fn open(&mut self, params: &Value) -> Result<Value, String> {
        let doc = &params["textDocument"];
        let uri = uri(params)?;
        if self.0.contains_key(uri) {
            return Err("Document is already open".into());
        }
        let text = doc["text"].as_str().ok_or("Missing document text")?;
        let language = doc["languageId"].as_str().ok_or("Missing language id")?;
        if !matches!(language, "sysml" | "kerml") {
            return Err("Unsupported document language".into());
        }
        self.budget(uri, text)?;
        self.0.insert(
            uri.into(),
            Document {
                text: text.into(),
                language: language.into(),
                version: version(params)?,
            },
        );
        Ok(Value::Null)
    }
    pub fn change(&mut self, params: &Value) -> Result<Value, String> {
        let uri = uri(params)?;
        let version = version(params)?;
        let document = self.0.get(uri).ok_or("Document is not open")?;
        if version <= document.version {
            return Err("Document version is stale".into());
        }
        let changes = params["contentChanges"]
            .as_array()
            .ok_or("Missing content changes")?;
        if changes.len() != 1 || changes[0].get("range").is_some() {
            return Err("Exactly one full-document change is required".into());
        }
        let text = changes[0]["text"].as_str().ok_or("Missing change text")?;
        self.budget(uri, text)?;
        let document = self.0.get_mut(uri).ok_or("Document is not open")?;
        document.text = text.into();
        document.version = version;
        Ok(Value::Null)
    }
    pub fn close(&mut self, params: &Value) -> Result<Value, String> {
        self.0.remove(uri(params)?);
        Ok(Value::Null)
    }
    pub fn complete(&self, params: &Value) -> Result<Value, String> {
        let doc = self.0.get(uri(params)?).ok_or("Document is not open")?;
        // Optional extension used by Mercurio to reject queued stale requests.
        if params["textDocument"].get("version").is_some() && version(params)? != doc.version {
            return Err("Document version is stale".into());
        }
        let offset = position_byte(&doc.text, &params["position"])?;
        if !in_code(&doc.text[..offset]) {
            return Ok(json!({"isIncomplete":false,"items":[]}));
        }
        let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
        let start = doc.text[..offset]
            .char_indices()
            .rev()
            .find(|(_, c)| !word(*c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let end = doc.text[offset..]
            .char_indices()
            .find(|(_, c)| !word(*c))
            .map_or(doc.text.len(), |(i, _)| offset + i);
        let prefix = &doc.text[start..offset];
        let range =
            json!({"start":byte_position(&doc.text,start),"end":byte_position(&doc.text,end)});
        let sysml = doc.language == "sysml";
        let mut keywords: Vec<_> = COMMON
            .split_whitespace()
            .chain(if sysml { SYSML } else { "" }.split_whitespace())
            .collect();
        keywords.sort_unstable();
        keywords.dedup();
        let mut items: Vec<_> = keywords
            .into_iter()
            .filter(|k| k.starts_with(prefix))
            .map(|keyword| {
                json!({"label":keyword,"kind":14,"detail":"Lexical keyword",
                "textEdit":{"range":range,"newText":keyword}})
            })
            .collect();
        items.extend(
            SNIPPETS
                .iter()
                .filter(|(label, _, _, only_sysml)| {
                    (!only_sysml || sysml) && label.starts_with(prefix)
                })
                .map(|(label, text, detail, _)| {
                    json!({"label":label,"kind":15,
                "detail":format!("{detail} snippet"),"insertTextFormat":2,
                "textEdit":{"range":range,"newText":text}})
                }),
        );
        Ok(json!({"isIncomplete":false,"items":items}))
    }
}
fn uri(params: &Value) -> Result<&str, String> {
    params["textDocument"]["uri"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing document URI".into())
}
fn version(params: &Value) -> Result<i64, String> {
    params["textDocument"]["version"]
        .as_i64()
        .ok_or_else(|| "Missing document version".into())
}

// Bounded lexical scan of the current buffer, including unfinished comments,
// strings and unrestricted names. Escaped quotes do not terminate a token.
fn in_code(text: &str) -> bool {
    #[derive(PartialEq)]
    enum State {
        Code,
        Line,
        Block,
        Quote(u8),
    }
    let bytes = text.as_bytes();
    let mut state = State::Code;
    let mut i = 0;
    while i < bytes.len() {
        let pair = bytes.get(i..i + 2);
        match state {
            State::Code if pair == Some(b"//") => {
                state = State::Line;
                i += 1;
            }
            State::Code if pair == Some(b"/*") => {
                state = State::Block;
                i += 1;
            }
            State::Code if matches!(bytes[i], b'\'' | b'"') => state = State::Quote(bytes[i]),
            State::Line if bytes[i] == b'\n' => state = State::Code,
            State::Block if pair == Some(b"*/") => {
                state = State::Code;
                i += 1;
            }
            State::Quote(_) if bytes[i] == b'\\' => i += 1,
            State::Quote(quote) if bytes[i] == quote => state = State::Code,
            _ => {}
        }
        i += 1;
    }
    state == State::Code
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(docs: &mut Documents, language: &str, text: &str) -> Result<Value, String> {
        docs.open(&json!({"textDocument":{"uri":"file:///test","languageId":language,"version":1,"text":text}}))
    }
    fn complete(docs: &Documents, character: usize, version: i64) -> Result<Value, String> {
        docs.complete(&json!({"textDocument":{"uri":"file:///test","version":version},"position":{"line":0,"character":character}}))
    }
    #[test]
    fn incomplete_buffer_versions_and_replacement() -> Result<(), String> {
        let mut docs = Documents::default();
        open(&mut docs, "sysml", "package P { parX")?;
        let result = complete(&docs, 15, 1)?;
        let part = result["items"]
            .as_array()
            .ok_or("items")?
            .iter()
            .find(|i| i["label"] == "part")
            .ok_or("part")?;
        assert_eq!(
            part["textEdit"]["range"],
            json!({"start":{"line":0,"character":12},"end":{"line":0,"character":16}})
        );
        docs.change(&json!({"textDocument":{"uri":"file:///test","version":2},"contentChanges":[{"text":"feat"}]}))?;
        assert!(complete(&docs, 4, 1).is_err());
        assert!(
            !complete(&docs, 4, 2)?["items"]
                .as_array()
                .ok_or("items")?
                .is_empty()
        );
        assert!(docs.change(&json!({"textDocument":{"uri":"file:///test","version":2},"contentChanges":[{"text":""}]})).is_err());
        docs.close(&json!({"textDocument":{"uri":"file:///test"}}))?;
        assert!(complete(&docs, 0, 2).is_err());
        Ok(())
    }
    #[test]
    fn language_and_lexical_context() -> Result<(), String> {
        for text in ["// par", "/* par", "\"par", "'par", "\"escaped\\\" par"] {
            let mut docs = Documents::default();
            open(&mut docs, "sysml", text)?;
            assert_eq!(complete(&docs, text.len(), 1)?["items"], json!([]));
        }
        let mut docs = Documents::default();
        open(&mut docs, "kerml", "/* 😀 */ par")?;
        let result = complete(&docs, 12, 1)?;
        assert!(
            !result["items"]
                .as_array()
                .ok_or("items")?
                .iter()
                .any(|i| i["label"] == "part")
        );
        assert!(complete(&docs, 4, 1).is_err()); // inside surrogate pair
        Ok(())
    }
}
