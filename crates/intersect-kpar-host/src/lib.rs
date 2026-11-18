//! Handwritten host bridge from checked KerML project archives to core locks.
#[cfg(all(feature = "components", not(target_arch = "wasm32")))]
pub mod hosted;
use intersect_kpar::{ArchiveLimits, ErrorKind, KparError, ProjectArchive};
use intersect_model::{
    definitions::{bundle::SourceId, pack::FileDigest},
    project_usages::{PackageCatalog, PackageIdentity, PackageManifest, Usage},
};
use intersect_project_host::{ManifestError, PackageSource, SourcePackage};
use iri_string::types::IriStr;
use semver::{Version, VersionReq};
use std::collections::BTreeMap;

/// Archive bytes and explicit logical and acquisition addresses supplied by a host.
pub struct KparSource<'a> {
    pub package_uri: &'a str,
    pub archive_uri: &'a str,
    pub bytes: &'a [u8],
    pub limits: ArchiveLimits,
}

/// The checked archive with its diagnostics and exact original bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct KparPayload<'a> {
    pub archive: ProjectArchive,
    pub bytes: &'a [u8],
}

impl<'a> PackageSource for KparSource<'a> {
    type Payload = KparPayload<'a>;
    type Error = KparError;

    fn acquire(&self) -> Result<SourcePackage<Self::Payload>, KparError> {
        IriStr::new(self.package_uri).map_err(|error| {
            KparError::new(
                self.archive_uri,
                None,
                ErrorKind::Dependency,
                format!(
                    "Invalid logical package IRI {:?}: {error}",
                    self.package_uri
                ),
            )
        })?;
        let archive = intersect_kpar::read(self.archive_uri, self.bytes, &self.limits)?;
        let usages = archive
            .project
            .usage
            .iter()
            .map(|usage| {
                if let Some(constraint) = &usage.version_constraint {
                    VersionReq::parse(constraint).map_err(|error| {
                        KparError::new(
                            self.archive_uri,
                            Some(&format!("{}.project.json", archive.root)),
                            ErrorKind::Dependency,
                            format!(
                                "versionConstraint {constraint:?} of {} is not a Semantic Versioning requirement: {error}",
                                usage.resource
                            ),
                        )
                    })?;
                }
                Ok(Usage {
                    resource_uri: usage.resource.clone(),
                    version_constraint: usage.version_constraint.clone(),
                })
            })
            .collect::<Result<_, _>>()?;
        let digest = FileDigest::of(self.bytes).sha256;
        Ok(SourcePackage {
            uri: self.package_uri.to_owned(),
            version: archive.project.version.clone(),
            provenance: SourceId {
                uri: self.archive_uri.to_owned(),
                sha256: digest.clone(),
            },
            content_digest: digest,
            usages,
            payload: KparPayload {
                archive,
                bytes: self.bytes,
            },
        })
    }
}

/// In-memory catalog of checked, lock-bound archive manifests.
#[derive(Default)]
pub struct KparCatalog {
    manifests: BTreeMap<String, Vec<PackageManifest>>,
}

impl KparCatalog {
    /// Resolve and register a package after registering its dependencies.
    /// The host supplies exact identities for the engine and schema.
    pub fn register(
        &mut self,
        package: &SourcePackage<KparPayload<'_>>,
        engine: &str,
        schema: &str,
    ) -> Result<PackageIdentity, ManifestError> {
        let manifest = package.manifest(engine, schema, self)?;
        Ok(self.register_manifest(manifest))
    }

    /// Register a manifest already resolved by `SourcePackage::manifest`
    /// against an external/combined catalog. This lets a host stage that
    /// resolution while borrowing this catalog, then publish the manifest.
    pub fn register_manifest(&mut self, manifest: PackageManifest) -> PackageIdentity {
        let identity = manifest.identity.clone();
        let candidates = self.manifests.entry(manifest.uri.clone()).or_default();
        if !candidates.contains(&manifest) {
            candidates.push(manifest);
        }
        identity
    }
}

impl PackageCatalog for KparCatalog {
    fn validate_usage(&self, usage: &Usage) -> Result<(), String> {
        usage
            .version_constraint
            .as_deref()
            .map(VersionReq::parse)
            .transpose()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn candidates(&self, uri: &str, constraint: Option<&str>) -> Vec<PackageManifest> {
        let requirement = match constraint.map(VersionReq::parse).transpose() {
            Ok(requirement) => requirement,
            Err(_) => return Vec::new(),
        };
        self.manifests
            .get(uri)
            .into_iter()
            .flatten()
            .filter(|manifest| {
                requirement.as_ref().is_none_or(|requirement| {
                    Version::parse(&manifest.version)
                        .is_ok_and(|version| requirement.matches(&version))
                })
            })
            .cloned()
            .collect()
    }
}
