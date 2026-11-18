//! Native prototype artifact cache and acquisition orchestration.
use intersect_kerml::components::SysmlComponentLanguage;
use intersect_kpar_host::{
    KparCatalog, KparSource,
    hosted::{self, FetchLimits},
};
use intersect_model::{
    components::{ComponentDescriptor, ComponentImage},
    definitions::{bundle::SourceId, pack::FileDigest},
    project_usages::{DependencyLock, PackageManifest, Usage, UsageResolver},
};
use intersect_project_host::PackageSource;
use intersect_text::{
    Source,
    components::{ComponentLanguage, ComponentSession},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    format: String,
    archive_sha256: String,
    requested_url: String,
    effective_url: String,
    compiler: BTreeMap<String, String>,
    descriptor: ComponentDescriptor,
    manifest: PackageManifest,
}

#[derive(Clone)]
pub struct PreparedUsage {
    pub image: Arc<ComponentImage>,
    pub key: String,
    pub archive_sha256: String,
    pub manifest: PackageManifest,
    pub cache_hit: bool,
}

pub struct UsageStore<'a, 'b> {
    pub language: &'a SysmlComponentLanguage<'b>,
    pub root: PathBuf,
    pool: BTreeMap<String, PreparedUsage>,
    pub preparations: usize,
    pub restores: usize,
    pub downloads: usize,
}

fn compiler() -> BTreeMap<String, String> {
    [
        ("model".into(), intersect_model::SOURCE_IDENTITY.into()),
        ("text".into(), intersect_text::SOURCE_IDENTITY.into()),
        ("kerml".into(), intersect_kerml::SOURCE_IDENTITY.into()),
        (
            "recipe".into(),
            FileDigest::of(include_bytes!("usage.rs")).sha256,
        ),
    ]
    .into()
}
fn key(uri: &str, archive_sha256: &str) -> Result<String, String> {
    Ok(FileDigest::of(
        &serde_json::to_vec(&("intersect.hosted-kpar.v1", uri, archive_sha256, compiler()))
            .map_err(|e| e.to_string())?,
    )
    .sha256)
}
fn checked_key(key: &str) -> Result<(), String> {
    if key.len() != 64
        || !key
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("Invalid component cache key".into());
    }
    Ok(())
}
fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err(format!("Cache file exceeds its limit: {}", path.display()));
    }
    Ok(bytes)
}

impl<'a, 'b> UsageStore<'a, 'b> {
    pub fn new(language: &'a SysmlComponentLanguage<'b>, root: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        Ok(Self {
            language,
            root: root.into(),
            pool: BTreeMap::new(),
            preparations: 0,
            restores: 0,
            downloads: 0,
        })
    }

    pub fn add_hosted(&mut self, url: &str, uri: &str) -> Result<PreparedUsage, String> {
        self.downloads += 1;
        let downloaded =
            hosted::download(url, &self.root.join("downloads"), FetchLimits::default())?;
        self.prepare_bytes(
            uri,
            &downloaded.requested_url,
            &downloaded.effective_url,
            &downloaded.bytes,
        )
    }

    pub fn prepare_bytes(
        &mut self,
        uri: &str,
        requested_url: &str,
        effective_url: &str,
        bytes: &[u8],
    ) -> Result<PreparedUsage, String> {
        let archive_sha256 = FileDigest::of(bytes).sha256;
        let key = key(uri, &archive_sha256)?;
        if let Some(cached) = self.pool.get(&key) {
            let mut cached = cached.clone();
            cached.cache_hit = true;
            return Ok(cached);
        }
        let directory = self.root.join("components").join(&key);
        if directory.exists() {
            return self.restore(&key);
        }
        // Acquisition location is provenance, not the identity of an element.
        // A content URI makes the prepared image stable across HTTP mirrors.
        let archive_uri = format!("urn:intersect:kpar:sha256:{archive_sha256}");
        let package = KparSource {
            package_uri: uri,
            archive_uri: &archive_uri,
            bytes,
            limits: Default::default(),
        }
        .acquire()
        .map_err(|e| e.to_string())?;
        if !package.usages.is_empty() {
            return Err(format!(
                "This prototype requires a self-contained textual KPAR; unresolved archive usages: {:?}",
                package.usages
            ));
        }
        let sources: Vec<Source> = package
            .payload
            .archive
            .sources
            .iter()
            .map(|source| Source {
                uri: source.uri.clone(),
                text: source.text.clone(),
            })
            .collect();
        if sources.is_empty() {
            return Err("KPAR contains no textual SysML/KerML model sources".into());
        }
        for note in &package.payload.archive.notes {
            println!("KPAR note: {note}");
        }
        for warning in &package.payload.archive.warnings {
            println!("KPAR metadata warning: {warning}");
        }
        self.preparations += 1;
        let (image, snapshot) = self
            .language
            .prepare_component_with_snapshot(uri, &sources)?;
        let manifest = PackageManifest {
            uri: uri.into(),
            version: package.version,
            provenance: SourceId {
                uri: effective_url.into(),
                sha256: archive_sha256.clone(),
            },
            identity: image.identity().clone(),
            usages: vec![],
            dependency_lock: DependencyLock::empty(),
        };
        let entry = Entry {
            format: "intersect.hosted-kpar.v1".into(),
            archive_sha256: archive_sha256.clone(),
            requested_url: requested_url.into(),
            effective_url: effective_url.into(),
            compiler: compiler(),
            descriptor: image.descriptor(),
            manifest: manifest.clone(),
        };
        write_entry(&directory, &entry, &snapshot, bytes)?;
        let prepared = PreparedUsage {
            image,
            key: key.clone(),
            archive_sha256,
            manifest,
            cache_hit: false,
        };
        self.pool.insert(key, prepared.clone());
        Ok(prepared)
    }

    /// Locked local loading never downloads or silently rebuilds an artifact.
    pub fn restore(&mut self, cache_key: &str) -> Result<PreparedUsage, String> {
        checked_key(cache_key)?;
        if let Some(cached) = self.pool.get(cache_key) {
            let mut cached = cached.clone();
            cached.cache_hit = true;
            return Ok(cached);
        }
        let directory = self.root.join("components").join(cache_key);
        let entry: Entry =
            serde_json::from_slice(&read_bounded(&directory.join("entry.json"), 1024 * 1024)?)
                .map_err(|e| e.to_string())?;
        if entry.format != "intersect.hosted-kpar.v1"
            || entry.compiler != compiler()
            || key(&entry.manifest.uri, &entry.archive_sha256)? != cache_key
            || entry.manifest.provenance.sha256 != entry.archive_sha256
            || entry.manifest.provenance.uri != entry.effective_url
            || !entry.manifest.usages.is_empty()
            || entry.manifest.dependency_lock != DependencyLock::empty()
        {
            return Err("Cached KPAR preparation context is incompatible".into());
        }
        let source = read_bounded(&directory.join("source.kpar"), 16 * 1024 * 1024)?;
        if FileDigest::of(&source).sha256 != entry.archive_sha256 {
            return Err("Cached KPAR source digest mismatch".into());
        }
        let snapshot = read_bounded(&directory.join("image.bin"), 256 * 1024 * 1024)?;
        let image = ComponentImage::restore(self.language.context(), &entry.descriptor, &snapshot)?;
        if image.identity() != &entry.manifest.identity || image.uri() != entry.manifest.uri {
            return Err("Cached KPAR manifest differs from its image".into());
        }
        self.restores += 1;
        let prepared = PreparedUsage {
            image,
            key: cache_key.into(),
            archive_sha256: entry.archive_sha256,
            manifest: entry.manifest,
            cache_hit: true,
        };
        self.pool.insert(cache_key.into(), prepared.clone());
        Ok(prepared)
    }

    pub fn lock(&self, usages: &[Arc<ComponentImage>]) -> Result<DependencyLock, String> {
        let mut catalog = KparCatalog::default();
        let mut requested = Vec::new();
        for image in usages {
            let prepared = self
                .pool
                .values()
                .find(|item| {
                    item.image.identity() == image.identity() && item.image.uri() == image.uri()
                })
                .ok_or("Mounted artifact has no prepared KPAR manifest")?;
            catalog.register_manifest(prepared.manifest.clone());
            requested.push(Usage {
                resource_uri: image.uri().into(),
                version_constraint: None,
            });
        }
        UsageResolver::resolve(&requested, &catalog).map_err(|e| e.to_string())
    }

    pub fn adopt(
        &self,
        project: &mut ComponentSession<'_>,
        usage: &PreparedUsage,
    ) -> Result<(), String> {
        let mut usages = project.usages().to_vec();
        usages.push(usage.image.clone());
        let lock = self.lock(&usages)?;
        let proposal = project.prepare_locked_usages(usages, lock)?;
        project.adopt_usages(proposal)
    }
}

fn write_entry(
    directory: &Path,
    entry: &Entry,
    snapshot: &[u8],
    archive: &[u8],
) -> Result<(), String> {
    let parent = directory.parent().ok_or("Missing artifact directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let staging = directory.with_extension(format!("staging-{}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    struct Stage(PathBuf);
    impl Drop for Stage {
        fn drop(&mut self) {
            for name in ["entry.json", "image.bin", "source.kpar"] {
                let _ = std::fs::remove_file(self.0.join(name));
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }
    let staged = Stage(staging);
    std::fs::write(staged.0.join("source.kpar"), archive).map_err(|e| e.to_string())?;
    std::fs::write(staged.0.join("image.bin"), snapshot).map_err(|e| e.to_string())?;
    std::fs::write(
        staged.0.join("entry.json"),
        serde_json::to_vec_pretty(entry).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&staged.0, directory)
        .map_err(|e| format!("Artifact publication failed: {e}"))?;
    Ok(())
}
