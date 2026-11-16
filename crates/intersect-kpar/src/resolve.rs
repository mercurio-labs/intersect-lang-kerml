//! Handwritten (not generated): offline resolution of `usage` dependencies.
//!
//! A `usage` names a project by IRI. Resolution never fetches anything:
//!
//! - an OMG standard library archive IRI
//!   (`https://www.omg.org/spec/{KerML,SysML}/<release>/<Library>.kpar`) is
//!   provided by the host's prepared standard library;
//! - any other IRI resolves to the one candidate archive whose file name is
//!   the IRI's last path segment (the archives the user listed and those
//!   found on the host's project path).
//!
//! Mutual usage is allowed (OMG's own library archives use each other):
//! each archive loads once, and all load in one pass.
//!
//! A `versionConstraint` is a Semantic Versioning requirement (as Cargo
//! spells it: `1.0.0` means `^1.0.0`), checked against the candidate's
//! `version`. Constraints on standard library IRIs are recorded, not
//! checked: the prepared library carries no project version.
use crate::archive::{ErrorKind, KparError, METAMODEL_PREFIXES, ProjectArchive};
use std::collections::BTreeMap;

/// The OMG standard library archives (KerML and SysML 20250201 releases).
pub const STANDARD_LIBRARY_ARCHIVES: [&str; 10] = [
    "Semantic-Library.kpar",
    "Data-Type-Library.kpar",
    "Function-Library.kpar",
    "Systems-Library.kpar",
    "Analysis-Domain-Library.kpar",
    "Cause-and-Effect-Domain-Library.kpar",
    "Geometry-Domain-Library.kpar",
    "Metadata-Domain-Library.kpar",
    "Quantities-and-Units-Domain-Library.kpar",
    "Requirement-Derivation-Domain-Library.kpar",
];

/// Archive names OMG's own archives use for a standard library archive
/// published under another name: the Analysis and Geometry archives
/// (20250201) use `Quantities-and-Units-Library.kpar` for
/// `Quantities-and-Units-Domain-Library.kpar`.
pub const STANDARD_LIBRARY_ALIASES: [(&str, &str); 1] = [(
    "Quantities-and-Units-Library.kpar",
    "Quantities-and-Units-Domain-Library.kpar",
)];

/// Whether `iri` names an OMG standard library archive (or one of the
/// [`STANDARD_LIBRARY_ALIASES`]).
pub fn is_standard_library(iri: &str) -> bool {
    METAMODEL_PREFIXES.iter().any(|prefix| {
        iri.strip_prefix(prefix)
            .and_then(|rest| rest.split_once('/'))
            .is_some_and(|(release, name)| {
                release.len() == 8
                    && release.bytes().all(|b| b.is_ascii_digit())
                    && (STANDARD_LIBRARY_ARCHIVES.contains(&name)
                        || STANDARD_LIBRARY_ALIASES
                            .iter()
                            .any(|(alias, _)| *alias == name))
            })
    })
}

/// The release of the OMG standard library archives this crate names.
pub const STANDARD_LIBRARY_RELEASE: &str = "20250201";

/// The standard library archive a library folder is published as, by the
/// folder's path in the Pilot's `sysml.library` layout.
const STANDARD_LIBRARY_FOLDERS: [(&str, &str); 10] = [
    (
        "Kernel Libraries/Kernel Semantic Library/",
        "Semantic-Library.kpar",
    ),
    (
        "Kernel Libraries/Kernel Data Type Library/",
        "Data-Type-Library.kpar",
    ),
    (
        "Kernel Libraries/Kernel Function Library/",
        "Function-Library.kpar",
    ),
    ("/Systems Library/", "Systems-Library.kpar"),
    ("Domain Libraries/Analysis/", "Analysis-Domain-Library.kpar"),
    (
        "Domain Libraries/Cause and Effect/",
        "Cause-and-Effect-Domain-Library.kpar",
    ),
    ("Domain Libraries/Geometry/", "Geometry-Domain-Library.kpar"),
    ("Domain Libraries/Metadata/", "Metadata-Domain-Library.kpar"),
    (
        "Domain Libraries/Quantities and Units/",
        "Quantities-and-Units-Domain-Library.kpar",
    ),
    (
        "Domain Libraries/Requirement Derivation/",
        "Requirement-Derivation-Domain-Library.kpar",
    ),
];

/// The OMG archive IRI (release [`STANDARD_LIBRARY_RELEASE`]) of the
/// standard library archive a library resource belongs to: a file of one
/// of the OMG archives (`.../Function-Library.kpar!/BaseFunctions.kerml`)
/// or of the matching `sysml.library` folder
/// (`sysml.library/Kernel%20Libraries/Kernel%20Function%20Library/...`).
pub fn standard_library_iri(resource_uri: &str) -> Option<String> {
    let decoded = format!("/{}", percent_decode(resource_uri));
    let archive = match decoded.split_once(".kpar!/") {
        Some((archive, _)) => {
            let name = format!("{}.kpar", file_name(archive));
            STANDARD_LIBRARY_ARCHIVES
                .into_iter()
                .find(|known| *known == name)?
        }
        None => STANDARD_LIBRARY_FOLDERS
            .iter()
            .find(|(folder, _)| decoded.contains(&format!("/{}", folder.trim_start_matches('/'))))
            .map(|(_, archive)| *archive)?,
    };
    let language = match archive {
        "Semantic-Library.kpar" | "Data-Type-Library.kpar" | "Function-Library.kpar" => "KerML",
        _ => "SysML",
    };
    Some(format!(
        "https://www.omg.org/spec/{language}/{STANDARD_LIBRARY_RELEASE}/{archive}"
    ))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The decoded last path segment of a URI or IRI (query and fragment
/// dropped; for a URN, the part after the last `:`): the file name a
/// `usage` and an archive are matched on.
pub fn file_name(uri: &str) -> String {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    percent_decode(path.rsplit(['/', ':']).next().unwrap_or(path))
}

/// A standard library `usage`, provided by the host's prepared library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandardLibraryUse {
    /// The archive that declared it.
    pub by: String,
    pub resource: String,
    pub version_constraint: Option<String>,
}

/// The archives to load and the standard library uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    /// Indices into the candidates: every root and everything it uses,
    /// each once, dependencies before the archives that use them (except
    /// within a cycle).
    pub order: Vec<usize>,
    pub standard_library: Vec<StandardLibraryUse>,
}

fn check_version(
    user: &ProjectArchive,
    resource: &str,
    constraint: &str,
    used: &ProjectArchive,
) -> Result<(), KparError> {
    let refuse = |message: String| {
        KparError::new(
            &user.uri,
            Some(".project.json"),
            ErrorKind::Dependency,
            message,
        )
    };
    let requirement = semver::VersionReq::parse(constraint).map_err(|e| {
        refuse(format!(
            "versionConstraint '{constraint}' of {resource} is not a Semantic Versioning requirement: {e}"
        ))
    })?;
    let version = semver::Version::parse(&used.project.version).map_err(|e| {
        refuse(format!(
            "{resource} resolves to {}, whose version '{}' is not a Semantic Version: {e}",
            used.uri, used.project.version
        ))
    })?;
    if requirement.matches(&version) {
        Ok(())
    } else {
        Err(refuse(format!(
            "{resource} resolves to {} version {version}, which does not satisfy '{constraint}'",
            used.uri
        )))
    }
}

/// Resolve the dependencies of `roots` (indices into `candidates`).
/// Every problem is returned, not just the first.
pub fn resolve(
    candidates: &[ProjectArchive],
    roots: &[usize],
) -> Result<Resolution, Vec<KparError>> {
    let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, archive) in candidates.iter().enumerate() {
        by_name
            .entry(file_name(&archive.uri))
            .or_default()
            .push(index);
    }
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        New,
        Open,
        Done,
    }
    struct Walk<'a> {
        candidates: &'a [ProjectArchive],
        by_name: BTreeMap<String, Vec<usize>>,
        state: Vec<State>,
        stack: Vec<usize>,
        resolution: Resolution,
        errors: Vec<KparError>,
    }
    impl Walk<'_> {
        fn visit(&mut self, index: usize) {
            match self.state[index] {
                State::Done => return,
                // A cycle (the OMG Semantic and Function libraries use each
                // other): the archive is already being loaded. Every archive
                // loads in one pass, so a cycle needs no order.
                State::Open => return,
                State::New => {}
            }
            self.state[index] = State::Open;
            self.stack.push(index);
            let archive = &self.candidates[index];
            for usage in &archive.project.usage {
                if is_standard_library(&usage.resource) {
                    self.resolution.standard_library.push(StandardLibraryUse {
                        by: archive.uri.clone(),
                        resource: usage.resource.clone(),
                        version_constraint: usage.version_constraint.clone(),
                    });
                    continue;
                }
                let name = file_name(&usage.resource);
                let found = self.by_name.get(&name).cloned().unwrap_or_default();
                let used = match found.as_slice() {
                    [used] => *used,
                    [] => {
                        self.errors.push(KparError::new(
                            &archive.uri,
                            Some(".project.json"),
                            ErrorKind::Dependency,
                            format!(
                                "usage {} is not resolved: no archive named '{name}' was given or found on the project path",
                                usage.resource
                            ),
                        ));
                        continue;
                    }
                    several => {
                        let uris: Vec<&str> = several
                            .iter()
                            .map(|&i| self.candidates[i].uri.as_str())
                            .collect();
                        self.errors.push(KparError::new(
                            &archive.uri,
                            Some(".project.json"),
                            ErrorKind::Dependency,
                            format!(
                                "usage {} is ambiguous: {} archives are named '{name}' ({})",
                                usage.resource,
                                several.len(),
                                uris.join(", ")
                            ),
                        ));
                        continue;
                    }
                };
                if let Some(constraint) = &usage.version_constraint {
                    let checked =
                        check_version(archive, &usage.resource, constraint, &self.candidates[used]);
                    if let Err(error) = checked {
                        self.errors.push(error);
                    }
                }
                self.visit(used);
            }
            self.stack.pop();
            self.state[index] = State::Done;
            self.resolution.order.push(index);
        }
    }
    let mut walk = Walk {
        candidates,
        by_name,
        state: vec![State::New; candidates.len()],
        stack: Vec::new(),
        resolution: Resolution::default(),
        errors: Vec::new(),
    };
    for &root in roots {
        if root < candidates.len() {
            walk.visit(root);
        }
    }
    if walk.errors.is_empty() {
        Ok(walk.resolution)
    } else {
        Err(walk.errors)
    }
}

#[cfg(test)]
#[path = "resolve_tests.rs"]
mod tests;
