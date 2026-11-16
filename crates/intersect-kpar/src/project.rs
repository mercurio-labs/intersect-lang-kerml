//! Handwritten (not generated): the two metadata documents of a project
//! archive, `.project.json` (`Project`) and `.meta.json` (`Meta`), as the
//! OMG JSON schema `KerML-Model-Interchange.json` (ptc/25-04-20, KerML
//! 20250201) defines them. Members the schema does not define are kept in
//! `extra` and written back, so reading and writing loses nothing.
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// The archive file name of the interchange project document.
pub const PROJECT_FILE: &str = ".project.json";
/// The archive file name of the interchange project metadata document.
pub const META_FILE: &str = ".meta.json";

/// `.project.json`: the interchange project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub maintainer: Vec<String>,
    pub website: Option<String>,
    pub topic: Vec<String>,
    pub usage: Vec<Usage>,
    /// Members the schema does not define, kept as read.
    pub extra: BTreeMap<String, Value>,
}

/// One `usage` entry: a project this one depends on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    /// The IRI of the used project.
    pub resource: String,
    pub version_constraint: Option<String>,
    pub extra: BTreeMap<String, Value>,
}

/// `.meta.json`: the interchange project metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    /// Top-level member name to the archive path of the file declaring it.
    pub index: BTreeMap<String, String>,
    /// RFC 3339 date-time.
    pub created: String,
    pub metamodel: Option<String>,
    pub includes_derived: Option<bool>,
    pub includes_implied: Option<bool>,
    /// Archive path to checksum.
    pub checksum: BTreeMap<String, Checksum>,
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checksum {
    pub value: String,
    pub algorithm: String,
}

/// Why a metadata document was refused: the document and the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataError {
    pub file: &'static str,
    pub message: String,
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file, self.message)
    }
}

struct Reader {
    file: &'static str,
    members: Map<String, Value>,
}

impl Reader {
    fn new(file: &'static str, bytes: &[u8]) -> Result<Self, MetadataError> {
        let error = |message: String| MetadataError { file, message };
        let text = std::str::from_utf8(bytes)
            .map_err(|_| error("not UTF-8".into()))?
            .trim_start_matches('\u{feff}');
        match serde_json::from_str(text) {
            Ok(Value::Object(members)) => Ok(Self { file, members }),
            Ok(_) => Err(error("not a JSON object".into())),
            Err(e) => Err(error(format!("not JSON: {e}"))),
        }
    }
    fn error(&self, message: String) -> MetadataError {
        MetadataError {
            file: self.file,
            message,
        }
    }
    fn string(&mut self, key: &str) -> Result<Option<String>, MetadataError> {
        match self.members.remove(key) {
            None => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            Some(_) => Err(self.error(format!("'{key}' must be a string"))),
        }
    }
    fn required(&mut self, key: &str) -> Result<String, MetadataError> {
        self.string(key)?
            .ok_or_else(|| self.error(format!("required member '{key}' is missing")))
    }
    fn boolean(&mut self, key: &str) -> Result<Option<bool>, MetadataError> {
        match self.members.remove(key) {
            None => Ok(None),
            Some(Value::Bool(value)) => Ok(Some(value)),
            Some(_) => Err(self.error(format!("'{key}' must be a boolean"))),
        }
    }
    fn strings(&mut self, key: &str) -> Result<Vec<String>, MetadataError> {
        match self.members.remove(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .into_iter()
                .map(|item| match item {
                    Value::String(value) => Ok(value),
                    _ => Err(self.error(format!("'{key}' must be an array of strings"))),
                })
                .collect(),
            Some(_) => Err(self.error(format!("'{key}' must be an array of strings"))),
        }
    }
    fn object(&mut self, key: &str) -> Result<Option<Map<String, Value>>, MetadataError> {
        match self.members.remove(key) {
            None => Ok(None),
            Some(Value::Object(value)) => Ok(Some(value)),
            Some(_) => Err(self.error(format!("'{key}' must be an object"))),
        }
    }
    fn rest(self) -> BTreeMap<String, Value> {
        self.members.into_iter().collect()
    }
}

/// Whether `value` is an absolute IRI (the schema's `format: iri`).
pub fn is_iri(value: &str) -> bool {
    iri_string::types::IriStr::new(value).is_ok()
}

/// Whether `value` is an RFC 3339 date-time (the schema's
/// `format: date-time`): `YYYY-MM-DDTHH:MM:SS`, optional fraction, then `Z`
/// or `+HH:MM`/`-HH:MM`.
pub fn is_date_time(value: &str) -> bool {
    fn digits(text: &str, count: usize) -> Option<(u32, &str)> {
        let (head, tail) = text.split_at_checked(count)?;
        if !head.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((head.parse().ok()?, tail))
    }
    fn expect<'a>(text: &'a str, set: &[char]) -> Option<&'a str> {
        let mut chars = text.chars();
        set.contains(&chars.next()?).then_some(chars.as_str())
    }
    let parse = || -> Option<()> {
        let (_, rest) = digits(value, 4)?;
        let (month, rest) = digits(expect(rest, &['-'])?, 2)?;
        let (day, rest) = digits(expect(rest, &['-'])?, 2)?;
        let (hour, rest) = digits(expect(rest, &['T', 't'])?, 2)?;
        let (minute, rest) = digits(expect(rest, &[':'])?, 2)?;
        let (second, mut rest) = digits(expect(rest, &[':'])?, 2)?;
        if !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || hour > 23
            || minute > 59
            || second > 60
        {
            return None;
        }
        if let Some(fraction) = rest.strip_prefix('.') {
            let end = fraction
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(fraction.len());
            if end == 0 {
                return None;
            }
            rest = &fraction[end..];
        }
        if rest == "Z" || rest == "z" {
            return Some(());
        }
        let rest = expect(rest, &['+', '-'])?;
        let (zone_hour, rest) = digits(rest, 2)?;
        let (zone_minute, rest) = digits(expect(rest, &[':'])?, 2)?;
        (rest.is_empty() && zone_hour <= 23 && zone_minute <= 59).then_some(())
    };
    parse().is_some()
}

impl Project {
    /// Parse and check a `.project.json` document.
    pub fn parse(bytes: &[u8]) -> Result<Self, MetadataError> {
        let mut reader = Reader::new(PROJECT_FILE, bytes)?;
        let name = reader.required("name")?;
        let version = reader.required("version")?;
        let description = reader.string("description")?;
        let license = reader.string("license")?;
        let maintainer = reader.strings("maintainer")?;
        let website = reader.string("website")?;
        if let Some(website) = website.as_deref().filter(|website| !is_iri(website)) {
            return Err(reader.error(format!("'website' is not an IRI: {website}")));
        }
        let topic = reader.strings("topic")?;
        let usage = match reader.members.remove("usage") {
            None => Vec::new(),
            Some(Value::Array(items)) => items
                .into_iter()
                .map(|item| Usage::from_value(item).map_err(|message| reader.error(message)))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(reader.error("'usage' must be an array".into())),
        };
        Ok(Self {
            name,
            version,
            description,
            license,
            maintainer,
            website,
            topic,
            usage,
            extra: reader.rest(),
        })
    }

    /// The document as JSON.
    pub fn to_value(&self) -> Value {
        let mut members: Map<String, Value> = self.extra.clone().into_iter().collect();
        members.insert("name".into(), self.name.clone().into());
        members.insert("version".into(), self.version.clone().into());
        for (key, value) in [
            ("description", &self.description),
            ("license", &self.license),
            ("website", &self.website),
        ] {
            if let Some(value) = value {
                members.insert(key.into(), value.clone().into());
            }
        }
        for (key, values) in [("maintainer", &self.maintainer), ("topic", &self.topic)] {
            if !values.is_empty() {
                members.insert(key.into(), values.clone().into());
            }
        }
        if !self.usage.is_empty() {
            members.insert(
                "usage".into(),
                Value::Array(self.usage.iter().map(Usage::to_value).collect()),
            );
        }
        Value::Object(members)
    }
}

impl Usage {
    fn from_value(value: Value) -> Result<Self, String> {
        let Value::Object(mut members) = value else {
            return Err("each 'usage' entry must be an object".into());
        };
        let resource = match members.remove("resource") {
            Some(Value::String(resource)) if is_iri(&resource) => resource,
            Some(Value::String(resource)) => {
                return Err(format!("usage 'resource' is not an IRI: {resource}"));
            }
            Some(_) => return Err("usage 'resource' must be a string".into()),
            None => return Err("a 'usage' entry has no 'resource'".into()),
        };
        let version_constraint = match members.remove("versionConstraint") {
            None => None,
            Some(Value::String(constraint)) => Some(constraint),
            Some(_) => return Err("usage 'versionConstraint' must be a string".into()),
        };
        Ok(Self {
            resource,
            version_constraint,
            extra: members.into_iter().collect(),
        })
    }

    fn to_value(&self) -> Value {
        let mut members: Map<String, Value> = self.extra.clone().into_iter().collect();
        members.insert("resource".into(), self.resource.clone().into());
        if let Some(constraint) = &self.version_constraint {
            members.insert("versionConstraint".into(), constraint.clone().into());
        }
        Value::Object(members)
    }
}

impl Meta {
    /// Parse and check a `.meta.json` document.
    pub fn parse(bytes: &[u8]) -> Result<Self, MetadataError> {
        let mut reader = Reader::new(META_FILE, bytes)?;
        let index = match reader.object("index")? {
            None => return Err(reader.error("required member 'index' is missing".into())),
            Some(index) => index
                .into_iter()
                .map(|(name, path)| match path {
                    Value::String(path) => Ok((name, path)),
                    _ => Err(reader.error(format!("index entry '{name}' must be a string"))),
                })
                .collect::<Result<_, _>>()?,
        };
        let created = reader.required("created")?;
        if !is_date_time(&created) {
            return Err(reader.error(format!("'created' is not an RFC 3339 date-time: {created}")));
        }
        let metamodel = reader.string("metamodel")?;
        if let Some(metamodel) = metamodel.as_deref().filter(|metamodel| !is_iri(metamodel)) {
            return Err(reader.error(format!("'metamodel' is not an IRI: {metamodel}")));
        }
        let includes_derived = reader.boolean("includesDerived")?;
        let includes_implied = reader.boolean("includesImplied")?;
        let checksum = match reader.object("checksum")? {
            None => BTreeMap::new(),
            Some(entries) => entries
                .into_iter()
                .map(|(path, value)| {
                    let mut entry = match value {
                        Value::Object(entry) => entry,
                        _ => {
                            return Err(
                                reader.error(format!("checksum of '{path}' must be an object"))
                            );
                        }
                    };
                    let mut field =
                        |key: &str| match entry.remove(key) {
                            Some(Value::String(value)) => Ok(value),
                            _ => Err(reader
                                .error(format!("checksum of '{path}' needs a string '{key}'"))),
                        };
                    let value = field("value")?;
                    let algorithm = field("algorithm")?;
                    Ok((path, Checksum { value, algorithm }))
                })
                .collect::<Result<_, _>>()?,
        };
        Ok(Self {
            index,
            created,
            metamodel,
            includes_derived,
            includes_implied,
            checksum,
            extra: reader.rest(),
        })
    }

    /// The document as JSON.
    pub fn to_value(&self) -> Value {
        let mut members: Map<String, Value> = self.extra.clone().into_iter().collect();
        members.insert(
            "index".into(),
            Value::Object(
                self.index
                    .iter()
                    .map(|(name, path)| (name.clone(), path.clone().into()))
                    .collect(),
            ),
        );
        members.insert("created".into(), self.created.clone().into());
        if let Some(metamodel) = &self.metamodel {
            members.insert("metamodel".into(), metamodel.clone().into());
        }
        for (key, value) in [
            ("includesDerived", self.includes_derived),
            ("includesImplied", self.includes_implied),
        ] {
            if let Some(value) = value {
                members.insert(key.into(), value.into());
            }
        }
        if !self.checksum.is_empty() {
            members.insert(
                "checksum".into(),
                Value::Object(
                    self.checksum
                        .iter()
                        .map(|(path, checksum)| {
                            (
                                path.clone(),
                                serde_json::json!({
                                    "value": checksum.value,
                                    "algorithm": checksum.algorithm,
                                }),
                            )
                        })
                        .collect(),
                ),
            );
        }
        Value::Object(members)
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
