//! Handwritten prepared-library catalog policy. No acquisition I/O or scoping.
use crate::language_pack::{
    LibraryContext, PreparedLibrary, identity_key, library_package_identity,
};
use intersect_model::{
    core::{
        frozen::Model,
        project_usages::{DependencyLock, PackageCatalog, PackageIdentity, PackageManifest, Usage},
    },
    definitions::{bundle::SourceId, library_part::LibraryManifest, pack::FileDigest},
};
use semver::{Version, VersionReq};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

/// One prepared payload serves these exact OMG addresses. The last two are
/// published usage spellings of the quantities-and-units archive (see kpar-plan).
pub const STANDARD_LIBRARY_URIS: [&str; 12] = [
    "https://www.omg.org/spec/KerML/20250201/Semantic-Library.kpar",
    "https://www.omg.org/spec/KerML/20250201/Data-Type-Library.kpar",
    "https://www.omg.org/spec/KerML/20250201/Function-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Systems-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Analysis-Domain-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Cause-and-Effect-Domain-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Geometry-Domain-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Metadata-Domain-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Quantities-and-Units-Domain-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Requirement-Derivation-Domain-Library.kpar",
    "https://www.omg.org/spec/SysML/20250201/Quantities-and-Units-Library.kpar",
    "https://www.omg.org/spec/KerML/20250201/Quantities-and-Units-Library.kpar",
];
pub const IMPLICIT_STDLIB_URI: &str = STANDARD_LIBRARY_URIS[3];
// The pinned Pilot's own .project.json versions, not the published OMG 1.0/2.0
// archive versions. Exact source URLs and the distinction are in kpar-plan.md.
pub const PINNED_KERML_LIBRARY_VERSION: &str = "1.1.0-dev.20260801";
pub const PINNED_SYSML_LIBRARY_VERSION: &str = "2.1.0-dev.20260801";

pub fn implicit_stdlib_usage() -> Usage {
    Usage {
        resource_uri: IMPLICIT_STDLIB_URI.into(),
        version_constraint: None,
    }
}

#[derive(serde::Deserialize)]
struct PinnedSource {
    language: String,
    sha256: String,
    bytes: usize,
}
#[derive(serde::Deserialize)]
struct PinnedCorpus {
    libraries: Vec<PinnedSource>,
}

/// Match source content and parse language, allowing host-specific logical URI
/// prefixes. Those exact logical URIs remain protected by the package identity.
fn is_pinned_library(manifest: &LibraryManifest) -> Result<bool, String> {
    let pinned: PinnedCorpus = serde_json::from_str(include_str!(
        "../../../fixtures/native-pilot/v1/manifest.json"
    ))
    .map_err(|e| format!("Invalid embedded library profile: {e}"))?;
    let mut expected: Vec<_> = pinned
        .libraries
        .into_iter()
        .map(|source| (source.language, source.sha256, source.bytes))
        .collect();
    let mut actual: Vec<_> = manifest
        .entries
        .iter()
        .map(|source| (source.language.clone(), source.sha256.clone(), source.bytes))
        .collect();
    expected.sort();
    actual.sort();
    Ok(actual == expected)
}

/// Catalog aliases of a single prepared model package. Its own lock is empty:
/// dependencies between library parts are internal to this combined payload.
pub struct StdlibCatalog {
    identity: PackageIdentity,
    manifests: BTreeMap<String, PackageManifest>,
}
impl StdlibCatalog {
    pub fn new(library: &PreparedLibrary) -> Result<Self, String> {
        if library.key != identity_key(&library.identity)? {
            return Err("Prepared library cache key differs from its package identity".into());
        }
        if library.manifest.digest().map_err(|e| e.to_string())? != library.identity.content_digest
        {
            return Err("Prepared library manifest differs from its package identity".into());
        }
        let records = library
            .model
            .resource_records()
            .map_err(|e| e.to_string())?;
        if records
            .values()
            .any(|record| record.package.as_ref() != Some(&library.identity))
        {
            return Err("Prepared library resources differ from its package identity".into());
        }
        Self::from_identity(
            library.identity.clone(),
            is_pinned_library(&library.manifest)?,
        )
    }

    fn from_identity(identity: PackageIdentity, pinned: bool) -> Result<Self, String> {
        let dependency_lock = DependencyLock::empty();
        if identity.dependency_lock_digest != dependency_lock.digest()? {
            return Err("Prepared library must have an empty dependency lock".into());
        }
        let manifests = STANDARD_LIBRARY_URIS
            .iter()
            .enumerate()
            .map(|(index, uri)| {
                let version = if !pinned {
                    "unversioned"
                } else if index < 3 {
                    PINNED_KERML_LIBRARY_VERSION
                } else {
                    PINNED_SYSML_LIBRARY_VERSION
                };
                (
                    (*uri).to_owned(),
                    PackageManifest {
                        uri: (*uri).into(),
                        version: version.into(),
                        identity: identity.clone(),
                        provenance: SourceId {
                            uri: "urn:intersect:prepared-library".into(),
                            sha256: identity.content_digest.clone(),
                        },
                        usages: Vec::new(),
                        dependency_lock: dependency_lock.clone(),
                    },
                )
            })
            .collect();
        Ok(Self {
            identity,
            manifests,
        })
    }

    pub fn identity(&self) -> &PackageIdentity {
        &self.identity
    }
}
impl PackageCatalog for StdlibCatalog {
    fn validate_usage(&self, usage: &Usage) -> Result<(), String> {
        usage
            .version_constraint
            .as_deref()
            .map(VersionReq::parse)
            .transpose()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn candidates(&self, uri: &str, constraint: Option<&str>) -> Vec<PackageManifest> {
        let Ok(requirement) = constraint.map(VersionReq::parse).transpose() else {
            return Vec::new();
        };
        self.manifests
            .get(uri)
            .filter(|manifest| {
                requirement.as_ref().is_none_or(|requirement| {
                    Version::parse(&manifest.version)
                        .is_ok_and(|version| requirement.matches(&version))
                })
            })
            .cloned()
            .into_iter()
            .collect()
    }
}

/// Compatibility for the pre-package `load_on_library(&Model, ..)` API. Tagged
/// models retain their identity. An old untagged model lacks its source manifest:
/// bind its exact snapshot bytes to the current engine/schema, mark it, and use
/// the same lock/load path. It cannot claim a standard-library release version.
pub(crate) fn package_for_model<'a>(
    model: &'a Model,
    context: LibraryContext<'_>,
) -> Result<(Cow<'a, Model>, StdlibCatalog), String> {
    let records = model.resource_records().map_err(|e| e.to_string())?;
    let identities: BTreeSet<_> = records
        .values()
        .filter_map(|record| record.package.as_ref())
        .collect();
    if identities.len() > 1
        || (!identities.is_empty() && records.values().any(|record| record.package.is_none()))
    {
        return Err("Prepared library has mixed package origins".into());
    }
    if let Some(identity) = identities.first() {
        return Ok((
            Cow::Borrowed(model),
            StdlibCatalog::from_identity((*identity).clone(), false)?,
        ));
    }
    let mut identity = library_package_identity(
        &LibraryManifest {
            entries: Vec::new(),
        },
        context,
    )?;
    identity.content_digest =
        FileDigest::of(&model.to_snapshot(&[0; 32]).map_err(|e| e.to_string())?).sha256;
    let tagged = model
        .clone()
        .with_package_identity(&identity)
        .map_err(|e| e.to_string())?;
    Ok((
        Cow::Owned(tagged),
        StdlibCatalog::from_identity(identity, false)?,
    ))
}

/// Combine catalogs using this bridge's compatible SemVer constraint policy.
/// Source routes and library aliases remain outside core.
pub struct CombinedCatalog<'a>(pub &'a [&'a dyn PackageCatalog]);
impl PackageCatalog for CombinedCatalog<'_> {
    fn validate_usage(&self, usage: &Usage) -> Result<(), String> {
        for catalog in self.0 {
            catalog.validate_usage(usage)?;
        }
        Ok(())
    }
    fn candidates(&self, uri: &str, constraint: Option<&str>) -> Vec<PackageManifest> {
        self.0
            .iter()
            .flat_map(|catalog| catalog.candidates(uri, constraint))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pinned_profile_and_alias_versions_match_the_published_project_evidence() -> Result<(), String>
    {
        let pinned: PinnedCorpus = serde_json::from_str(include_str!(
            "../../../fixtures/native-pilot/v1/manifest.json"
        ))
        .map_err(|e| e.to_string())?;
        let mut manifest = LibraryManifest {
            entries: pinned
                .libraries
                .into_iter()
                .enumerate()
                .map(
                    |(i, source)| intersect_model::definitions::library_part::LibraryEntry {
                        uri: format!("host:/library/{i:03}"),
                        language: source.language,
                        sha256: source.sha256,
                        bytes: source.bytes,
                    },
                )
                .collect(),
        };
        assert!(is_pinned_library(&manifest)?);
        let identity = PackageIdentity {
            content_digest: manifest.digest().map_err(|e| e.to_string())?,
            engine: "engine".into(),
            schema: "schema".into(),
            dependency_lock_digest: DependencyLock::empty().digest()?,
        };
        let catalog = StdlibCatalog::from_identity(identity, true)?;
        let evidence: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/stdlib/pilot-project-versions.json"
        ))
        .map_err(|e| e.to_string())?;
        for project in evidence["projects"].as_array().ok_or("project evidence")? {
            let uri = project["uri"].as_str().ok_or("IRI")?;
            let version = project["version"].as_str().ok_or("version")?;
            let candidates = catalog.candidates(uri, Some(version));
            assert_eq!(candidates.len(), 1, "{uri}@{version}");
            assert_eq!(candidates[0].version, version);
        }
        assert!(
            catalog
                .candidates(STANDARD_LIBRARY_URIS[0], Some("1.0.0"))
                .is_empty()
        );
        assert_eq!(
            catalog.candidates(STANDARD_LIBRARY_URIS[11], None)[0].version,
            PINNED_SYSML_LIBRARY_VERSION
        );
        manifest.entries[0].language.push('x');
        assert!(!is_pinned_library(&manifest)?);
        Ok(())
    }
}
