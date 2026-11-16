//! Handwritten prepared-standard-library part of the KerML/SysML language
//! pack (roadmap decision D10). Not generated.
//!
//! The optional `library/sysml` part holds the SysML v2 standard library
//! (every `.kerml` and `.sysml` library file) parsed, linked, completed and
//! validated, as a content-keyed binary model snapshot, together with the
//! library manifest and the library source texts
//! (`intersect_model::definitions::library_part`). A host asks for the
//! prepared library with [`prepare_library`]: the snapshot is loaded when its
//! key equals the key recomputed for this binary; a missing part, another
//! key or bytes that fail any check fall back to parsing the sources (the
//! host's own, else those stored in the part), and the host saves the new
//! payload when it can write ([`with_library_part`]).
//!
//! The key ([`library_key`]) hashes the library manifest (logical URI,
//! language and SHA-256 of each source), the part keys of both program parts
//! (which parse it), the identity of the Rcore bindings that link and
//! complete it (a host input: the combined KerML+SysML bindings are compiled
//! by the host), `Metamodel::digest` of the metamodel it is stored under,
//! the program compiler components (the crates that parse, link, complete
//! and encode it) and both payload formats. The part's manifest entry
//! carries that key as its `input_digest`, lists the program compiler
//! components, and has no inputs or dependencies of its own: its key covers
//! the programs' keys directly. Reuse during a pack build is
//! `plan_parts` over that entry ([`plan_library`]).
//!
//! `build_language_pack` rebuilds the three recipe parts and does not carry
//! a library part over: a host adds it afterwards (reusing the previous
//! payload when `plan_library` says so). The library is a derived cache; no
//! other part depends on it.
//!
//! This module performs no I/O and uses no clocks, threads or environment.
use super::{
    LANGUAGE, PROGRAM_COMPONENTS, PartDecision, PartKeyInputs, RebuildReason, check_layout,
    expected_compiler, plan_parts, recipe,
};
use crate::native_pilot::ProgramRole;
use intersect_model::{
    core::{
        frozen::{
            Metamodel, Model as FrozenModel, SNAPSHOT_FORMAT, SnapshotTrust, snapshot_header,
        },
        project_usages::{DependencyLock, PackageIdentity},
    },
    definitions::{
        library_part::{
            LIBRARY_KEY_DOMAIN, LIBRARY_PART_FORMAT, LibraryKeyInput, LibraryManifest,
            LibrarySource, decode_library_part, encode_library_part, key_bytes,
        },
        pack::{
            FileDigest, LanguagePack, PackBuilder, PackHeader, PackPart, PackRecipe,
            PackSectionEncoding, PackSectionFlag, PackSectionFlags, canonical_json,
        },
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// The section (and part) holding the prepared standard library.
pub const LIBRARY_SECTION: &str = "library/sysml";
/// Section kind of the library part.
pub const LIBRARY_KIND: &str = "intersect.kerml.library-snapshot.v1";
/// Bump when what the library part stores changes without a format change.
const LIBRARY_PART_VERSION: u32 = 2;
/// The `language` of a library source parsed with the KerML program.
pub const KERML_SOURCE: &str = "kerml";
/// The `language` of a library source parsed with the SysML program.
pub const SYSML_SOURCE: &str = "sysml";

/// What a prepared library is keyed on besides its sources.
#[derive(Clone, Copy)]
pub struct LibraryContext<'a> {
    /// The metamodel the library is stored under (both programs share it).
    pub metamodel: &'a Arc<Metamodel>,
    /// Identity of the Rcore bindings that link and complete the library,
    /// chosen by the host (for example the compiler identity and the
    /// source digests of the binding bundle).
    pub bindings: &'a str,
}

/// Exact model-package identity, partitioning the same inputs as the cache key.
pub fn library_package_identity(
    manifest: &LibraryManifest,
    context: LibraryContext<'_>,
) -> Result<PackageIdentity, String> {
    let programs: BTreeMap<String, String> = PartKeyInputs::current()?
        .parts()?
        .into_iter()
        .filter(|part| {
            [ProgramRole::KerML, ProgramRole::SysML]
                .iter()
                .any(|role| role.section() == part.name)
        })
        .map(|part| (part.name, part.input_digest))
        .collect();
    let compiler: BTreeMap<String, String> = expected_compiler()
        .into_iter()
        .filter(|(name, _)| PROGRAM_COMPONENTS.contains(&name.as_str()))
        .collect();
    let metamodel = context
        .metamodel
        .digest()
        .map_err(|error| error.to_string())?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    identity_for_inputs(LibraryKeyInput {
        part: LIBRARY_SECTION,
        part_version: LIBRARY_PART_VERSION,
        recipe: &recipe(),
        manifest,
        programs: &programs,
        bindings: context.bindings,
        metamodel: &metamodel,
        compiler: &compiler,
        snapshot_format: SNAPSHOT_FORMAT,
        part_format: LIBRARY_PART_FORMAT,
    })
}

/// The cache key is the digest of the complete package identity: the two
/// contracts cannot protect different inputs. This deliberately invalidates
/// snapshots written before package provenance was recorded.
pub fn library_key(
    manifest: &LibraryManifest,
    context: LibraryContext<'_>,
) -> Result<String, String> {
    identity_key(&library_package_identity(manifest, context)?)
}

pub(crate) fn identity_key(identity: &PackageIdentity) -> Result<String, String> {
    Ok(FileDigest::of(&canonical_json(identity).map_err(|e| e.to_string())?).sha256)
}

#[derive(serde::Serialize)]
struct LibraryEngine<'a> {
    domain: &'static str,
    key_domain: &'static str,
    part: &'a str,
    part_version: u32,
    recipe: &'a PackRecipe,
    programs: &'a BTreeMap<String, String>,
    bindings: &'a str,
    compiler: &'a BTreeMap<String, String>,
    snapshot_format: u32,
    part_format: u32,
}

fn identity_for_inputs(input: LibraryKeyInput<'_>) -> Result<PackageIdentity, String> {
    input.manifest.check().map_err(|e| e.to_string())?;
    if input.bindings.trim().is_empty() || input.metamodel.trim().is_empty() {
        return Err("Prepared library requires exact bindings and metamodel identities".into());
    }
    let engine = LibraryEngine {
        domain: "intersect.kerml.library-engine.v1",
        key_domain: LIBRARY_KEY_DOMAIN,
        part: input.part,
        part_version: input.part_version,
        recipe: input.recipe,
        programs: input.programs,
        bindings: input.bindings,
        compiler: input.compiler,
        snapshot_format: input.snapshot_format,
        part_format: input.part_format,
    };
    Ok(PackageIdentity {
        content_digest: input.manifest.digest().map_err(|e| e.to_string())?,
        engine: FileDigest::of(&canonical_json(&engine).map_err(|e| e.to_string())?).sha256,
        schema: input.metamodel.to_owned(),
        dependency_lock_digest: DependencyLock::empty().digest()?,
    })
}

/// The manifest entry of the library part with this key and payload digest.
fn library_entry(key: &str, output_digest: String) -> PackPart {
    let mut components: Vec<String> = PROGRAM_COMPONENTS.iter().map(|c| (*c).to_owned()).collect();
    components.sort();
    PackPart {
        name: LIBRARY_SECTION.into(),
        components,
        inputs: Vec::new(),
        depends: Vec::new(),
        input_digest: key.to_owned(),
        output_digest,
    }
}

/// The library part's section of a structurally verified pack, after the
/// checks every part gets: the recipe's language and part layout, the entry
/// lists the program components and nothing else, the section has the
/// library kind, is required and binary, and its digests are the part's.
/// `None` when the pack has no library part.
pub fn pack_library_payload(pack: &LanguagePack) -> Option<Result<&[u8], String>> {
    let entry = pack
        .manifest()
        .parts
        .iter()
        .find(|part| part.name == LIBRARY_SECTION)?;
    Some((|| {
        check_layout(pack.manifest())?;
        if *entry != library_entry(&entry.input_digest, entry.output_digest.clone()) {
            return Err(format!(
                "Language pack {LIBRARY_SECTION} part entry differs from this recipe"
            ));
        }
        let section = pack
            .section_entry(LIBRARY_SECTION)
            .ok_or_else(|| format!("Language pack has no {LIBRARY_SECTION} section"))?;
        if section.kind != LIBRARY_KIND
            || section.encoding != PackSectionEncoding::Binary
            || !section.flags.contains(PackSectionFlag::Required)
        {
            return Err(format!(
                "Language pack {LIBRARY_SECTION} section kind differs from this recipe"
            ));
        }
        if section.part_digest != entry.input_digest
            || section.output_digest != entry.output_digest
            || section.digest.sha256 != entry.output_digest
        {
            return Err(format!(
                "Language pack {LIBRARY_SECTION} section digest differs from this recipe"
            ));
        }
        pack.required_section(LIBRARY_SECTION)
            .map_err(|error| error.to_string())
    })())
}

/// Load the snapshot of a library part payload (from a pack or a debug
/// file): the payload decodes (manifest, source digests), the key recomputed
/// for `expected` (the host's own library manifest) or else for the stored
/// manifest equals the snapshot's key, and the snapshot loads under the
/// context's metamodel. Returns the model and the key.
pub fn load_library_snapshot(
    payload: &[u8],
    context: LibraryContext<'_>,
    expected: Option<&LibraryManifest>,
    trust: SnapshotTrust,
) -> Result<(FrozenModel, String), String> {
    load_library_snapshot_parts(payload, context, expected, trust)
        .map(|(model, key, _, _)| (model, key))
}

fn load_library_snapshot_parts(
    payload: &[u8],
    context: LibraryContext<'_>,
    expected: Option<&LibraryManifest>,
    trust: SnapshotTrust,
) -> Result<(FrozenModel, String, PackageIdentity, LibraryManifest), String> {
    let part = decode_library_part(payload)
        .map_err(|error| format!("Library part does not decode: {error}"))?;
    if let Some(expected) = expected
        && *expected != part.manifest
    {
        return Err("Library part was built from other library sources".into());
    }
    let identity = library_package_identity(&part.manifest, context)?;
    let key = identity_key(&identity)?;
    let model = FrozenModel::from_snapshot(
        context.metamodel.clone(),
        part.snapshot,
        &key_bytes(&key).map_err(|error| error.to_string())?,
        trust,
    )
    .map_err(|error| format!("Library snapshot refused: {error}"))?;
    let model = model
        .with_package_identity(&identity)
        .map_err(|e| e.to_string())?;
    Ok((model, key, identity, part.manifest))
}

/// Where a prepared library came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryOrigin {
    /// Loaded from the snapshot.
    Snapshot,
    /// Parsed, because the snapshot was missing, stale or refused (the
    /// reason), or because the host asked not to use it.
    Parsed(String),
}

/// A prepared standard library.
pub struct PreparedLibrary {
    pub model: FrozenModel,
    /// The manifest and exact model-package identity protected by `key`.
    pub manifest: LibraryManifest,
    pub identity: PackageIdentity,
    pub origin: LibraryOrigin,
    /// The library key it was loaded or built under.
    pub key: String,
    /// When parsed: the library part payload a host that can write saves
    /// (`with_library_part`, or a debug snapshot file).
    pub payload: Option<Vec<u8>>,
}

/// Choices of [`prepare_library`].
#[derive(Clone, Copy, Debug)]
pub struct LibraryOptions {
    /// Ignore any snapshot and parse (`--no-library-cache`).
    pub use_snapshot: bool,
    pub trust: SnapshotTrust,
}

impl Default for LibraryOptions {
    fn default() -> Self {
        Self {
            use_snapshot: true,
            trust: SnapshotTrust::HostCache,
        }
    }
}

/// The prepared standard library ("SysML with its standard library"): the
/// snapshot in `payload` (a pack's library part, see
/// `pack_library_payload`, or a debug file) when it loads under `context`
/// for the host's `sources` (when given), otherwise `parse` of the sources
/// (the host's, else those stored in the payload) with a new payload to
/// save. `parse` returns the validated model or an error describing the
/// failed load. Fails when there is neither a usable snapshot nor sources.
pub fn prepare_library(
    payload: Option<&[u8]>,
    sources: Option<&[LibrarySource]>,
    context: LibraryContext<'_>,
    options: LibraryOptions,
    parse: impl FnOnce(&[LibrarySource]) -> Result<FrozenModel, String>,
) -> Result<PreparedLibrary, String> {
    let expected = sources
        .map(LibraryManifest::of_sources)
        .transpose()
        .map_err(|error| error.to_string())?;
    let reason = match (payload, options.use_snapshot) {
        (_, false) => "the library cache is disabled".to_owned(),
        (None, true) => "no library snapshot".to_owned(),
        (Some(payload), true) => {
            match load_library_snapshot_parts(payload, context, expected.as_ref(), options.trust) {
                Ok((model, key, identity, manifest)) => {
                    return Ok(PreparedLibrary {
                        model,
                        identity,
                        manifest,
                        origin: LibraryOrigin::Snapshot,
                        key,
                        payload: None,
                    });
                }
                Err(reason) => reason,
            }
        }
    };
    let stored;
    let sources = match sources {
        Some(sources) => sources,
        None => {
            let part = payload
                .map(decode_library_part)
                .transpose()
                .map_err(|error| {
                    format!("{reason}; the library sources in the part do not decode: {error}")
                })?;
            stored = part
                .map(|part| part.sources())
                .ok_or_else(|| format!("{reason}, and no library sources to parse"))?;
            &stored
        }
    };
    let model = parse(sources)?;
    let manifest = LibraryManifest::of_sources(sources).map_err(|error| error.to_string())?;
    let identity = library_package_identity(&manifest, context)?;
    let key = identity_key(&identity)?;
    if model.metamodel().digest().map_err(|e| e.to_string())?
        != context.metamodel.digest().map_err(|e| e.to_string())?
    {
        return Err("Parsed library uses another metamodel".into());
    }
    let model = model
        .with_package_identity(&identity)
        .map_err(|e| e.to_string())?;
    let snapshot = model
        .to_snapshot(&key_bytes(&key).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let payload = encode_library_part(sources, &snapshot).map_err(|error| error.to_string())?;
    Ok(PreparedLibrary {
        model,
        identity,
        manifest,
        origin: LibraryOrigin::Parsed(reason),
        key,
        payload: Some(payload),
    })
}

/// The library key a payload was built under (its snapshot header's key).
pub fn payload_key(payload: &[u8]) -> Result<String, String> {
    let part = decode_library_part(payload).map_err(|error| error.to_string())?;
    let header = snapshot_header(part.snapshot).map_err(|error| error.to_string())?;
    Ok(header
        .key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// `pack` with its library part replaced by (or extended with) `payload`;
/// every other part and section is kept byte for byte. Deterministic.
pub fn with_library_part(pack: &LanguagePack, payload: Vec<u8>) -> Result<Vec<u8>, String> {
    rebuild(pack, LIBRARY_SECTION, Some(payload))
}

/// `pack` without its library part.
pub fn without_library_part(pack: &LanguagePack) -> Result<Vec<u8>, String> {
    rebuild(pack, LIBRARY_SECTION, None)
}

/// `pack` without its KerML bindings part (`BINDINGS_SECTION`), for a host
/// that runs only the combined KerML+SysML bindings
/// (`load_pack_sysml_bindings`) and would otherwise fetch a part it never
/// opens. Every other part and section, the library part included, is kept
/// byte for byte. Deterministic. `load_pack_bindings` refuses the result, and
/// a build given it as the previous pack compiles the KerML bindings again,
/// so a host keeps the full pack to build from.
pub fn without_kerml_bindings(pack: &LanguagePack) -> Result<Vec<u8>, String> {
    check_layout(pack.manifest())?;
    rebuild(pack, super::BINDINGS_SECTION, None)
}

/// `pack` without the part and section `dropped`, and with the library part
/// `payload` when one is given (`dropped` is then the library part).
fn rebuild(
    pack: &LanguagePack,
    dropped: &str,
    payload: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let manifest = pack.manifest();
    if manifest.language != LANGUAGE {
        return Err("Language pack language differs from this recipe".into());
    }
    let mut parts: Vec<PackPart> = manifest
        .parts
        .iter()
        .filter(|part| part.name != dropped)
        .cloned()
        .collect();
    let library = match payload {
        Some(payload) => {
            let key = payload_key(&payload)?;
            let output = FileDigest::of(&payload).sha256;
            parts.push(library_entry(&key, output.clone()));
            Some((payload, key, output))
        }
        None => None,
    };
    let mut builder = PackBuilder::new(PackHeader {
        language: manifest.language.clone(),
        recipe: manifest.recipe.clone(),
        compiler: manifest.compiler.clone(),
        recipe_constants: manifest.recipe_constants.clone(),
        inputs: manifest.inputs.clone(),
        parts,
        requires_hooks: manifest.requires_hooks.clone(),
        depends_on: manifest.depends_on.clone(),
    });
    for section in &manifest.sections {
        if section.name == dropped {
            continue;
        }
        let bytes = pack
            .required_section(&section.name)
            .map_err(|error| error.to_string())?
            .to_vec();
        builder = match section.encoding {
            PackSectionEncoding::Binary => builder.binary_section(
                section.name.clone(),
                section.kind.clone(),
                section.flags,
                bytes,
                section.part_digest.clone(),
                section.output_digest.clone(),
            ),
            _ => builder.section(
                section.name.clone(),
                section.kind.clone(),
                section.flags,
                bytes,
                section.part_digest.clone(),
                section.output_digest.clone(),
            ),
        };
    }
    if let Some((payload, key, output)) = library {
        builder = builder.binary_section(
            LIBRARY_SECTION,
            LIBRARY_KIND,
            PackSectionFlags::REQUIRED,
            payload,
            key,
            output,
        );
    }
    builder.build().map_err(|error| error.to_string())
}

/// Whether a pack build keeps `previous`'s library part for the library
/// `key` recomputed now: `plan_parts` over the library entry, where the
/// previous part counts as loadable when it passes `pack_library_payload`
/// and its snapshot loads under `context`. With the decision, the payload
/// to keep (on `Reuse`) and the refusal (when the key matched but the part
/// did not load).
pub fn plan_library<'p>(
    previous: Option<&'p LanguagePack>,
    key: &str,
    context: LibraryContext<'_>,
) -> (PartDecision, Option<&'p [u8]>, Option<String>) {
    let expected = [library_entry(key, String::new())];
    let entries = previous.map(|pack| pack.manifest().parts.as_slice());
    let all: BTreeSet<String> = [LIBRARY_SECTION.to_owned()].into();
    let candidate = plan_parts(entries, &expected, &all);
    if candidate.first().map(|(_, decision)| *decision) != Some(PartDecision::Reuse) {
        let decision = candidate.first().map_or(
            PartDecision::Rebuild(RebuildReason::NoPrevious),
            |(_, d)| *d,
        );
        return (decision, None, None);
    }
    let loaded = previous.and_then(pack_library_payload).map(|payload| {
        payload.and_then(|payload| {
            load_library_snapshot(payload, context, None, SnapshotTrust::HostCache).and_then(
                |(_, loaded_key)| {
                    if loaded_key == key {
                        Ok(payload)
                    } else {
                        Err("Library part was built from other library sources".into())
                    }
                },
            )
        })
    });
    match loaded {
        Some(Ok(payload)) => (PartDecision::Reuse, Some(payload), None),
        Some(Err(error)) => (
            PartDecision::Rebuild(RebuildReason::LoadRefused),
            None,
            Some(error),
        ),
        None => (
            PartDecision::Rebuild(RebuildReason::LoadRefused),
            None,
            Some(format!("Language pack has no {LIBRARY_SECTION} part")),
        ),
    }
}

/// Library sources in parse order: KerML sources by URI, then SysML
/// sources by URI (the order `frozen_library_load` always used). Other
/// languages are refused.
pub fn library_parse_order(sources: &[LibrarySource]) -> Result<Vec<&LibrarySource>, String> {
    let mut ordered: Vec<&LibrarySource> = sources.iter().collect();
    for source in &ordered {
        if source.language != KERML_SOURCE && source.language != SYSML_SOURCE {
            return Err(format!(
                "Library source {} has unknown language {:?}",
                source.uri, source.language
            ));
        }
    }
    ordered.sort_by(|left, right| {
        (left.language != KERML_SOURCE, &left.uri)
            .cmp(&(right.language != KERML_SOURCE, &right.uri))
    });
    Ok(ordered)
}

#[cfg(test)]
#[path = "language_pack_library_tests.rs"]
mod tests;
