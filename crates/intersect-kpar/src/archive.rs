//! Handwritten (not generated): reading and writing a project archive.
//!
//! [`read`] checks the container and both metadata documents, verifies the
//! checksums it can, and returns the textual model files with resource URIs
//! that keep the archive and the entry path (`<archive>!/<path>`). Content
//! this crate does not load (other serializations, derived or implied
//! values, nested archives, other metamodels) is refused, never skipped
//! silently; files that are not model content are listed as notes.
use crate::project::{Checksum, META_FILE, Meta, PROJECT_FILE, Project};
use crate::zip::{self, ArchiveLimits, ZipEntry, ZipError};
use sha2::Digest;
use std::fmt;

/// The textual notation a model file is in, from its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    KerML,
    SysML,
}

impl Language {
    pub fn of_path(path: &str) -> Option<Self> {
        if path.ends_with(".kerml") {
            Some(Self::KerML)
        } else if path.ends_with(".sysml") {
            Some(Self::SysML)
        } else {
            None
        }
    }
}

/// One textual model file of an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveSource {
    /// The path relative to the project root (what `.meta.json` names).
    pub path: String,
    /// The entry path inside the archive (the project root's folder, if
    /// any, then `path`).
    pub entry: String,
    /// The resource URI it loads under: `<archive uri>!/<encoded entry>`.
    pub uri: String,
    pub language: Language,
    pub text: String,
}

/// A read and checked project archive.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectArchive {
    /// The URI the archive was read under (its resources' prefix).
    pub uri: String,
    /// The folder holding `.project.json` and `.meta.json`, with a trailing
    /// `/`; empty when they are at the archive root.
    pub root: String,
    pub project: Project,
    pub meta: Meta,
    /// The model files, sorted by path.
    pub sources: Vec<ArchiveSource>,
    /// Things a reader should know that do not refuse the archive (ignored
    /// files, checksums that could not be verified).
    pub notes: Vec<String>,
    /// Metadata faults that do not stop the files from loading, such as an
    /// index entry naming a file the archive does not hold (OMG's
    /// Requirement Derivation archive has one).
    pub warnings: Vec<String>,
}

/// What kind of problem refused an archive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The ZIP container is damaged or uses an unsupported feature.
    Container,
    /// Over an [`ArchiveLimits`] bound.
    Limit,
    /// An unsafe or duplicate entry path.
    Path,
    /// A metadata document is missing or invalid.
    Metadata,
    /// A file does not match its checksum.
    Checksum,
    /// Content or metadata this reader does not load.
    Unsupported,
    /// A `usage` dependency cannot be resolved.
    Dependency,
}

/// Why an archive was refused: the archive, the entry when one is at fault,
/// and the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KparError {
    pub archive: String,
    pub entry: Option<String>,
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for KparError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.entry {
            Some(entry) => write!(f, "{}!/{entry}: {}", self.archive, self.message),
            None => write!(f, "{}: {}", self.archive, self.message),
        }
    }
}

impl std::error::Error for KparError {}

impl KparError {
    pub fn new(archive: &str, entry: Option<&str>, kind: ErrorKind, message: String) -> Self {
        Self {
            archive: archive.to_owned(),
            entry: entry.map(str::to_owned),
            kind,
            message,
        }
    }
    fn zip(archive: &str, error: ZipError) -> Self {
        let kind = match &error {
            ZipError::Corrupt(_) | ZipError::Unsupported(_) => ErrorKind::Container,
            ZipError::Limit(_) => ErrorKind::Limit,
            ZipError::Path { .. } => ErrorKind::Path,
        };
        Self::new(archive, None, kind, error.to_string())
    }
}

/// The metamodel IRI prefixes of the languages this package loads.
pub const METAMODEL_PREFIXES: [&str; 2] = [
    "https://www.omg.org/spec/KerML/",
    "https://www.omg.org/spec/SysML/",
];

/// Extensions of model content in serializations this crate does not load.
const UNSUPPORTED_CONTENT: [&str; 7] = [
    ".json", ".xmi", ".kermlx", ".sysmlx", ".xml", ".kpar", ".zip",
];

/// `path` with every character outside the IRI path characters
/// (unreserved, sub-delims, `:`, `@`, `/`) percent-encoded as UTF-8.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        let keep = byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte);
        if keep {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The resource URI of entry `path` of the archive read as `archive_uri`.
pub fn entry_uri(archive_uri: &str, path: &str) -> String {
    format!("{archive_uri}!/{}", encode_path(path))
}

/// The hex digest of `bytes` under a checksum algorithm name, or `None`
/// for an algorithm this crate does not compute.
pub fn digest(algorithm: &str, bytes: &[u8]) -> Option<String> {
    fn hex(digest: &[u8]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }
    Some(match algorithm {
        "SHA224" | "SHA-224" => hex(&sha2::Sha224::digest(bytes)),
        "SHA256" | "SHA-256" => hex(&sha2::Sha256::digest(bytes)),
        "SHA384" | "SHA-384" => hex(&sha2::Sha384::digest(bytes)),
        "SHA512" | "SHA-512" => hex(&sha2::Sha512::digest(bytes)),
        _ => return None,
    })
}

/// The project folder of an archive: where its one `.project.json` is
/// (the archive root, or a folder such as `Kernel Semantic Library/` in
/// the OMG library archives), with a trailing `/`.
fn project_root(entries: &[ZipEntry]) -> Result<String, String> {
    let roots: Vec<&str> = entries
        .iter()
        .filter_map(|entry| {
            if entry.path == PROJECT_FILE {
                Some("")
            } else {
                entry
                    .path
                    .strip_suffix(&format!("/{PROJECT_FILE}"))
                    .map(|_| &entry.path[..entry.path.len() - PROJECT_FILE.len()])
            }
        })
        .collect();
    match roots.as_slice() {
        [root] => Ok((*root).to_owned()),
        [] => Err(format!("no {PROJECT_FILE} in the archive")),
        several => Err(format!(
            "{} {PROJECT_FILE} files ({}): the project folder is ambiguous",
            several.len(),
            several
                .iter()
                .map(|root| format!("{root}{PROJECT_FILE}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Read and check the archive `bytes`, naming its resources after
/// `archive_uri` (already a valid URI path, such as `deps/vehicles.kpar`).
pub fn read(
    archive_uri: &str,
    bytes: &[u8],
    limits: &ArchiveLimits,
) -> Result<ProjectArchive, KparError> {
    let error = |entry: Option<&str>, kind, message: String| {
        KparError::new(archive_uri, entry, kind, message)
    };
    let entries = zip::read(bytes, limits).map_err(|e| KparError::zip(archive_uri, e))?;
    let root =
        project_root(&entries).map_err(|message| error(None, ErrorKind::Metadata, message))?;
    let find = |name: &str| {
        let wanted = format!("{root}{name}");
        entries.iter().find(|entry| entry.path == wanted)
    };
    let metadata = |e: crate::project::MetadataError| {
        error(
            Some(&format!("{root}{}", e.file)),
            ErrorKind::Metadata,
            e.message,
        )
    };
    let project = find(PROJECT_FILE)
        .ok_or_else(|| error(None, ErrorKind::Metadata, format!("no {PROJECT_FILE}")))
        .and_then(|entry| Project::parse(&entry.bytes).map_err(metadata))?;
    let meta = find(META_FILE)
        .ok_or_else(|| {
            error(
                None,
                ErrorKind::Metadata,
                format!("no {META_FILE} beside {root}{PROJECT_FILE}"),
            )
        })
        .and_then(|entry| Meta::parse(&entry.bytes).map_err(metadata))?;
    let mut notes = Vec::new();

    let foreign = meta.metamodel.as_deref().filter(|metamodel| {
        !METAMODEL_PREFIXES
            .iter()
            .any(|prefix| metamodel.starts_with(prefix))
    });
    if let Some(metamodel) = foreign {
        return Err(error(
            Some(META_FILE),
            ErrorKind::Unsupported,
            format!("metamodel {metamodel} is not the OMG KerML or SysML metamodel"),
        ));
    }
    for (flag, value) in [
        ("includesDerived", meta.includes_derived),
        ("includesImplied", meta.includes_implied),
    ] {
        if value == Some(true) {
            return Err(error(
                Some(META_FILE),
                ErrorKind::Unsupported,
                format!("{flag} is true: derived and implied values are not loaded from archives"),
            ));
        }
    }

    for (path, Checksum { value, algorithm }) in &meta.checksum {
        let Some(entry) = find(path) else {
            return Err(error(
                Some(META_FILE),
                ErrorKind::Checksum,
                format!("checksum for '{path}', which is not in the archive"),
            ));
        };
        if algorithm == "NONE" {
            notes.push(format!("{path}: checksum algorithm NONE, not verified"));
            continue;
        }
        let Some(actual) = digest(algorithm, &entry.bytes) else {
            return Err(error(
                Some(path),
                ErrorKind::Unsupported,
                format!(
                    "checksum algorithm {algorithm} is not supported (SHA224, SHA256, SHA384, SHA512)"
                ),
            ));
        };
        if !actual.eq_ignore_ascii_case(value) {
            return Err(error(
                Some(path),
                ErrorKind::Checksum,
                format!(
                    "{algorithm} checksum mismatch: .meta.json says {value}, the file is {actual}"
                ),
            ));
        }
    }

    let mut sources = Vec::new();
    for ZipEntry { path: entry, bytes } in &entries {
        let Some(path) = entry.strip_prefix(root.as_str()) else {
            notes.push(format!(
                "{entry}: outside the project folder {root}, not loaded"
            ));
            continue;
        };
        if path == PROJECT_FILE || path == META_FILE {
            continue;
        }
        if let Some(language) = Language::of_path(path) {
            let text = std::str::from_utf8(bytes).map_err(|_| {
                error(
                    Some(path),
                    ErrorKind::Unsupported,
                    "model file is not UTF-8".into(),
                )
            })?;
            sources.push(ArchiveSource {
                path: path.to_owned(),
                entry: entry.clone(),
                uri: entry_uri(archive_uri, entry),
                language,
                text: text.trim_start_matches('\u{feff}').to_owned(),
            });
        } else if let Some(extension) = UNSUPPORTED_CONTENT
            .iter()
            .find(|extension| path.to_ascii_lowercase().ends_with(*extension))
        {
            return Err(error(
                Some(path),
                ErrorKind::Unsupported,
                format!(
                    "{extension} content is not loaded: only KerML/SysML textual notation (.kerml, .sysml)"
                ),
            ));
        } else {
            notes.push(format!("{path}: not a model file, ignored"));
        }
    }
    let mut warnings = Vec::new();
    for (name, path) in &meta.index {
        if !sources.iter().any(|source| &source.path == path) {
            warnings.push(format!(
                "{root}{META_FILE}: index entry '{name}' names '{path}', which is not a model file of the archive"
            ));
        }
    }
    if sources.is_empty() {
        notes.push("the archive holds no model files".into());
    }
    Ok(ProjectArchive {
        uri: archive_uri.to_owned(),
        root,
        project,
        meta,
        sources,
        notes,
        warnings,
    })
}

/// One textual model file to write into an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveFile {
    pub path: String,
    pub text: String,
}

fn json_bytes(value: &serde_json::Value) -> Vec<u8> {
    // Serializing a `Value` cannot fail.
    let mut bytes = serde_json::to_vec_pretty(value).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}

/// Write a project archive of `files` with `project` and `meta`. The
/// SHA256 checksum of every file replaces `meta.checksum`; derived and
/// implied values are declared absent. Equal inputs give equal bytes.
pub fn write(project: &Project, meta: &Meta, files: &[ArchiveFile]) -> Result<Vec<u8>, KparError> {
    let error = |entry: Option<&str>, kind, message: String| {
        KparError::new("<output>", entry, kind, message)
    };
    if project.name.is_empty() || project.version.is_empty() {
        return Err(error(
            Some(PROJECT_FILE),
            ErrorKind::Metadata,
            "name and version must not be empty".into(),
        ));
    }
    if !crate::project::is_date_time(&meta.created) {
        return Err(error(
            Some(META_FILE),
            ErrorKind::Metadata,
            format!("'created' is not an RFC 3339 date-time: {}", meta.created),
        ));
    }
    let mut meta = meta.clone();
    meta.includes_derived = Some(false);
    meta.includes_implied = Some(false);
    meta.checksum.clear();
    let mut entries = Vec::with_capacity(files.len() + 2);
    for file in files {
        if Language::of_path(&file.path).is_none() {
            return Err(error(
                Some(&file.path),
                ErrorKind::Unsupported,
                "not a .kerml or .sysml file".into(),
            ));
        }
        let bytes = file.text.as_bytes().to_vec();
        meta.checksum.insert(
            file.path.clone(),
            Checksum {
                value: digest("SHA256", &bytes).unwrap_or_default(),
                algorithm: "SHA256".into(),
            },
        );
        entries.push(ZipEntry {
            path: file.path.clone(),
            bytes,
        });
    }
    for (name, path) in &meta.index {
        if !files.iter().any(|file| &file.path == path) {
            return Err(error(
                Some(META_FILE),
                ErrorKind::Metadata,
                format!("index entry '{name}' names '{path}', which is not written"),
            ));
        }
    }
    entries.push(ZipEntry {
        path: PROJECT_FILE.into(),
        bytes: json_bytes(&project.to_value()),
    });
    entries.push(ZipEntry {
        path: META_FILE.into(),
        bytes: json_bytes(&meta.to_value()),
    });
    zip::write(&entries).map_err(|e| KparError::zip("<output>", e))
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
